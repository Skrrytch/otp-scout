//! Local HTTP API (127.0.0.1 only) to trigger the code/link popup from outside,
//! e.g. from a Tampermonkey script. Every request needs the `X-OTP-Scout-Token`
//! header; links must be https and, if configured, match an allowed prefix.

use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, State},
    http::{header, HeaderMap, HeaderName, Method, StatusCode},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager};
use tower_http::cors::{Any, CorsLayer};

use crate::config;
use crate::rules::RuleKind;

pub const TOKEN_HEADER: &str = "x-otp-scout-token";
const MAX_BODY: usize = 16 * 1024;
const MAX_CODE_LEN: usize = 64;
const MAX_LINK_LEN: usize = 2048;
const MAX_RULE_LEN: usize = 100;

#[derive(Debug, Clone, Serialize)]
pub struct ApiStatus {
    /// `off`, `listening` or `error`
    pub state: &'static str,
    pub message: String,
    /// Unix timestamp of the last accepted request
    pub last_request: Option<u64>,
}

/// Running server handle, shared via Tauri state.
pub struct ApiServer {
    token: Arc<RwLock<String>>,
    stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    status: Mutex<ApiStatus>,
}

impl Default for ApiServer {
    fn default() -> Self {
        Self {
            token: Arc::new(RwLock::new(String::new())),
            stop: Mutex::new(None),
            status: Mutex::new(ApiStatus { state: "off", message: "Disabled".into(), last_request: None }),
        }
    }
}

impl ApiServer {
    pub fn status(&self) -> ApiStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn set_token(&self, token: String) {
        *self.token.write().unwrap() = token;
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn set_status(app: &AppHandle, state: &'static str, message: impl Into<String>) {
    let server = app.state::<ApiServer>();
    let status = {
        let mut st = server.status.lock().unwrap();
        st.state = state;
        st.message = message.into();
        st.clone()
    };
    let _ = app.emit("api-status", &status);
}

/// (Re)starts the server according to the current config. Stops it if disabled.
pub async fn restart(app: AppHandle) {
    let server = app.state::<ApiServer>();
    if let Some(stop) = server.stop.lock().unwrap().take() {
        let _ = stop.send(());
    }

    let cfg = config::get_api_config_async(&app.state::<crate::AppState>().config).await;
    if !cfg.enabled {
        set_status(&app, "off", "Disabled");
        return;
    }
    match config::api_token() {
        Ok(token) => server.set_token(token),
        Err(e) => {
            set_status(&app, "error", format!("No API token: {e:#}"));
            return;
        }
    }

    let addr = format!("127.0.0.1:{}", cfg.port);
    // The old listener may need a moment to release the port.
    let mut listener = tokio::net::TcpListener::bind(&addr).await;
    if listener.is_err() {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        listener = tokio::net::TcpListener::bind(&addr).await;
    }
    let listener = match listener {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("API: cannot listen on {addr}: {e}");
            let msg = if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("Port {} is already in use", cfg.port)
            } else {
                format!("Cannot listen on {addr}: {e}")
            };
            set_status(&app, "error", msg);
            return;
        }
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    *server.stop.lock().unwrap() = Some(stop_tx);
    set_status(&app, "listening", format!("Listening on http://{addr}"));
    tracing::info!("API listening on {addr}");

    let router = router(Ctx { app: app.clone(), token: server.token.clone() });
    tauri::async_runtime::spawn(async move {
        let result = axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await;
        if let Err(e) = result {
            tracing::error!("API server failed: {e}");
        }
    });
}

#[derive(Clone)]
struct Ctx {
    app: AppHandle,
    token: Arc<RwLock<String>>,
}

fn router(ctx: Ctx) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, HeaderName::from_static(TOKEN_HEADER)]);
    Router::new()
        .route("/api/otp/code", post(post_code))
        .route("/api/otp/link", post(post_link))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(cors)
        .with_state(ctx)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodeRequest {
    code: String,
    #[serde(default)]
    rule_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkRequest {
    link: String,
    #[serde(default)]
    rule_name: String,
}

type Reply = (StatusCode, Json<serde_json::Value>);

fn fail(status: StatusCode, error: impl Into<String>) -> Reply {
    (status, Json(json!({ "ok": false, "error": error.into() })))
}

/// Constant-time comparison, so the token can't be guessed by timing.
fn token_matches(headers: &HeaderMap, expected: &str) -> bool {
    let Some(given) = headers.get(TOKEN_HEADER).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    !expected.is_empty()
        && given.len() == expected.len()
        && given.bytes().zip(expected.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn validate_code(code: &str) -> Result<(), String> {
    if code.is_empty() || code.len() > MAX_CODE_LEN {
        return Err(format!("'code' must be 1–{MAX_CODE_LEN} characters"));
    }
    if code.chars().any(char::is_control) {
        return Err("'code' contains control characters".into());
    }
    Ok(())
}

/// Only https links; if `prefixes` is non-empty, the link must start with one of them.
fn validate_link(link: &str, prefixes: &[String]) -> Result<(), (StatusCode, String)> {
    if link.is_empty() || link.len() > MAX_LINK_LEN || link.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err((StatusCode::BAD_REQUEST, format!("'link' must be a URL of 1–{MAX_LINK_LEN} characters")));
    }
    if !link.starts_with("https://") {
        return Err((StatusCode::BAD_REQUEST, "'link' must start with https://".into()));
    }
    let lower = link.to_lowercase();
    if !prefixes.is_empty() && !prefixes.iter().any(|p| lower.starts_with(p)) {
        return Err((StatusCode::FORBIDDEN, "Link is not in the API's allowed links".into()));
    }
    Ok(())
}

fn rule_label(rule_name: &str) -> String {
    let name: String = rule_name.trim().chars().filter(|c| !c.is_control()).take(MAX_RULE_LEN).collect();
    if name.is_empty() { "API".into() } else { name }
}

fn authorize(ctx: &Ctx, headers: &HeaderMap) -> Result<(), Reply> {
    if token_matches(headers, &ctx.token.read().unwrap()) {
        Ok(())
    } else {
        Err(fail(StatusCode::UNAUTHORIZED, format!("Missing or wrong '{TOKEN_HEADER}' header")))
    }
}

fn deliver(ctx: &Ctx, kind: RuleKind, value: &str, rule: &str) -> Reply {
    let payload = json!({
        "code": value,
        "kind": kind,
        "account": "Browser",
        "rule": rule,
        "from": "via OTP-Scout API",
        "subject": null,
        "timestamp": now(),
    })
    .to_string();
    tracing::info!("API: received {kind:?} (rule: {rule})");
    let _ = ctx.app.emit("auth-code", &payload);
    crate::show_code_popup(&ctx.app, payload);
    let server = ctx.app.state::<ApiServer>();
    let status = {
        let mut st = server.status.lock().unwrap();
        st.last_request = Some(now());
        st.clone()
    };
    let _ = ctx.app.emit("api-status", &status);
    (StatusCode::OK, Json(json!({ "ok": true })))
}

async fn post_code(State(ctx): State<Ctx>, headers: HeaderMap, body: Result<Json<CodeRequest>, JsonRejection>) -> Reply {
    if let Err(r) = authorize(&ctx, &headers) {
        return r;
    }
    let Json(req) = match body {
        Ok(b) => b,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e.body_text()),
    };
    let code = req.code.trim();
    if let Err(e) = validate_code(code) {
        return fail(StatusCode::BAD_REQUEST, e);
    }
    deliver(&ctx, RuleKind::Code, code, &rule_label(&req.rule_name))
}

async fn post_link(State(ctx): State<Ctx>, headers: HeaderMap, body: Result<Json<LinkRequest>, JsonRejection>) -> Reply {
    if let Err(r) = authorize(&ctx, &headers) {
        return r;
    }
    let Json(req) = match body {
        Ok(b) => b,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e.body_text()),
    };
    let link = req.link.trim();
    let api_cfg = config::get_api_config_async(&ctx.app.state::<crate::AppState>().config).await;
    let prefixes = allowed_prefixes(&api_cfg);
    if let Err((status, e)) = validate_link(link, &prefixes) {
        return fail(status, e);
    }
    deliver(&ctx, RuleKind::Link, link, &rule_label(&req.rule_name))
}

