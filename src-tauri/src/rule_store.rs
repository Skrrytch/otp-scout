//! Rules live in `rules.json`, independent of accounts. Tags assign them:
//! every account has a fixed tag, a rule is checked by each account whose
//! tag it carries, `*` stands for all accounts.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::config::AccountConfig;
use crate::rules::DetectionRule;

/// Highest `rules.json` format version this build understands.
const RULES_VERSION: u32 = 1;
/// Tag for accounts whose name yields no usable characters.
const FALLBACK_TAG: &str = "account";

#[derive(Serialize, Deserialize)]
struct RuleFile {
    version: u32,
    rules: Vec<DetectionRule>,
}

pub fn load(path: &Path) -> Result<Vec<DetectionRule>> {
    let data = std::fs::read_to_string(path)?;
    let file: RuleFile = serde_json::from_str(&data).context("Invalid rules.json")?;
    if file.version > RULES_VERSION {
        bail!("rules.json has version {}, this build supports up to {RULES_VERSION}", file.version);
    }
    Ok(file.rules)
}

pub fn save(path: &Path, rules: &[DetectionRule]) -> Result<()> {
    let file = RuleFile { version: RULES_VERSION, rules: rules.to_vec() };
    std::fs::write(path, serde_json::to_string_pretty(&file)?)?;
    Ok(())
}

/// Lowercase ASCII slug of `label`, numbered if already taken: `gmail`, `gmail-2`.
pub fn unique_tag<'a>(label: &str, taken: impl IntoIterator<Item = &'a str>) -> String {
    let mut slug = String::new();
    for c in label.trim().to_lowercase().chars() {
        match c {
            'ä' => slug.push_str("ae"),
            'ö' => slug.push_str("oe"),
            'ü' => slug.push_str("ue"),
            'ß' => slug.push_str("ss"),
            c if c.is_ascii_alphanumeric() => slug.push(c),
            _ if !slug.ends_with('-') => slug.push('-'),
            _ => {}
        }
    }
    let slug = match slug.trim_matches('-') {
        "" => FALLBACK_TAG.to_string(),
        s => s.to_string(),
    };
    let taken: Vec<&str> = taken.into_iter().collect();
    std::iter::once(slug.clone())
        .chain((2..).map(|n| format!("{slug}-{n}")))
        .find(|t| !taken.contains(&t.as_str()))
        .unwrap()
}

pub fn rules_for(rules: &[DetectionRule], tag: &str) -> Vec<DetectionRule> {
    rules.iter().filter(|r| r.applies_to(tag)).cloned().collect()
}

pub fn needs_migration(accounts: &[AccountConfig]) -> bool {
    accounts.iter().any(|a| a.tag.is_empty() || !a.rules.is_empty())
}

/// Gives every account a tag and moves `accounts[].rules` into `rules`, each
/// tagged with its former account. Identical rules are merged, their tags joined.
pub fn migrate(accounts: &mut [AccountConfig], rules: &mut Vec<DetectionRule>) {
    for i in 0..accounts.len() {
        if accounts[i].tag.is_empty() {
            let tag = unique_tag(&accounts[i].label, accounts.iter().map(|a| a.tag.as_str()));
            accounts[i].tag = tag;
        }
    }
    for account in accounts.iter_mut() {
        for rule in std::mem::take(&mut account.rules) {
            match rules.iter_mut().find(|r| r.same_content(&rule)) {
                Some(same) if same.tags.contains(&account.tag) => {}
                Some(same) => same.tags.push(account.tag.clone()),
                None => rules.push(DetectionRule {
                    id: new_id(),
                    tags: vec![account.tag.clone()],
                    ..rule
                }),
            }
        }
    }
}

/// Accepts lowercase letters, digits and dashes, as `unique_tag` produces.
pub fn validate_tag(tag: &str) -> Result<()> {
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        bail!("Tag may only contain lowercase letters, digits and dashes");
    }
    Ok(())
}

pub fn rename_tag(rules: &mut [DetectionRule], old: &str, new: &str) {
    for rule in rules {
        for tag in rule.tags.iter_mut().filter(|t| *t == old) {
            *tag = new.to_string();
        }
        rule.tags.dedup();
    }
}

/// Replaces the rule with the same id, or appends it with a new id.
/// Tags are trimmed and deduplicated.
pub fn upsert(rules: &mut Vec<DetectionRule>, mut rule: DetectionRule) {
    let mut tags = Vec::new();
    for tag in rule.tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !tags.iter().any(|t| t == tag) {
            tags.push(tag.to_string());
        }
    }
    rule.tags = tags;
    match rules.iter_mut().find(|r| !rule.id.is_empty() && r.id == rule.id) {
        Some(existing) => *existing = rule,
        None => rules.push(DetectionRule { id: new_id(), ..rule }),
    }
}

