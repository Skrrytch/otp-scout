# OTP-Scout — Development

Technical notes for building, packaging and extending OTP-Scout. For the user guide see [README.md](README.md); for files and keyring entries the app creates see [INTERNAL.md](INTERNAL.md).

## How It Works

- One IMAP connection per account in **IDLE** mode ([RFC 2177](https://tools.ietf.org/html/rfc2177)): the server pushes new mail, nothing is polled. IDLE is re-issued every 5 minutes; commands time out after 30 s, so silently dropped connections are noticed. Failed connections reconnect with exponential backoff (5 s … 5 min).
- On new mail only the **headers** of messages newer than the last handled UID are fetched, read or not (`BODY.PEEK[HEADER]`). Rules match sender and subject first; the full message (`BODY.PEEK[]`) is fetched only if a rule needs the body. Nothing is marked as read.
- Bodies are decoded with `mail-parser` (multipart, quoted-printable, base64, charsets). Code rules search the plain text, link rules the HTML `href`s and the text.
- *… and destroy* deletes via a short second connection: the message is moved to the server's `\Trash` folder if there is one, otherwise flagged `\Deleted` and expunged.

## Building from Source

Requirements: Linux with GTK 3, WebKit2GTK 4.1 and libayatana-appindicator3; Rust 1.90+.

```bash
sudo apt install build-essential pkg-config \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libssl-dev libdbus-1-dev

git clone https://github.com/Skrrytch/otp-scout.git
cd otp-scout/src-tauri
cargo build --release
./target/release/otp-scout
```

### Tests

```bash
cargo test                    # unit tests
cargo test -- --ignored       # + integration test against a local Proton Mail Bridge
cargo clippy --all-targets
```

## Packaging & Releases

```bash
cargo install tauri-cli --version "^2" --locked
cargo tauri build --bundles deb      # → target/release/bundle/deb/
```

A package built locally only runs on systems with the same or a newer glibc. Official releases are therefore built by GitHub Actions on Ubuntu 22.04 ([`.github/workflows/release.yml`](.github/workflows/release.yml)):

1. Bump `version` in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`.
2. Commit, then tag and push: `git tag v0.2.0 && git push origin v0.2.0`
3. The workflow tests, builds the `.deb`, and publishes a GitHub release with the package and `SHA256SUMS`.

## Architecture

```
dist/                — Settings, popup and about pages (HTML/CSS/JS, no build step)
src-tauri/src/
├── main.rs          — App setup, tray menu, IPC commands, popup, reconnect loop
├── config.rs        — JSON config, keyring storage, legacy migration
├── rule_store.rs    — rules.json, account tags, migration from config.json
├── rules.rs         — Rule model and matching (sender, subject, code, link)
├── imap.rs          — IMAP IDLE loop, SSL/STARTTLS, two-stage fetch, MIME parsing, delete
├── api.rs           — Local HTTP API
├── status.rs        — Per-account connection status
├── log.rs           — In-memory log of checked mails and connection events (Log tab)
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
| Secrets | [keyring](https://crates.io/crates/keyring) (Secret Service) |

## Rule Matching

| Field | Meaning |
|---|---|
| `sender` | Comma-separated globs (`*` = any text), case-insensitive. Empty = any sender |
| `subject` | Pattern: literal text, `*` = any text, whitespace = any whitespace, `{code}` = the code. Case-insensitive, matched anywhere in the subject |
| `body` | Code rules only, if the subject has no `{code}`: pattern containing `{code}`. Empty = first token matching `code_pattern` |
| `code_pattern` | Regex for the code itself (default `[A-Za-z0-9-]{4,12}`) |
| `link_prefix` | Link rules only: the first `http(s)://` URL starting with this (case-insensitive) is offered |

A rule needs a sender or a subject. Each account checks the rules tagged with its tag or `*`, in order; the first match wins. The full config schema is in [INTERNAL.md](INTERNAL.md).

## Local HTTP API

Enabled in **Settings → API** (off by default). Listens on `127.0.0.1` only, default port `6870`.

| Endpoint | Body |
|---|---|
| `POST /api/otp/code` | `{"code": "123456", "ruleName": "GitHub"}` |
| `POST /api/otp/link` | `{"link": "https://…", "ruleName": "My site"}` |

- Every request needs the header `X-OTP-Scout-Token` (shown in the API tab, stored in the keyring, regenerable). It is compared in constant time.
- Links must use `https://`. If *Allowed links* is set, they must start with one of its entries.
- Limits: code ≤ 64 characters, link ≤ 2048, body ≤ 16 KB.
- CORS is open (`*`), but nothing is accepted without the token. In Tampermonkey use `GM_xmlhttpRequest` with `@connect 127.0.0.1`, so the visited page never sees the token.
- Responses: `200 {"ok":true}`, `400` invalid body, `401` missing/wrong token, `403` link not allowed.

```js
// ==UserScript==
// @grant   GM_xmlhttpRequest
// @connect 127.0.0.1
// ==/UserScript==
GM_xmlhttpRequest({
  method: 'POST',
  url: 'http://127.0.0.1:6870/api/otp/code',
  headers: { 'Content-Type': 'application/json', 'X-OTP-Scout-Token': 'YOUR_TOKEN' },
  data: JSON.stringify({ code: '123456', ruleName: 'My site' }),
})
```
