use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Placeholder marking the code's position inside a subject/body pattern.
pub const CODE_PLACEHOLDER: &str = "{code}";
pub const DEFAULT_CODE_PATTERN: &str = "[A-Za-z0-9-]{4,12}";

/// What a rule extracts from a matching mail.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    /// A code, shown large and copied to the clipboard.
    #[default]
    Code,
    /// A login link (e.g. a magic link), offered for opening.
    Link,
}

/// A detection rule. Matching happens in two stages:
/// 1. Headers: `sender` (optional) and `subject` must match.
/// 2. Value: a code is taken from the subject if it contains `{code}`,
///    otherwise from the body; a link always comes from the body. The body is
///    only fetched once stage 1 matched.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(from = "RawRule")]
pub struct DetectionRule {
    /// Stable identifier in `rules.json`. Empty for catalog and unsaved rules.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// Accounts checking this rule: account tags or `*` for all. Empty = none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Origin in the catalog, if the rule was taken from there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog: Option<CatalogRef>,
    pub label: String,
    pub kind: RuleKind,
    /// Comma-separated sender globs, e.g. `*@github.com, noreply@*`. Empty = any sender.
    pub sender: String,
    /// Subject pattern, e.g. `{code} is your code`. `*` = any text. Empty = any subject.
    pub subject: String,
    /// Body pattern, e.g. `Your code: {code}`. Empty = first token matching `code_pattern`.
    pub body: String,
    /// Regex describing what a code looks like.
    pub code_pattern: String,
    /// Link mode: the link must start with this, e.g. `https://claude.ai/magic-link`.
    pub link_prefix: String,
    pub enabled: bool,
}

/// Tag that assigns a rule to every account, including future ones.
pub const ALL_ACCOUNTS: &str = "*";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogRef {
    pub app: String,
    pub lang: String,
}

impl DetectionRule {
    /// Whether the account with `tag` checks this rule.
    pub fn applies_to(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == ALL_ACCOUNTS || t == tag)
    }

    /// Same matching behaviour, ignoring id, tags and catalog origin.
    pub fn same_content(&self, other: &Self) -> bool {
        let strip = |r: &Self| Self { id: String::new(), tags: vec![], catalog: None, ..r.clone() };
        strip(self) == strip(other)
    }
}

/// Decoded mail body: the plain text (HTML converted if there is no text
/// part) and the raw HTML, which still contains the `href` targets.
#[derive(Debug, Default)]
pub struct MailBody {
    pub text: String,
    pub html: String,
}

/// Accepts both the current and the legacy (search_field/context/capture) format.
#[derive(Deserialize)]
struct RawRule {
    #[serde(default)]
    id: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    catalog: Option<CatalogRef>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    kind: RuleKind,
    #[serde(default)]
    link_prefix: String,
    #[serde(default)]
    sender: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    code_pattern: Option<String>,
    #[serde(default = "default_enabled")]
    enabled: bool,
    // legacy
    #[serde(default)]
    search_field: Option<String>,
    #[serde(default)]
    context_pattern: Option<String>,
    #[serde(default)]
    capture_pattern: Option<String>,
}

fn default_enabled() -> bool {
    true
}

impl From<RawRule> for DetectionRule {
    fn from(r: RawRule) -> Self {
        let legacy = r.subject.is_none() && r.body.is_none() && r.search_field.is_some();
        if !legacy {
            return Self {
                id: r.id,
                tags: r.tags,
                catalog: r.catalog,
                label: r.label,
                kind: r.kind,
                link_prefix: r.link_prefix,
                sender: r.sender.unwrap_or_default(),
                subject: r.subject.unwrap_or_default(),
                body: r.body.unwrap_or_default(),
                code_pattern: r
                    .code_pattern
                    .filter(|p| !p.is_empty())
                    .unwrap_or_else(|| DEFAULT_CODE_PATTERN.into()),
                enabled: r.enabled,
            };
        }
        let ctx = r.context_pattern.unwrap_or_default();
        let ctx_glob = if ctx.is_empty() { String::new() } else { format!("*{ctx}*") };
        let code = format!("{CODE_PLACEHOLDER} {ctx}").trim().to_string();
        let (sender, subject, body) = match r.search_field.as_deref() {
            Some("body") | Some("all") => (String::new(), String::new(), code),
            Some("from") => (ctx_glob, String::new(), String::new()),
            _ => (String::new(), code, String::new()),
        };
        Self {
            id: r.id,
            tags: r.tags,
            catalog: r.catalog,
            label: r.label,
            kind: RuleKind::Code,
            link_prefix: String::new(),
            sender,
            subject,
            body,
            code_pattern: r
                .capture_pattern
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| DEFAULT_CODE_PATTERN.into()),
            enabled: r.enabled,
        }
    }
}

