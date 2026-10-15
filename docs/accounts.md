# Setting Up Accounts

OTP-Scout reads your mail over **IMAP**, the standard protocol nearly every mail provider offers. Each account keeps one connection open and waits for the server to report new mail (IMAP IDLE). Codes therefore appear within a second or two, without polling.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/accounts-dark.png">
  <img src="images/accounts-light.png" width="600" alt="Account list with three connected mailboxes and their status">
</picture>

## Adding an Account

Open **Settings → Accounts → + Add account**:

| Field | What to enter |
|---|---|
| **Display name** | Shown in the popup and the account list, e.g. *Gmail* or *Work*. |
| **Mailbox** | The folder to watch, usually `INBOX`. |
| **IMAP server**, **Port** | From your provider, see the [table below](#common-providers). |
| **Encryption** | *SSL/TLS* (port 993) for almost every provider. *STARTTLS* (port 143) is mainly needed for the Proton Mail Bridge. |
| **Accept self-signed certificate** | Only for local servers such as the Proton Mail Bridge. Leave it off otherwise. |
| **Username** | Usually your full email address. |
| **Password** | Your mail password or an *app password* (see below). It's stored in the system keyring. |

**Test connection** logs in and opens the mailbox before you save, and reports how many mails it contains. After saving, the account card shows the connection status.

### Tag

Every account gets a **tag** derived from its name, e.g. `gmail`. Rules use it to say which accounts check them, see [Rules](rules.md#which-accounts-check-a-rule). The tag stays the same when you rename the account. If you change the tag itself under **Tag → Rename**, all rules follow.

### Watching more than one folder

An account watches exactly one folder. If your provider or a server-side filter moves login mails to another folder, add the same mailbox a second time with that folder under **Mailbox**.

## Common Providers

Many providers don't allow IMAP with your normal password. They require an **app password**: a separate password you create in the provider's security settings, usually only after turning on two-factor authentication.

| Provider | Server | Port / encryption | Password |
|---|---|---|---|
| Gmail / Google Workspace | `imap.gmail.com` | 993 SSL/TLS | [App password](https://myaccount.google.com/apppasswords) |
| iCloud Mail | `imap.mail.me.com` | 993 SSL/TLS | [App-specific password](https://support.apple.com/en-us/102654); username without `@icloud.com` |
| Yahoo Mail | `imap.mail.yahoo.com` | 993 SSL/TLS | App password |
| GMX | `imap.gmx.net` | 993 SSL/TLS | Normal password; first enable *POP3/IMAP access* in the GMX settings |
| WEB.DE | `imap.web.de` | 993 SSL/TLS | Normal password; first enable *POP3/IMAP access* in the WEB.DE settings |
| Posteo | `posteo.de` | 993 SSL/TLS | Normal password |
| mailbox.org | `imap.mailbox.org` | 993 SSL/TLS | Normal password |
| Proton Mail | `127.0.0.1` | 1143 STARTTLS | Bridge password, see [below](#proton-mail) |
| Outlook.com, Microsoft 365 | – | – | Not via IMAP; see the [workarounds below](#microsoft-outlookcom-microsoft-365) |

Providers change their settings now and then. If a connection fails, check your provider's help page for the current IMAP settings.

### Proton Mail

Proton Mail encrypts your mail end to end and has no direct IMAP access. Install the [Proton Mail Bridge](https://proton.me/mail/bridge). It runs on your computer and offers your mailbox over local IMAP. In OTP-Scout use:

- Server `127.0.0.1`, port `1143`
- Encryption **STARTTLS**
- **Accept self-signed certificate** ✔ (the Bridge creates its own certificate)
- The username and the *bridge password* shown in the Bridge app, not your Proton password

**Expect a delay of up to about 40 seconds.** The Bridge asks Proton for new mail only every 20–40 seconds. A code therefore reaches your local mailbox that much later than your phone. OTP-Scout shows it as soon as the Bridge reports it. If you need codes faster, have the login mails of those services sent or forwarded to a mailbox with real IMAP push (any provider above) and add that account instead.

### Microsoft (Outlook.com, Microsoft 365)

Microsoft no longer accepts passwords or app passwords for IMAP. It requires an OAuth sign-in, which OTP-Scout doesn't support yet. Two workarounds:

- **Outlook on the web:** if you keep Outlook open in the browser anyway, a Tampermonkey script can pass codes from new mails to OTP-Scout. See [Example: Outlook on the web](api.md#example-outlook-on-the-web).
- **Forwarding:** forward login mails to a mailbox at another provider and add that one.

### Company mail servers

Ask your IT department for the IMAP server and port. If the server uses a certificate from an internal certificate authority, install that CA in the system trust store (`/usr/local/share/ca-certificates/`, then `sudo update-ca-certificates`). Prefer that over *Accept self-signed certificate*.

## Connection Problems

The account card shows the last status, e.g. *Connected – watching INBOX* or an error. Errors also appear as a desktop notification. After a lost connection, OTP-Scout reconnects on its own with growing pauses (5 seconds up to 5 minutes). **Reconnect all** and *Check now* in the tray menu retry immediately.

| Message | Cause and fix |
|---|---|
| Login failed for … | Wrong password, or the provider requires an [app password](#common-providers). For GMX and WEB.DE, check that IMAP access is enabled. |
| Login OK, but mailbox '…' not found / Failed to select mailbox | The folder name under **Mailbox** doesn't exist. Some providers treat folder names as case-sensitive. |
| No password stored – edit the account and enter it | The keyring holds no password for this account, e.g. after moving to a new computer. Edit the account and enter the password again. |
| TLS handshake failed | Check server and port. For the Proton Mail Bridge, turn on *Accept self-signed certificate*. |
| STARTTLS failed – try SSL/TLS instead | The server expects implicit TLS. Switch the encryption to *SSL/TLS* and the port to 993. |
| Failed to connect / Timeout connecting | Wrong server or port, or a firewall. For the Proton Mail Bridge: is the Bridge running? |
| Server did not respond – connection lost | The connection dropped, e.g. after sleep or a network change. OTP-Scout reconnects on its own. |

**Passwords need a keyring.** OTP-Scout stores passwords via the Secret Service, provided by GNOME Keyring or KWallet. Most desktops run one. On minimal setups, install `gnome-keyring`.

**More detail:** the *Log* tab, switched to *Technical*, lists connects, wake-ups and errors per account. For timing details, start OTP-Scout from a terminal with `RUST_LOG=otp_scout=debug otp-scout`.
