//! In-memory log of checked mails and connection events, shown in the
//! settings UI. Never written to disk; lost on exit.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::rules::{MatchTrace, RuleKind};

/// Entries kept; the oldest are dropped first.
const CAPACITY: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// Increasing number, unique per app run.
    pub seq: u64,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    pub account_id: String,
    pub account: String,
    #[serde(flatten)]
    pub kind: LogKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogKind {
    Mail(MailEntry),
    Event(EventEntry),
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// A rule found a code or link.
    Found,
    /// A rule matched sender and subject but found nothing in the body.
    Partial,
    /// No rule matched the headers.
    NoMatch,
}

/// One checked mail. Holds headers and results, never the mail text.
#[derive(Debug, Clone, Serialize)]
pub struct MailEntry {
    pub uid: u32,
    pub from: Option<String>,
    pub subject: Option<String>,
    pub outcome: Outcome,
    pub kind: Option<RuleKind>,
    /// Code, or link cut after its path (no query or fragment tokens).
    pub value: Option<String>,
    /// Checked rules in order, up to the first match.
    pub traces: Vec<MatchTrace>,
    pub timings: Timings,
}

/// Milliseconds since the server reported new mail (IDLE wake-up).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Timings {
    pub headers: u64,
    pub body: Option<u64>,
    pub done: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventEntry {
    /// `info` or `error`
    pub level: &'static str,
    pub message: String,
}

#[derive(Default)]
pub struct LogBuffer {
    entries: Mutex<VecDeque<LogEntry>>,
}

impl LogBuffer {
    pub fn all(&self) -> Vec<LogEntry> {
        self.entries.lock().unwrap().iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    fn push(&self, account_id: &str, account: &str, kind: LogKind) -> LogEntry {
        let mut entries = self.entries.lock().unwrap();
        let entry = LogEntry {
            seq: entries.back().map_or(1, |e| e.seq + 1),
            timestamp: now_ms(),
            account_id: account_id.to_string(),
            account: account.to_string(),
            kind,
        };
        if entries.len() == CAPACITY {
            entries.pop_front();
        }
        entries.push_back(entry.clone());
        entry
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Stores the entry and pushes it to the settings UI.
fn add(app: &AppHandle, account_id: &str, account: &str, kind: LogKind) {
    let Some(buffer) = app.try_state::<LogBuffer>() else { return };
    let entry = buffer.push(account_id, account, kind);
    let _ = app.emit("log-entry", &entry);
}

pub fn mail(app: &AppHandle, account_id: &str, account: &str, mut entry: MailEntry) {
    if entry.kind == Some(RuleKind::Link) {
        entry.value = entry.value.as_deref().map(strip_link);
        for trace in &mut entry.traces {
            trace.value = trace.value.as_deref().map(strip_link);
        }
    }
    add(app, account_id, account, LogKind::Mail(entry));
}

pub fn event(app: &AppHandle, account_id: &str, account: &str, level: &'static str, message: impl Into<String>) {
    add(app, account_id, account, LogKind::Event(EventEntry { level, message: message.into() }));
}

/// Cuts a link after its path, dropping query and fragment (login tokens).
pub fn strip_link(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    url[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_kind(n: usize) -> LogKind {
        LogKind::Event(EventEntry { level: "info", message: n.to_string() })
    }

    #[test]
    fn test_ring_buffer_drops_oldest() {
        let buffer = LogBuffer::default();
        for n in 0..CAPACITY + 3 {
            buffer.push("id", "acc", event_kind(n));
        }
        let all = buffer.all();
        assert_eq!(all.len(), CAPACITY);
        assert_eq!(all[0].seq, 4);
        assert_eq!(all.last().unwrap().seq, CAPACITY as u64 + 3);
    }

    #[test]
    fn test_strip_link() {
        assert_eq!(strip_link("https://claude.ai/magic-link#abc:def"), "https://claude.ai/magic-link");
        assert_eq!(strip_link("https://x.de/login?token=1&a=2"), "https://x.de/login");
        assert_eq!(strip_link("https://x.de/a/b"), "https://x.de/a/b");
    }

    #[test]
    fn test_entry_serializes_flat() {
        let buffer = LogBuffer::default();
        let entry = buffer.push("id", "acc", event_kind(1));
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["type"], "event");
        assert_eq!(json["level"], "info");
        assert_eq!(json["account"], "acc");
    }
}
