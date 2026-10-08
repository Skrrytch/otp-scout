use anyhow::{Context, Result};
use async_imap::extensions::idle::IdleResponse;
use async_imap::types::NameAttribute;
use futures_util::TryStreamExt;
use native_tls::TlsConnector;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tokio::net::TcpStream;
use tokio::sync::{mpsc::Sender, Notify};
use tokio_native_tls::TlsConnector as TokioTlsConnector;

use crate::config::{AccountConfig, Security};
use crate::rules::{self, CompiledRules, HeaderMatch, MailBody, RuleKind};
use crate::status;

/// Re-issue IDLE this often. Short enough to notice silently dropped
/// connections (NAT, VPN, suspend) quickly; the server must answer DONE.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Max time for the server to answer while leaving IDLE / checking.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

type Session = async_imap::Session<tokio_native_tls::TlsStream<TcpStream>>;

async fn connect(account: &AccountConfig) -> Result<Session> {
    tokio::time::timeout(CONNECT_TIMEOUT, connect_inner(account))
        .await
        .with_context(|| format!("Timeout connecting to {}", account.server))?
}

async fn connect_inner(account: &AccountConfig) -> Result<Session> {
    let addr = format!("{}:{}", account.server, account.port);
    let tcp = TcpStream::connect(&addr)
        .await
        .with_context(|| format!("Failed to connect to {addr}"))?;

    let tcp = match account.security {
        Security::Ssl => tcp,
        Security::Starttls => {
            let mut plain = async_imap::Client::new(tcp);
            plain
                .read_response()
                .await
                .context("No greeting from server")?
                .context("Connection closed before greeting")?;
            plain
                .run_command_and_check_ok("STARTTLS", None)
                .await
                .context("STARTTLS failed – try SSL/TLS instead")?;
            plain.into_inner()
        }
    };

    let tls = TokioTlsConnector::from(
        TlsConnector::builder()
            .danger_accept_invalid_certs(account.allow_invalid_certs)
            .danger_accept_invalid_hostnames(account.allow_invalid_certs)
            .build()?,
    );
    let tls_stream = tls.connect(&account.server, tcp).await.map_err(|e| {
        let msg = e.to_string();
        let hint = if msg.contains("wrong version number") && account.security == Security::Ssl {
            " – the server expects STARTTLS, change the encryption"
        } else if msg.contains("certificate") && !account.allow_invalid_certs {
            " – self-signed certificate? enable \"Accept self-signed certificate\""
        } else {
            ""
        };
        anyhow::anyhow!("TLS handshake failed: {msg}{hint}")
    })?;

    async_imap::Client::new(tls_stream)
        .login(&account.user, &account.pass)
        .await
        .map_err(|(e, _)| anyhow::anyhow!("Login failed for {}: {e}", account.label))
}

/// Deletes a message via a short-lived second connection (the main one is in IDLE).
/// Moves it to the server's \Trash folder if there is one, otherwise flags and expunges it.
pub async fn delete_message(account: &AccountConfig, uid: u32) -> Result<()> {
    let mut session = connect(account).await?;
    session
        .select(&account.mailbox)
        .await
        .with_context(|| format!("Failed to select mailbox '{}'", account.mailbox))?;

    let trash = {
        let names: Vec<_> = session.list(Some(""), Some("*")).await?.try_collect().await?;
        names
            .iter()
            .find(|n| n.attributes().iter().any(|a| matches!(a, NameAttribute::Trash)))
            .map(|n| n.name().to_string())
            .filter(|name| name != &account.mailbox)
    };

    match trash {
        Some(trash) => session
            .uid_mv(uid.to_string(), &trash)
            .await
            .with_context(|| format!("Failed to move message to '{trash}'"))?,
        None => {
            let _: Vec<_> = session
                .uid_store(uid.to_string(), "+FLAGS (\\Deleted)")
                .await?
                .try_collect()
                .await?;
            let _: Vec<_> = session.expunge().await?.try_collect().await?;
        }
    }
    session.logout().await.ok();
    Ok(())
}

