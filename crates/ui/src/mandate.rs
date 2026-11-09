//! Spending-mandate panel: sign once, approve one PayPal Sandbox wallet save, then the buyer agent
//! may purchase within the signed limits without further clicks.
use crate::html::{esc, minor, seg, str_of, text, time_html, truncate, usd};
use serde_json::Value;

pub(crate) const AUTO_CAPTURE_LABEL: &str =
    "Auto-capture only clean, low-risk deliveries (anything else waits for me)";

/// Convert days since 1970-01-01 to a proleptic Gregorian (year, month, day).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Unix seconds → RFC 3339 UTC (`2026-11-09T10:00:00Z`). Out-of-range values yield `None`.
pub(crate) fn rfc3339(secs: i64) -> Option<String> {
    if !(0..=253_402_300_799).contains(&secs) {
        return None;
    }
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    ))
}

/// A timestamp that may arrive as unix seconds or as an RFC 3339 string.
pub(crate) fn timestamp(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.is_finite()).map(|f| f as i64))
            .and_then(rfc3339),
        _ => None,
    }
}

fn time_of(v: &Value) -> String {
    timestamp(v)
        .map(|t| time_html(&Value::String(t)))
        .unwrap_or_default()
}

fn money(v: &Value) -> String {
    minor(v).map(usd).unwrap_or_else(|| "—".into())
}

