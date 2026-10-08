use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::rule_store;
use crate::rules::{default_rules, CompiledRules, DetectionRule};

const KEYRING_SERVICE: &str = "otp-scout";
const CONFIG_DIR: &str = "otp-scout";
/// Names used before the rename to OTP-Scout; migrated on first access.
const LEGACY_KEYRING_SERVICE: &str = "authscout";
const LEGACY_CONFIG_DIR: &str = "authscout";
const CONFIG_FILE: &str = "config.json";
const RULES_FILE: &str = "rules.json";

/// How the IMAP connection is encrypted.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Security {
    /// Implicit TLS from the first byte (usually port 993).
    #[default]
    Ssl,
    /// Plain connection upgraded via STARTTLS (usually port 143; Proton Mail Bridge: 1143).
    Starttls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub id: String,
    pub label: String,
    pub server: String,
    pub port: u16,
    #[serde(default)]
    pub security: Security,
    /// Accept self-signed/invalid certificates (e.g. a local Proton Mail Bridge).
    #[serde(default)]
    pub allow_invalid_certs: bool,
    pub user: String,
    /// Accepted from the UI, never written to config.json (lives in the keyring).
    #[serde(default, skip_serializing)]
    pub pass: String,
    pub mailbox: String,
    /// Fixed on creation, kept on rename; assigns rules to this account.
    #[serde(default)]
    pub tag: String,
    /// Rules checked by this account. Filled from `rules.json` for the UI and
    /// the IMAP loop, never written to config.json (only read for migration).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<DetectionRule>,
    #[serde(default)]
    pub sender_filter: Vec<String>,
}

impl Default for AccountConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            label: String::new(),
            server: String::new(),
            port: 993,
            security: Security::Ssl,
            allow_invalid_certs: false,
            user: String::new(),
            pass: String::new(),
            mailbox: "INBOX".to_string(),
            tag: String::new(),
            rules: vec![],
            sender_filter: vec![],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub api: ApiConfig,
    /// Contents of `rules.json`.
    #[serde(skip)]
    pub rules: Vec<DetectionRule>,
}

pub const DEFAULT_API_PORT: u16 = 6870;

/// Local HTTP API (127.0.0.1 only). The token lives in the keyring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_api_port")]
    pub port: u16,
    /// Allowed link prefixes for /api/otp/link. Empty = any https link.
    #[serde(default)]
    pub allowed_links: Vec<String>,
}

fn default_api_port() -> u16 {
    DEFAULT_API_PORT
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_API_PORT, allowed_links: vec![] }
    }
}

pub type SharedConfig = Arc<Mutex<AppConfig>>;

/// `~/.config/otp-scout`, created if missing.
pub fn config_dir() -> Result<std::path::PathBuf> {
    let dir = dirs::config_dir().context("No config dir")?.join(CONFIG_DIR);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn config_path() -> Result<std::path::PathBuf> {
    let base = dirs::config_dir().context("No config dir")?;
    let path = config_dir()?.join(CONFIG_FILE);
    let legacy = base.join(LEGACY_CONFIG_DIR).join(CONFIG_FILE);
    if !path.exists() && legacy.exists() {
        std::fs::copy(&legacy, &path).context("Failed to migrate legacy config")?;
        tracing::info!("Migrated config from {}", legacy.display());
    }
    Ok(path)
}

/// Loads config.json and rules.json; moves rules out of config.json first if
/// needed (old config kept as config.json.bak).
pub fn load_config() -> Result<AppConfig> {
    let path = config_path()?;
    let mut config: AppConfig = if path.exists() {
        let data = std::fs::read_to_string(&path)?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        AppConfig::default()
    };

    let rules_path = config_dir()?.join(RULES_FILE);
    if rules_path.exists() {
        config.rules = rule_store::load(&rules_path).or_else(|e| {
            // Keep the file for repair instead of overwriting it on the next save.
            let broken = rules_path.with_extension("json.broken");
            std::fs::rename(&rules_path, &broken)?;
            tracing::error!("{e:#} – moved to {}, starting without rules", broken.display());
            anyhow::Ok(vec![])
        })?;
    }

    if rule_store::needs_migration(&config.accounts) {
        std::fs::copy(&path, path.with_extension("json.bak")).context("Failed to back up config.json")?;
        rule_store::migrate(&mut config.accounts, &mut config.rules);
        save_rules(&config)?;
        save_config(&config)?;
        tracing::info!("Moved rules to {} ({} rules)", rules_path.display(), config.rules.len());
    }
    Ok(config)
}

fn save_config(config: &AppConfig) -> Result<()> {
    let path = config_path()?;
    let data = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, data)?;
    Ok(())
}

fn save_rules(config: &AppConfig) -> Result<()> {
    rule_store::save(&config_dir()?.join(RULES_FILE), &config.rules)
}

/// Accounts with the rules each one checks.
fn accounts_with_rules(config: &AppConfig) -> Vec<AccountConfig> {
    config
        .accounts
        .iter()
        .map(|a| AccountConfig { rules: rule_store::rules_for(&config.rules, &a.tag), ..a.clone() })
        .collect()
}

/// Stores the rules edited in the account dialog; no rules = default rules.
fn apply_account_rules(config: &mut AppConfig, tag: &str, mut edited: Vec<DetectionRule>) {
    if edited.is_empty() {
        edited = default_rules();
    }
    let others: Vec<String> = config.accounts.iter().map(|a| a.tag.clone()).filter(|t| t != tag).collect();
    rule_store::replace_for_account(&mut config.rules, tag, &others, edited);
}