pub async fn run_idle_loop(
    app: AppHandle,
    tx: Sender<String>,
    account: AccountConfig,
    check: Arc<Notify>,
    last_uid: Arc<AtomicU32>,
) -> Result<()> {
    let rules = if account.rules.is_empty() {
        CompiledRules::compile(&rules::default_rules())?
    } else {
        CompiledRules::compile(&account.rules)?
    };

    status::set(&app, &account.id, "connecting", format!("Connecting to {}…", account.server));

    let mut session = connect(&account).await?;

    let mailbox = session
        .select(&account.mailbox)
        .await
        .with_context(|| format!("Failed to select mailbox '{}'", account.mailbox))?;

    // Start after the newest existing mail. A UIDNEXT below the stored UID
    // means the server renumbered the mailbox (UIDVALIDITY change).
    if let Some(next) = mailbox.uid_next {
        let seen = last_uid.load(Ordering::Relaxed);
        if seen == 0 || next <= seen {
            last_uid.store(next.saturating_sub(1), Ordering::Relaxed);
        }
    }

    tracing::info!(
        "[{}] Connected to {} – watching {} for auth codes",
        account.label,
        account.server,
        account.mailbox
    );

    status::set(&app, &account.id, "ok", format!("Connected – watching {}", account.mailbox));


    loop {
        let mut handle = session.idle();
        handle.init().await.context("Failed to init IDLE")?;

        // A manual check (tray click) drops the StopSource, which ends IDLE
        // cleanly with `ManualInterrupt`.
        let idle_started = Instant::now();
        let response = {
            let (wait_fut, stop) = handle.wait_with_timeout(IDLE_TIMEOUT);
            tokio::pin!(wait_fut);
            tokio::select! {
                r = &mut wait_fut => r?,
                _ = check.notified() => {
                    drop(stop);
                    wait_fut.await?
                }
            }
        };
        match response {
            IdleResponse::NewData(_) => {
                tracing::debug!("[{}] IDLE woke up after {:.1?}", account.label, idle_started.elapsed())
            }
            IdleResponse::Timeout => tracing::debug!("[{}] IDLE timeout – re-issuing", account.label),
            IdleResponse::ManualInterrupt => tracing::info!("[{}] Manual check", account.label),
        }

        session = tokio::time::timeout(COMMAND_TIMEOUT, handle.done())
            .await
            .context("Server did not respond – connection lost")?
            .context("Failed to end IDLE")?;

        // All mails newer than the last one seen, read or not: another client
        // may already have marked a code mail as read.
        let check_started = Instant::now();
        let seen = last_uid.load(Ordering::Relaxed);
        let mut uids: Vec<u32> = tokio::time::timeout(COMMAND_TIMEOUT, session.uid_search(format!("UID {}:*", seen + 1)))
            .await
            .context("Server did not respond – connection lost")?
            .context("UID SEARCH failed")?
            .into_iter()
            // `n:*` always includes the newest mail, even if its UID is below n.
            .filter(|&uid| uid > seen)
            .collect();
        uids.sort_unstable();
        status::set(&app, &account.id, "ok", format!("Mailbox checked – {} new", uids.len()));

        for uid in uids {
            let header_fetches: Vec<_> = session
                .uid_fetch(uid.to_string(), "BODY.PEEK[HEADER]")
                .await?
                .try_collect()
                .await?;

            for fetch in header_fetches {
                let header_bytes = fetch.header().unwrap_or(&[]);
                let subject = parse_subject(header_bytes);
                let from = parse_from(header_bytes);
                tracing::debug!(
                    "[{}] UID {uid}: headers after {:.1?} – {:?} {:?}",
                    account.label,
                    check_started.elapsed(),
                    from,
                    subject
                );

                if !rules::sender_matches(&account.sender_filter, from.as_deref().unwrap_or("")) {
                    tracing::debug!("[{}] Skipping mail from {:?} – sender filter", account.label, from);
                    continue;
                }

                let subject_str = subject.as_deref().unwrap_or("");
                let from_str = from.as_deref().unwrap_or("");
                // Body is fetched lazily, at most once, and only for header matches.
                let mut body: Option<MailBody> = None;

                for rule in &rules.rules {
                    let code = match rule.match_headers(from_str, subject_str) {
                        None => continue,
                        Some(HeaderMatch::Code(code)) => Some(code),
                        Some(HeaderMatch::NeedsBody) => {
                            if body.is_none() {
                                let body_fetches: Vec<_> = session
                                    .uid_fetch(uid.to_string(), "BODY.PEEK[]")
                                    .await?
                                    .try_collect()
                                    .await?;
                                let raw = body_fetches.first().and_then(|f| f.body()).unwrap_or(&[]);
                                body = Some(parse_body(raw));
                                tracing::debug!("[{}] UID {uid}: body after {:.1?}", account.label, check_started.elapsed());
                            }
                            rule.match_body(body.as_ref().unwrap())
                        }
                    };
                    if let Some(code) = code {
                        let payload = build_payload(&code, rule.kind, &rule.label, &account, uid, &from, &subject);
                        tracing::info!(
                            "[{}] Auth code found: {code} (rule: {}) after {:.1?}",
                            account.label,
                            rule.label,
                            check_started.elapsed()
                        );
                        let _ = tx.send(payload).await;
                        break;
                    }
                }
            }

            last_uid.store(uid, Ordering::Relaxed);
        }
    }
}

