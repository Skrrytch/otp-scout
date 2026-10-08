use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::config;
use crate::rules::DetectionRule;

/// The catalog is maintained on GitHub, independent of releases.
const CATALOG_URL: &str = "https://raw.githubusercontent.com/Skrrytch/otp-scout/main/catalog/catalog.json";
/// Last successfully downloaded catalog, used when GitHub is unreachable.
const CACHE_FILE: &str = "catalog-cache.json";
/// Highest catalog format version this build understands.
const SUPPORTED_VERSION: u32 = 1;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Catalog of preconfigured rules: category → app → language → rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub languages: Vec<Language>,
    pub categories: Vec<Category>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Language {
    /// ISO 639-1 code, e.g. `de`.
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    /// Display name per language id.
    pub name: BTreeMap<String, String>,
    pub apps: Vec<CatalogApp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogApp {
    pub id: String,
    pub name: String,
    /// Languages whose rules were not yet checked against a real mail.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unverified: Vec<String>,
    /// Rules per language id. A language may use code and link rules alike.
    pub variants: BTreeMap<String, Vec<DetectionRule>>,
}

/// Catalog plus a note if it came from the local cache instead of GitHub.
#[derive(Debug, Serialize)]
pub struct LoadedCatalog {
    pub catalog: Catalog,
    pub offline_reason: Option<String>,
}

fn parse(json: &str) -> Result<Catalog> {
    let catalog: Catalog = serde_json::from_str(json).context("Invalid catalog")?;
    if catalog.version > SUPPORTED_VERSION {
        bail!("Catalog format v{} is newer than supported (v{SUPPORTED_VERSION}) – please update OTP-Scout", catalog.version);
    }
    Ok(catalog)
}

async fn fetch() -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("OTP-Scout/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = client.get(CATALOG_URL).send().await?.error_for_status()?;
    Ok(resp.text().await?)
}

/// Downloads the catalog from GitHub and caches it; falls back to the cache.
pub async fn load() -> Result<LoadedCatalog> {
    let cache = config::config_dir()?.join(CACHE_FILE);
    let err = match fetch().await.and_then(|json| Ok((parse(&json)?, json))) {
        Ok((catalog, json)) => {
            if let Err(e) = std::fs::write(&cache, json) {
                tracing::warn!("Failed to cache catalog: {e}");
            }
            return Ok(LoadedCatalog { catalog, offline_reason: None });
        }
        Err(e) => format!("{e:#}"),
    };
    tracing::warn!("Catalog download failed: {err}");
    let json = std::fs::read_to_string(&cache)
        .with_context(|| format!("Catalog could not be loaded from GitHub ({err}) and no cached copy exists"))?;
    Ok(LoadedCatalog { catalog: parse(&json)?, offline_reason: Some(err) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{test_rule, CompiledRule, RuleKind};

    /// Validates the catalog file in this repository before it is pushed.
    fn local() -> Catalog {
        parse(include_str!("../../catalog/catalog.json")).unwrap()
    }

    #[test]
    fn test_catalog_parses_and_rules_compile() {
        let catalog = local();
        let langs: Vec<_> = catalog.languages.iter().map(|l| l.id.as_str()).collect();
        for cat in &catalog.categories {
            for lang in &langs {
                assert!(cat.name.contains_key(*lang), "{}: missing name for {lang}", cat.id);
            }
            for app in &cat.apps {
                assert!(!app.variants.is_empty(), "{}: no variants", app.id);
                for lang in &app.unverified {
                    assert!(app.variants.contains_key(lang), "{}: unverified {lang} has no rules", app.id);
                }
                for (lang, rules) in &app.variants {
                    assert!(langs.contains(&lang.as_str()), "{}: unknown language {lang}", app.id);
                    assert!(!rules.is_empty(), "{}/{lang}: no rules", app.id);
                    for rule in rules {
                        CompiledRule::compile(rule).unwrap();
                        if rule.kind == RuleKind::Link {
                            assert!(rule.link_prefix.starts_with("https://"), "{}/{lang}: link_prefix", app.id);
                        }
                    }
                }
            }
        }
    }

    /// Real sender/subject pairs from received mails.
    #[test]
    fn test_catalog_rules_match_real_mails() {
        let cases = [
            ("spotify", "de", "no-reply@alerts.spotify.com", "913088\u{a0}\u{2013} dein Spotify Anmeldecode", "913088"),
            ("twitch", "de", "no-reply@twitch.tv", "214859 \u{2013} Dein Twitch-Bestätigungscode", "214859"),
            ("openai", "en", "noreply@tm.openai.com", "Your OpenAI code is 600160", "600160"),
            ("openrouter", "en", "notifications@openrouter.ai", "898272 is your verification code", "898272"),
            ("silentwood", "de", "contact@silentwood.com", "Dein Code lautet 923660", "923660"),
        ];
        let catalog = local();
        for (app_id, lang, from, subject, expected) in cases {
            let app = catalog.categories.iter().flat_map(|c| &c.apps).find(|a| a.id == app_id).unwrap();
            let code = test_rule(&app.variants[lang][0], from, subject, "").unwrap().value;
            assert_eq!(code.as_deref(), Some(expected), "{app_id}/{lang}");
        }
    }
}