fn seller_names(ids: &Value) -> Vec<String> {
    ids.as_array()
        .map(|a| {
            a.iter()
                .map(|id| {
                    let id = text(id);
                    quarantinepay_core::contract::seller(&id)
                        .map(|s| s.name.to_string())
                        .or_else(|| {
                            quarantinepay_core::product::reseller(&id).map(|r| r.name.to_string())
                        })
                        .unwrap_or(id)
                })
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn all_seller_names() -> String {
    quarantinepay_core::contract::sellers()
        .iter()
        .map(|s| s.name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn all_reseller_names() -> String {
    quarantinepay_core::product::resellers()
        .iter()
        .map(|r| r.name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn purchase_types(kinds: &Value) -> String {
    let names: Vec<&str> = kinds
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|k| match k.as_str() {
                    Some("research_report") => Some("Research reports"),
                    Some(quarantinepay_core::product::PRODUCT_KIND) => Some("Physical products"),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if names.is_empty() {
        "—".into()
    } else {
        names.join(" · ")
    }
}

fn http_url(v: &Value) -> Option<String> {
    let url = text(v);
    (url.starts_with("https://") || url.starts_with("http://")).then_some(url)
}

/// The mandate the console should act on: an active one first, else one awaiting the wallet save.
pub(crate) fn current(mandates: &[Value]) -> Option<&Value> {
    ["active", "pending_vault"].iter().find_map(|state| {
        mandates
            .iter()
            .find(|m| m.is_object() && str_of(&m["state"]) == *state)
    })
}

pub(crate) fn active(mandates: &[Value]) -> Option<&Value> {
    current(mandates).filter(|m| str_of(&m["state"]) == "active")
}

fn terms(m: &Value) -> String {
    let t = &m["mandate"];
    let sellers = seller_names(&t["allowed_sellers"]);
    let sellers = if sellers.is_empty() {
        "—".to_string()
    } else {
        format!(
            r#"<span class="tags">{}</span>"#,
            sellers
                .iter()
                .map(|s| format!(r#"<span class="tag">{}</span>"#, esc(s)))
                .collect::<String>()
        )
    };
    let floors = match (text(&t["min_competitors"]), text(&t["min_sources"])) {
        (c, s) if c.is_empty() && s.is_empty() => "—".to_string(),
        (c, s) => format!(
            "≥ {} competitors · ≥ {} sources",
            esc(if c.is_empty() { "—" } else { &c }),
            esc(if s.is_empty() { "—" } else { &s })
        ),
    };
    let auto = match t["auto_capture_clean"].as_bool() {
        Some(true) => "Clean, low-risk deliveries only",
        Some(false) => "Never — I review every delivery",
        None => "—",
    };
    let expires = time_of(&t["expires_at"]);
    let rows = [
        ("Per-purchase cap", money(&t["max_per_purchase_minor"])),
        ("Total budget", money(&t["max_total_minor"])),
        ("Allowed sellers", sellers),
        ("Purchase types", purchase_types(&t["allowed_kinds"])),
        ("Report scope floors", floors),
        ("Auto-capture", auto.to_string()),
        (
            "Expires",
            if expires.is_empty() {
                "—".into()
            } else {
                expires
            },
        ),
    ];
    let mut h = String::from(r#"<dl class="kv mandate-terms">"#);
    for (k, v) in rows {
        h.push_str(&format!("<div><dt>{k}</dt><dd>{v}</dd></div>"));
    }
    h.push_str("</dl>");
    h
}

fn digest(m: &Value, label: &str) -> String {
    let d = text(&m["digest"]);
    if d.is_empty() {
        return String::new();
    }
    format!(
        r#"<div class="digest secondary-digest"><span>{label}</span><code title="{}">{}</code></div>"#,
        esc(&d),
        esc(&truncate(&d, 24))
    )
}

fn create_form() -> String {
    format!(
        concat!(
            r#"<span class="eyebrow">SPENDING MANDATE · OPTIONAL</span><h2 id="mandate-title">Let the agent buy without asking</h2>"#,
            r#"<p class="muted small">You approve once at PayPal to save your wallet (a USD 1.00 verification hold, voided immediately). The agent can then buy within these limits without asking; payment is still held until delivery is verified.</p>"#,
            r#"<form method="post" action="/ui/mandates" class="mandate-form"><div class="mandate-fields">"#,
            r#"<label class="field">Per-purchase cap (USD)<input type="number" name="max_per_purchase_usd" value="60" min="1" max="100" step="0.01" inputmode="decimal" required/></label>"#,
            r#"<label class="field">Total budget (USD)<input type="number" name="max_total_usd" value="150" min="1" max="500" step="0.01" inputmode="decimal" required/></label>"#,
            r#"<label class="field">Valid for (hours)<input type="number" name="hours" value="24" min="1" max="168" step="1" inputmode="numeric" required/></label>"#,
            r#"</div><label class="check"><input type="checkbox" name="auto_capture_clean" value="true" checked/><span>{auto}</span></label>"#,
            r#"<p class="muted small">Allowed sellers: report sellers {sellers}; reseller agents {resellers} — all fictional. Purchase types: research reports and physical products (real listings, simulated fulfilment). Limits are clamped by policy (at most USD 100 per purchase, USD 500 total, 7 days). Sandbox test money only.</p>"#,
            r#"<button class="primary-lg">Sign spending mandate</button></form>"#
        ),
        auto = esc(AUTO_CAPTURE_LABEL),
        sellers = esc(&all_seller_names()),
        resellers = esc(&all_reseller_names()),
    )
}

fn pending(m: &Value, replay: bool) -> String {
    let id = seg(str_of(&m["id"]));
    let mut h = format!(
        concat!(
            r#"<div class="mandate-head"><div><span class="eyebrow">SPENDING MANDATE · SIGNED</span><h2 id="mandate-title">Save your wallet to activate</h2></div>{chip}</div>"#,
        ),
        chip = r#"<span class="chip tone-warn">Awaiting wallet save</span>"#,
    );
    h.push_str(&terms(m));
    h.push_str(&digest(m, "Mandate SHA-256 · the exact terms you signed"));
    if replay {
        h.push_str(r#"<p class="muted small">Replay mode: the wallet save and the USD 1.00 verification hold are simulated, not provider-confirmed.</p><div class="actions">"#);
        h.push_str(&format!(
            r#"<form method="post" action="/ui/mandates/{id}/activate"><button>Activate simulated mandate</button></form>"#
        ));
    } else {
        h.push_str(r#"<ol class="mandate-steps"><li>Approve the USD 1.00 verification at PayPal Sandbox. This saves your PayPal wallet; QuarantinePay voids the $1 hold immediately.</li><li>Come back here and activate the mandate.</li></ol><div class="actions">"#);
        match http_url(&m["approval_url"]) {
            Some(url) => h.push_str(&format!(
                r#"<a class="button" href="{}" target="_blank" rel="noopener noreferrer">Approve at PayPal Sandbox ↗</a>"#,
                esc(&url)
            )),
            None => h.push_str(r#"<p class="notice warn">The PayPal approval link is unavailable. Sign a new mandate to retry.</p>"#),
        }
        h.push_str(&format!(
            r#"<form method="post" action="/ui/mandates/{id}/activate"><button class="secondary">I approved — activate mandate</button></form>"#
        ));
    }
    h.push_str(&format!(
        r#"<form method="post" action="/ui/mandates/{id}/revoke"><button class="danger">Cancel mandate</button></form></div>"#
    ));
    h
}

fn active_card(m: &Value, replay: bool) -> String {
    let t = &m["mandate"];
    let id = seg(str_of(&m["id"]));
    let total = minor(&t["max_total_minor"]).unwrap_or(0);
    let committed = minor(&m["committed_minor"]).unwrap_or(0);
    let remaining = minor(&m["remaining_minor"]).unwrap_or(total.saturating_sub(committed));
    let wallet = match (m["vault_saved"].as_bool() == Some(true), replay) {
        (true, false) => r#"<span class="chip tone-good">PayPal wallet saved</span>"#,
        (true, true) => {
            r#"<span class="chip tone-neutral">Simulated wallet · not provider-confirmed</span>"#
        }
        (false, _) => r#"<span class="chip tone-warn">Wallet not saved</span>"#,
    };
    let auto = if t["auto_capture_clean"].as_bool() == Some(true) {
        r#"<span class="chip tone-good">Auto-capture: clean deliveries only</span>"#
    } else {
        r#"<span class="chip tone-neutral">Auto-capture off · you review every delivery</span>"#
    };
    let expiry = match timestamp(&t["expires_at"]) {
        Some(iso) => format!(
            r#"<p class="countdown mandate-expiry">Expires in <time data-deadline="{d}" datetime="{d}">{short}</time></p>"#,
            d = esc(&iso),
            short = esc(&iso.replacen('T', " ", 1).replace('Z', " UTC")),
        ),
        None => String::new(),
    };
    let purchases = m["purchases"].as_u64().unwrap_or(0);
    let mut h = format!(
        concat!(
            r#"<div class="mandate-head"><div><span class="eyebrow">SPENDING MANDATE · ACTIVE</span><h2 id="mandate-title">The agent may buy within these limits</h2></div><span class="chip tone-good">Active</span></div>"#,
            r#"<div class="budget"><div class="budget-figures"><strong>{remaining}</strong><span class="muted small">left of {total} · {committed} committed</span></div>"#,
            r#"<progress class="budget-meter" value="{value}" max="{max}" aria-label="Mandate budget committed">{committed}</progress></div>"#,
            r#"{expiry}<dl class="kv mandate-terms"><div><dt>Per-purchase cap</dt><dd>{cap}</dd></div><div><dt>Purchases</dt><dd>{purchases}</dd></div></dl>"#,
            r#"<div class="chips">{wallet}{auto}</div>"#
        ),
        remaining = usd(remaining),
        total = usd(total),
        committed = usd(committed),
        value = committed.min(total),
        max = total.max(1),
        expiry = expiry,
        cap = money(&t["max_per_purchase_minor"]),
        purchases = purchases,
        wallet = wallet,
        auto = auto,
    );
    h.push_str(&digest(m, "Mandate SHA-256"));
    h.push_str(&format!(
        r#"<p class="muted small">Every purchase is still an authorization hold; nothing is captured until the delivery is verified.</p><form method="post" action="/ui/mandates/{id}/revoke"><button class="danger">Revoke mandate</button></form>"#
    ));
    h
}

fn past_line(m: &Value) -> String {
    let state = str_of(&m["state"]);
    let (label, when) = match state {
        "revoked" => ("Revoked", time_of(&m["revoked_at"])),
        _ => ("Expired", time_of(&m["mandate"]["expires_at"])),
    };
    let purchases = m["purchases"].as_u64().unwrap_or(0);
    format!(
        r#"<li class="muted small">{label}{when} · {spent} committed across {purchases} purchase{s} · <code>{id}</code></li>"#,
        when = if when.is_empty() {
            String::new()
        } else {
            format!(" {when}")
        },
        spent = money(&m["committed_minor"]),
        s = if purchases == 1 { "" } else { "s" },
        id = esc(&truncate(&text(&m["id"]), 14)),
    )
}

/// The mandate panel shown next to the request box.
pub(crate) fn panel(mandates: &[Value], replay: bool) -> String {
    let (class, body) = match current(mandates) {
        Some(m) if str_of(&m["state"]) == "active" => ("mandate-active", active_card(m, replay)),
        Some(m) => ("mandate-pending", pending(m, replay)),
        None => ("mandate-new", create_form()),
    };
    let past: String = mandates
        .iter()
        .filter(|m| matches!(str_of(&m["state"]), "revoked" | "expired"))
        .take(3)
        .map(past_line)
        .collect();
    let history = if past.is_empty() {
        String::new()
    } else {
        format!(r#"<ul class="mandate-history" aria-label="Previous mandates">{past}</ul>"#)
    };
    format!(
        r#"<section class="panel mandate-card {class}" aria-labelledby="mandate-title">{body}{history}</section>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unix_seconds_become_rfc3339() {
        assert_eq!(rfc3339(0).as_deref(), Some("1970-01-01T00:00:00Z"));
        assert_eq!(
            rfc3339(1_700_000_000).as_deref(),
            Some("2023-11-14T22:13:20Z")
        );
        assert_eq!(
            rfc3339(951_782_400).as_deref(),
            Some("2000-02-29T00:00:00Z")
        );
        assert_eq!(rfc3339(-1), None);
        assert_eq!(
            timestamp(&json!("2026-11-09T10:00:00Z")).as_deref(),
            Some("2026-11-09T10:00:00Z")
        );
        assert_eq!(timestamp(&json!(null)), None);
    }
}
