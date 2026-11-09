use leptos::prelude::*;
use serde_json::Value;
mod agent;
mod html;
mod intelligence;
pub use agent::{EXAMPLE_REQUEST, render_agent};
pub use intelligence::render_intelligence;
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
                Some(
                    "authorized"
                        | "quarantined"
                        | "buyer_review"
                        | "capture_pending"
                        | "void_pending"
                )
            ) || (o["state"] == "reconciliation_required" && o["authorization_id"].is_string())
        })
        .count();
    let captured = orders.iter().filter(|o| o["state"] == "captured").count();
    let voided = orders.iter().filter(|o| o["state"] == "voided").count();
    let simulation = mode.eq_ignore_ascii_case("replay");
    let payment_label = if simulation {
        "Simulated payment outcome"
    } else {
        "PayPal Sandbox status"
    };
    let purchases = quarantinepay_core::scenarios().into_iter().map(|scenario| {
        let id = scenario.id.to_string();
        let terms = scenario.terms.to_string();
        let seller = scenario.seller.to_string();
        let item = scenario.item.to_string();
        let price = format!("USD {:.2}", scenario.amount_minor as f64 / 100.0);
        let review = if scenario.review_required { "Buyer approval required after checks." } else { "Passing checks authorize settlement." };
        view! { <article class="panel purchase">
            <span class="eyebrow">"FICTIONAL SELLER · SIMULATED DELIVERY"</span>
            <h3>{item}</h3><p>{seller}" · "{price}</p>
            <p class="terms">{terms}</p><p class="muted">{review}" Accelerated 120-second delivery window."</p>
            <form method="post" action="/ui/purchase">
                <input type="hidden" name="sku" value=id/>
                <div hidden=!simulation><label>"Simulated payment outcome"<select name="outcome"><option value="approve">"Normal authorization and settlement"</option><option value="decline">"Authorization declined"</option><option value="ambiguous">"Unknown capture result (requires reconciliation)"</option></select></label></div>
                <button>"Agree to terms & begin →"</button>
            </form>
        </article> }
    }).collect_view();
    let cards=orders.into_iter().map(|o|{
  let id=o["id"].as_str().unwrap_or("").to_string(); let state=o["state"].as_str().unwrap_or("").to_string();
  let deadline=o["deadline"].as_str().unwrap_or("Awaiting authorization").to_string();
  let approval=o["approval_url"].as_str().unwrap_or("/").to_string();
  let intent=&o["intent"]["intent"];
  let scenario_id=intent["sku"].as_str().unwrap_or("");
  let scenario=quarantinepay_core::scenarios().into_iter().find(|s| s.id == scenario_id);
  let signed_terms=&intent["scenario"];
  let title=signed_terms["item"].as_str().or_else(|| scenario.as_ref().map(|s| s.item)).unwrap_or("Legacy invoice artifact").to_string();
  let seller=signed_terms["seller"].as_str().or_else(|| scenario.as_ref().map(|s| s.seller)).unwrap_or("Legacy merchant").to_string();
  let terms=signed_terms["terms"].as_str().or_else(|| scenario.as_ref().map(|s| s.terms)).unwrap_or("Legacy invoice verification policy").to_string();
  let verification=o["verification"]["reason"].as_str().unwrap_or("Waiting for delivery").to_string();
  let artifact=if o["artifact"].is_null() { "No delivery submitted".to_string() } else { o["artifact"].to_string() };
  let authorize=format!("/ui/orders/{id}/authorize"); let deliver=format!("/ui/orders/{id}/deliveries"); let review=format!("/ui/orders/{id}/review");
  let can_authorize=state=="approval_pending"; let can_deliver=state=="authorized"; let can_review=state=="buyer_review";
  let class=format!("order {state}");
  view!{<article class=class><div class="order-head"><strong>{title}</strong><span class="badge">{state.clone()}</span></div>
   <p class="muted">{seller}" · fictional seller · "{payment_label}</p><p class="muted mono">{id}</p>
   <p><strong>"Agreed terms: "</strong>{terms}</p><p><strong>"Verification: "</strong>{verification}</p>
   <div class="track"><div class="progress"></div></div><p class="muted">"Accelerated deadline: "<time data-deadline=deadline.clone()>{deadline.clone()}</time></p>
   <div hidden=!can_review><p>"Review the submitted simulated evidence before deciding. Delivery evidence does not prove product quality."</p><pre class="evidence">{artifact}</pre></div>
   <div class="actions"><div hidden=!can_authorize><a hidden=simulation class="button secondary" href=approval.clone()>"Approve at PayPal Sandbox"</a><form method="post" action=authorize.clone()><button>{if simulation { "Authorize simulated payment" } else { "Confirm Sandbox approval / authorize" }}</button></form></div>
   <div hidden=!can_deliver><p>"Seller workspace · simulated submission"</p><form method="post" action=deliver.clone()><input type="hidden" name="fixture" value="pass"/><button>"Submit passing example"</button></form><form method="post" action=deliver.clone()><input type="hidden" name="fixture" value="fail"/><button class="danger">"Submit failing example"</button></form></div>
   <div hidden=!can_review><form method="post" action=review.clone()><input type="hidden" name="decision" value="approve"/><button>"Approve delivery"</button></form><form method="post" action=review.clone()><input type="hidden" name="decision" value="reject"/><button class="danger">"Reject delivery"</button></form></div></div>
  </article>}
 }).collect_view();
    let audit=events.into_iter().map(|e|{let kind=e["kind"].as_str().unwrap_or("").to_string();let detail=e["detail"].to_string();view!{<li><span class="dot"></span><div><strong>{kind}</strong><p class="muted mono">{detail}</p></div></li>}}).collect_view();
    let document = view! {
    <html lang="en"><head><meta charset="utf-8"/><meta name="viewport" content="width=device-width,initial-scale=1"/><title>"QuarantinePay · Payment control center"</title><link rel="stylesheet" href="/style.css"/><script type="module" src="/assets/loader.js"></script></head>
    <body data-event-id=event_id data-auth=logged_in.to_string()><aside><div class="brand"><span class="logo">"Q"</span>"QuarantinePay"</div><div class="workspace-label">"PAYMENT OPERATIONS"</div><nav class="side-nav" aria-label="Primary"><a class="nav active" href="/" aria-current="page">"◈  Command center"</a><a class="nav" href="/agent">"✦  Agent console"</a><a class="nav" href="/intelligence">"▦  Intelligence"</a><a class="nav" href="/docs">"↗  API & setup"</a></nav><div class="aside-foot"><span class="dot"></span>"Rust security gateway"<p>"Verify delivery. Then settle."</p></div></aside>
    <main><header><div><div class="eyebrow">"TRUST IS VERIFIED"</div><h1>"Payment control center"</h1><p class="muted">"Every purchase bound. Every delivery verified. Every settlement accounted for."</p></div><div class="runtime-badges"><span class="mode">{mode.clone()}" MODE"</span><span class="privacy-label">{inference_label}</span></div></header>
    <div hidden=!show_message><div class="notice">{message.clone()}</div></div>
    <div hidden=logged_in><section class="panel login"><h2>"Open your workspace"</h2><p class="muted">"Sign in with your local administrator password."</p><form method="post" action="/login"><input type="password" name="password" placeholder="Workspace password" required/><button>"Sign in"</button></form></section></div>
    <div hidden=!logged_in><section class="stats"><div><span>"Active authorization holds"</span><strong>{holds}</strong><small>"Awaiting verified resolution"</small></div><div><span>"Verified & captured"</span><strong>{captured}</strong><small>{payment_label}</small></div><div><span>"Voided authorizations"</span><strong>{voided}</strong><small>"Failed checks or expired windows"</small></div></section>
    <section><div class="section-head"><h2>"Fictional storefront"</h2><span class="badge">"ALL GOODS AND SELLERS SIMULATED"</span></div><p class="muted">"Choose a scenario and agree to its terms. The 120-second clock accelerates the demonstration."</p><div class="storefront">{purchases}</div></section><div class="columns"><section><div class="section-head"><h2>"Orders & seller workspace"</h2><a href="/" class="muted">"Refresh status ↻"</a></div>{if simulation { Some(view! { <form method="post" action="/ui/reset"><button class="danger">"Reset simulated demo data"</button><p class="muted">"Deletes replay orders, intents and their events. Sandbox records remain."</p></form> }) } else { None }}{cards}
    <section class="panel"><h2>"Shopping assistant"</h2><p class="muted">"Recommendations are checked against your confirmed purchase intent."</p><form method="post" action="/ui/agent"><input name="query" placeholder="Ask for a recommendation" required maxlength="2000"/><button>"Ask assistant"</button></form><form method="post" action="/ui/catalog"><input name="query" placeholder="Search the product catalog" required maxlength="200"/><button class="secondary">"Search catalog"</button></form><form method="post" action="/ui/catalog/sync"><button class="secondary">"Import public Channel3 catalog"</button><p class="muted">"Uses the configured public seed. Your private searches stay local."</p></form></section></section>
    <section><div class="panel"><h2>"Shopify development store"</h2><p class="muted">"View test catalog and orders from your configured development store. Shopify test orders are independent of QuarantinePay settlement."</p><a class="button secondary" href="/api/shopify/dev-store">"Open development store data"</a></div><div class="section-head"><h2>"Security activity"</h2><span class="badge">"AUDIT LOG"</span></div><div class="panel"><ul class="audit">{audit}</ul></div><div class="panel jurisdiction"><form method="post" action="/ui/redteam"><button class="danger">"Simulate merchant substitution"</button></form><h3>"Agent jurisdiction"</h3><p><b>"L0"</b>" Read screened catalog"</p><p><b>"L1"</b>" Recommend a purchase"</p><p><b>"L2"</b>" Buyer confirms signed intent"</p><p><b>"L3"</b>" Gateway authorizes payment"</p><p><b>"L4"</b>" Verifier gates settlement"</p><small class="muted">"Application permission levels. Model output grants no payment authority."</small></div></section></div></div>
    <footer>"Fictional demonstration · No card details are collected here · Sandbox checkout, when enabled, is hosted by PayPal · "<a href="https://github.com/manvendersingh21/QuarantinePay">"Source code ↗"</a></footer></main></body></html>};
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
.purchase { border-color: #b8ddcb; background: #fff; display: flex; flex-direction: column; }
.storefront { display: grid; grid-template-columns: repeat(auto-fit, minmax(210px, 1fr)); gap: 14px; margin-bottom: 28px; perspective: 1100px; }
.purchase { transform-origin: center bottom; animation: offer-arrive .55s both; transition: transform .2s ease, box-shadow .2s ease; }
.purchase:nth-child(2) { animation-delay: .06s; } .purchase:nth-child(3) { animation-delay: .12s; } .purchase:nth-child(4) { animation-delay: .18s; } .purchase:nth-child(5) { animation-delay: .24s; }
@media (hover: hover) { .purchase:hover { transform: translateY(-4px) rotateX(2deg); box-shadow: 0 14px 28px #153d2c19; } }
@keyframes offer-arrive { from { opacity: 0; transform: translateY(12px) rotateX(-5deg); } to { opacity: 1; transform: translateY(0) rotateX(0); } }
@media (prefers-reduced-motion: reduce) { .purchase { animation: none; transition: none; } .purchase:hover { transform: none; } }
.purchase form { margin-top: auto; }
.terms { font-size: 13px; }
.evidence { white-space: pre-wrap; overflow-wrap: anywhere; background: #f6f8f7; padding: 12px; border-radius: 6px; }
select { display: block; width: 100%; padding: 8px; margin: 8px 0 12px; border: 1px solid #bbb; border-radius: 6px; background: white; }
.actions form { display: inline-block; margin: 3px; }
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
/* ───────── Agent console & intelligence (appended) ───────── */
:root { --ink: #262626; --soft: #f7faf8; --mint-soft: #edf8f2; --mint-line: #b8ddcb; --amber: #8a5300; --amber-soft: #fff7e6; --amber-line: #f0d39a; --red: #a3233a; --red-soft: #fff1f2; --red-line: #f1c4cc; --violet: #5143b5; --violet-soft: #f4f2ff; --violet-line: #d8d2fa; --blue: #1f5f99; --blue-soft: #eef5fc; --blue-line: #c7dcf0; --radius: 12px; }
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
.small { font-size: 12px; }
.side-nav { display: block; }
.nav span { display: inline-block; width: 18px; }
.chip { display: inline-flex; align-items: center; gap: 4px; border-radius: 999px; padding: 2px 10px; font-size: 11px; font-weight: 600; line-height: 1.6; white-space: nowrap; max-width: 100%; overflow: hidden; text-overflow: ellipsis; border: 1px solid transparent; }
.tone-good { background: var(--mint-soft); color: var(--mint); border-color: var(--mint-line); }
.tone-warn { background: var(--amber-soft); color: var(--amber); border-color: var(--amber-line); }
.tone-bad { background: var(--red-soft); color: var(--red); border-color: var(--red-line); }
.tone-info { background: var(--blue-soft); color: var(--blue); border-color: var(--blue-line); }
.tone-ai { background: var(--violet-soft); color: var(--violet); border-color: var(--violet-line); }
.tone-neutral { background: #f5f5f5; color: #525252; border-color: #e0e0e0; }
.dot.tone-warn { background: #d48a12; } .dot.tone-ai { background: var(--violet); } .dot.tone-good { background: var(--mint); }
.chips { display: flex; flex-wrap: wrap; gap: 6px; margin: 4px 0 12px; }
.tags { display: inline-flex; flex-wrap: wrap; gap: 4px; }
.tag { display: inline-block; font-size: 11px; padding: 1px 8px; border-radius: 6px; background: #f3f4f3; color: #404040; border: 1px solid #e6e6e6; }
.tag-mint { background: var(--mint-soft); color: var(--mint); border-color: var(--mint-line); }
code { font: 12px ui-monospace, SFMono-Regular, Menlo, monospace; overflow-wrap: anywhere; }
.notice.warn { background: var(--amber-soft); border-color: var(--amber-line); color: #5c3a00; }
.primary-lg { padding: 13px 20px; font-size: 13px; box-shadow: 0 1px 0 #05543c, 0 6px 16px #087f5b2e; }
textarea { display: block; width: 100%; min-height: 84px; resize: vertical; padding: 14px 16px; margin: 10px 0 12px; border: 1px solid #d4d4d4; border-radius: 10px; font: 15px/1.5 system-ui, -apple-system, sans-serif; color: var(--ink); background: #fff; }
textarea:focus { border-color: var(--mint); outline: 3px solid #087f5b33; outline-offset: 0; }
.request-box { border-color: var(--mint-line); background: linear-gradient(180deg, #f4fbf7, #fff 70%); padding: 24px 26px; }
.request-box h2 { font-size: 20px; letter-spacing: -.4px; margin: 4px 0 0; }
.request-foot { display: flex; align-items: center; justify-content: space-between; gap: 18px; }
.request-foot p { margin: 0; max-width: 640px; }
.request-foot button { flex: none; }
.agent-layout { display: grid; grid-template-columns: minmax(0, 1fr) minmax(280px, 360px); gap: 25px; align-items: start; }
.agent-side { position: sticky; top: 20px; }
.task-detail, .agent-side { min-width: 0; }
.task-list ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
.task-item { display: block; padding: 11px 12px; border-radius: 10px; border: 1px solid transparent; color: var(--ink); text-decoration: none; }
.task-item:hover { background: var(--soft); border-color: var(--line); }
.task-item.active { background: var(--mint-soft); border-color: var(--mint-line); }
.task-item-top { display: flex; justify-content: space-between; align-items: center; gap: 8px; font-size: 11px; color: var(--muted); }
.task-item p { margin: 6px 0 2px; font-size: 13px; line-height: 1.45; overflow-wrap: anywhere; }
.task-item small { color: var(--muted); font-size: 11px; }
.task-head-top { display: flex; justify-content: space-between; align-items: center; gap: 10px; }
.request-quote { margin: 10px 0 8px; padding: 2px 0 2px 16px; border-left: 3px solid var(--mint); font-size: 17px; line-height: 1.5; letter-spacing: -.2px; overflow-wrap: anywhere; }
.task-meta { margin: 0; overflow-wrap: anywhere; }
.task-meta code { font-size: 11px; }
.pipeline { list-style: none; margin: 0 0 18px; padding: 0; }
.step { position: relative; padding: 0 0 22px 50px; }
.step::before { content: ""; position: absolute; left: 15px; top: 34px; bottom: 2px; width: 2px; border-radius: 2px; background: var(--line); }
.step.done::before { background: var(--mint-line); }
.step:last-child::before { display: none; }
.step-marker { position: absolute; left: 0; top: 0; width: 32px; height: 32px; display: grid; place-items: center; border-radius: 50%; font-size: 13px; font-weight: 700; background: #fff; color: #8a8a8a; border: 2px solid var(--line); }
.step.done .step-marker { background: var(--mint); border-color: var(--mint); color: #fff; }
.step.active .step-marker { border-color: var(--mint); color: var(--mint); box-shadow: 0 0 0 5px #087f5b1f; animation: step-pulse 2s ease-in-out infinite; }
.step.blocked .step-marker { background: var(--amber-soft); border-color: #d48a12; color: var(--amber); }
.step-body { min-width: 0; background: #fff; border: 1px solid var(--line); border-radius: var(--radius); padding: 14px 18px; }
.step.pending .step-body { background: transparent; border-style: dashed; padding: 8px 16px; }
.step.pending h3 { color: #8a8a8a; }
.step.active .step-body { border-color: var(--mint-line); box-shadow: 0 8px 24px #153d2c12; }
.step.blocked .step-body { border-color: var(--amber-line); }
.step-head { display: flex; justify-content: space-between; align-items: center; gap: 10px; flex-wrap: wrap; }
.step-head h3 { margin: 4px 0; font-size: 15px; }
.step-body > :last-child { margin-bottom: 0; }
@keyframes step-pulse { 50% { box-shadow: 0 0 0 9px #087f5b0d; } }
.kv { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); margin: 10px 0 12px; background: #fff; border: 1px solid var(--line); border-radius: 10px; overflow: hidden; }
.kv > div { padding: 9px 12px; min-width: 0; box-shadow: 1px 0 0 var(--line), 0 1px 0 var(--line); }
.kv dt { font-size: 10px; letter-spacing: 1px; text-transform: uppercase; color: #737373; }
.kv dd { margin: 2px 0 0; overflow-wrap: anywhere; }
.kv code { font-size: 11px; }
.working { color: var(--blue); }
.spinner { display: inline-block; width: 12px; height: 12px; margin-right: 8px; border: 2px solid var(--blue-line); border-top-color: var(--blue); border-radius: 50%; vertical-align: -1px; animation: spin .9s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.sellers { list-style: none; margin: 8px 0 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fit, minmax(190px, 1fr)); gap: 8px; }
.sellers li { display: flex; align-items: center; gap: 10px; padding: 10px; border: 1px solid var(--line); border-radius: 10px; min-width: 0; }
.sellers li > div { flex: 1; min-width: 0; display: flex; flex-direction: column; align-items: flex-start; gap: 2px; line-height: 1.35; }
.sellers li { align-items: flex-start; }
.seller-avatar { flex: none; display: grid; place-items: center; width: 30px; height: 30px; border-radius: 8px; background: var(--soft); border: 1px solid var(--line); font-weight: 700; color: var(--mint); }
.transcript summary, .report summary { cursor: pointer; font-weight: 600; font-size: 13px; padding: 4px 0; }
.chat { list-style: none; margin: 8px 0 14px; padding: 12px; display: flex; flex-direction: column; gap: 8px; max-height: 460px; overflow-y: auto; background: var(--soft); border-radius: 10px; border: 1px solid var(--line); }
.chat-divider { display: flex; align-items: center; gap: 10px; font-size: 10px; letter-spacing: 1.2px; text-transform: uppercase; color: #737373; margin: 6px 0 0; }
.chat-divider::before, .chat-divider::after { content: ""; flex: 1; height: 1px; background: var(--line); }
.msg { display: flex; }
.msg.buyer { justify-content: flex-end; }
.bubble { max-width: min(78%, 520px); padding: 8px 12px; border-radius: 14px 14px 14px 4px; background: #fff; border: 1px solid var(--line); min-width: 0; }
.msg.buyer .bubble { border-radius: 14px 14px 4px 14px; background: var(--mint-soft); border-color: var(--mint-line); }
.msg.kind-decline .bubble, .msg.kind-walk_away .bubble { background: #fafafa; border-style: dashed; }
.msg.kind-accept .bubble { box-shadow: inset 3px 0 0 var(--mint); }
.bubble-meta { font-size: 11px; color: var(--muted); }
.bubble p { margin: 2px 0; font-size: 13px; overflow-wrap: anywhere; }
.bubble-amount { display: inline-block; margin-top: 2px; font: 600 12px ui-monospace, monospace; color: var(--ink); }
.table-wrap { overflow-x: auto; margin: 10px 0; border: 1px solid var(--line); border-radius: 10px; }
.offers { width: 100%; border-collapse: collapse; font-size: 13px; }
.offers th { text-align: left; font-size: 10px; letter-spacing: 1px; text-transform: uppercase; color: #737373; font-weight: 600; background: var(--soft); }
.offers th, .offers td { padding: 9px 12px; border-bottom: 1px solid var(--line); vertical-align: top; }
.offers tr:last-child td { border-bottom: 0; }
.offers tr.selected td { background: #f6fcf9; }
.offers td.muted { min-width: 200px; font-size: 12px; }
.num { font-variant-numeric: tabular-nums; white-space: nowrap; }
.rationale { padding: 12px 14px; border-radius: 10px; background: var(--soft); border: 1px solid var(--line); }
.rationale p { margin: 4px 0 0; }
.contract-card { border: 1px solid var(--mint-line); border-radius: var(--radius); padding: 16px; background: linear-gradient(180deg, #f7fcf9, #fff 40%); }
.contract-head { display: flex; justify-content: space-between; align-items: flex-start; gap: 14px; }
.contract-head h4 { margin: 4px 0 0; font-size: 17px; letter-spacing: -.3px; }
.contract-head p { margin: 0; }
.contract-price { font-size: 28px; font-weight: 600; letter-spacing: -1px; font-variant-numeric: tabular-nums; color: var(--ink); }
.digest { display: grid; gap: 3px; margin-top: 10px; padding: 10px 12px; border-radius: 8px; background: #0f2a20; color: #c8f0de; }
.digest span { font-size: 10px; letter-spacing: 1px; text-transform: uppercase; color: #8fcfb2; }
.digest code { color: #e8fff4; font-size: 12px; }
.secondary-digest { background: var(--soft); border: 1px solid var(--line); }
.secondary-digest span { color: #737373; } .secondary-digest code { color: #404040; }
.inline-digest { background: var(--soft); padding: 1px 6px; border-radius: 4px; }
.decision { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 10px; margin-top: 12px; }
.approve-form { flex: 1 1 280px; }
.field { display: block; font-size: 12px; color: var(--muted); max-width: 420px; }
.field select { margin-top: 4px; }
.payment { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin: 6px 0 12px; }
.money { padding: 14px 16px; border-radius: 10px; border: 1px solid var(--line); background: var(--soft); min-width: 0; }
.money span { display: block; font-size: 11px; color: var(--muted); letter-spacing: .5px; text-transform: uppercase; }
.money strong { display: block; font-size: 30px; font-weight: 600; letter-spacing: -1px; font-variant-numeric: tabular-nums; line-height: 1.3; }
.money small { color: var(--muted); font-size: 11px; }
.money.captured { background: var(--mint-soft); border-color: var(--mint-line); }
.money.captured strong { color: var(--mint); }
.simulator { border: 1px dashed #c9c9c9; border-radius: var(--radius); padding: 14px 16px; background: #fcfcfc; }
.sim-head { display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 8px; }
.countdown { font-variant-numeric: tabular-nums; margin: 4px 0 8px; }
.countdown time { font-weight: 700; color: var(--amber); }
.report { margin-top: 12px; border: 1px solid var(--line); border-radius: 10px; padding: 8px 12px; }
.report .evidence { max-height: 420px; overflow: auto; font: 12px/1.55 ui-monospace, monospace; margin: 8px 0 4px; }
.verdict-head { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; margin: 4px 0; }
.checklist { list-style: none; margin: 8px 0 12px; padding: 0; display: grid; gap: 4px; }
.checklist li { display: flex; gap: 10px; align-items: flex-start; padding: 7px 10px; border-radius: 8px; background: var(--soft); }
.checklist li.fail { background: var(--red-soft); }
.checklist li > div { display: flex; flex-direction: column; min-width: 0; line-height: 1.4; overflow-wrap: anywhere; }
.check-icon { flex: none; display: grid; place-items: center; width: 20px; height: 20px; border-radius: 50%; font-size: 12px; font-weight: 700; color: #fff; background: var(--mint); }
.fail .check-icon { background: var(--red); }
.analyst-card { margin-top: 12px; border: 1px solid var(--violet-line); border-radius: var(--radius); padding: 16px; background: linear-gradient(180deg, #faf9ff, #fff 50%); }
.analyst-card.risk-high { border-color: var(--red-line); }
.analyst-head { display: flex; justify-content: space-between; align-items: flex-start; gap: 10px; flex-wrap: wrap; }
.analyst-head h4 { margin: 2px 0 0; font-size: 16px; }
.analyst-card .eyebrow { color: var(--violet); }
.analyst-card .chips { margin: 0; }
.banner { display: flex; gap: 10px; align-items: flex-start; margin: 12px 0; padding: 12px 14px; border-radius: 10px; }
.banner p { margin: 0; }
.danger-banner { background: var(--red-soft); color: #7d1a2d; border: 1px solid var(--red-line); }
.warn-banner { background: var(--amber-soft); color: #5c3a00; border: 1px solid var(--amber-line); }
.summary { font-size: 14px; }
.sub { margin: 12px 0; }
.concerns { margin: 6px 0; padding-left: 18px; }
.findings { list-style: none; margin: 6px 0; padding: 0; display: grid; gap: 8px; }
.findings li { border: 1px solid var(--line); border-radius: 8px; padding: 8px 10px; background: #fff; min-width: 0; }
.finding-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.excerpt { display: block; margin-top: 6px; padding: 8px 10px; border-radius: 6px; background: #1d1d1f; color: #f5d0d6; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; }
.authority { margin: 12px 0 0; padding-top: 10px; border-top: 1px solid var(--line); font-size: 12px; color: #525252; }
.review-actions { display: flex; flex-wrap: wrap; gap: 12px; align-items: flex-start; margin-top: 10px; }
.override-form { flex: 1 1 260px; padding: 12px; border: 1px solid var(--red-line); border-radius: 10px; background: #fffafa; }
.check { display: flex; gap: 10px; align-items: flex-start; font-size: 13px; margin-bottom: 10px; cursor: pointer; }
.check input { display: inline-block; flex: none; width: 18px; height: 18px; margin: 2px 0 0; padding: 0; accent-color: var(--mint); }
.outcome { padding: 14px 16px; border-radius: 10px; border: 1px solid var(--line); }
.outcome strong { font-size: 16px; }
.outcome p { margin: 4px 0 0; }
.outcome.good { background: var(--mint-soft); border-color: var(--mint-line); color: #05543c; }
.outcome.bad { background: var(--red-soft); border-color: var(--red-line); color: #7d1a2d; }
.outcome.warn { background: var(--amber-soft); border-color: var(--amber-line); color: #5c3a00; }
.feed li > div { min-width: 0; flex: 1; }
.feed-head { display: flex; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
.feed-head time { font-size: 11px; color: var(--muted); }
.feed p { margin: 2px 0; overflow-wrap: anywhere; }
.empty-state h2 { font-size: 20px; margin-top: 6px; }
.how { margin: 10px 0 0; padding-left: 20px; display: grid; gap: 8px; }
/* Intelligence */
.kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 14px; margin-bottom: 22px; }
.kpi { border: 1px solid var(--line); border-radius: var(--radius); padding: 18px 20px; background: #fff; min-width: 0; }
.kpi span, .kpi small { display: block; color: var(--muted); font-size: 12px; }
.kpi small { font-size: 11px; }
.kpi strong { display: block; font-size: 30px; font-weight: 600; letter-spacing: -1px; margin: 4px 0; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
.kpi-good strong { color: var(--mint); } .kpi-warn strong { color: var(--amber); } .kpi-bad strong { color: var(--red); }
.grid-panel { padding: 18px; }
.grid-toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 14px; }
.grid-toolbar .search { flex: 1 1 260px; }
.grid-toolbar .search input { margin: 0; padding: 10px 12px; }
.presets { display: flex; flex-wrap: wrap; gap: 6px; }
.preset { background: #fff; color: #404040; box-shadow: inset 0 0 0 1px #d4d4d4; padding: 8px 12px; border-radius: 999px; font-weight: 600; }
.preset.active { background: var(--mint-soft); color: var(--mint); box-shadow: inset 0 0 0 1px var(--mint-line); }
.violations { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 8px 14px; margin: 14px 0 12px; padding: 10px 12px; border-radius: 10px; background: var(--soft); border: 1px solid var(--line); }
.violations .check { margin: 0; flex: 1 1 260px; }
.grid-host { height: 560px; width: 100%; }
.ag-row.row-violation { background-color: #fff3e0; }
.ag-row.row-violation .ag-cell:first-child { box-shadow: inset 4px 0 0 #d48a12; }
.cell-chip { display: inline-block; border-radius: 999px; padding: 0 8px; font-size: 11px; font-weight: 600; line-height: 20px; border: 1px solid transparent; }
.cell-mono { font: 11px ui-monospace, monospace; }
.cell-flag { color: var(--red); font-weight: 600; }
:where(.ag-root-wrapper, .ag-popup) input { display: revert; width: revert; margin: revert; padding: revert; border: revert; border-radius: revert; background: revert; }
:where(.ag-root-wrapper, .ag-popup) button { all: revert; }
:where(.ag-root-wrapper, .ag-popup) select { display: revert; width: revert; margin: revert; padding: revert; }
@media (max-width: 1200px) { .agent-layout { grid-template-columns: minmax(0, 1fr); } .agent-side { position: static; } .kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
@media (max-width: 650px) {
  .side-nav { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 12px; }
  .side-nav .nav { display: block; margin: 0; padding: 6px 10px; font-size: 12px; border: 1px solid var(--line); }
  .side-nav .nav.active { border-color: var(--mint-line); }
  header { flex-wrap: wrap; } .runtime-badges { align-items: flex-start; } .mode { margin-top: 0; }
  textarea { min-height: 136px; font-size: 16px; }
  .request-box { padding: 18px 16px; } .request-foot { flex-direction: column; align-items: stretch; } .request-foot button { width: 100%; }
  .step { padding-left: 40px; } .step::before { left: 12px; top: 28px; } .step-marker { width: 26px; height: 26px; font-size: 12px; }
  .step-body { padding: 12px; } .contract-head { flex-direction: column; } .contract-price { font-size: 24px; }
  .payment { grid-template-columns: 1fr; } .money strong { font-size: 26px; } .bubble { max-width: 90%; }
  .decision form, .decision button, .review-actions form, .review-actions button { width: 100%; }
  .chip { white-space: normal; text-align: left; } .chips, .analyst-head > div { min-width: 0; max-width: 100%; }
  .kpis { gap: 8px; } .kpi { padding: 12px; } .kpi strong { font-size: 22px; } .grid-host { height: 480px; } .grid-panel { padding: 12px; }
}
@media (prefers-reduced-motion: reduce) { .step.active .step-marker, .spinner { animation: none; } html { scroll-behavior: auto; } }
"#;

#[cfg(test)]
mod tests {
    use super::render;
    use serde_json::json;

    #[test]
    fn storefront_and_review_show_simulation_boundaries() {
        let order = json!({"id":"order-1", "state":"buyer_review", "intent":{"intent":{"sku":"physical", "scenario":{"item":"Desk lamp", "seller":"Cedar Supply", "terms":"Signed delivery terms"}}}, "artifact":{"tracking":"DEMO-1"}, "verification":{"reason":"Evidence passed"}});
        let page = render(
            "REPLAY".into(),
            "Local".into(),
            vec![order],
            vec![],
            true,
            String::new(),
        );
        for content in [
            "City mobility CSV",
            "Market report",
            "Weather response",
            "Design milestone",
            "Desk lamp",
            "Signed delivery terms",
            "Simulated payment outcome",
            "Approve delivery",
            "Reject delivery",
            "Delivery evidence does not prove product quality",
        ] {
            assert!(page.contains(content), "missing {content}");
        }
        assert!(page.contains("/ui/orders/order-1/review"));
        assert!(page.contains("Unknown capture result"));
        assert!(page.contains("/ui/reset"));
        let sandbox = render(
            "SANDBOX".into(),
            "Local".into(),
            vec![],
            vec![],
            true,
            String::new(),
        );
        assert!(!sandbox.contains("/ui/reset"));
    }
}
