use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// Outcome of the last communication with an account's IMAP server.
#[derive(Debug, Clone, Serialize)]
pub struct ConnStatus {
    pub account_id: String,
    /// `connecting`, `ok` or `error`
    pub state: &'static str,
    pub message: String,
    /// Unix timestamp (seconds)
    pub timestamp: u64,
}

#[derive(Default)]
pub struct StatusStore(Mutex<HashMap<String, ConnStatus>>);

impl StatusStore {
    pub fn all(&self) -> Vec<ConnStatus> {
        self.0.lock().unwrap().values().cloned().collect()
    }
}

/// Stores the status and pushes it to the settings UI.
pub fn set(app: &AppHandle, account_id: &str, state: &'static str, message: impl Into<String>) {
    let status = ConnStatus {
        account_id: account_id.to_string(),
        state,
        message: message.into(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    if let Some(store) = app.try_state::<StatusStore>() {
        store.0.lock().unwrap().insert(account_id.to_string(), status.clone());
    }
    let _ = app.emit("account-status", &status);
}
