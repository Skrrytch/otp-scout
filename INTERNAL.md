# INTERNAL.md — OTP-Scout File Formats & System Artifacts

This document describes every file and system artifact that OTP-Scout creates or reads on the user's machine.

---

## 1. `~/.config/otp-scout/config.json`

> Early development versions used `~/.config/authscout/config.json`; it is copied to the new location on first start if no new config exists yet.

**Purpose**: Persistent configuration for all IMAP accounts and the API. Detection rules live in [`rules.json`](#2-configotp-scoutrulesjson).

**Created by**: OTP-Scout on first account save.

**Format**: JSON

### Schema

```jsonc
{
  "accounts": [
    {
      // Unique identifier (UUID v4), auto-generated
      "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",

      // Display name shown in the UI
      "label": "My Gmail",

      // IMAP server hostname
      "server": "imap.gmail.com",

      // IMAP port (usually 993 for TLS)
      "port": 993,

      // IMAP username (usually the email address)
      "user": "user@gmail.com",

      // Mailbox to watch (default: "INBOX")
      "mailbox": "INBOX",

      // Glob patterns for sender filtering.
      // Empty array = accept all senders.
      // "*" matches any substring, case-insensitive.
      "sender_filter": ["*@example.com", "noreply@github.com"],

      // Assigns rules to this account (see rules.json). Derived from the
      // label on creation, unchanged when the account is renamed.
      "tag": "my-gmail"
    }
  ]
}
```

### API section

```jsonc
"api": {
  "enabled": false,        // local HTTP API on/off
  "port": 6870,            // bound to 127.0.0.1 only
  "allowed_links": [       // prefixes for /api/otp/link; empty = any https link
    "https://claude.ai/magic-link"
  ]
}
```

The API token is stored in the keyring (service `otp-scout`, username `api-token`).

### Field Reference

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `accounts` | array | yes | `[]` | List of IMAP account configurations |
| `accounts[].id` | string (UUID) | yes | auto | Unique account identifier |
| `accounts[].label` | string | no | `""` | Display name in UI |
| `accounts[].server` | string | yes | `""` | IMAP server hostname |
| `accounts[].port` | u16 | no | `993` | IMAP port |
| `accounts[].security` | enum | no | `"ssl"` | `"ssl"` (implicit TLS) or `"starttls"` |
| `accounts[].allow_invalid_certs` | bool | no | `false` | Accept self-signed certificates (e.g. Proton Mail Bridge) |
| `accounts[].user` | string | yes | `""` | IMAP login username |
| `accounts[].mailbox` | string | no | `"INBOX"` | Mailbox to monitor |
| `accounts[].sender_filter` | string[] | no | `[]` | Glob patterns for sender whitelist |
| `accounts[].tag` | string | no | auto | Unique tag assigning rules to this account |

Versions up to 0.2.0 kept the rules in `accounts[].rules`. On first start they are moved to `rules.json`, each tagged with its account; identical rules are merged. The old file is kept as `config.json.bak`.

---

## 2. `~/.config/otp-scout/rules.json`

**Purpose**: All detection rules, independent of accounts.

**Created by**: OTP-Scout on first account save, or when migrating an older `config.json`. An unreadable file is renamed to `rules.json.broken`.

### Schema

```jsonc
{
  "version": 1,
  "rules": [
    {
      // Stable identifier (UUID v4)
      "id": "2f6c1e0a-8a52-4f0e-9d3b-6c1f0e2a7b11",

      // Accounts checking this rule: account tags, or "*" for all accounts.
      // Empty = no account (the rule is not checked).
      "tags": ["my-gmail"],

      // Optional: origin in the catalog
      "catalog": { "app": "spotify", "lang": "de" },

      // Human-readable name for this rule
      "label": "Default",

      // "code" (extract a code) or "link" (offer a login link). Default "code".
      "kind": "code",

      // Optional comma-separated sender globs. "" = any sender.
      "sender": "",

      // Subject pattern. {code} marks the code, * matches any text,
      // whitespace matches any whitespace, case-insensitive.
      // Without {code} the code is read from the body. "" = any subject.
      "subject": "{code} is your verification code",

      // Body pattern (only used if subject has no {code}). Must contain {code}.
      // "" = first token in the body matching code_pattern.
      "body": "",

      // Regex describing what a code looks like.
      "code_pattern": "[A-Za-z0-9-]{4,12}",

      // Whether this rule is active
      "enabled": true
    },
    {
      // Link mode: the first body link starting with link_prefix is offered for opening.
      "id": "9a0d4c55-1f3e-4b7a-8c2d-5e6f7a8b9c0d",
      "tags": ["*"],
      "label": "Claude",
      "kind": "link",
      "sender": "*@mail.anthropic.com",
      "subject": "Secure link to log in*",
      "link_prefix": "https://claude.ai/magic-link",
      "enabled": true
    }
  ]
}
```

An account that checks no rule uses the built-in default rule.

The account dialog still edits the rules of one account: changing a shared rule changes it for all its accounts, removing it there only detaches it from this account. A deleted account's tag is removed from all rules; rules left without tags stay in the file.

### Field Reference

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `version` | u32 | yes | `1` | File format version |
| `rules[].id` | string (UUID) | yes | auto | Stable rule identifier |
| `rules[].tags` | string[] | no | `[]` | Account tags or `*` |
| `rules[].catalog` | object | no | – | `{ "app", "lang" }` of the catalog entry |
| `rules[].label` | string | no | `""` | Rule name |
| `rules[].sender` | string | no | `""` | Comma-separated sender globs |
| `rules[].subject` | string | * | `""` | Subject pattern with optional `{code}` |
| `rules[].body` | string | no | `""` | Body pattern with `{code}` |
| `rules[].kind` | enum | no | `"code"` | `"code"` or `"link"` |
| `rules[].link_prefix` | string | link | `""` | Link mode: required URL prefix (`http(s)://…`) |
| `rules[].code_pattern` | string | no | `[A-Za-z0-9-]{4,12}` | Regex for the code itself |

\* `sender` or `subject` must be set. Legacy rules (`search_field`/`context_pattern`/`capture_pattern`) are converted on load.
| `rules[].enabled` | bool | no | `true` | Whether the rule is active |

### Sender Filter Glob Syntax

| Pattern | Matches |
|---------|---------|
| `*@example.com` | Any address at example.com |
| `noreply@*` | Any address starting with `noreply@` |
| `*@*` or `*` | Everything |
| `user@domain.com` | Exact match only |

Matching is case-insensitive.

### Detection Rule Evaluation

New emails are all emails with a UID above the last one handled, read or not; on connect the newest existing email marks the start. For each new email only the header is fetched first, then:

1. **Account sender filter**: if `sender_filter` is non-empty and `From` matches none of its globs, the email is skipped.
2. The rules carrying the account's tag or `*` are evaluated in order; the first rule that yields a code wins. Per rule:
   - `sender` (if set) must match `From`, and `subject` (if set) must match the subject.
   - If `subject` contains `{code}`, the code is taken from the subject.
   - Otherwise the body is fetched (`BODY.PEEK[]`, at most once per email, mail stays unread) and decoded with `mail-parser` (multipart, quoted-printable, base64, charsets).
   - Code rules apply `body` to the plain text (HTML converted if there is no text part).
   - Link rules search the HTML (`href` targets) and the text for the first `http(s)://` URL starting with `link_prefix` (case-insensitive).

---

## 3. System Keyring

**Purpose**: Secure storage of IMAP passwords and the API token.

**Service name**: `otp-scout` (legacy `authscout` entries are copied on first use)

**Entries**:

| Key (username) | Value |
|---|---|
| account `id` (UUID from config.json) | IMAP password |
| `api-token` | Token for the local HTTP API (created on first use) |

**Backend**: The freedesktop Secret Service via D-Bus (`keyring` crate, feature `sync-secret-service`), e.g. GNOME Keyring or KWallet (KDE). A running Secret Service provider is required.

### Manual Inspection

```bash
# List otp-scout entries (Secret Service / gnome-keyring)
secret-tool search service otp-scout

# Look up a specific password
secret-tool lookup service otp-scout <account-id>
```

### Lifecycle

- **Created**: When an account is saved with a non-empty password
- **Updated**: When an account is edited with a new non-empty password
- **Deleted**: When an account is removed from OTP-Scout
- **Read**: On app startup and on "Reconnect All"

Passwords are **never** written to `config.json`. The `pass` field is marked `#[serde(skip_serializing)]`: it is accepted from the settings UI, stored in the keyring and otherwise exists only in memory.

---

## 4. `$XDG_RUNTIME_DIR/tray-icon/` (or `/tmp/tray-icon/`)

**Purpose**: Temporary PNG files for the system tray icon.

**Created by**: The `tray-icon` Rust crate (dependency of Tauri).

**Files**: `tray-icon-<id>-<counter>.png`

**Lifecycle**: Created on app start, deleted on app exit. Old files from crashed sessions may accumulate — they are safe to delete.

**Note**: This directory is managed entirely by the `tray-icon` crate. OTP-Scout does not interact with it directly.

---

## 5. Files NOT Created (v0.1.0)

The following are explicitly **not** created by OTP-Scout in the current version:

- **No log files** — logging goes to stdout/stderr only; the Log tab keeps the last 500 entries in memory
- **No code history** — detected codes are not persisted to disk
- **No cache directory** — no `~/.cache/otp-scout/`
- **No autostart entry** — no `~/.config/autostart/otp-scout.desktop`
- **No lock file or PID file**

---

## Summary

| Path | Type | Contains |
|------|------|----------|
| `~/.config/otp-scout/config.json` | JSON | Accounts, sender filters, API settings |
| `~/.config/otp-scout/rules.json` | JSON | Detection rules and their account tags |
| System keyring (`otp-scout`) | Encrypted | IMAP passwords (one per account ID), API token |
| `$XDG_RUNTIME_DIR/tray-icon/` | PNG files | Temporary tray icons (auto-managed) |