fn keyring_entry(account_id: &str) -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, account_id)
        .context("Failed to create keyring entry")
}

fn load_password(account_id: &str) -> Result<String> {
    if let Ok(pass) = keyring_entry(account_id)?.get_password() {
        return Ok(pass);
    }
    // Copy from the legacy service; the old entry is kept as a fallback.
    let pass = Entry::new(LEGACY_KEYRING_SERVICE, account_id)?
        .get_password()
        .context("Password not found in keyring")?;
    store_password(account_id, &pass)?;
    tracing::info!("Migrated keyring password for account {account_id}");
    Ok(pass)
}

fn store_password(account_id: &str, password: &str) -> Result<()> {
    keyring_entry(account_id)?
        .set_password(password)
        .context("Failed to store password in keyring")
}

fn delete_password(account_id: &str) -> Result<()> {
    keyring_entry(account_id)?
        .delete_credential()
        .context("Failed to delete password from keyring")
}

// -- Tauri command helpers --

pub fn add_account(config: &SharedConfig, mut account: AccountConfig) -> Result<Vec<AccountConfig>> {
    let password = std::mem::take(&mut account.pass);
    if password.is_empty() {
        anyhow::bail!("Password is required");
    }
    let edited = std::mem::take(&mut account.rules);
    CompiledRules::compile(&edited)?;
    let mut cfg = config.blocking_lock();
    store_password(&account.id, &password)?;
    account.tag = rule_store::unique_tag(&account.label, cfg.accounts.iter().map(|a| a.tag.as_str()));
    let tag = account.tag.clone();
    cfg.accounts.push(account);
    apply_account_rules(&mut cfg, &tag, edited);
    save_rules(&cfg)?;
    save_config(&cfg)?;
    Ok(accounts_with_rules(&cfg))
}

pub fn update_account(
    config: &SharedConfig,
    mut account: AccountConfig,
) -> Result<Vec<AccountConfig>> {
    CompiledRules::compile(&account.rules)?;
    let mut cfg = config.blocking_lock();
    let mut tag = None;
    if let Some(existing) = cfg.accounts.iter_mut().find(|a| a.id == account.id) {
        if !account.pass.is_empty() {
            store_password(&account.id, &account.pass)?;
        }
        existing.label = std::mem::take(&mut account.label);
        existing.server = std::mem::take(&mut account.server);
        existing.port = account.port;
        existing.security = account.security;
        existing.allow_invalid_certs = account.allow_invalid_certs;
        existing.user = std::mem::take(&mut account.user);
        existing.mailbox = std::mem::take(&mut account.mailbox);
        existing.sender_filter = std::mem::take(&mut account.sender_filter);
        tag = Some(existing.tag.clone());
    }
    if let Some(tag) = tag {
        apply_account_rules(&mut cfg, &tag, account.rules);
        save_rules(&cfg)?;
    }
    save_config(&cfg)?;
    Ok(accounts_with_rules(&cfg))
}

pub fn remove_account(config: &SharedConfig, id: &str) -> Result<Vec<AccountConfig>> {
    let mut cfg = config.blocking_lock();
    if let Some(tag) = cfg.accounts.iter().find(|a| a.id == id).map(|a| a.tag.clone()) {
        rule_store::remove_tag(&mut cfg.rules, &tag);
        save_rules(&cfg)?;
    }
    cfg.accounts.retain(|a| a.id != id);
    let _ = delete_password(id);
    save_config(&cfg)?;
    Ok(accounts_with_rules(&cfg))
}

pub fn get_api_config(config: &SharedConfig) -> ApiConfig {
    config.blocking_lock().api.clone()
}

pub async fn get_api_config_async(config: &SharedConfig) -> ApiConfig {
    config.lock().await.api.clone()
}

pub fn set_api_config(config: &SharedConfig, api: ApiConfig) -> Result<()> {
    if api.port < 1024 {
        anyhow::bail!("Port must be between 1024 and 65535");
    }
    if let Some(bad) = api.allowed_links.iter().find(|p| !p.trim().starts_with("https://")) {
        anyhow::bail!("Allowed link must start with https:// – '{bad}'");
    }
    let mut cfg = config.blocking_lock();
    cfg.api = api;
    save_config(&cfg)
}

const API_TOKEN_KEY: &str = "api-token";

fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Returns the API token, creating one on first use.
pub fn api_token() -> Result<String> {
    let entry = keyring_entry(API_TOKEN_KEY)?;
    match entry.get_password() {
        Ok(token) => Ok(token),
        Err(_) => regenerate_api_token(),
    }
}

pub fn regenerate_api_token() -> Result<String> {
    let token = new_token();
    store_password(API_TOKEN_KEY, &token)?;
    Ok(token)
}

pub fn get_accounts(config: &SharedConfig) -> Vec<AccountConfig> {
    accounts_with_rules(&config.blocking_lock())
}

/// Load all accounts with their rules, injecting passwords from keyring.
pub fn load_accounts_with_passwords() -> Result<Vec<AccountConfig>> {
    let mut accounts = accounts_with_rules(&load_config()?);
    for account in &mut accounts {
        account.pass = load_password(&account.id).unwrap_or_default();
    }
    Ok(accounts)
}