fn build_payload(
    code: &str,
    kind: RuleKind,
    rule: &str,
    account: &AccountConfig,
    uid: u32,
    from: &Option<String>,
    subject: &Option<String>,
) -> String {
    serde_json::json!({
        "code": code,
        "kind": kind,
        "account": account.label,
        "account_id": account.id,
        "uid": uid,
        "rule": rule,
        "from": from,
        "subject": subject,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    })
    .to_string()
}

/// Decodes the subject (folded lines, RFC 2047 encoded words, any charset).
fn parse_subject(header_bytes: &[u8]) -> Option<String> {
    let msg = mail_parser::MessageParser::default().parse_headers(header_bytes)?;
    msg.subject().map(str::to_string)
}

fn parse_from(header_bytes: &[u8]) -> Option<String> {
    let msg = mail_parser::MessageParser::default().parse_headers(header_bytes)?;
    msg.from()?.first()?.address().map(str::to_string)
}

/// Decodes a full RFC 822 message (multipart, quoted-printable, base64, charsets).
fn parse_body(raw: &[u8]) -> MailBody {
    let Some(msg) = mail_parser::MessageParser::default().parse(raw) else {
        return MailBody { text: String::from_utf8_lossy(raw).into_owned(), html: String::new() };
    };
    let html = msg.body_html(0).map(|h| h.into_owned()).unwrap_or_default();
    let text = match msg.text_part(0) {
        Some(part) if !part.is_text_html() => msg.body_text(0).map(|t| t.into_owned()).unwrap_or_default(),
        _ => crate::html::strip_html(&html),
    };
    MailBody { text, html }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_headers_folded_utf8_subject() {
        let raw = b"From: Spotify <no-reply@alerts.spotify.com>\r\n\
Subject: =?UTF-8?Q?913088=C2=A0=E2=80=93_dein_Spotify?=\r\n =?UTF-8?Q?_Anmeldecode?=\r\n\r\n";
        let subject = parse_subject(raw).unwrap();
        assert_eq!(subject, "913088\u{a0}\u{2013} dein Spotify Anmeldecode");
        assert_eq!(parse_from(raw).as_deref(), Some("no-reply@alerts.spotify.com"));
    }

    #[test]
    fn test_parse_body_quoted_printable_multipart() {
        let raw = "From: no-reply@mail.anthropic.com\r\n\
Subject: Secure link to log in to Claude.ai\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
Your code: 123456 =E2=80=93 thanks\r\n\
--b1\r\n\
Content-Type: text/html; charset=utf-8\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
<a href=3D\"https://claude.ai/magic-link#abcdefghijklmnopqrstuvwxyz0123456789abcdefgh=\r\n\
ijklmnop\">Sign in</a>\r\n\
--b1--\r\n";
        let body = parse_body(raw.as_bytes());
        assert!(body.text.contains("Your code: 123456 – thanks"));
        assert!(body.html.contains("https://claude.ai/magic-link#abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnop\""));
    }

    /// Needs a running Proton Mail Bridge: `cargo test -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn test_starttls_proton_bridge() {
        let mut account = AccountConfig {
            label: "Bridge".into(),
            server: "127.0.0.1".into(),
            port: 1143,
            security: Security::Starttls,
            user: "nobody@example.com".into(),
            pass: "wrong".into(),
            ..Default::default()
        };
        let err = connect(&account).await.err().unwrap().to_string();
        assert!(err.contains("self-signed certificate?"), "{err}");
        account.security = Security::Ssl;
        let err = connect(&account).await.err().unwrap().to_string();
        assert!(err.contains("expects STARTTLS"), "{err}");
        account.security = Security::Starttls;
        account.allow_invalid_certs = true;
        let err = connect(&account).await.err().unwrap().to_string();
        assert!(err.contains("Login failed"), "{err}");
    }
}