/// Detaches a deleted account. Its rules stay in `rules.json`, without a tag
/// if it was their only account.
pub fn remove_tag(rules: &mut [DetectionRule], tag: &str) {
    for rule in rules {
        rule.tags.retain(|t| t != tag);
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleKind;

    fn rule(label: &str, subject: &str) -> DetectionRule {
        DetectionRule {
            label: label.into(),
            kind: RuleKind::Code,
            subject: subject.into(),
            ..DetectionRule::default()
        }
    }

    fn account(label: &str, rules: Vec<DetectionRule>) -> AccountConfig {
        AccountConfig { label: label.into(), rules, ..Default::default() }
    }

    fn tags(rule: &DetectionRule) -> Vec<&str> {
        rule.tags.iter().map(String::as_str).collect()
    }

    #[test]
    fn test_unique_tag() {
        assert_eq!(unique_tag("Gmail", []), "gmail");
        assert_eq!(unique_tag("Gmail", ["gmail"]), "gmail-2");
        assert_eq!(unique_tag("Gmail", ["gmail", "gmail-2"]), "gmail-3");
        assert_eq!(unique_tag("  Büro (Proton) ", []), "buero-proton");
        assert_eq!(unique_tag("***", []), "account");
        assert_eq!(unique_tag("", ["account"]), "account-2");
    }

    #[test]
    fn test_migrate_tags_merges_and_clears_accounts() {
        let spotify = rule("Spotify", "{code} spotify");
        let mut accounts = vec![
            account("Proton", vec![spotify.clone(), rule("Work", "{code} work")]),
            account("Proton", vec![spotify.clone()]),
            account("Empty", vec![]),
        ];
        let mut rules = vec![];
        assert!(needs_migration(&accounts));
        migrate(&mut accounts, &mut rules);

        assert_eq!(accounts.iter().map(|a| a.tag.as_str()).collect::<Vec<_>>(), ["proton", "proton-2", "empty"]);
        assert!(accounts.iter().all(|a| a.rules.is_empty()));
        assert!(!needs_migration(&accounts));
        assert_eq!(rules.len(), 2);
        assert_eq!(tags(&rules[0]), ["proton", "proton-2"]);
        assert_eq!(tags(&rules[1]), ["proton"]);
        assert!(rules.iter().all(|r| !r.id.is_empty()) && rules[0].id != rules[1].id);
        // Behaviour unchanged: each account checks exactly its former rules.
        assert_eq!(rules_for(&rules, "proton").len(), 2);
        assert_eq!(rules_for(&rules, "proton-2").len(), 1);
        assert!(rules_for(&rules, "empty").is_empty());
    }

    #[test]
    fn test_migrate_keeps_existing_tag() {
        let mut accounts = vec![AccountConfig { tag: "old".into(), ..account("Renamed", vec![rule("A", "{code} a")]) }];
        let mut rules = vec![];
        migrate(&mut accounts, &mut rules);
        assert_eq!(accounts[0].tag, "old");
        assert_eq!(tags(&rules[0]), ["old"]);
    }

    fn stored(id: &str, tag_list: &[&str]) -> DetectionRule {
        DetectionRule { id: id.into(), tags: tag_list.iter().map(|t| t.to_string()).collect(), ..rule(id, "{code} x") }
    }

    #[test]
    fn test_upsert() {
        let mut rules = vec![stored("a", &["x"])];
        upsert(&mut rules, DetectionRule { label: "changed".into(), ..stored("a", &[" y ", "y", ""]) });
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].label, "changed");
        assert_eq!(tags(&rules[0]), ["y"]);
        upsert(&mut rules, stored("", &["*"]));
        assert_eq!(rules.len(), 2);
        assert!(!rules[1].id.is_empty());
    }

    #[test]
    fn test_rename_tag() {
        let mut rules = vec![stored("a", &["old", "b"]), stored("b", &["*"])];
        rename_tag(&mut rules, "old", "new");
        assert_eq!(tags(&rules[0]), ["new", "b"]);
        assert_eq!(tags(&rules[1]), ["*"]);
    }

    #[test]
    fn test_validate_tag() {
        assert!(validate_tag("gmail-2").is_ok());
        assert!(validate_tag("*").is_err());
        assert!(validate_tag("Gmail").is_err());
        assert!(validate_tag("").is_err());
    }

    #[test]
    fn test_remove_tag_keeps_rules() {
        let mut rules = vec![stored("x", &["a"]), stored("y", &["a", "b"])];
        remove_tag(&mut rules, "a");
        assert!(rules[0].tags.is_empty());
        assert_eq!(tags(&rules[1]), ["b"]);
    }

    #[test]
    fn test_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("otp-scout-test-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rules.json");
        let rules = vec![DetectionRule {
            catalog: Some(crate::rules::CatalogRef { app: "spotify".into(), lang: "de".into() }),
            ..stored("id1", &["*"])
        }];
        save(&path, &rules).unwrap();
        assert_eq!(load(&path).unwrap(), rules);
        std::fs::write(&path, r#"{"version":99,"rules":[]}"#).unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
