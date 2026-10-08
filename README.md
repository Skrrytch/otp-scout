# OTP-Scout

*your one-time password helper*

**OTP-Scout watches your mailboxes and pops up one-time passwords and magic login links the moment they arrive — copy the code or open the link with one click, without opening your mail client.**

You log in somewhere, the site sends you a code by email — and instead of switching to your mail program, waiting, searching and copying, the code simply appears on your screen:

```
┌──────────────────────────────────┐
│  Gmail · GitHub               ✕  │
│                                  │
│            4 8 2 9 1 3           │
│                                  │
│  [   Copy   ] [Copy and destroy] │
└──────────────────────────────────┘
```

One click and it is in your clipboard. Paste it, done.

## What It Does for You

- **Codes appear instantly.** As soon as the email arrives, the code pops up in the middle of your screen — no refreshing, no waiting.
- **One click to copy.** Click the code (or press Enter) and paste it wherever you need it.
- **Login links, too.** Some services (like Claude) send a link instead of a code. OTP-Scout recognizes it and opens it in your browser with one click.
- **Keep your inbox clean.** *Copy and destroy* copies the code and deletes the email in one go.
- **All your mailboxes.** Gmail, Outlook, your company account, Proton Mail via the Bridge — as many as you like.
- **You decide what counts.** Tell OTP-Scout what the emails look like (sender, subject) and try your rules with a sample email right in the settings.
- **Out of your way.** It lives quietly in the system tray and only shows up when there is something for you.
- **Your passwords stay safe.** They are kept in your system's keyring, never in a plain text file.

## Install

For Ubuntu 22.04+, Linux Mint 21+ and Debian 12+:

1. Download the `.deb` file from the [latest release](https://github.com/Skrrytch/otp-scout/releases/latest).
2. Install it — double-click it, or in a terminal:
   ```bash
   sudo apt install ./otp-scout_*_amd64.deb
   ```
3. Start **OTP-Scout** from your application menu. A magnifying-glass icon appears in the system tray.

## Getting Started

1. Click the tray icon and choose **Settings**.
2. Click **+ Add account** and enter your mail server, username and password.
   - **Gmail:** server `imap.gmail.com`, and an [app password](https://myaccount.google.com/apppasswords) instead of your normal password.
   - **Proton Mail:** see [below](#proton-mail).
3. Save — the account card shows whether the connection works.
4. Send yourself a test: an email with the subject `123456 is your verification code` pops up right away.

## Rules: Telling OTP-Scout What to Look For

Out of the box, OTP-Scout recognizes emails whose subject reads like *"123456 is your verification code"*. For other services, add your own rules when editing an account.

### Code rules

Describe the email the way it arrives and mark where the code is with `{code}`:

| You enter | Matches emails like |
|---|---|
| Subject: `{code} is your code` | *482913 is your code* |
| Subject: `Your * login code: {code}` | *Your GitHub login code: 482913* |
| Sender: `noreply@github.com` | only emails from GitHub |

Use `*` for parts that change. Upper/lower case doesn't matter.

If the code is not in the subject but in the email text, leave `{code}` out of the subject and describe the text around the code instead, e.g. `Your verification code is {code}`.

### Link rules

For services that send a login link instead of a code: enter the sender and/or subject, and how the link starts, e.g. `https://claude.ai/magic-link`. Only links starting exactly like this are ever offered.

### Try before you save

Every rule has a **Test this rule** box: paste a sample subject or email text and see immediately whether the code or link is found.

## Proton Mail

OTP-Scout works with the [Proton Mail Bridge](https://proton.me/mail/bridge). In the account settings use:

- Server `127.0.0.1`, port `1143`
- Encryption **STARTTLS**
- **Accept self-signed certificate** ✔
- The username and the *bridge password* shown in the Bridge app

**Expect a delay of up to about 40 seconds.** The Bridge polls Proton only every 20–40 seconds, so a code arrives in your local mailbox that much later. OTP-Scout shows it as soon as the Bridge reports it. If you need codes faster, have the login mails of those services forwarded to a mailbox with real IMAP push (most providers) and add that account instead.

## Codes from Your Browser

Optionally, OTP-Scout can receive codes and links from browser scripts (e.g. Tampermonkey) and show them in the same popup. Turn it on in **Settings → API** — you'll find a ready-to-use script there. Details for script authors are in [DEVELOPMENT.md](DEVELOPMENT.md#local-http-api).

## Good to Know

- **Tray menu:** *Check now* checks all mailboxes immediately, *Settings*, *About*, *Quit*.
- **Connection problems** are shown on the account card and as a desktop notification; OTP-Scout reconnects automatically.
- **Several codes at once** are stacked in the popup; each one disappears after two minutes.
- **Your emails stay unread.** OTP-Scout only reads what it needs and doesn't mark anything as read.
- **Why wasn't a code detected?** The *Log* tab in the settings lists every checked mail and, per rule, whether sender, subject and body matched. From there you can jump to the rule or create a new one from the mail. The log stays in memory only.
- **Debug output:** start with `RUST_LOG=otp_scout=debug otp-scout` to see when the server reported a mail and how long headers and body took.
- **Uninstall:** `sudo apt remove otp-scout`. Your settings in `~/.config/otp-scout/` are kept.

## For Developers

Building from source, architecture and the API reference: see [DEVELOPMENT.md](DEVELOPMENT.md).

## License

[MIT](LICENSE) © 2026 Bert Speckels
