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

## Example: Outlook on the Web

Microsoft accounts (Outlook.com, Microsoft 365) [can't be added over IMAP](accounts.md#microsoft-outlookcom-microsoft-365). If you read that mail in the browser anyway, a Tampermonkey script can watch the open Outlook tab instead. Outlook adds a new mail to the message list as soon as it arrives, with sender, subject and the start of the text. The script searches the visible text of the page for the mails described in its rules. When it finds one, it sends the code to OTP-Scout and marks the entry in Outlook.

This script is in daily use with Atlassian verification mails in Microsoft 365.

```js
// ==UserScript==
// @name         OTP-Scout for Outlook on the web
// @description  Sends codes and links from new mails in Outlook on the web to OTP-Scout
// @match        https://outlook.cloud.microsoft/mail/*
// @match        https://outlook.office.com/mail/*
// @match        https://outlook.office365.com/mail/*
// @match        https://outlook.live.com/mail/*
// @grant        GM_xmlhttpRequest
// @connect      127.0.0.1
// ==/UserScript==

(function () {
  'use strict'

  const PORT = 6870
  const TOKEN = 'YOUR_TOKEN' // from OTP-Scout: Settings → API

  // One entry per service. `indicator` identifies the mail; `extract` takes
  // the code or link from the text, its first group (…) is the value.
  // `type` is 'code' or 'link'.
  const RULES = [
    {
      ruleName: 'Atlassian',
      type: 'code',
      indicator: 'is your verification code',
      extract: /\b(\d+)\s+is your verification code/i,
    },
  ]

  // Codes already visible while Outlook loads are old and not reported.
  const WARMUP_MS = 5000
  const startedAt = Date.now()
  const processed = new Set()

  function checkMails() {
    const reporting = Date.now() - startedAt > WARMUP_MS
    // Innermost elements first, so the mark lands on the entry, not on a container.
    const nodes = [...document.querySelectorAll('span, div')].reverse()
    for (const node of nodes) {
      const text = node.innerText || node.textContent || ''
      for (const rule of RULES) {
        if (!text.includes(rule.indicator)) continue
        const value = text.match(rule.extract)?.[1]
        if (!value || processed.has(value)) continue
        processed.add(value)
        if (!reporting) continue
        console.log(`[OTP-Scout] ${rule.ruleName}: ${rule.type} found`)
        send(rule, value)
        // Mark it in Outlook: codes green, links blue.
        node.style.backgroundColor = rule.type === 'code' ? 'lightgreen' : 'lightblue'
        node.style.color = 'black'
      }
    }
  }

  function send(rule, value) {
    GM_xmlhttpRequest({
      method: 'POST',
      url: `http://127.0.0.1:${PORT}/api/otp/${rule.type}`,
      headers: { 'Content-Type': 'application/json', 'X-OTP-Scout-Token': TOKEN },
      // { ruleName, code } or { ruleName, link }
      data: JSON.stringify({ ruleName: rule.ruleName, [rule.type]: value }),
      onload: r => console.log(`[OTP-Scout] Sent, answer ${r.status}: ${r.responseText}`),
      onerror: e => console.error(`[OTP-Scout] Not sent – is OTP-Scout running with the API on port ${PORT}?`, e),
    })
  }

  // Check whenever Outlook changes the page. Unlike a timer, this isn't
  // slowed down in a background tab.
  let pending = null
  new MutationObserver(() => {
    clearTimeout(pending)
    pending = setTimeout(checkMails, 500)
  }).observe(document.body, { childList: true, subtree: true, characterData: true })
  console.log('[OTP-Scout] Watching Outlook for configured mails…')
})()
```

**Setup:**

1. Install [Tampermonkey](https://www.tampermonkey.net/) and add the script.
2. Replace `YOUR_TOKEN` with the token from **Settings → API**, and adjust `PORT` if you changed it.
3. Adjust `RULES` to your mails. `indicator` should be specific enough to hit only login mails. `extract` is a regular expression whose first group is the code.
4. Keep Outlook open in a tab, e.g. pinned. Request a code and watch for the popup. The browser console (F12) shows `[OTP-Scout]` messages.

**Limits:**

- **Mainly for codes.** The script sees the text of the page: the message list with subject and start of the text, and the opened mail. A link is only found if its address appears as text. Mails with a *Log in* button hide it.
- **The tab must stay alive.** Chrome and Edge may put background tabs to sleep to save memory. Then Outlook stops updating, and the script sees nothing until you switch back. Add Outlook to the exceptions: in Chrome under *Settings → Performance → Always keep these sites active*, in Edge under *Settings → System and performance → Never put these sites to sleep*.
- **Each value is reported once** per page load. Codes visible in the first five seconds after loading are treated as old and ignored.
- **Outlook's page structure isn't an interface.** Microsoft can change it at any time. Since the script only searches text, it doesn't depend on particular elements, but it does depend on the text the mail shows.

## Security

- The server binds to `127.0.0.1` only.
- The token is compared in constant time, so it can't be guessed by measuring response times.
- CORS is open (`*`) so browser scripts can call the API, but nothing is accepted without the token. Don't put the token into scripts that run in the page itself. Use `GM_xmlhttpRequest` as shown above.
- Links are never opened automatically. The popup only offers them, and *Allowed links* restricts which ones may appear at all.
