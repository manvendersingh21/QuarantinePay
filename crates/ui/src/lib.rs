use leptos::prelude::*;
use serde_json::Value;
pub fn render(
    mode: String,
    inference_label: String,
    orders: Vec<Value>,
    events: Vec<Value>,
    logged_in: bool,
    message: String,
) -> String {
    let event_id = events
        .first()
        .and_then(|e| e["id"].as_i64())
        .unwrap_or(0)
        .to_string();
    let show_message = !message.is_empty();
    let holds = orders
        .iter()
        .filter(|o| {
            matches!(
                o["state"].as_str(),
                Some("authorized" | "quarantined" | "capture_pending" | "void_pending")
            ) || (o["state"] == "reconciliation_required" && o["authorization_id"].is_string())
        })
        .count();
    let captured = orders.iter().filter(|o| o["state"] == "captured").count();
    let voided = orders.iter().filter(|o| o["state"] == "voided").count();
    let cards=orders.into_iter().map(|o|{
  let id=o["id"].as_str().unwrap_or("").to_string(); let state=o["state"].as_str().unwrap_or("").to_string();
  let deadline=o["deadline"].as_str().unwrap_or("Awaiting authorization").to_string();
  let approval=o["approval_url"].as_str().unwrap_or("/").to_string();
  let authorize=format!("/ui/orders/{id}/authorize"); let deliver=format!("/ui/orders/{id}/deliveries");
  let can_authorize=state=="approval_pending"; let can_deliver=state=="authorized";
  let class=format!("order {state}");
  view!{<article class=class><div class="order-head"><strong>"Invoice validation artifact"</strong><span class="badge">{state.clone()}</span></div>
   <p class="muted mono">{id}</p><div class="track"><div class="progress"></div></div><p class="muted">"Verification deadline: "<time data-deadline=deadline.clone()>{deadline.clone()}</time></p>
   <div class="actions"><div hidden=!can_authorize><a class="button secondary" href=approval.clone()>"Approve at PayPal"</a><form method="post" action=authorize.clone()><button>"Confirm approval / authorize"</button></form></div>
   <div hidden=!can_deliver><form method="post" action=deliver.clone()><input type="hidden" name="fixture" value="pass"/><button>"Deliver passing artifact"</button></form><form method="post" action=deliver.clone()><input type="hidden" name="fixture" value="fail"/><button class="danger">"Deliver failing artifact"</button></form></div></div>
  </article>}
 }).collect_view();
    let audit=events.into_iter().map(|e|{let kind=e["kind"].as_str().unwrap_or("").to_string();let detail=e["detail"].to_string();view!{<li><span class="dot"></span><div><strong>{kind}</strong><p class="muted mono">{detail}</p></div></li>}}).collect_view();
    let document = view! {
    <html lang="en"><head><meta charset="utf-8"/><meta name="viewport" content="width=device-width,initial-scale=1"/><title>"QuarantinePay · Payment control center"</title><link rel="stylesheet" href="/style.css"/><script type="module" src="/assets/loader.js"></script></head>
    <body data-event-id=event_id data-auth=logged_in.to_string()><aside><div class="brand"><span class="logo">"Q"</span>"QuarantinePay"</div><div class="workspace-label">"PAYMENT OPERATIONS"</div><a class="nav active" href="/">"◈  Command center"</a><a class="nav" href="/docs">"↗  API & setup"</a><div class="aside-foot"><span class="dot"></span>"Rust security gateway"<p>"Verify delivery. Then settle."</p></div></aside>
    <main><header><div><div class="eyebrow">"TRUST IS VERIFIED"</div><h1>"Payment control center"</h1><p class="muted">"Every purchase bound. Every delivery verified. Every settlement accounted for."</p></div><div class="runtime-badges"><span class="mode">{mode.clone()}" MODE"</span><span class="privacy-label">{inference_label}</span></div></header>
    <div hidden=!show_message><div class="notice">{message.clone()}</div></div>
    <div hidden=logged_in><section class="panel login"><h2>"Open your workspace"</h2><p class="muted">"Sign in with your local administrator password."</p><form method="post" action="/login"><input type="password" name="password" placeholder="Workspace password" required/><button>"Sign in"</button></form></section></div>
    <div hidden=!logged_in><section class="stats"><div><span>"Active authorization holds"</span><strong>{holds}</strong><small>"Awaiting verified resolution"</small></div><div><span>"Verified & captured"</span><strong>{captured}</strong><small>"Provider-confirmed settlements"</small></div><div><span>"Voided authorizations"</span><strong>{voided}</strong><small>"Failed checks or expired windows"</small></div></section>
    <div class="columns"><section><div class="section-head"><h2>"Quarantine timeline"</h2><a href="/" class="muted">"Refresh status ↻"</a></div><div class="panel purchase"><div><span class="eyebrow">"CONTROLLED DIGITAL DELIVERY"</span><h3>"Validated invoice artifact"</h3><p class="muted">"USD 1.00 · deterministic acceptance policy · 120-second window"</p></div><form method="post" action="/ui/purchase"><button>"Confirm intent & checkout →"</button></form></div>{cards}
    <section class="panel"><h2>"Shopping assistant"</h2><p class="muted">"Recommendations are checked against your confirmed purchase intent."</p><form method="post" action="/ui/agent"><input name="query" placeholder="Ask for a recommendation" required maxlength="2000"/><button>"Ask assistant"</button></form><form method="post" action="/ui/catalog"><input name="query" placeholder="Search the product catalog" required maxlength="200"/><button class="secondary">"Search catalog"</button></form><form method="post" action="/ui/catalog/sync"><button class="secondary">"Import public Channel3 catalog"</button><p class="muted">"Uses the configured public seed. Your private searches stay local."</p></form></section></section>
    <section><div class="section-head"><h2>"Security activity"</h2><span class="badge">"AUDIT LOG"</span></div><div class="panel"><ul class="audit">{audit}</ul></div><div class="panel jurisdiction"><form method="post" action="/ui/redteam"><button class="danger">"Simulate merchant substitution"</button></form><h3>"Agent jurisdiction"</h3><p><b>"L0"</b>" Read screened catalog"</p><p><b>"L1"</b>" Recommend a purchase"</p><p><b>"L2"</b>" Buyer confirms signed intent"</p><p><b>"L3"</b>" Gateway authorizes payment"</p><p><b>"L4"</b>" Verifier gates settlement"</p><small class="muted">"Application permission levels. Model output grants no payment authority."</small></div></section></div></div>
    <footer>"Sandbox demonstration · Payment fields go to PayPal · "<a href="https://github.com/manvendersingh21/QuarantinePay">"Source code ↗"</a></footer></main></body></html>};
    format!("<!DOCTYPE html>{}", document.to_html())
}
pub const CSS: &str = r#"
:root { color-scheme: light; --bg: #fff; --panel: #fff; --line: #e5e5e5; --muted: #666; --mint: #087f5b; }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: #262626; font: 14px system-ui, -apple-system, sans-serif; line-height: 1.6; }
aside { position: fixed; inset: 0 auto 0 0; width: 240px; padding: 32px 22px; border-right: 1px solid var(--line); background: #fff; }
.brand { display: flex; align-items: center; gap: 10px; font-size: 19px; font-weight: 700; letter-spacing: -.7px; }
.logo { display: grid; place-items: center; border: 1px solid #a9d6c5; color: var(--mint); border-radius: 12px; width: 37px; height: 39px; font-size: 24px; }
.workspace-label { font-size: 10px; color: #737373; letter-spacing: 1.5px; margin: 55px 12px 14px; }
.nav { display: block; padding: 11px 14px; margin: 6px 0; border-radius: 8px; color: var(--muted); text-decoration: none; }
.nav.active { background: #edf8f2; color: var(--mint); }
.aside-foot { position: absolute; bottom: 25px; color: #525252; font-size: 12px; }
.aside-foot p { color: #737373; margin-left: 15px; }
main { margin-left: 240px; padding: 45px 4vw; max-width: 1700px; }
header { display: flex; align-items: flex-start; justify-content: space-between; margin-bottom: 35px; }
h1 { font-size: 32px; letter-spacing: -1.2px; line-height: 1.2; margin: 9px 0 12px; }
h2 { font-size: 17px; margin: 0 0 10px; }
h3 { font-size: 16px; margin: 8px 0; }
.eyebrow { font-size: 10px; letter-spacing: 2px; color: var(--mint); font-weight: 650; }
.muted { color: var(--muted); }
.mode, .badge { white-space: nowrap; border: 1px solid #d4d4d4; border-radius: 6px; padding: 4px 9px; font-size: 10px; letter-spacing: 1px; color: #525252; }
.runtime-badges { display: flex; flex-direction: column; align-items: flex-end; gap: 8px; }
.privacy-label { color: var(--mint); font-size: 10px; letter-spacing: .6px; text-align: right; }
.mode { background: #edf8f2; color: var(--mint); border-color: #b8ddcb; margin-top: 14px; text-transform: uppercase; }
.stats { display: grid; grid-template-columns: repeat(3, 1fr); gap: 18px; margin-bottom: 35px; }
.stats > div, .panel, .order { background: var(--panel); border: 1px solid var(--line); border-radius: 12px; padding: 22px; }
.stats span, .stats small { display: block; color: var(--muted); font-size: 12px; }
.stats strong { font-size: 38px; font-weight: 550; display: block; margin: 5px 0; }
.stats small { font-size: 11px; }
.columns { display: grid; grid-template-columns: minmax(0, 1.8fr) minmax(280px, 1fr); gap: 25px; }
.section-head { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
.section-head h2 { margin: 0; }
.panel, .order { margin-bottom: 18px; }
.purchase { border-color: #b8ddcb; background: #fff; }
button, .button { display: inline-block; background: var(--mint); border: 0; border-radius: 7px; color: #fff; font: 600 12px system-ui; padding: 11px 16px; cursor: pointer; text-decoration: none; }
.secondary { background: #f5f5f5; color: #404040; box-shadow: inset 0 0 0 1px #d4d4d4; }
.danger { background: #fff1f2; color: #a3233a; box-shadow: inset 0 0 0 1px #f1c4cc; }
button:hover, .button:hover { filter: brightness(.96); }
:focus-visible { outline: 3px solid #087f5b; outline-offset: 3px; }
.order-head { display: flex; justify-content: space-between; gap: 10px; }
.mono { font: 11px ui-monospace, monospace; overflow-wrap: anywhere; }
.actions { display: flex; flex-wrap: wrap; gap: 10px; }
.track { height: 7px; background: #f0f0f0; border-radius: 6px; overflow: hidden; margin: 20px 0 12px; }
.progress { height: 100%; width: 45%; background: #b77918; }
.captured .progress { width: 100%; background: var(--mint); }
.voided .progress { width: 100%; background: #c53e58; }
.captured .badge { color: var(--mint); }
.voided .badge { color: #a3233a; }
.audit { list-style: none; padding: 0; margin: 0; max-height: 430px; overflow: auto; }
.audit li { display: flex; gap: 12px; padding: 13px 0; border-bottom: 1px solid var(--line); }
.audit strong { font-size: 12px; }
.dot { display: inline-block; flex-shrink: 0; width: 6px; height: 6px; background: var(--mint); border-radius: 50%; margin: 8px 6px 0 0; }
.jurisdiction b { color: var(--mint); display: inline-block; width: 30px; }
.jurisdiction p { font-size: 12px; color: #525252; }
input { display: block; width: 100%; padding: 12px; background: #fff; border: 1px solid #d4d4d4; color: #262626; border-radius: 7px; margin: 12px 0; }
footer { font-size: 11px; color: #737373; margin-top: 30px; }
a { color: var(--mint); }
.notice { border: 1px solid #b8ddcb; background: #edf8f2; padding: 15px; border-radius: 8px; margin-bottom: 20px; white-space: pre-wrap; overflow-wrap: anywhere; }
.login { max-width: 500px; }
@media (max-width: 1000px) { aside { width: 190px; } main { margin-left: 190px; padding: 30px 24px; } .columns { grid-template-columns: 1fr; } }
@media (max-width: 650px) { aside { position: static; width: auto; padding: 15px; border-right: 0; border-bottom: 1px solid var(--line); } .workspace-label, .nav, .aside-foot { display: none; } main { margin: 0; padding: 22px 16px; } .stats { gap: 8px; } .stats > div { padding: 12px; } .stats strong { font-size: 29px; } .stats small { display: none; } h1 { font-size: 25px; } header { gap: 12px; } }
"#;