fn allowed_prefixes(api: &config::ApiConfig) -> Vec<String> {
    api.allowed_links
        .iter()
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_matches() {
        let mut h = HeaderMap::new();
        assert!(!token_matches(&h, "secret"));
        h.insert(TOKEN_HEADER, "secret".parse().unwrap());
        assert!(token_matches(&h, "secret"));
        assert!(!token_matches(&h, "secreT"));
        assert!(!token_matches(&h, "secret2"));
        assert!(!token_matches(&h, ""));
    }

    #[test]
    fn test_validate_code() {
        assert!(validate_code("123456").is_ok());
        assert!(validate_code("").is_err());
        assert!(validate_code(&"1".repeat(65)).is_err());
        assert!(validate_code("12\n34").is_err());
    }

    #[test]
    fn test_validate_link() {
        let p = vec!["https://claude.ai/magic-link".to_string()];
        assert!(validate_link("https://claude.ai/magic-link#abc", &p).is_ok());
        assert!(validate_link("https://Claude.ai/magic-link#abc", &p).is_ok());
        assert_eq!(validate_link("https://evil.example/claude.ai/magic-link", &p).unwrap_err().0, StatusCode::FORBIDDEN);
        assert_eq!(validate_link("http://claude.ai/magic-link", &p).unwrap_err().0, StatusCode::BAD_REQUEST);
        assert_eq!(validate_link("javascript:alert(1)", &p).unwrap_err().0, StatusCode::BAD_REQUEST);
        assert_eq!(validate_link("https://claude.ai/magic-link#a b", &p).unwrap_err().0, StatusCode::BAD_REQUEST);
        assert!(validate_link("https://any.example/login", &[]).is_ok());
        assert_eq!(validate_link("http://any.example/login", &[]).unwrap_err().0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_rule_label() {
        assert_eq!(rule_label("  GitHub "), "GitHub");
        assert_eq!(rule_label(""), "API");
    }
}
