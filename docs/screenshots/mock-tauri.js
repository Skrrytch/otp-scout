// Stand-in for the Tauri API with demo data, so the pages can be rendered in a
// plain browser for screenshots. Loaded before the page script by make.py.
// The URL hash selects the scenario, see SCENARIOS at the bottom.
;(() => {
  const ALL = '*'
  // Catalog language and dates follow the browser; keep the demo in English.
  Object.defineProperty(navigator, 'language', { get: () => 'en-US' })
  const now = Date.now()
  const accounts = [
    { id: 'acc-gmail', label: 'Gmail', server: 'imap.gmail.com', port: 993, security: 'ssl', allow_invalid_certs: false, user: 'alex.doe@gmail.com', mailbox: 'INBOX', tag: 'gmail' },
    { id: 'acc-work', label: 'Work', server: 'imap.example-corp.com', port: 993, security: 'ssl', allow_invalid_certs: false, user: 'a.doe@example-corp.com', mailbox: 'INBOX', tag: 'work' },
    { id: 'acc-proton', label: 'Proton', server: '127.0.0.1', port: 1143, security: 'starttls', allow_invalid_certs: true, user: 'alex.doe@proton.me', mailbox: 'INBOX', tag: 'proton' },
  ]
  const statuses = [
    { account_id: 'acc-gmail', state: 'ok', message: 'Connected – watching INBOX', timestamp: now / 1000 - 3600 },
    { account_id: 'acc-work', state: 'ok', message: 'Mailbox checked – 1 new', timestamp: now / 1000 - 240 },
    { account_id: 'acc-proton', state: 'ok', message: 'Connected – watching INBOX', timestamp: now / 1000 - 3500 },
  ]
  const rule = (id, label, tags, r) => ({ id, label, tags, enabled: true, sender: '', subject: '', body: '', code_pattern: '[0-9]{4,8}', link_prefix: '', kind: 'code', ...r })
  let rules = [
    rule('github', 'GitHub (English)', [ALL], { sender: 'noreply@github.com', subject: '[GitHub] Please verify your device', body: 'Verification code: {code}', code_pattern: '[0-9]{6}', catalog: { app: 'github', lang: 'en' } }),
    rule('spotify', 'Spotify (English)', ['gmail'], { sender: '*@alerts.spotify.com', subject: '{code} * your Spotify login code', catalog: { app: 'spotify', lang: 'en' } }),
    rule('claude', 'Claude AI (English)', ['proton'], { kind: 'link', sender: '*mail.anthropic.com', subject: 'Secure link to log in to Claude.ai', link_prefix: 'https://claude.ai/magic-link', catalog: { app: 'claude-ai', lang: 'en' } }),
    rule('vpn', 'Company VPN', ['work'], { sender: 'security@example-corp.com', subject: 'Your VPN sign-in code is {code}', code_pattern: '[0-9]{6}' }),
  ]
  const ok = (stage, detail) => ({ stage, ok: true, detail })
  const fail = (stage, detail) => ({ stage, ok: false, detail })
  const mail = (seq, minutesAgo, account_id, from, subject, outcome, kind, value, traces) => {
    const acc = accounts.find(a => a.id === account_id)
    return { seq, timestamp: now - minutesAgo * 60000, account_id, account: acc.label, type: 'mail', uid: 1000 + seq,
             from, subject, outcome, kind, value, traces, timings: { headers: 84, body: kind === 'link' || outcome === 'partial' ? 212 : null, done: 230 } }
  }
  const log = [
    mail(1, 52, 'acc-gmail', 'Newsletter <news@shop.example>', 'Our autumn sale starts today', 'no_match', null, null,
      [{ rule: 'GitHub (English)', rule_id: 'github', stages: [fail('sender', 'does not match "noreply@github.com"')], value: null },
       { rule: 'Spotify (English)', rule_id: 'spotify', stages: [fail('sender', 'does not match "*@alerts.spotify.com"')], value: null }]),
    mail(2, 31, 'acc-proton', 'Anthropic <no-reply@mail.anthropic.com>', 'Secure link to log in to Claude.ai', 'found', 'link', 'https://claude.ai/magic-link',
      [{ rule: 'Claude AI (English)', rule_id: 'claude', stages: [ok('sender', 'matches "*mail.anthropic.com"'), ok('subject', 'matches "Secure link to log in to Claude.ai"'), ok('link', 'found link starting with "https://claude.ai/magic-link"')], value: 'https://claude.ai/magic-link' }]),
    mail(3, 12, 'acc-work', 'IT Security <security@example-corp.com>', 'Your VPN sign-in code is 731905', 'found', 'code', '731905',
      [{ rule: 'Company VPN', rule_id: 'vpn', stages: [ok('sender', 'matches "security@example-corp.com"'), ok('subject', 'matches "Your VPN sign-in code is {code}", code in subject')], value: '731905' }]),
    mail(4, 2, 'acc-gmail', 'GitHub <noreply@github.com>', '[GitHub] Please verify your device', 'found', 'code', '482913',
      [{ rule: 'GitHub (English)', rule_id: 'github', stages: [ok('sender', 'matches "noreply@github.com"'), ok('subject', 'matches "[GitHub] Please verify your device"'), ok('body', '"Verification code: {code}" found in the text')], value: '482913' }]),
  ]

  // Rough imitation of the backend rule test, good enough for the demo sample.
  function testRule({ rule: r, from, subject, body }) {
    const stages = [ok('sender', r.sender ? `matches "${r.sender}"` : 'any sender')]
    if (r.subject.includes('{code}')) {
      stages.push(ok('subject', `matches "${r.subject}", code in subject`))
      return { rule: r.label, rule_id: r.id, stages, value: (subject.match(/[0-9]{4,8}/) || [null])[0] }
    }
    stages.push(ok('subject', r.subject ? `matches "${r.subject}"` : 'any subject'))
    if (!body) return { rule: r.label, rule_id: r.id, stages, value: null }
    const value = (body.match(/[0-9]{6}/) || [null])[0]
    stages.push(r.kind === 'link' ? ok('link', `found link starting with "${r.link_prefix}"`) : ok('body', `"${r.body}" found in the text`))
    return { rule: r.label, rule_id: r.id, stages, value }
  }

  const commands = {
    get_accounts: () => accounts,
    get_rules: () => rules,
    get_status: () => statuses,
    get_log: () => [...log],
    get_catalog: () => ({ catalog: window.__DEMO_CATALOG__, offline_reason: null }),
    get_api_settings: () => ({ token: 'demo-token', config: { enabled: false, port: 6870, allowed_links: [] }, status: { state: 'off', message: '' } }),
    get_app_info: () => ({ name: 'OTP-Scout', tagline: 'your one-time password helper', version: '0.4.0' }),
    test_rule: testRule,
    save_rule: ({ rule: r }) => rules,
    get_last_code: () => null,
  }

  async function invoke(cmd, args = {}) {
    const fn = commands[cmd]
    return fn ? fn(args) : null
  }

  window.__TAURI__ = {
    core: { invoke },
    event: { listen: async (name, cb) => { if (name === 'show-code') window.__showCode = p => cb({ payload: p }) } },
  }

  const popup = (kind, code, account, rule, from, subject) =>
    ({ code, kind, account, account_id: 'x', uid: 1, rule, from, subject })
  const SCENARIOS = {
    accounts: () => go('accounts'),
    rules: () => go('rules'),
    'rule-editor': async () => {
      go('rules')
      await openRule('github', { sample: { from: 'noreply@github.com', subject: '[GitHub] Please verify your device',
        body: 'Hey alex-doe!\n\nVerification code: 482913\n\nIf you did not attempt to sign in, change your password.' } })
    },
    catalog: () => go('catalog'),
    log: async () => {
      go('log')
      await new Promise(r => setTimeout(r, 200))
      logOpen = 4
      renderLog()
    },
    code: () => window.__showCode(popup('code', '482913', 'Gmail', 'GitHub (English)', 'GitHub <noreply@github.com>', '[GitHub] Please verify your device')),
    link: () => window.__showCode(popup('link', 'https://claude.ai/magic-link#token', 'Proton', 'Claude AI (English)', 'Anthropic <no-reply@mail.anthropic.com>', 'Secure link to log in to Claude.ai')),
  }

  window.addEventListener('load', () => setTimeout(async () => {
    const run = SCENARIOS[location.hash.slice(1)]
    if (run) await run()
    // The settings page starts with go('accounts'); the scenario may switch away.
  }, 100))
})()