impl Default for DetectionRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            tags: vec![],
            catalog: None,
            label: "Default".into(),
            kind: RuleKind::Code,
            link_prefix: String::new(),
            sender: String::new(),
            subject: "{code} is your verification code".into(),
            body: String::new(),
            code_pattern: DEFAULT_CODE_PATTERN.into(),
            enabled: true,
        }
    }
}

/// Turns a user pattern into a case-insensitive regex:
/// literal text is escaped, `*` matches anything, whitespace matches any
/// whitespace run, `{code}` becomes the named capture group `code`.
fn pattern_to_regex(pattern: &str, code_pattern: &str) -> Result<Regex> {
    let mut re = String::from("(?is)");
    for (i, part) in pattern.trim().split(CODE_PLACEHOLDER).enumerate() {
        if i == 1 {
            re.push_str(&format!("(?P<code>{code_pattern})"));
        } else if i > 1 {
            bail!("'{CODE_PLACEHOLDER}' may only appear once");
        }
        let mut literal = String::new();
        let flush = |lit: &mut String, re: &mut String| {
            re.push_str(&regex::escape(lit));
            lit.clear();
        };
        let mut chars = part.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '*' {
                flush(&mut literal, &mut re);
                re.push_str(".*?");
            } else if c.is_whitespace() {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                flush(&mut literal, &mut re);
                re.push_str(r"\s+");
            } else {
                literal.push(c);
            }
        }
        flush(&mut literal, &mut re);
    }
    Regex::new(&re).with_context(|| format!("Invalid pattern '{pattern}'"))
}

pub struct CompiledRule {
    pub id: String,
    pub label: String,
    pub kind: RuleKind,
    senders: Vec<String>,
    subject_pat: String,
    subject: Option<Regex>,
    subject_has_code: bool,
    body_pat: String,
    /// Code mode: extracts the code from the body text.
    body: Option<Regex>,
    /// Link mode: lowercase prefix the link must start with.
    link_prefix: String,
}

/// Stage of the rule check, in the order they run.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Sender,
    Subject,
    Body,
    Link,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StageResult {
    pub stage: Stage,
    pub ok: bool,
    pub detail: String,
}

/// Why a rule did or did not match a mail, stage by stage. Ends at the first
/// failed stage, or before the body stage if no body was given.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MatchTrace {
    pub rule: String,
    /// Id of the rule in `rules.json`; empty for unsaved rules.
    pub rule_id: String,
    pub stages: Vec<StageResult>,
    /// The code or link found.
    pub value: Option<String>,
}

/// Result of matching the headers of a mail against a rule.
#[derive(Debug, PartialEq)]
pub enum HeaderMatch {
    /// Code found in the subject.
    Code(String),
    /// Headers match, the code/link must be taken from the body.
    NeedsBody,
}

static URL_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s"'<>()\[\]]+"#).unwrap());

