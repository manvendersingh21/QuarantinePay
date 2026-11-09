//! Agent console: buyer request → negotiation → contract → PayPal hold → verified settlement.
use crate::html::{
    Page, Shell, esc, humanize, minor, seg, shell, str_of, text, time_html, truncate, usd,
    usd_plain,
};
use serde_json::Value;

pub const EXAMPLE_REQUEST: &str = "Find me a market research report on AI note-taking apps under $100 covering three competitors, including 5 credible sources";

const INJECTION_BANNER: &str = "Untrusted instruction detected — the seller's content tried to change the verification outcome. It was treated as data, not executed.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum St {
    Done,
    Active,
    Blocked,
    Pending,
}

impl St {
    fn class(self) -> &'static str {
        match self {
            St::Done => "done",
            St::Active => "active",
            St::Blocked => "blocked",
            St::Pending => "pending",
        }
    }
}

#[derive(Clone, Debug)]
struct Stage {
    status: St,
    label: String,
}

fn stage(status: St, label: &str) -> Stage {
    Stage {
        status,
        label: label.to_string(),
    }
}

fn done_through(p: &mut [Stage], n: usize) {
    for s in p.iter_mut().take(n) {
        *s = stage(St::Done, "Done");
    }
}

fn chip(tone: &str, label: &str) -> String {
    format!(r#"<span class="chip tone-{tone}">{}</span>"#, esc(label))
}

fn policy_hold(order: &Value) -> bool {
    order["analysis"]["policy_hold"].as_bool() == Some(true)
}

fn verification_stage(order: &Value) -> Stage {
    if order["verification"].is_null() {
        stage(St::Pending, "No verdict recorded")
    } else if policy_hold(order) {
        if order["risk_override"].as_bool() == Some(true) {
            stage(St::Blocked, "Hold overridden")
        } else {
            stage(St::Blocked, "Policy hold")
        }
    } else if order["verification"]["passed"].as_bool() == Some(true) {
        stage(St::Done, "Passed")
    } else {
        stage(St::Blocked, "Checks failed")
    }
}

/// Derive the nine pipeline stages from task.state and order.state.
fn progress(task: &Value) -> Vec<Stage> {
    let mut p: Vec<Stage> = (0..9).map(|_| stage(St::Pending, "Pending")).collect();
    let order = &task["order"];
    let state = str_of(&task["state"]);
    match state {
        "planning" => p[0] = stage(St::Active, "Reading request"),
        "negotiating" => {
            done_through(&mut p, 2);
            p[2] = stage(St::Active, "Negotiating");
        }
        "no_deal" => {
            done_through(&mut p, 2);
            p[2] = stage(St::Blocked, "No deal within budget");
        }
        "awaiting_approval" => {
            done_through(&mut p, 4);
            p[4] = stage(St::Active, "Your decision");
        }
        "cancelled" => {
            done_through(&mut p, 4);
            p[4] = stage(St::Blocked, "Declined by you");
        }
        "failed" if !order.is_object() => {
            let k = if task["objective"].is_null() {
                0
            } else if task["negotiation"].is_null() {
                2
            } else if task["proposal"].is_null() {
                3
            } else {
                5
            };
            done_through(&mut p, k);
            p[k] = stage(St::Blocked, "Failed");
        }
        _ => {
            done_through(&mut p, 5);
            order_progress(&mut p, order);
            if state == "failed"
                && !p.iter().any(|s| s.status == St::Blocked)
                && let Some(s) = p.iter_mut().find(|s| s.status == St::Active)
            {
                *s = stage(St::Blocked, "Failed");
            }
        }
    }
    p
}

fn order_progress(p: &mut [Stage], order: &Value) {
    if !order.is_object() {
        p[5] = stage(St::Active, "Creating PayPal order");
        return;
    }
    let has_artifact = !order["artifact"].is_null();
    let settled_through = |p: &mut [Stage]| {
        if has_artifact {
            done_through(p, 8);
            p[7] = verification_stage(order);
        } else {
            done_through(p, 6);
        }
    };
    match str_of(&order["state"]) {
        "approval_pending" => p[5] = stage(St::Active, "Awaiting PayPal approval"),
        "declined" => p[5] = stage(St::Blocked, "Authorization declined"),
        "authorized" => {
            done_through(p, 6);
            p[6] = stage(St::Active, "Awaiting delivery");
        }
        "quarantined" => {
            done_through(p, 7);
            p[7] = stage(St::Active, "Analysing");
        }
        "buyer_review" => {
            done_through(p, 8);
            p[7] = verification_stage(order);
            p[8] = stage(St::Active, "Awaiting your review");
        }
        "capture_pending" => {
            settled_through(p);
            p[8] = stage(St::Active, "Capturing");
        }
        "void_pending" => {
            settled_through(p);
            p[8] = stage(St::Active, "Voiding");
        }
        "captured" => {
            done_through(p, 9);
            p[7] = verification_stage(order);
            p[8] = stage(St::Done, "Captured");
        }
        "voided" => {
            settled_through(p);
            if !has_artifact {
                p[6] = stage(St::Blocked, "No delivery in window");
            }
            p[8] = stage(St::Blocked, "Voided · hold released");
        }
        "reconciliation_required" => {
            if order["authorization_id"].is_string() {
                settled_through(p);
                p[8] = stage(St::Blocked, "Reconciliation required");
            } else {
                p[5] = stage(St::Blocked, "Reconciliation required");
            }
        }
        "" => p[5] = stage(St::Active, "Creating PayPal order"),
        other => p[5] = stage(St::Active, &humanize(other)),
    }
}

fn task_state_chip(state: &str) -> String {
    let (tone, label) = match state {
        "planning" => ("info", "Planning"),
        "negotiating" => ("info", "Negotiating"),
        "awaiting_approval" => ("warn", "Needs approval"),
        "approved" => ("info", "Approved"),
        "purchasing" => ("info", "Purchasing"),
        "no_deal" => ("neutral", "No deal"),
        "cancelled" => ("neutral", "Declined"),
        "failed" => ("bad", "Failed"),
        "captured" => ("good", "Captured"),
        "voided" => ("bad", "Voided · hold released"),
        "declined" => ("bad", "Payment declined"),
        "reconciliation_required" => ("warn", "Reconciliation required"),
        "" => ("neutral", "Unknown"),
        other => return chip("neutral", &humanize(other)),
    };
    chip(tone, label)
}

fn order_state_chip(state: &str) -> String {
    let tone = match state {
        "captured" => "good",
        "voided" | "declined" => "bad",
        "reconciliation_required" | "buyer_review" => "warn",
        "quarantined" => "ai",
        _ => "info",
    };
    let label = match state {
        "approval_pending" => "Awaiting PayPal approval".to_string(),
        "authorized" => "Authorized · funds held".to_string(),
        "quarantined" => "Quarantined · analysing".to_string(),
        "buyer_review" => "Buyer review".to_string(),
        "" => "Unknown".to_string(),
        other => humanize(other),
    };
    chip(tone, &label)
}

fn kv(rows: &[(&str, String)]) -> String {
    let mut h = String::from(r#"<dl class="kv">"#);
    for (k, v) in rows {
        h.push_str(&format!("<div><dt>{}</dt><dd>{v}</dd></div>", esc(k)));
    }
    h.push_str("</dl>");
    h
}

fn or_dash(v: &Value) -> String {
    let t = text(v);
    if t.is_empty() { "—".into() } else { esc(&t) }
}

fn money_or_dash(v: &Value) -> String {
    minor(v).map(usd).unwrap_or_else(|| "—".into())
}

fn section_chips(v: &Value) -> String {
    let items: Vec<String> = v
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| format!(r#"<span class="tag">{}</span>"#, esc(&text(s))))
                .collect()
        })
        .unwrap_or_default();
    if items.is_empty() {
        "—".into()
    } else {
        format!(r#"<span class="tags">{}</span>"#, items.join(""))
    }
}

fn yes_no(v: &Value) -> String {
    match v.as_bool() {
        Some(true) => "Yes".into(),
        Some(false) => "No".into(),
        None => "—".into(),
    }
}

fn seconds_label(v: &Value) -> String {
    match v.as_u64() {
        Some(n) if n >= 60 && n % 60 == 0 => format!("{} min", n / 60),
        Some(n) => format!("{n} s"),
        None => "—".into(),
    }
}

// ───────────────────────────── pipeline step bodies ─────────────────────────────

fn understand(task: &Value) -> String {
    let planner = &task["planner"];
    let mut h = String::new();
    if planner.is_object() {
        let badge = if str_of(&planner["source"]) == "model" {
            let model = text(&planner["model"]);
            chip(
                "ai",
                &format!(
                    "AI planner · {}",
                    if model.is_empty() {
                        "local model"
                    } else {
                        &model
                    }
                ),
            )
        } else {
            let note = text(&planner["note"]);
            chip(
                "neutral",
                &if note.is_empty() {
                    "Rule parser".to_string()
                } else {
                    format!("Rule parser (model unavailable: {note})")
                },
            )
        };
        h.push_str(&format!(r#"<div class="chips">{badge}</div>"#));
    }
    let o = &task["objective"];
    if o.is_object() {
        h.push_str(&kv(&[
            ("Topic", or_dash(&o["topic"])),
            ("Budget ceiling", money_or_dash(&o["budget_minor"])),
            ("Minimum competitors", or_dash(&o["minimum_competitors"])),
            ("Minimum sources", or_dash(&o["minimum_sources"])),
            ("Required sections", section_chips(&o["required_sections"])),
        ]));
        h.push_str(r#"<p class="muted small">Parsed values are clamped by policy before any seller sees them. The budget is never disclosed to sellers.</p>"#);
    } else if str_of(&task["state"]) == "planning" {
        h.push_str(r#"<p class="muted working">Reading your request…</p>"#);
    }
    h
}

fn seller_name<'a>(task: &'a Value, id: &'a str) -> &'a str {
    if id == "buyer-agent" {
        return "Buyer agent";
    }
    task["negotiation"]["offers"]
        .as_array()
        .and_then(|offers| {
            offers
                .iter()
                .find(|o| str_of(&o["seller_id"]) == id)
                .and_then(|o| o["seller"].as_str())
        })
        .unwrap_or(id)
}

fn offer_status_chip(status: &str) -> String {
    match status {
        "agreed" => chip("good", "Agreed"),
        "declined_scope" => chip("neutral", "Declined scope"),
        "no_deal" => chip("bad", "No deal"),
        "" => chip("neutral", "—"),
        other => chip("neutral", &humanize(other)),
    }
}

fn discover(task: &Value) -> String {
    let offers = task["negotiation"]["offers"].as_array();
    match offers {
        Some(offers) if !offers.is_empty() => {
            let mut h = format!(
                r#"<p class="muted small">Requests for quote sent to {} fictional seller agents. Each seller answers within its own scope and price floor.</p><ul class="sellers">"#,
                offers.len()
            );
            for o in offers {
                h.push_str(&format!(
                    r#"<li><span class="seller-avatar" aria-hidden="true">{}</span><div><strong>{}</strong><span class="muted small">List {} · fictional seller</span><span>{}</span></div></li>"#,
                    esc(&text(&o["seller"]).chars().next().unwrap_or('·').to_string()),
                    esc(&text(&o["seller"])),
                    money_or_dash(&o["list_minor"]),
                    offer_status_chip(str_of(&o["status"])),
                ));
            }
            h.push_str("</ul>");
            h
        }
        _ if matches!(str_of(&task["state"]), "planning" | "negotiating") => {
            r#"<p class="muted working">Contacting fictional seller agents…</p>"#.into()
        }
        _ => String::new(),
    }
}

fn negotiate(task: &Value) -> String {
    let n = &task["negotiation"];
    if !n.is_object() {
        return if str_of(&task["state"]) == "negotiating" {
            r#"<p class="muted working">Negotiating within your budget…</p>"#.into()
        } else {
            String::new()
        };
    }
    let mut h = String::new();
    if let Some(msgs) = n["transcript"].as_array().filter(|m| !m.is_empty()) {
        h.push_str(&format!(
            r#"<details class="transcript" open><summary>Negotiation transcript · {} messages</summary><ol class="chat">"#,
            msgs.len()
        ));
        let mut counterparty = String::new();
        for m in msgs {
            let from = str_of(&m["from"]);
            let to = str_of(&m["to"]);
            let buyer = from == "buyer-agent";
            let other = if buyer { to } else { from };
            if other != counterparty {
                counterparty = other.to_string();
                h.push_str(&format!(
                    r#"<li class="chat-divider"><span>{}</span></li>"#,
                    esc(seller_name(task, other))
                ));
            }
            let kind = str_of(&m["kind"]);
            let kind_label = match kind {
                "request_for_quote" => "Request for quote".to_string(),
                "quote" => "Quote".into(),
                "counter" => "Counter-offer".into(),
                "accept" => "Accepted".into(),
                "decline" => "Declined".into(),
                "walk_away" => "Walked away".into(),
                other => humanize(other),
            };
            let amount = minor(&m["amount_minor"])
                .map(|a| format!(r#"<span class="bubble-amount">{}</span>"#, usd(a)))
                .unwrap_or_default();
            h.push_str(&format!(
                r#"<li class="msg {side} kind-{kindc}"><div class="bubble"><div class="bubble-meta"><strong>{who}</strong> · round {round} · {kind_label}</div><p>{body}</p>{amount}</div></li>"#,
                side = if buyer { "buyer" } else { "seller" },
                kindc = esc(&seg(kind)),
                who = esc(seller_name(task, from)),
                round = or_dash(&m["round"]),
                kind_label = esc(&kind_label),
                body = esc(&text(&m["text"])),
            ));
        }
        h.push_str("</ol></details>");
    }
    if let Some(offers) = n["offers"].as_array().filter(|o| !o.is_empty()) {
        let selected = str_of(&n["selected"]);
        h.push_str(r#"<div class="table-wrap"><table class="offers"><thead><tr><th scope="col">Seller</th><th scope="col">List</th><th scope="col">Agreed</th><th scope="col">Status</th><th scope="col">Reason</th></tr></thead><tbody>"#);
        for o in offers {
            let is_selected = !selected.is_empty() && str_of(&o["seller_id"]) == selected;
            h.push_str(&format!(
                r#"<tr{cls}><td><strong>{seller}</strong>{sel}</td><td class="num">{list}</td><td class="num">{agreed}</td><td>{status}</td><td class="muted">{reason}</td></tr>"#,
                cls = if is_selected { r#" class="selected""# } else { "" },
                seller = esc(&text(&o["seller"])),
                sel = if is_selected { r#" <span class="tag tag-mint">Selected</span>"# } else { "" },
                list = money_or_dash(&o["list_minor"]),
                agreed = money_or_dash(&o["agreed_minor"]),
                status = offer_status_chip(str_of(&o["status"])),
                reason = esc(&text(&o["reason"])),
            ));
        }
        h.push_str("</tbody></table></div>");
    }
    let rationale = text(&n["rationale"]);
    if !rationale.is_empty() {
        h.push_str(&format!(
            r#"<div class="rationale"><span class="eyebrow">AGENT RATIONALE</span><p>{}</p></div>"#,
            esc(&rationale)
        ));
    }
    h
}

fn contract(task: &Value) -> String {
    let c = &task["proposal"];
    if !c.is_object() {
        return String::new();
    }
    let mut h = format!(
        r#"<div class="contract-card"><div class="contract-head"><div><span class="eyebrow">CANONICAL PURCHASE CONTRACT · v{}</span><h4>{}</h4><p class="muted small">{} · fictional seller</p></div><div class="contract-price">{}</div></div>"#,
        or_dash(&c["version"]),
        esc(&text(&c["item"])),
        esc(&text(&c["seller"])),
        money_or_dash(&c["amount_minor"]),
    );
    h.push_str(&kv(&[
        ("Topic", or_dash(&c["topic"])),
        (
            "Price",
            format!(
                "{} {}",
                money_or_dash(&c["amount_minor"]),
                esc(&text(&c["currency"]))
            ),
        ),
        ("Budget ceiling", money_or_dash(&c["budget_minor"])),
        ("Delivery window", seconds_label(&c["deadline_seconds"])),
        ("Required sections", section_chips(&c["required_sections"])),
        ("Minimum competitors", or_dash(&c["minimum_competitors"])),
        ("Minimum sources", or_dash(&c["minimum_sources"])),
        (
            "HTTPS sources required",
            yes_no(&c["require_https_sources"]),
        ),
        ("Buyer review required", yes_no(&c["buyer_review_required"])),
        ("Seller ID", or_dash(&c["seller_id"])),
        ("Kind", or_dash(&c["kind"])),
    ]));
    let negotiation_digest = text(&c["negotiation_digest"]);
    if !negotiation_digest.is_empty() {
        h.push_str(&format!(
            r#"<div class="digest secondary-digest"><span>Negotiation transcript SHA-256</span><code>{}</code></div>"#,
            esc(&negotiation_digest)
        ));
    }
    let digest = text(&task["proposal_digest"]);
    if !digest.is_empty() {
        h.push_str(&format!(
            r#"<div class="digest"><span>Contract SHA-256 · the exact value you sign</span><code>{}</code></div>"#,
            esc(&digest)
        ));
    }
    h.push_str("</div>");
    h
}

fn approval(task: &Value, replay: bool) -> String {
    let id = seg(str_of(&task["id"]));
    let digest = text(&task["proposal_digest"]);
    let amount = minor(&task["proposal"]["amount_minor"]);
    let seller = text(&task["proposal"]["seller"]);
    match str_of(&task["state"]) {
        "awaiting_approval" => {
            let mut h = format!(
                r#"<p>Approving signs an Ed25519 intent bound to this contract digest. The gateway will only authorize exactly <strong>{}</strong> for <strong>{}</strong>'s contract, payable to the allowlisted PayPal Sandbox merchant (the seller is fictional). Authorization is a hold, not a payment; any change to the contract changes its digest and is refused.</p>"#,
                amount.map(usd).unwrap_or_else(|| "the agreed amount".into()),
                if seller.is_empty() { "the selected seller".to_string() } else { esc(&seller) },
            );
            h.push_str(r#"<div class="decision">"#);
            if digest.is_empty() {
                h.push_str(r#"<p class="notice warn">The contract digest is missing, so this proposal cannot be approved.</p>"#);
            } else {
                h.push_str(&format!(
                    r#"<form method="post" action="/ui/agent/tasks/{id}/approve" class="approve-form"><input type="hidden" name="proposal_digest" value="{}"/>"#,
                    esc(&digest)
                ));
                if replay {
                    h.push_str(r#"<label class="field">Simulated payment outcome<select name="outcome"><option value="approve">Normal authorization and settlement</option><option value="decline">Authorization declined</option><option value="ambiguous">Unknown capture result (requires reconciliation)</option></select></label>"#);
                }
                h.push_str(&format!(
                    r#"<button class="primary-lg">Approve exact contract &amp; authorize {}</button></form>"#,
                    esc(&amount.map(usd_plain).unwrap_or_else(|| "agreed amount".into()))
                ));
            }
            h.push_str(&format!(
                r#"<form method="post" action="/ui/agent/tasks/{id}/cancel"><button class="secondary">Decline</button></form></div>"#
            ));
            h
        }
        "cancelled" => r#"<p class="muted">You declined this contract. No payment intent was signed and no PayPal order was created.</p>"#.into(),
        "approved" | "purchasing" => format!(
            r#"<p class="muted">Approved. A signed intent binds the payment to contract digest <code class="inline-digest">{}</code>.</p>"#,
            esc(&truncate(&digest, 20))
        ),
        "failed" if !task["order"].is_object() && !task["proposal"].is_null() => String::new(),
        _ if task["order"].is_object() => format!(
            r#"<p class="muted">Approved. A signed intent binds the payment to contract digest <code class="inline-digest">{}</code>.</p>"#,
            esc(&truncate(&digest, 20))
        ),
        _ => String::new(),
    }
}

fn order_is_replay(order: &Value, replay: bool) -> bool {
    match str_of(&order["mode"]) {
        "" => replay,
        m => m.eq_ignore_ascii_case("replay"),
    }
}

fn provider_label(order: &Value, replay: bool) -> String {
    let state = str_of(&order["state"]);
    if order_is_replay(order, replay) {
        return chip("neutral", "Simulated — not provider-confirmed");
    }
    match state {
        "authorized" | "quarantined" | "buyer_review" | "captured" | "voided" => {
            chip("good", "PayPal Sandbox · provider-confirmed")
        }
        "approval_pending" => chip("info", "PayPal Sandbox · awaiting your approval at PayPal"),
        "declined" => chip("bad", "PayPal Sandbox · authorization not granted"),
        "reconciliation_required" => chip("warn", "PayPal Sandbox · outcome not confirmed"),
        _ => chip("info", "PayPal Sandbox · awaiting provider confirmation"),
    }
}

fn order_amount(task: &Value) -> Option<u64> {
    let order = &task["order"];
    minor(&order["intent"]["intent"]["amount_minor"])
        .or_else(|| minor(&order["amount_minor"]))
        .or_else(|| minor(&task["proposal"]["amount_minor"]))
}

fn payment(task: &Value, replay: bool) -> String {
    let order = &task["order"];
    if !order.is_object() {
        return if matches!(str_of(&task["state"]), "approved" | "purchasing") {
            r#"<p class="muted working">Creating the authorization order…</p>"#.into()
        } else {
            String::new()
        };
    }
    let state = str_of(&order["state"]);
    let amount = order_amount(task);
    let held = matches!(
        state,
        "authorized"
            | "quarantined"
            | "buyer_review"
            | "capture_pending"
            | "void_pending"
            | "captured"
            | "voided"
    ) || (state == "reconciliation_required" && order["authorization_id"].is_string());
    let (auth_value, auth_note) = if held {
        (
            amount.map(usd).unwrap_or_else(|| "—".into()),
            match state {
                "voided" => "Hold released · buyer not charged",
                "captured" => "Converted to capture",
                _ => "Held, not yet paid to seller",
            },
        )
    } else if state == "declined" {
        ("—".into(), "Authorization declined")
    } else {
        ("—".into(), "Not yet authorized")
    };
    let (cap_value, cap_note) = if state == "captured" {
        (
            amount.map(usd).unwrap_or_else(|| "—".into()),
            "Paid after verified delivery",
        )
    } else {
        ("—".into(), "Nothing captured")
    };
    let id = seg(str_of(&order["id"]));
    let mut h = format!(
        r#"<div class="payment"><div class="money"><span>Authorized</span><strong>{}</strong><small>{}</small></div><div class="money{}"><span>Captured</span><strong>{}</strong><small>{}</small></div></div>"#,
        esc(&auth_value),
        auth_note,
        if state == "captured" { " captured" } else { "" },
        esc(&cap_value),
        cap_note,
    );
    h.push_str(&format!(
        r#"<div class="chips">{}{}</div>"#,
        order_state_chip(state),
        provider_label(order, replay)
    ));
    h.push_str(&kv(&[
        (
            "PayPal order ID",
            format!(r#"<code>{}</code>"#, or_dash(&order["paypal_order"])),
        ),
        (
            "Authorization ID",
            format!(r#"<code>{}</code>"#, or_dash(&order["authorization_id"])),
        ),
        (
            "QuarantinePay order",
            format!(r#"<code>{}</code>"#, or_dash(&order["id"])),
        ),
    ]));
    if state == "approval_pending" {
        h.push_str(r#"<div class="actions">"#);
        if order_is_replay(order, replay) {
            h.push_str(&format!(
                r#"<form method="post" action="/ui/orders/{id}/authorize"><button>Authorize simulated payment</button></form>"#
            ));
        } else {
            let url = text(&order["approval_url"]);
            if !url.is_empty() && (url.starts_with("https://") || url.starts_with("http://")) {
                h.push_str(&format!(
                    r#"<a class="button" href="{}" target="_blank" rel="noopener noreferrer">Approve at PayPal Sandbox ↗</a>"#,
                    esc(&url)
                ));
            }
            h.push_str(&format!(
                r#"<form method="post" action="/ui/orders/{id}/authorize"><button class="secondary">I approved — authorize</button></form>"#
            ));
        }
        h.push_str("</div>");
    }
    h
}

fn report_content(artifact: &Value) -> String {
    match &artifact["content"] {
        Value::String(s) => s.clone(),
        _ => serde_json::to_string_pretty(artifact).unwrap_or_default(),
    }
}

fn delivery(task: &Value) -> String {
    let order = &task["order"];
    let state = str_of(&order["state"]);
    let mut h = String::new();
    if state == "authorized" {
        let id = seg(str_of(&order["id"]));
        let deadline = text(&order["deadline"]);
        h.push_str(r#"<div class="simulator"><div class="sim-head"><strong>Seller simulator</strong><span class="badge">FICTIONAL SELLER · SIMULATED DELIVERY</span></div><p class="muted small">Choose what the fictional seller delivers. Every delivery goes through the same contract checks and analyst.</p>"#);
        if !deadline.is_empty() {
            h.push_str(&format!(
                r#"<p class="countdown">Delivery window: <time data-deadline="{d}">{d}</time></p>"#,
                d = esc(&deadline)
            ));
        }
        h.push_str(&format!(
            concat!(
                r#"<div class="actions">"#,
                r#"<form method="post" action="/ui/orders/{id}/deliveries"><input type="hidden" name="fixture" value="pass"/><button>Deliver compliant report</button></form>"#,
                r#"<form method="post" action="/ui/orders/{id}/deliveries"><input type="hidden" name="fixture" value="fail"/><button class="secondary">Deliver incomplete report</button></form>"#,
                r#"<form method="post" action="/ui/orders/{id}/deliveries"><input type="hidden" name="fixture" value="malicious"/><button class="danger">Deliver report with injected instructions</button></form>"#,
                "</div></div>"
            ),
            id = id
        ));
    }
    let artifact = &order["artifact"];
    if !artifact.is_null() {
        let content = report_content(artifact);
        h.push_str(&format!(
            r#"<details class="report"><summary>Delivered report · {} characters · treated as untrusted data</summary><pre class="evidence">{}</pre></details>"#,
            content.chars().count(),
            esc(&content)
        ));
    }
    h
}

fn analyst_card(order: &Value) -> String {
    let a = &order["analysis"];
    if !a.is_object() {
        return String::new();
    }
    let risk = str_of(&a["risk"]);
    let risk_tone = match risk {
        "low" => "good",
        "medium" => "warn",
        "high" => "bad",
        _ => "neutral",
    };
    let analyst = &a["analyst"];
    let source = if str_of(&analyst["source"]) == "model" {
        let model = text(&analyst["model"]);
        chip(
            "ai",
            &format!(
                "Local model · {}",
                if model.is_empty() { "unnamed" } else { &model }
            ),
        )
    } else {
        let err = text(&analyst["error"]);
        chip(
            "neutral",
            &if err.is_empty() {
                "Rule engine".to_string()
            } else {
                format!("Rule engine (model unavailable: {})", truncate(&err, 80))
            },
        )
    };
    let mut h = format!(
        r#"<div class="analyst-card risk-{rc}"><div class="analyst-head"><div><span class="eyebrow">AI DELIVERY ANALYST · ADVISORY ONLY</span><h4>Delivery analysis</h4></div><div class="chips">{risk}{source}</div></div>"#,
        rc = esc(&seg(risk)),
        risk = chip(
            risk_tone,
            &format!("Risk: {}", if risk.is_empty() { "unknown" } else { risk })
        ),
    );
    if a["injection_detected"].as_bool() == Some(true) {
        h.push_str(&format!(
            r#"<div class="banner danger-banner" role="alert"><strong>⚠</strong><p>{INJECTION_BANNER}</p></div>"#
        ));
    }
    if a["policy_hold"].as_bool() == Some(true) {
        h.push_str(r#"<div class="banner warn-banner"><strong>⏸</strong><p>Policy hold — capture is blocked unless you explicitly override it. Overrides are recorded in the audit log.</p></div>"#);
    }
    let summary = text(&a["summary"]);
    if !summary.is_empty() {
        h.push_str(&format!(r#"<p class="summary">{}</p>"#, esc(&summary)));
    }
    let action = str_of(&a["recommended_action"]);
    let action_chip = match action {
        "approve" => chip("good", "Approve"),
        "reject" => chip("bad", "Reject"),
        "escalate" => chip("warn", "Escalate to buyer"),
        "" => "—".into(),
        other => chip("neutral", &humanize(other)),
    };
    h.push_str(&kv(&[
        ("Relevance", or_dash(&a["relevance"])),
        ("Recommended action", action_chip),
        (
            "Evidence SHA-256",
            format!("<code>{}</code>", or_dash(&a["evidence_sha256"])),
        ),
    ]));
    if let Some(concerns) = a["quality_concerns"].as_array().filter(|c| !c.is_empty()) {
        h.push_str(r#"<div class="sub"><span class="eyebrow">QUALITY CONCERNS</span><ul class="concerns">"#);
        for c in concerns {
            h.push_str(&format!("<li>{}</li>", esc(&text(c))));
        }
        h.push_str("</ul></div>");
    }
    if let Some(findings) = a["findings"].as_array().filter(|f| !f.is_empty()) {
        h.push_str(
            r#"<div class="sub"><span class="eyebrow">FINDINGS</span><ul class="findings">"#,
        );
        for f in findings {
            let src = str_of(&f["source"]);
            h.push_str(&format!(
                r#"<li><div class="finding-head">{}<strong>{}</strong></div><code class="excerpt">{}</code></li>"#,
                if src == "model" { chip("ai", "model") } else { chip("neutral", "rules") },
                esc(&humanize(&text(&f["rule"]))),
                esc(&text(&f["excerpt"])),
            ));
        }
        h.push_str("</ul></div>");
    }
    h.push_str(r#"<p class="authority"><strong>Model output has no payment authority.</strong> The analyst can only add caution; capture requires passing contract checks and your decision.</p></div>"#);
    h
}

fn verification(task: &Value) -> String {
    let order = &task["order"];
    let state = str_of(&order["state"]);
    let mut h = String::new();
    if state == "quarantined" {
        h.push_str(r#"<p class="working analysing"><span class="spinner" aria-hidden="true"></span>Analysing delivery… Contract checks and the AI analyst run while the funds stay on hold.</p>"#);
    }
    let v = &order["verification"];
    if v.is_object() {
        let checks = v["checks"].as_array().cloned().unwrap_or_default();
        let passed = checks
            .iter()
            .filter(|c| c["passed"].as_bool() == Some(true))
            .count();
        let ok = v["passed"].as_bool() == Some(true);
        h.push_str(&format!(
            r#"<div class="verdict-head">{}<strong>{passed} / {} contract checks passed</strong><span class="muted small">{} competitors · {} sources found</span></div>"#,
            if ok { chip("good", "Structural checks passed") } else { chip("bad", "Structural checks failed") },
            checks.len(),
            or_dash(&v["competitors"]),
            or_dash(&v["sources"]),
        ));
        let reason = text(&v["reason"]);
        if !reason.is_empty() {
            h.push_str(&format!(r#"<p class="muted">{}</p>"#, esc(&reason)));
        }
        if !checks.is_empty() {
            h.push_str(r#"<ul class="checklist">"#);
            for c in &checks {
                let ok = c["passed"].as_bool() == Some(true);
                h.push_str(&format!(
                    r#"<li class="{}"><span class="check-icon" aria-label="{}">{}</span><div><strong>{}</strong><span class="muted small">{}</span></div></li>"#,
                    if ok { "ok" } else { "fail" },
                    if ok { "passed" } else { "failed" },
                    if ok { "✓" } else { "✗" },
                    esc(&humanize(&text(&c["name"]))),
                    esc(&text(&c["detail"])),
                ));
            }
            h.push_str("</ul>");
        }
        if let Some(found) = v["untrusted_instructions"]
            .as_array()
            .filter(|f| !f.is_empty())
        {
            h.push_str(r#"<div class="sub"><span class="eyebrow">UNTRUSTED INSTRUCTIONS FOUND BY RULES</span><ul class="findings">"#);
            for f in found {
                h.push_str(&format!(
                    r#"<li><div class="finding-head">{}<strong>{}</strong></div><code class="excerpt">{}</code></li>"#,
                    chip("neutral", "rules"),
                    esc(&humanize(&text(&f["rule"]))),
                    esc(&text(&f["excerpt"])),
                ));
            }
            h.push_str("</ul></div>");
        }
        let evidence = text(&v["evidence_sha256"]);
        if !evidence.is_empty() {
            h.push_str(&format!(
                r#"<div class="digest secondary-digest"><span>Delivered evidence SHA-256</span><code>{}</code></div>"#,
                esc(&evidence)
            ));
        }
    }
    h.push_str(&analyst_card(order));
    h
}

fn settlement(task: &Value) -> String {
    let order = &task["order"];
    let state = str_of(&order["state"]);
    let id = seg(str_of(&order["id"]));
    let amount = order_amount(task)
        .map(usd_plain)
        .unwrap_or_else(|| "agreed amount".into());
    let mut h = String::new();
    match state {
        "buyer_review" => {
            let action = format!("/ui/orders/{id}/review");
            if policy_hold(order) {
                h.push_str(&format!(
                    concat!(
                        r#"<p>The delivery is on a <strong>policy hold</strong>. Rejecting voids the authorization and releases the buyer’s funds — this is the recommended action.</p><div class="review-actions">"#,
                        r#"<form method="post" action="{a}"><input type="hidden" name="decision" value="reject"/><button class="primary-lg">Reject delivery &amp; void hold (recommended)</button></form>"#,
                        r#"<form method="post" action="{a}" class="override-form"><input type="hidden" name="decision" value="approve"/>"#,
                        r#"<label class="check"><input type="checkbox" name="acknowledge_risk" value="true" required/><span>I reviewed the flagged content and override the policy hold</span></label>"#,
                        r#"<button class="danger">Override hold &amp; capture {amt}</button></form></div>"#
                    ),
                    a = action,
                    amt = esc(&amount)
                ));
            } else {
                h.push_str(&format!(
                    concat!(
                        r#"<p>Review the delivered report and verdict. Approving captures the held funds; rejecting voids the authorization.</p><div class="review-actions">"#,
                        r#"<form method="post" action="{a}"><input type="hidden" name="decision" value="approve"/><button class="primary-lg">Approve delivery &amp; capture {amt}</button></form>"#,
                        r#"<form method="post" action="{a}"><input type="hidden" name="decision" value="reject"/><button class="danger">Reject delivery &amp; void hold</button></form></div>"#
                    ),
                    a = action,
                    amt = esc(&amount)
                ));
            }
        }
        "captured" => h.push_str(&format!(
            r#"<div class="outcome good"><strong>Captured {}</strong><p>Funds were paid to the fictional seller after the delivery was verified and approved.</p></div>"#,
            esc(&amount)
        )),
        "voided" => h.push_str(r#"<div class="outcome bad"><strong>Authorization voided</strong><p>The hold was released. The buyer was not charged.</p></div>"#),
        "reconciliation_required" => h.push_str(r#"<div class="outcome warn"><strong>Reconciliation required</strong><p>Provider outcome uncertain or permission refused — reconciliation required; no success is claimed.</p></div>"#),
        "declined" => h.push_str(r#"<div class="outcome bad"><strong>Authorization declined</strong><p>No funds were held and nothing will be captured.</p></div>"#),
        "capture_pending" => h.push_str(r#"<p class="working">Capture requested — waiting for the provider to confirm.</p>"#),
        "void_pending" => h.push_str(r#"<p class="working">Void requested — waiting for the provider to confirm.</p>"#),
        _ => {}
    }
    let decision = text(&order["review_decision"]);
    let override_used = order["risk_override"].as_bool() == Some(true);
    if !decision.is_empty() || override_used {
        h.push_str(r#"<div class="chips">"#);
        if !decision.is_empty() {
            h.push_str(&chip(
                "neutral",
                &format!("Buyer decision: {}", humanize(&decision)),
            ));
        }
        if override_used {
            h.push_str(&chip("warn", "Policy hold overridden by buyer · audited"));
        }
        h.push_str("</div>");
    }
    if !order["provider_result"].is_null() && state == "reconciliation_required" {
        h.push_str(&format!(
            r#"<details class="report"><summary>Provider response</summary><pre class="evidence">{}</pre></details>"#,
            esc(&serde_json::to_string_pretty(&order["provider_result"]).unwrap_or_default())
        ));
    }
    if matches!(str_of(&task["state"]), "no_deal" | "cancelled") {
        h.push_str(r#"<p class="muted">Nothing was purchased.</p>"#);
    }
    h
}

const STEP_TITLES: [&str; 9] = [
    "Understand",
    "Discover sellers",
    "Negotiate",
    "Contract",
    "Buyer approval",
    "PayPal authorization",
    "Delivery",
    "Verification & AI analysis",
    "Settlement",
];

fn pipeline(task: &Value, replay: bool) -> String {
    let stages = progress(task);
    let bodies = [
        understand(task),
        discover(task),
        negotiate(task),
        contract(task),
        approval(task, replay),
        payment(task, replay),
        delivery(task),
        verification(task),
        settlement(task),
    ];
    let mut h = String::from(r#"<ol class="pipeline">"#);
    for (i, ((title, st), body)) in STEP_TITLES.iter().zip(&stages).zip(bodies).enumerate() {
        let marker = match st.status {
            St::Done => "✓".to_string(),
            St::Blocked => "!".to_string(),
            _ => (i + 1).to_string(),
        };
        let current = if st.status == St::Active {
            r#" aria-current="step""#
        } else {
            ""
        };
        let tone = match st.status {
            St::Done => "good",
            St::Active => "info",
            St::Blocked => "warn",
            St::Pending => "neutral",
        };
        h.push_str(&format!(
            r#"<li class="step {cls}"{current}><span class="step-marker" aria-hidden="true">{marker}</span><div class="step-body"><div class="step-head"><h3>{title}</h3>{chip}</div>{body}</div></li>"#,
            cls = st.status.class(),
            chip = chip(tone, &st.label),
        ));
    }
    h.push_str("</ol>");
    h
}

fn task_detail(task: &Value, replay: bool) -> String {
    let error = text(&task["error"]);
    let mut h = format!(
        r#"<section class="panel task-head"><div class="task-head-top"><span class="eyebrow">BUYER REQUEST</span>{}</div><blockquote class="request-quote">{}</blockquote><p class="muted small task-meta"><code>{}</code>{}{}</p>"#,
        task_state_chip(str_of(&task["state"])),
        esc(&text(&task["request"])),
        or_dash(&task["id"]),
        if task["created_at"].is_null() {
            String::new()
        } else {
            format!(" · created {}", time_html(&task["created_at"]))
        },
        if task["updated_at"].is_null() {
            String::new()
        } else {
            format!(" · updated {}", time_html(&task["updated_at"]))
        },
    );
    if !error.is_empty() {
        h.push_str(&format!(
            r#"<div class="notice warn" role="alert">{}</div>"#,
            esc(&error)
        ));
    }
    h.push_str("</section>");
    h.push_str(&pipeline(task, replay));
    h
}

fn task_list(tasks: &[Value], selected_id: &str) -> String {
    let mut h = String::from(
        r#"<section class="panel task-list"><div class="section-head"><h2>Recent tasks</h2><a class="muted small" href="/agent">New ↻</a></div>"#,
    );
    if tasks.is_empty() {
        h.push_str(
            r#"<p class="muted small">No agent tasks yet. Run the buyer agent to start one.</p>"#,
        );
    } else {
        h.push_str("<ul>");
        for t in tasks {
            let id = str_of(&t["id"]);
            let active = !id.is_empty() && id == selected_id;
            let mut meta: Vec<String> = Vec::new();
            let seller = text(&t["seller"]);
            if !seller.is_empty() {
                meta.push(esc(&seller));
            }
            if let Some(a) = minor(&t["amount_minor"]) {
                meta.push(usd(a));
            }
            h.push_str(&format!(
                r#"<li><a class="task-item{}" href="/agent/{}"{}><div class="task-item-top">{}{}</div><p>{}</p>{}</a></li>"#,
                if active { " active" } else { "" },
                esc(&seg(id)),
                if active { r#" aria-current="true""# } else { "" },
                task_state_chip(str_of(&t["state"])),
                time_html(&t["created_at"]),
                esc(&truncate(&text(&t["request"]), 110)),
                if meta.is_empty() { String::new() } else { format!(r#"<small>{}</small>"#, meta.join(" · ")) },
            ));
        }
        h.push_str("</ul>");
    }
    h.push_str("</section>");
    h
}

fn event_label(kind: &str) -> String {
    let known = match kind {
        "agent_task_created" | "agent_created" | "agent_request" => "Buyer request received",
        "agent_planned" | "agent_plan" | "agent_objective" => "Objective understood",
        "agent_negotiated" | "agent_negotiation" | "agent_negotiation_completed" => {
            "Negotiation completed"
        }
        "agent_proposed" | "agent_contract_proposed" | "agent_proposal" => "Contract proposed",
        "agent_approved" | "agent_contract_approved" | "agent_buyer_approved" => {
            "Contract approved by buyer · signed intent issued"
        }
        "agent_discovery" => "Seller agents contacted",
        "ai_analysis_started" => "AI analyst reviewing delivery",
        "ai_analysis_complete" => "AI analyst finished",
        "policy_hold" => "Policy hold · capture blocked",
        "seller_delivery" => "Seller delivered (simulated)",
        "intent_bound" => "Signed intent bound to order",
        "verification_complete" => "Deterministic contract checks finished",
        "buyer_review_required" => "Buyer review required",
        "buyer_review" => "Buyer decision recorded",
        "provider_confirmed" => "PayPal Sandbox confirmed",
        "simulated_payment_result" => "Simulated payment step (not provider-confirmed)",
        "provider_permission_required" => "PayPal refused · reconciliation required",
        "provider_refused" => "PayPal refused the operation · reconciliation required",
        "provider_state_conflict" => {
            "PayPal state forbids this operation · reconciliation required"
        }
        "reconciliation_required" => "Reconciliation required",
        "agent_cancelled" | "agent_task_cancelled" | "agent_declined" => {
            "Contract declined by buyer"
        }
        "agent_no_deal" => "No deal within budget",
        "agent_failed" | "agent_task_failed" => "Agent task failed",
        "delivery_analyzed" | "analysis_completed" | "agent_analysis" => "AI analyst finished",
        "risk_override" | "policy_override" => "Policy hold overridden by buyer",
        _ => "",
    };
    if !known.is_empty() {
        return known.into();
    }
    match kind.strip_prefix("agent_") {
        Some(rest) => format!("Agent · {}", humanize(rest)),
        None if kind.is_empty() => "Event".into(),
        None => humanize(kind),
    }
}

fn event_tone(kind: &str) -> &'static str {
    let k = kind.to_ascii_lowercase();
    if [
        "fail", "reject", "void", "inject", "override", "reconcil", "declin", "red",
    ]
    .iter()
    .any(|w| k.contains(w))
    {
        "warn"
    } else if k.starts_with("agent") || k.contains("analy") {
        "ai"
    } else {
        "good"
    }
}

fn event_detail(v: &Value) -> String {
    let s = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Object(map) => map
            .iter()
            .filter(|(_, v)| !v.is_object() && !v.is_array() && !v.is_null())
            .take(5)
            .map(|(k, v)| match (k.strip_suffix("_minor"), minor(v)) {
                (Some(base), Some(m)) => format!("{}: {}", humanize(base), usd(m)),
                _ => format!("{}: {}", humanize(k), text(v)),
            })
            .collect::<Vec<_>>()
            .join(" · "),
        other => other.to_string(),
    };
    truncate(&s, 220)
}

fn activity(events: &[Value]) -> String {
    let mut h = String::from(
        r#"<section class="panel"><div class="section-head"><h2>Agent activity</h2><span class="badge">AUDIT LOG</span></div>"#,
    );
    if events.is_empty() {
        h.push_str(r#"<p class="muted small">No activity yet.</p>"#);
    } else {
        h.push_str(r#"<ul class="audit feed">"#);
        for e in events {
            let kind = str_of(&e["kind"]);
            let detail = event_detail(&e["detail"]);
            h.push_str(&format!(
                r#"<li><span class="dot tone-{}"></span><div><div class="feed-head"><strong>{}</strong>{}</div>{}<span class="mono muted">{}</span></div></li>"#,
                event_tone(kind),
                esc(&event_label(kind)),
                time_html(&e["at"]),
                if detail.is_empty() { String::new() } else { format!(r#"<p class="muted small">{}</p>"#, esc(&detail)) },
                esc(kind),
            ));
        }
        h.push_str("</ul>");
    }
    h.push_str("</section>");
    h
}

fn request_box() -> String {
    format!(
        concat!(
            r#"<section class="panel request-box"><form method="post" action="/ui/agent/tasks">"#,
            r#"<label for="agent-request"><span class="eyebrow">NEW BUYER TASK</span><h2>What should the buyer agent buy?</h2></label>"#,
            r#"<textarea id="agent-request" name="request" maxlength="2000" rows="3" required>{}</textarea>"#,
            r#"<div class="request-foot"><p class="muted small">Budget and scope are extracted, then clamped by policy. Sellers are fictional and deliveries are simulated. Nothing is paid until you approve an exact contract and the delivery is verified.</p>"#,
            r#"<button class="primary-lg">Run buyer agent</button></div></form></section>"#
        ),
        esc(EXAMPLE_REQUEST)
    )
}

fn empty_detail() -> String {
    r#"<section class="panel empty-state"><span class="eyebrow">NO TASK SELECTED</span><h2>Bounded authority, end to end</h2><ol class="how"><li><strong>You describe</strong> what you need and your budget.</li><li><strong>The agent negotiates</strong> with three fictional sellers and compiles a canonical contract.</li><li><strong>You approve</strong> the exact contract digest; PayPal places an authorization hold.</li><li><strong>The delivery is verified</strong> by contract checks and an advisory AI analyst before anything is captured.</li></ol></section>"#.into()
}

pub fn render_agent(
    mode: String,
    inference_label: String,
    tasks: Vec<Value>,
    selected: Option<Value>,
    events: Vec<Value>,
    logged_in: bool,
    message: String,
) -> String {
    let replay = mode.eq_ignore_ascii_case("replay");
    let event_id = events
        .iter()
        .filter_map(|e| e["id"].as_i64())
        .max()
        .unwrap_or(0);
    let selected = selected.filter(Value::is_object);
    let selected_id = selected
        .as_ref()
        .map(|t| str_of(&t["id"]).to_string())
        .unwrap_or_default();
    let detail = match &selected {
        Some(task) => task_detail(task, replay),
        None => empty_detail(),
    };
    let content = format!(
        r#"{}<div class="agent-layout"><div class="task-detail">{}</div><div class="agent-side">{}{}</div></div>"#,
        request_box(),
        detail,
        task_list(&tasks, &selected_id),
        activity(&events),
    );
    shell(Shell {
        title: "Agent console",
        eyebrow: "AGENTIC COMMERCE · BOUNDED AUTHORITY",
        heading: "Agent console",
        subtitle: "Describe what you need. A buyer agent negotiates with fictional sellers, you approve the exact contract, and PayPal holds the funds until the delivery is verified.",
        page: Page::Agent,
        mode: &mode,
        inference_label: &inference_label,
        logged_in,
        message: &message,
        body_attrs: format!(r#" data-event-id="{event_id}" data-auth="{logged_in}""#),
        head: r#"<script type="module" src="/assets/loader.js"></script>"#.into(),
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarantinepay_core::contract::{
        compile_contract, contract_fixture, negotiate, parse_objective, verify_contract_delivery,
    };
    use serde_json::json;

    fn page(mode: &str, task: Value) -> String {
        render_agent(
            mode.into(),
            "Local inference".into(),
            vec![
                json!({"id": task["id"], "request": task["request"], "state": task["state"], "created_at": "2026-11-09T10:00:00Z", "amount_minor": 5340, "seller": "Alder Insights"}),
            ],
            Some(task),
            vec![
                json!({"id": 7, "kind": "agent_contract_proposed", "detail": {"seller": "Alder Insights"}, "at": "2026-11-09T10:00:02Z"}),
                json!({"id": 9, "kind": "authorized", "detail": "hold placed", "at": "2026-11-09T10:00:05Z", "order_id": "order-1"}),
            ],
            true,
            String::new(),
        )
    }

    fn proposed_task() -> Value {
        let objective = parse_objective(EXAMPLE_REQUEST);
        let negotiation = negotiate(&objective);
        let contract = compile_contract(&objective, &negotiation).expect("contract");
        json!({
            "id": "task-1",
            "request": EXAMPLE_REQUEST,
            "state": "awaiting_approval",
            "planner": {"source": "rules", "note": "connection refused"},
            "objective": objective,
            "negotiation": negotiation,
            "proposal_digest": contract.digest(),
            "proposal": contract,
            "order_id": null,
            "error": null,
            "created_at": "2026-11-09T10:00:00Z",
            "updated_at": "2026-11-09T10:00:01Z",
            "order": null
        })
    }

    #[test]
    fn awaiting_approval_shows_contract_digest_and_approval_forms() {
        let task = proposed_task();
        let digest = task["proposal_digest"].as_str().unwrap().to_string();
        let amount = task["proposal"]["amount_minor"].as_u64().unwrap();
        let html = page("SANDBOX", task);
        assert!(html.contains(r#"action="/ui/agent/tasks/task-1/approve""#));
        assert!(html.contains(r#"action="/ui/agent/tasks/task-1/cancel""#));
        assert!(html.contains(&format!(r#"name="proposal_digest" value="{digest}""#)));
        assert!(html.contains(&format!(
            "Approve exact contract &amp; authorize {}",
            usd_plain(amount)
        )));
        assert!(html.contains("Rule parser (model unavailable: connection refused)"));
        for seller in [
            "Northstar Research",
            "Alder Insights",
            "Quillfire Analytics",
        ] {
            assert!(html.contains(seller), "missing {seller}");
        }
        assert!(html.contains(r#"class="msg buyer"#) && html.contains(r#"class="msg seller"#));
        assert!(html.contains(r#"name="request" maxlength="2000""#));
        assert!(html.contains(r#"action="/ui/agent/tasks""#));
        assert!(html.contains(r#"data-event-id="9""#) && html.contains(r#"data-auth="true""#));
        assert!(html.contains("/assets/loader.js"));
        assert!(html.contains(r#"href="/agent/task-1""#));
        assert!(html.contains("Contract proposed"));
        assert!(html.contains(r#"class="step active" aria-current="step""#));
        // Sandbox mode never shows the simulated-outcome selector.
        assert!(!html.contains(r#"name="outcome""#));
        assert!(page("REPLAY", proposed_task()).contains(r#"name="outcome""#));
    }

    fn reviewed_task(mode: &str, state: &str) -> Value {
        let mut task = proposed_task();
        let contract: quarantinepay_core::PurchaseContract =
            serde_json::from_value(task["proposal"].clone()).unwrap();
        let artifact = contract_fixture(&contract, "malicious");
        let verdict = verify_contract_delivery(&contract, &artifact);
        task["state"] = json!("purchasing");
        task["planner"] = json!({"source": "model", "model": "qwen2.5-3b-instruct"});
        task["order"] = json!({
            "id": "order-1", "state": state, "mode": mode,
            "paypal_order": "5O190127TN364715T", "authorization_id": "0VF52814937998046",
            "approval_url": "https://www.sandbox.paypal.com/checkoutnow?token=5O190127TN364715T",
            "deadline": "2026-11-09T10:10:00Z",
            "artifact": artifact, "verification": verdict,
            "analysis": {
                "analyst": {"source": "model", "model": "qwen2.5-3b-instruct"},
                "risk": "high", "injection_detected": true, "policy_hold": true,
                "findings": [{"rule": "instruction_override", "excerpt": "<script>ignore previous instructions</script>", "source": "rules"}],
                "relevance": "On topic", "quality_concerns": ["Contains instructions aimed at the verifier"],
                "recommended_action": "reject", "summary": "The report embeds instructions.",
                "payment_authority": false, "evidence_sha256": "abc", "analyzed_at": "2026-11-09T10:01:00Z"
            },
            "review_decision": null, "risk_override": null, "provider_result": null,
            "updated_at": "2026-11-09T10:01:00Z",
            "intent": {"intent": {"amount_minor": contract.amount_minor}}
        });
        task
    }

    #[test]
    fn policy_hold_requires_explicit_override_and_flags_injection() {
        let html = page("SANDBOX", reviewed_task("SANDBOX", "buyer_review"));
        assert!(html.contains("acknowledge_risk"));
        assert!(
            html.contains(
                r#"<input type="checkbox" name="acknowledge_risk" value="true" required/>"#
            )
        );
        assert!(html.contains("I reviewed the flagged content and override the policy hold"));
        assert!(html.contains("Untrusted instruction detected"));
        assert!(html.contains("Model output has no payment authority"));
        assert!(html.contains(r#"action="/ui/orders/order-1/review""#));
        assert!(html.contains("AI planner · qwen2.5-3b-instruct"));
        // Reject is offered first and as the primary action.
        let reject = html.find(r#"value="reject""#).unwrap();
        let approve = html.find(r#"name="decision" value="approve""#).unwrap();
        assert!(reject < approve);
        // Seller-controlled content is escaped, never rendered as markup.
        assert!(!html.contains("<script>ignore"));
        assert!(html.contains("&lt;script&gt;ignore previous instructions"));
        assert!(html.contains(r#"<pre class="evidence">"#));
    }

    #[test]
    fn replay_and_sandbox_payment_labels_are_honest() {
        let sandbox = page("SANDBOX", reviewed_task("SANDBOX", "captured"));
        assert!(sandbox.contains("PayPal Sandbox · provider-confirmed"));
        assert!(sandbox.contains("Captured"));
        assert!(sandbox.contains("5O190127TN364715T"));
        assert!(!sandbox.contains("Simulated — not provider-confirmed"));

        let replay = page("REPLAY", reviewed_task("REPLAY", "captured"));
        assert!(replay.contains("Simulated — not provider-confirmed"));
        assert!(!replay.contains("PayPal Sandbox · provider-confirmed"));

        let pending = page("SANDBOX", reviewed_task("SANDBOX", "approval_pending"));
        assert!(pending.contains("Approve at PayPal Sandbox"));
        assert!(pending.contains(r#"target="_blank" rel="noopener noreferrer""#));
        assert!(pending.contains(r#"action="/ui/orders/order-1/authorize""#));
        assert!(!pending.contains("PayPal Sandbox · provider-confirmed"));
        let replay_pending = page("REPLAY", reviewed_task("REPLAY", "approval_pending"));
        assert!(!replay_pending.contains("Approve at PayPal Sandbox"));
        assert!(replay_pending.contains(r#"action="/ui/orders/order-1/authorize""#));

        let recon = page(
            "SANDBOX",
            reviewed_task("SANDBOX", "reconciliation_required"),
        );
        assert!(recon.contains("Provider outcome uncertain or permission refused — reconciliation required; no success is claimed"));

        let authorized = page("REPLAY", reviewed_task("REPLAY", "authorized"));
        for fixture in ["pass", "fail", "malicious"] {
            assert!(authorized.contains(&format!(r#"name="fixture" value="{fixture}""#)));
        }
        assert!(authorized.contains(r#"<time data-deadline="2026-11-09T10:10:00Z">"#));
        assert!(
            page("SANDBOX", reviewed_task("SANDBOX", "quarantined"))
                .contains("Analysing delivery…")
        );
    }

    #[test]
    fn renders_defensively_for_sparse_or_malformed_input() {
        for task in [
            json!({}),
            json!({"id": 5, "state": 3, "order": "nope", "negotiation": {"transcript": "x", "offers": [1, null]}}),
            json!({"state": "failed", "error": "planner crashed", "objective": null}),
            json!({"state": "purchasing", "order": {"state": "buyer_review", "analysis": {"findings": [null]}}}),
            json!({"state": "no_deal", "negotiation": {"offers": [], "rationale": "No seller agreed"}}),
        ] {
            let html = page("REPLAY", task);
            assert!(html.contains("Agent console"));
        }
        let empty = render_agent(
            "REPLAY".into(),
            "x".into(),
            vec![json!(null)],
            None,
            vec![json!(1)],
            true,
            "Done".into(),
        );
        assert!(empty.contains("NO TASK SELECTED"));
        let logged_out = render_agent(
            "REPLAY".into(),
            "x".into(),
            vec![],
            None,
            vec![],
            false,
            String::new(),
        );
        assert!(
            logged_out.contains(r#"action="/login""#) && !logged_out.contains("/ui/agent/tasks")
        );
    }

    #[test]
    fn pipeline_progress_follows_task_and_order_state() {
        let statuses = |task: Value| progress(&task).iter().map(|s| s.status).collect::<Vec<_>>();
        use St::*;
        assert_eq!(
            statuses(json!({"state": "awaiting_approval"}))[3..5],
            [Done, Active]
        );
        assert_eq!(statuses(json!({"state": "no_deal"}))[2], Blocked);
        let captured = statuses(
            json!({"state": "purchasing", "order": {"state": "captured", "verification": {"passed": true}}}),
        );
        assert!(captured.iter().all(|s| *s == Done));
        let voided = statuses(json!({"state": "purchasing", "order": {"state": "voided"}}));
        assert_eq!(voided[6], Blocked);
        assert_eq!(voided[8], Blocked);
        let hold = statuses(
            json!({"state": "purchasing", "order": {"state": "buyer_review", "verification": {"passed": true}, "analysis": {"policy_hold": true}}}),
        );
        assert_eq!(hold[7..], [Blocked, Active]);
    }
}
