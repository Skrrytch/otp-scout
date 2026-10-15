# Codes from Your Browser: the Local API

Some codes never arrive by mail. A site may show a one-time code on another page, or a browser script may read it from a web app. OTP-Scout can receive such codes and links over a small local HTTP API and show them in the same popup as mail codes.

The API is **off by default**. It listens on `127.0.0.1` only, so other computers can't reach it, and every request needs a secret token.

## Setup

1. Open **Settings → API** and turn on **Enable API server**. The default port is `6870`.
2. **Save & apply.** The status dot next to *API* turns green.
3. Copy the **Tampermonkey example** at the bottom of the page. It already contains your port and token.

| Setting | Meaning |
|---|---|
| **Port** | Local port, 1024–65535. |
| **Token** | Sent by every request in the header `X-OTP-Scout-Token`. It's stored in the system keyring. **Regenerate** invalidates the old token, so scripts using it must be updated. |
| **Allowed links** | Prefixes for `/api/otp/link`, e.g. `https://claude.ai/magic-link`. If the list is set, only links starting with one of them are shown. Empty = any `https://` link is accepted. |

Codes from the API appear with the account *Browser*. Since they don't come from a mail, the popup offers no *… and destroy* button.

## Endpoints

| Endpoint | Body |
|---|---|
| `POST /api/otp/code` | `{"code": "123456", "ruleName": "GitHub"}` |
| `POST /api/otp/link` | `{"link": "https://…", "ruleName": "My site"}` |

- `ruleName` is optional and appears in the popup header; without it the popup shows *API*.
- Codes are 1–64 characters, links up to 2048 characters, rule names up to 100 characters, request bodies up to 16 KB.
- Links must use `https://`. If *Allowed links* is set, they must start with one of its entries.
- The same code sent twice refreshes the existing popup entry instead of adding a second one.

**Responses:**

| Status | Body | Meaning |
|---|---|---|
| `200` | `{"ok": true}` | Shown in the popup. |
| `400` | `{"ok": false, "error": "…"}` | Invalid body, e.g. an empty code or a link that doesn't start with `https://`. |
| `401` | `{"ok": false, "error": "…"}` | Header `X-OTP-Scout-Token` missing or wrong. |
| `403` | `{"ok": false, "error": "…"}` | Link doesn't start with one of the *Allowed links*. |

### curl

```bash
curl -X POST http://127.0.0.1:6870/api/otp/code \
  -H 'Content-Type: application/json' \
  -H 'X-OTP-Scout-Token: YOUR_TOKEN' \
  -d '{"code": "123456", "ruleName": "Test"}'
```

### Tampermonkey

Use `GM_xmlhttpRequest` with `@connect 127.0.0.1`. The request is then sent by the extension, and the visited page never sees your token.

```js
// ==UserScript==
// @name         OTP-Scout bridge
// @match        https://example.com/*
// @grant        GM_xmlhttpRequest
// @connect      127.0.0.1
// ==/UserScript==

function otpScout(kind, value, ruleName) {
  GM_xmlhttpRequest({
    method: 'POST',
    url: 'http://127.0.0.1:6870/api/otp/' + kind,
    headers: { 'Content-Type': 'application/json', 'X-OTP-Scout-Token': 'YOUR_TOKEN' },
    data: JSON.stringify(kind === 'code' ? { code: value, ruleName } : { link: value, ruleName }),
    onload: r => console.log('OTP-Scout:', r.status, r.responseText),
  })
}

// otpScout('code', '123456', 'My site')
// otpScout('link', 'https://example.com/login?token=…', 'My site')
```

## Security

- The server binds to `127.0.0.1` only.
- The token is compared in constant time, so it can't be guessed by measuring response times.
- CORS is open (`*`) so browser scripts can call the API, but nothing is accepted without the token. Don't put the token into scripts that run in the page itself. Use `GM_xmlhttpRequest` as shown above.
- Links are never opened automatically. The popup only offers them, and *Allowed links* restricts which ones may appear at all.
