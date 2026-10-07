# OTP-Scout Rule Catalog

`catalog.json` holds the preconfigured detection rules that OTP-Scout offers in the **Catalog** tab of the account dialog. The app downloads the file from this repository at runtime, so changes here reach all users without a new release.

## ⚠️ Data quality

Many rules in this catalog are **not verified**. Sender addresses, subject lines and code formats were partly collected from memory or generated with the help of AI tools. They have not been checked against real mails. Services also change their mails without notice and use different wording per language and region.

Rules that were not yet checked against a real mail are marked as `unverified` for the affected language and appear with an *unverified* badge in the app. Treat them as a starting point: test them with the rule tester in OTP-Scout and adjust them if needed.

## 🙌 Help wanted

This catalog only stays useful if people who actually receive these mails keep it up to date. Contributions are very welcome:

- **Verify a rule:** You received a login mail from a service in the catalog? Compare sender and subject with the rule. If it matches, remove the language from `unverified`. If it doesn't, fix the rule.
- **Add a service:** Add rules for services that log you in with a one-time code or magic link sent by mail.
- **Add a language:** Add a variant for a language you receive mails in.
- **Report broken rules:** Open an issue with the service name, the language and the (anonymized) sender and subject.

Only add services that send codes or links **for logging in**, either instead of a password or as a recurring device/login check. One-time confirmations during sign-up, newsletter opt-ins or order confirmations don't belong here.

Before opening a pull request, run the tests in `src-tauri/`:

```sh
cargo test catalog
```

They check that the file parses, that every rule compiles and that every category has a name for every language. They also check that verified rules still match real-world subjects.

## Structure

The catalog is a tree with three levels:

1. **Category** – groups services by topic, e.g. *AI*, *Entertainment* or *Shopping*. A category has a display name for every catalog language.
2. **App** – a single service, e.g. *Spotify*. An app contains one variant per language it supports. An app may support only some languages and then appears only when one of them is selected.
3. **Rules** – each language variant contains one or more detection rules. A rule is either a **code** rule (the code is shown and copied) or a **link** rule (a login link is offered for opening). Variants can mix both kinds. A service might, for example, send a code in one language and a link in another, or use two different mail templates.

When a user clicks **+** next to an app, all rules of the selected language are copied into the account's own rules. From then on they are independent of the catalog and can be edited freely.

## JSON format

```json
{
  "version": 1,
  "languages": [
    { "id": "de", "name": "Deutsch" },
    { "id": "en", "name": "English" }
  ],
  "categories": [
    {
      "id": "entertainment",
      "name": { "de": "Unterhaltung", "en": "Entertainment" },
      "apps": [
        {
          "id": "spotify",
          "name": "Spotify",
          "unverified": ["en"],
          "variants": {
            "de": [
              {
                "kind": "code",
                "sender": "*@alerts.spotify.com",
                "subject": "{code} * dein Spotify Anmeldecode",
                "body": "",
                "code_pattern": "[0-9]{4,8}"
              }
            ],
            "en": [
              {
                "kind": "code",
                "sender": "*@alerts.spotify.com",
                "subject": "{code} * your Spotify login code",
                "body": "",
                "code_pattern": "[0-9]{4,8}"
              }
            ]
          }
        }
      ]
    }
  ]
}
```

### Top level

| Field | Type | Description |
|---|---|---|
| `version` | number | Format version, currently `1`. The app rejects catalogs with a higher version and asks the user to update. Only increase it for incompatible changes. |
| `languages` | array | Languages offered in the language selector. `id` is an ISO 639-1 code, `name` the display name in that language. |
| `categories` | array | List of categories, see below. |

### Category

| Field | Type | Description |
|---|---|---|
| `id` | string | Stable identifier, lowercase with dashes, e.g. `entertainment`. |
| `name` | object | Display name per language id. Must contain every language from `languages`. |
| `apps` | array | Apps in this category, sorted alphabetically by name. |

### App

| Field | Type | Description |
|---|---|---|
| `id` | string | Stable identifier, lowercase with dashes, e.g. `disney-plus`. |
| `name` | string | Display name of the service. It also becomes the rule name, e.g. *Spotify (Deutsch)*. |
| `variants` | object | Rules per language id: `{ "<language id>": [ <rule>, … ] }`. At least one language, each with at least one rule. |
| `unverified` | array | *Optional.* Language ids whose rules were not yet checked against a real mail. Omit or leave empty once all languages are verified. |

### Rule

| Field | Type | Applies to | Description |
|---|---|---|---|
| `kind` | `"code"` \| `"link"` | both | What the rule extracts. |
| `sender` | string | both | Comma-separated sender patterns, e.g. `*@github.com, noreply@*`. `*` matches any text. Empty = any sender. Prefer a domain pattern over an exact address, since services often send from several addresses. |
| `subject` | string | both | Subject pattern, see *Patterns*. Empty = any subject; then set a specific sender. |
| `body` | string | code | Pattern for the mail text around the code, e.g. `Security code: {code}`. Only used if the subject doesn't contain `{code}`. Empty = the first standalone word in the body that matches `code_pattern`. |
| `code_pattern` | string | code | Regular expression describing the code, e.g. `[0-9]{6}` or `[A-Z0-9]{3}-[A-Z0-9]{3}`. Keep it as specific as possible: a loose pattern with an empty `body` may pick up a year, a postal code or a word instead of the code. |
| `link_prefix` | string | link | The first link in the mail that starts with this text is offered. Must start with `https://` and should include the domain and path, e.g. `https://claude.ai/magic-link`. |

Fields that don't apply to a rule's `kind` can be omitted. A rule's name and active state aren't stored in the catalog. The app sets them when the rule is added.

### Patterns

Subject and body patterns are written the way the text appears in the mail:

- `{code}` marks where the code is. It may appear at most once.
- `*` matches any text, e.g. `{code} * dein Spotify Anmeldecode` matches *913088 – dein Spotify Anmeldecode*.
- Any whitespace matches any run of whitespace, including non-breaking spaces and line breaks.
- Matching ignores case, and the pattern only has to appear somewhere in the text. Prefixes or suffixes such as a timestamp after the subject don't need a `*`.
- Everything else is matched literally. Characters like `.`, `+`, `(` or `[` need no escaping.

The matching runs in two steps: sender and subject are checked first, and the mail body is only downloaded if they match. Link rules always take the link from the body.