/// First URL in `body` starting with `prefix` (case-insensitive).
/// HTML is searched first since its `href`s hold the untruncated links.
fn find_link(body: &MailBody, prefix: &str) -> Option<String> {
    let html = body.html.replace("&amp;", "&");
    let found = [html.as_str(), body.text.as_str()].into_iter().find_map(|src| {
        URL_RE
            .find_iter(src)
            .map(|m| m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?']))
            .find(|url| url.to_lowercase().starts_with(prefix))
            .map(str::to_string)
    });
    found
}

impl CompiledRule {
    pub fn compile(rule: &DetectionRule) -> Result<Self> {
        let label = if rule.label.is_empty() { "Rule".to_string() } else { rule.label.clone() };
        let code_pattern = if rule.code_pattern.is_empty() {
            DEFAULT_CODE_PATTERN
        } else {
            rule.code_pattern.as_str()
        };
        Regex::new(code_pattern).with_context(|| format!("[{label}] Invalid code format"))?;

        let senders: Vec<String> = rule
            .sender
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let subject_pat = rule.subject.trim();
        if senders.is_empty() && subject_pat.is_empty() {
            bail!("[{label}] Sender or subject is required");
        }
        let subject_has_code = rule.kind == RuleKind::Code && subject_pat.contains(CODE_PLACEHOLDER);
        let subject = (!subject_pat.is_empty())
            .then(|| pattern_to_regex(subject_pat, code_pattern))
            .transpose()
            .with_context(|| format!("[{label}] Subject"))?;

        let link_prefix = rule.link_prefix.trim().to_lowercase();
        let body = match rule.kind {
            RuleKind::Link => {
                if !(link_prefix.starts_with("https://") || link_prefix.starts_with("http://"))
                    || link_prefix.len() <= "https://".len()
                {
                    bail!("[{label}] Link must start with https://… (e.g. https://claude.ai/magic-link)");
                }
                None
            }
            RuleKind::Code => {
                let body_pat = rule.body.trim();
                Some(if body_pat.is_empty() {
                    Regex::new(&format!(r"\b(?P<code>{code_pattern})\b"))?
                } else {
                    if !body_pat.contains(CODE_PLACEHOLDER) {
                        bail!("[{label}] Body pattern must contain {CODE_PLACEHOLDER}");
                    }
                    pattern_to_regex(body_pat, code_pattern).with_context(|| format!("[{label}] Body"))?
                })
            }
        };

        Ok(Self {
            id: rule.id.clone(),
            label,
            kind: rule.kind,
            senders,
            subject_pat: subject_pat.to_string(),
            subject,
            subject_has_code,
            body_pat: rule.body.trim().to_string(),
            body,
            link_prefix,
        })
    }

    pub fn match_headers(&self, from: &str, subject: &str) -> Option<HeaderMatch> {
        if !sender_matches(&self.senders, from) {
            return None;
        }
        let Some(re) = &self.subject else {
            return Some(HeaderMatch::NeedsBody);
        };
        let caps = re.captures(subject)?;
        if self.subject_has_code {
            caps.name("code").map(|m| HeaderMatch::Code(m.as_str().to_string()))
        } else {
            Some(HeaderMatch::NeedsBody)
        }
    }

    /// Checks the mail like `match_headers` + `match_body` and records each stage.
    /// `body` is `None` if it was not loaded; the trace then stops before it.
    pub fn trace(&self, from: &str, subject: &str, body: Option<&MailBody>) -> MatchTrace {
        let mut trace = MatchTrace { rule: self.label.clone(), rule_id: self.id.clone(), stages: vec![], value: None };
        let mut stage = |stage, ok, detail: String| trace.stages.push(StageResult { stage, ok, detail });

        let senders = self.senders.join(", ");
        match (self.senders.is_empty(), sender_matches(&self.senders, from)) {
            (true, _) => stage(Stage::Sender, true, "any sender".into()),
            (false, true) => stage(Stage::Sender, true, format!("matches \"{senders}\"")),
            (false, false) => {
                stage(Stage::Sender, false, format!("does not match \"{senders}\""));
                return trace;
            }
        }

        let needs_body = match self.match_headers(from, subject) {
            None => {
                stage(Stage::Subject, false, format!("does not match \"{}\"", self.subject_pat));
                return trace;
            }
            Some(HeaderMatch::Code(code)) => {
                stage(Stage::Subject, true, format!("matches \"{}\", code in subject", self.subject_pat));
                trace.value = Some(code);
                false
            }
            Some(HeaderMatch::NeedsBody) if self.subject.is_none() => {
                stage(Stage::Subject, true, "any subject".into());
                true
            }
            Some(HeaderMatch::NeedsBody) => {
                stage(Stage::Subject, true, format!("matches \"{}\"", self.subject_pat));
                true
            }
        };
        if !needs_body {
            return trace;
        }
        let Some(body) = body else { return trace };

        let value = self.match_body(body);
        let found = value.is_some();
        match self.kind {
            RuleKind::Code if self.body_pat.is_empty() => {
                stage(Stage::Body, found, if found { "code found".into() } else { "no code-like word in the text".into() })
            }
            RuleKind::Code => stage(
                Stage::Body,
                found,
                format!("\"{}\" {} in the text", self.body_pat, if found { "found" } else { "not found" }),
            ),
            RuleKind::Link => stage(
                Stage::Link,
                found,
                format!("{} link starting with \"{}\"", if found { "found" } else { "no" }, self.link_prefix),
            ),
        }
        trace.value = value;
        trace
    }

    pub fn match_body(&self, body: &MailBody) -> Option<String> {
        match &self.body {
            Some(re) => re
                .captures(&body.text)
                .and_then(|c| c.name("code"))
                .map(|m| m.as_str().to_string()),
            None => find_link(body, &self.link_prefix),
        }
    }
}

pub struct CompiledRules {
    pub rules: Vec<CompiledRule>,
}

impl CompiledRules {
    /// Compiles the enabled rules. Invalid ones are logged and skipped, so one
    /// broken rule does not stop the others (rules are validated on save).
    pub fn compile(rules: &[DetectionRule]) -> Self {
        let rules = rules
            .iter()
            .filter(|r| r.enabled)
            .filter_map(|r| CompiledRule::compile(r).inspect_err(|e| tracing::error!("Rule skipped: {e:#}")).ok())
            .collect();
        Self { rules }
    }
}

/// Checks a single rule against sample data (used by the settings UI).
pub fn test_rule(rule: &DetectionRule, from: &str, subject: &str, body: &str) -> Result<MatchTrace> {
    let compiled = CompiledRule::compile(rule)?;
    let body = MailBody { text: body.to_string(), html: body.to_string() };
    Ok(compiled.trace(from, subject, Some(&body)))
}

pub fn default_rules() -> Vec<DetectionRule> {
    vec![DetectionRule::default()]
}

pub fn glob_match(pattern: &str, value: &str) -> bool {
    let value = value.to_lowercase();
    let pattern = pattern.to_lowercase();

    if pattern == "*" {
        return true;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return value == parts[0];
    }

    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !value.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            if !value[pos..].ends_with(part) {
                return false;
            }
        } else {
            match value[pos..].find(part) {
                Some(idx) => pos = pos + idx + part.len(),
                None => return false,
            }
        }
    }
    true
}

