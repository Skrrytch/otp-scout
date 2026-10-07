# OTP-Scout

*your one-time password helper*

**OTP-Scout watches your IMAP mailboxes and pops up one-time passwords and magic login links the moment they arrive — copy the code or open the link with one click, without opening your mail client.**

A lightweight Linux system-tray app built with Rust and Tauri.

## How It Works

OTP-Scout sits in your system tray and keeps a connection to each of your IMAP accounts using [IDLE](https://tools.ietf.org/html/rfc2177) — the server pushes new mail instantly, nothing is polled. When a new email arrives, its sender and subject are checked against your rules. On a match, a small popup appears in the middle of your screen showing the code in large type (one click copies it) or the login link (one click opens it). The mail body is only downloaded when a rule needs it.

No more switching to your email client, waiting for sync, scrolling through threads, and squinting at 6-digit numbers.

## Features

- **Real-time IMAP IDLE** — push instead of polling, with automatic reconnect (exponential backoff) and detection of silently dropped connections
- **Multi-account support** — monitor several mailboxes at once, each with its own rules
- **Code rules** — match sender and subject like `{code} is your verification code`; the code is taken from the subject or from the body
- **Link rules** — detect login links such as magic links by their start (e.g. `https://claude.ai/magic-link`) and open them in the browser
- **Popup with actions** — copy / open with one click, optionally *… and destroy* to delete the mail right away; several messages stack up
- **Built-in rule tester** — try every rule with a sample mail directly in the settings dialog
- **Robust MIME parsing** — multipart, quoted-printable, base64, charsets and HTML mails
- **SSL/TLS and STARTTLS** — including self-signed certificates, e.g. for [Proton Mail Bridge](https://proton.me/mail/bridge)
- **Local HTTP API** — let browser scripts (Tampermonkey) show codes and links in the same popup, token-protected
- **Connection status** — every account shows the result of its last communication
- **System keyring** — passwords and the API token are stored in your OS keyring, never in plain text
- **Light and dark theme** — follows your desktop's color scheme

## System Requirements

- **Linux** with GTK 3, WebKit2GTK 4.1 and libayatana-appindicator3
- **Rust** 1.90 or later

### Install build dependencies (Debian / Ubuntu / Mint)

```bash
sudo apt install build-essential pkg-config \
  libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev \
  libssl-dev libdbus-1-dev
```

## Build & Run

```bash
git clone https://github.com/Skrrytch/otp-scout.git
cd otp-scout/src-tauri
cargo build --release
./target/release/otp-scout
```

The app starts in the system tray. Its menu offers **Check now**, **Settings**, **About** and **Quit**.

## Configuration

Everything is configured in the settings window (tray menu → **Settings**). It has two tabs:

- **Accounts** — your IMAP accounts, each with the tabs *Connection*, *Code rules* and *Link rules*
- **API** — the local HTTP API

The configuration is stored in `~/.config/otp-scout/config.json`; see [INTERNAL.md](INTERNAL.md) for the full schema.

### Code Rules

| Field | Example | Meaning |
|---|---|---|
| Sender (optional) | `noreply@github.com, *@bank.de` | Comma-separated globs, `*` = any text |
| Subject | `{code} is your code` | The subject as it arrives; `{code}` marks the code, `*` matches changing text; case-insensitive |
| Body | `Your code is {code}` | Only used if the subject contains no `{code}`. Empty = first code-like word in the body |
| Code format | Digits (4–8) | Digits, letters/digits, or a custom regex |

Without any rule, the built-in default `{code} is your verification code` is used.

### Link Rules

| Field | Example | Meaning |
|---|---|---|
| Sender (optional) | `*@mail.anthropic.com` | As above |
| Subject | `Secure link to log in*` | As above, without `{code}` |
| Link starts with | `https://claude.ai/magic-link` | The first link in the body with this prefix is offered for opening |

Rules are evaluated in order; the first match wins. A sender or a subject is required for every rule.

### Proton Mail Bridge

The bridge listens locally with STARTTLS and a self-signed certificate. Use server `127.0.0.1`, port `1143`, encryption **STARTTLS**, enable **Accept self-signed certificate**, and enter the bridge password shown in the Bridge app.

### Local API (Tampermonkey & co.)

In the **API** tab you can enable a local HTTP server (off by default, `127.0.0.1:6870`) that shows codes and links in the same popup.

| Endpoint | Body |
|---|---|
| `POST /api/otp/code` | `{"code": "123456", "ruleName": "GitHub"}` |
| `POST /api/otp/link` | `{"link": "https://…", "ruleName": "My site"}` |

- Every request needs the header `X-OTP-Scout-Token`. The token is shown in the API tab, stored in the keyring and can be regenerated.
- Links must use `https://`. If **Allowed links** is filled, they must start with one of its entries.
- CORS is open (`*`), but nothing is accepted without the token. In Tampermonkey, use `GM_xmlhttpRequest` with `@connect 127.0.0.1` so the visited page never sees the token. A ready-made script is shown in the API tab.
- Responses: `200 {"ok":true}`, `400` invalid body, `401` missing or wrong token, `403` link not allowed.

## Architecture

```
dist/                — Settings, popup and about pages (HTML/CSS/JS)
src-tauri/src/
├── main.rs          — App setup, tray menu, IPC commands, popup, reconnect loop
├── config.rs        — JSON config, keyring storage
├── rules.rs         — Rule model and matching (sender, subject, code, link)
├── imap.rs          — IMAP IDLE loop, SSL/STARTTLS, two-stage fetch, MIME parsing, delete
├── api.rs           — Local HTTP API
├── status.rs        — Per-account connection status
└── html.rs          — HTML-to-text conversion
```

| Component | Technology |
|---|---|
| Desktop framework | [Tauri v2](https://v2.tauri.app/) |
| Async runtime | [Tokio](https://tokio.rs/) |
| IMAP client | [async-imap](https://crates.io/crates/async-imap) |
| MIME parsing | [mail-parser](https://crates.io/crates/mail-parser) |
| TLS | native-tls (OpenSSL) |
| HTTP API | [axum](https://crates.io/crates/axum) |
| Password storage | [keyring](https://crates.io/crates/keyring) (Secret Service) |

## Development

```bash
cd src-tauri
cargo test                    # unit tests
cargo test -- --ignored       # + integration test against a local Proton Mail Bridge
cargo clippy --all-targets
```

## License

[MIT](LICENSE) © 2026 Bert Speckels
