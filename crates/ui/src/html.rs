//! Small, allocation-only HTML helpers shared by the agent console and intelligence pages.
//! Every dynamic value passes through [`esc`]; nothing here can panic on malformed JSON.
use serde_json::Value;

/// Escape text for use in HTML element content and double-quoted attributes.
pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Percent-encode a value for use as a single URL path segment.
pub(crate) fn seg(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// String content of a JSON value; numbers and booleans are stringified, everything else is "".
pub(crate) fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

pub(crate) fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// Minor currency units from a JSON number (integer or non-negative float).
pub(crate) fn minor(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| {
        v.as_f64()
            .filter(|f| f.is_finite() && *f >= 0.0)
            .map(|f| f.round() as u64)
    })
}

/// `$1,234.56`
pub(crate) fn usd(m: u64) -> String {
    let dollars = (m / 100).to_string();
    let mut grouped = String::new();
    for (i, c) in dollars.chars().enumerate() {
        if i > 0 && (dollars.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("${grouped}.{:02}", m % 100)
}

/// `USD 1234.56` — the plain form used on payment buttons.
pub(crate) fn usd_plain(m: u64) -> String {
    format!("USD {}.{:02}", m / 100, m % 100)
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// `<time>` element with a compact UTC rendering of an RFC 3339 timestamp.
pub(crate) fn time_html(v: &Value) -> String {
    let raw = str_of(v);
    if raw.is_empty() {
        return String::new();
    }
    let short = if raw.len() >= 16 && [10, 11, 16].iter().all(|i| raw.is_char_boundary(*i)) {
        format!("{} {} UTC", &raw[..10], &raw[11..16])
    } else {
        raw.to_string()
    };
    format!(
        r#"<time datetime="{}" title="{}">{}</time>"#,
        esc(raw),
        esc(raw),
        esc(&short)
    )
}

/// Humanize a snake_case identifier: `awaiting_approval` → `Awaiting approval`.
pub(crate) fn humanize(s: &str) -> String {
    let spaced = s.replace(['_', '-'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Command,
    Agent,
    Intelligence,
    Docs,
}

pub(crate) fn nav(active: Page) -> String {
    let links = [
        (Page::Command, "/", "◈", "Command center"),
        (Page::Agent, "/agent", "✦", "Agent console"),
        (Page::Intelligence, "/intelligence", "▦", "Intelligence"),
        (Page::Docs, "/docs", "↗", "API & setup"),
    ];
    let mut h = String::from(r#"<nav class="side-nav" aria-label="Primary">"#);
    for (page, href, icon, label) in links {
        let (class, current) = if page == active {
            ("nav active", r#" aria-current="page""#)
        } else {
            ("nav", "")
        };
        h.push_str(&format!(
            r#"<a class="{class}" href="{href}"{current}><span aria-hidden="true">{icon}</span> {label}</a>"#
        ));
    }
    h.push_str("</nav>");
    h
}

pub(crate) struct Shell<'a> {
    pub title: &'a str,
    pub eyebrow: &'a str,
    pub heading: &'a str,
    pub subtitle: &'a str,
    pub page: Page,
    pub mode: &'a str,
    pub inference_label: &'a str,
    pub logged_in: bool,
    pub message: &'a str,
    /// Extra attributes for `<body>` (already escaped, leading space included).
    pub body_attrs: String,
    /// Extra tags for `<head>` (scripts); must reference same-origin files only.
    pub head: String,
    pub content: String,
}

pub(crate) fn shell(s: Shell<'_>) -> String {
    let notice = if s.message.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="notice" role="status">{}</div>"#,
            esc(s.message)
        )
    };
    let login = if s.logged_in {
        String::new()
    } else {
        r#"<section class="panel login"><h2>Open your workspace</h2><p class="muted">Sign in with your local administrator password.</p><form method="post" action="/login"><input type="password" name="password" placeholder="Workspace password" autocomplete="current-password" required/><button>Sign in</button></form></section>"#.to_string()
    };
    format!(
        concat!(
            r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"/><meta name="viewport" content="width=device-width,initial-scale=1"/>"#,
            r#"<title>QuarantinePay · {title}</title><link rel="stylesheet" href="/style.css"/>{head}</head>"#,
            r#"<body{body_attrs}><aside><div class="brand"><span class="logo">Q</span>QuarantinePay</div><div class="workspace-label">PAYMENT OPERATIONS</div>{nav}"#,
            r#"<div class="aside-foot"><span class="dot"></span>Rust security gateway<p>Verify delivery. Then settle.</p></div></aside>"#,
            r#"<main><header><div><div class="eyebrow">{eyebrow}</div><h1>{heading}</h1><p class="muted">{subtitle}</p></div>"#,
            r#"<div class="runtime-badges"><span class="mode">{mode} MODE</span><span class="privacy-label">{inference}</span></div></header>"#,
            r#"{notice}{login}{content}"#,
            r#"<footer>Fictional demonstration · Sellers, resellers, reports, fulfilment and carrier are simulated; product listings and prices come from Channel3 · No card details are collected here · Sandbox checkout, when enabled, is hosted by PayPal · <a href="https://github.com/manvendersingh21/QuarantinePay">Source code ↗</a></footer></main></body></html>"#
        ),
        title = esc(s.title),
        head = s.head,
        body_attrs = s.body_attrs,
        nav = nav(s.page),
        eyebrow = esc(s.eyebrow),
        heading = esc(s.heading),
        subtitle = esc(s.subtitle),
        mode = esc(s.mode),
        inference = esc(s.inference_label),
        notice = notice,
        login = login,
        content = if s.logged_in {
            s.content
        } else {
            String::new()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn helpers_are_defensive() {
        assert_eq!(usd(0), "$0.00");
        assert_eq!(usd(5_340), "$53.40");
        assert_eq!(usd(123_456_789), "$1,234,567.89");
        assert_eq!(usd_plain(5_340), "USD 53.40");
        assert_eq!(
            esc(r#"<a href="x">'&"#),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;"
        );
        assert_eq!(seg("ab/c d"), "ab%2Fc%20d");
        assert_eq!(minor(&json!(12.0)), Some(12));
        assert_eq!(minor(&json!(-1)), None);
        assert_eq!(minor(&Value::Null), None);
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(humanize("awaiting_approval"), "Awaiting approval");
        assert!(time_html(&json!("2026-11-09T21:21:00Z")).contains("2026-11-09 21:21 UTC"));
        assert_eq!(
            time_html(&json!("ü")),
            r#"<time datetime="ü" title="ü">ü</time>"#
        );
    }
}