fn sender_matches(filters: &[String], from: &str) -> bool {
    if filters.is_empty() {
        return true;
    }
    let from_lower = from.to_lowercase();
    filters.iter().any(|f| glob_match(f, &from_lower))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(sender: &str, subject: &str, body: &str) -> DetectionRule {
        DetectionRule {
            id: String::new(),
            tags: vec![],
            catalog: None,
            label: "T".into(),
            kind: RuleKind::Code,
            link_prefix: String::new(),
            sender: sender.into(),
            subject: subject.into(),
            body: body.into(),
            code_pattern: "[0-9]{4,8}".into(),
            enabled: true,
        }
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*@example.com", "noreply@example.com"));
        assert!(glob_match("*@example.com", "test@example.com"));
        assert!(!glob_match("*@example.com", "test@gmail.com"));
        assert!(glob_match("noreply@*", "noreply@example.com"));
        assert!(!glob_match("noreply@*", "test@example.com"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*@*", "a@b"));
    }

    #[test]
    fn test_default_rule_subject_match() {
        let rules = CompiledRules::compile(&default_rules());
        let m = rules.rules[0].match_headers("x@y.z", "AB12-CD is your verification code");
        assert_eq!(m, Some(HeaderMatch::Code("AB12-CD".into())));
    }

    #[test]
    fn test_default_rule_no_match() {
        let rules = CompiledRules::compile(&default_rules());
        assert_eq!(rules.rules[0].match_headers("x@y.z", "Your invoice is ready"), None);
    }

    #[test]
    fn test_subject_code_case_insensitive() {
        let r = rule("", "{code} is your code", "");
        assert_eq!(test_rule(&r, "", "123456 IS YOUR CODE", "").unwrap().value, Some("123456".into()));
    }

    #[test]
    fn test_subject_wildcard_and_code_in_body() {
        let r = rule("", "Your * sign-in code", "Your code: {code}");
        let c = CompiledRule::compile(&r).unwrap();
        assert_eq!(c.match_headers("", "Your GitHub sign-in code"), Some(HeaderMatch::NeedsBody));
        let body = MailBody { text: "Hello,\nYour code:\n  987654\nBye".into(), html: String::new() };
        assert_eq!(c.match_body(&body), Some("987654".into()));
    }

    #[test]
    fn test_sender_filter() {
        let r = rule("*@github.com, noreply@*", "{code} is your code", "");
        assert_eq!(test_rule(&r, "a@github.com", "1234 is your code", "").unwrap().value, Some("1234".into()));
        assert_eq!(test_rule(&r, "noreply@x.de", "1234 is your code", "").unwrap().value, Some("1234".into()));
        assert_eq!(test_rule(&r, "evil@x.de", "1234 is your code", "").unwrap().value, None);
    }

    #[test]
    fn test_sender_only_body_fallback() {
        let r = rule("*@bank.de", "", "");
        assert_eq!(test_rule(&r, "tan@bank.de", "Ihre TAN", "Ihre TAN lautet 556677.").unwrap().value, Some("556677".into()));
    }

    #[test]
    fn test_trace_stops_at_failed_stage() {
        let c = CompiledRule::compile(&rule("*@service.com", "Your code for *", "Code: {code}")).unwrap();
        let body = MailBody { text: "Hello".into(), html: String::new() };

        let t = c.trace("x@other.com", "Your code for Service", Some(&body));
        assert_eq!(t.stages.iter().map(|s| (s.stage, s.ok)).collect::<Vec<_>>(), [(Stage::Sender, false)]);

        let t = c.trace("login@service.com", "Weekly news", Some(&body));
        assert_eq!(t.stages.last().map(|s| (s.stage, s.ok)), Some((Stage::Subject, false)));

        let t = c.trace("login@service.com", "Your code for Service", Some(&body));
        assert_eq!(t.stages.last().map(|s| (s.stage, s.ok)), Some((Stage::Body, false)));
        assert!(t.stages[2].detail.contains("not found"), "{}", t.stages[2].detail);
        assert_eq!(t.value, None);
    }

    #[test]
    fn test_trace_without_body_ends_after_headers() {
        let c = CompiledRule::compile(&rule("", "Your code for *", "Code: {code}")).unwrap();
        let t = c.trace("a@b.c", "Your code for Service", None);
        assert_eq!(t.stages.iter().map(|s| (s.stage, s.ok)).collect::<Vec<_>>(), [(Stage::Sender, true), (Stage::Subject, true)]);
    }

    #[test]
    fn test_trace_code_in_subject_and_link() {
        let c = CompiledRule::compile(&rule("", "{code} is your code", "")).unwrap();
        let t = c.trace("a@b.c", "4711 is your code", None);
        assert_eq!(t.value.as_deref(), Some("4711"));
        assert!(t.stages.iter().all(|s| s.ok && s.stage != Stage::Body));

        let c = CompiledRule::compile(&link_rule("https://claude.ai/magic-link")).unwrap();
        let body = MailBody { text: "https://claude.ai/help".into(), html: String::new() };
        let t = c.trace("no-reply@mail.anthropic.com", "Secure link to log in", Some(&body));
        assert_eq!(t.stages.last().map(|s| (s.stage, s.ok)), Some((Stage::Link, false)));
    }

    #[test]
    fn test_same_content_ignores_identity() {
        let a = DetectionRule { id: "1".into(), tags: vec!["a".into()], ..rule("", "x {code}", "") };
        let b = DetectionRule { id: "2".into(), tags: vec!["b".into()], ..rule("", "x {code}", "") };
        assert!(a.same_content(&b));
        assert!(!a.same_content(&rule("", "y {code}", "")));
        assert!(a.applies_to("a") && !a.applies_to("b"));
        assert!(DetectionRule { tags: vec![ALL_ACCOUNTS.into()], ..b }.applies_to("zzz"));
    }

    #[test]
    fn test_requires_sender_or_subject() {
        assert!(CompiledRule::compile(&rule("", "", "{code}")).is_err());
    }

    #[test]
    fn test_body_requires_placeholder() {
        assert!(CompiledRule::compile(&rule("", "Login", "no placeholder")).is_err());
    }

    #[test]
    fn test_legacy_rule_migration() {
        let json = r#"{"label":"Old","search_field":"subject","context_pattern":"is your code","capture_pattern":"[0-9]+","enabled":true}"#;
        let r: DetectionRule = serde_json::from_str(json).unwrap();
        assert_eq!(r.subject, "{code} is your code");
        assert_eq!(r.code_pattern, "[0-9]+");
        assert_eq!(test_rule(&r, "", "4711 is your code", "").unwrap().value, Some("4711".into()));
    }

    fn link_rule(prefix: &str) -> DetectionRule {
        DetectionRule {
            kind: RuleKind::Link,
            link_prefix: prefix.into(),
            ..rule("*@mail.anthropic.com", "Secure link to log in*", "")
        }
    }

    #[test]
    fn test_link_from_html_href() {
        let c = CompiledRule::compile(&link_rule("https://claude.ai/magic-link")).unwrap();
        assert_eq!(c.match_headers("no-reply@mail.anthropic.com", "Secure link to log in to Claude.ai"), Some(HeaderMatch::NeedsBody));
        let body = MailBody {
            text: "Click the button to log in.".into(),
            html: r#"<a href="https://claude.ai/magic-link#abc123:ZGVm?x=1&amp;y=2">Sign in</a> <a href="https://claude.ai/help">Help</a>"#.into(),
        };
        assert_eq!(c.match_body(&body), Some("https://claude.ai/magic-link#abc123:ZGVm?x=1&y=2".into()));
    }

    #[test]
    fn test_link_from_text_trims_punctuation() {
        let c = CompiledRule::compile(&link_rule("https://Claude.ai/magic-link")).unwrap();
        let body = MailBody { text: "Open https://claude.ai/magic-link#tok. Thanks".into(), html: String::new() };
        assert_eq!(c.match_body(&body), Some("https://claude.ai/magic-link#tok".into()));
    }

    #[test]
    fn test_link_other_prefix_no_match() {
        let c = CompiledRule::compile(&link_rule("https://claude.ai/magic-link")).unwrap();
        let body = MailBody { text: "https://evil.example/claude.ai/magic-link".into(), html: String::new() };
        assert_eq!(c.match_body(&body), None);
    }

    #[test]
    fn test_link_requires_url_prefix() {
        assert!(CompiledRule::compile(&link_rule("claude.ai")).is_err());
        assert!(CompiledRule::compile(&link_rule("https://")).is_err());
    }
}

#[cfg(test)]
mod spotify_tests {
    use super::*;

    #[test]
    fn test_code_in_subject_with_nbsp_and_dash() {
        let mut r = DetectionRule::default();
        r.sender = "*".into();
        r.subject = "{code} * dein Spotify Anmeldecode".into();
        r.body = String::new();
        let code = test_rule(&r, "no-reply@alerts.spotify.com", "913088\u{a0}\u{2013} dein Spotify Anmeldecode", "").unwrap().value;
        assert_eq!(code.as_deref(), Some("913088"));
    }
}
