//! Transaction intelligence: portfolio KPIs and an AG Grid Community table of every order.
//! All behaviour lives in `/assets/intelligence.js`; this page only provides the skeleton.
use crate::html::{Page, Shell, esc, shell};

pub fn render_intelligence(
    mode: String,
    inference_label: String,
    nonce: String,
    logged_in: bool,
) -> String {
    let content = concat!(
        r#"<section class="kpis" aria-live="polite">"#,
        r#"<div class="kpi"><span>Authorized</span><strong id="kpi-authorized">—</strong><small id="kpi-orders">Loading…</small></div>"#,
        r#"<div class="kpi kpi-good"><span>Captured</span><strong id="kpi-captured">—</strong><small>Paid after verified delivery</small></div>"#,
        r#"<div class="kpi kpi-warn"><span>Held</span><strong id="kpi-held">—</strong><small>Authorization holds awaiting resolution</small></div>"#,
        r#"<div class="kpi kpi-bad"><span>AI-flagged · blocked</span><strong id="kpi-flagged">—</strong><small id="kpi-blocked">—</small></div>"#,
        r#"</section>"#,
        r#"<section class="panel grid-panel">"#,
        r#"<div class="grid-toolbar"><label class="search"><span class="sr-only">Quick filter</span><input id="quick-filter" type="search" placeholder="Search seller, item, state, PayPal ID…" autocomplete="off"/></label>"#,
        r#"<div class="presets" role="group" aria-label="Preset filters">"#,
        r#"<button type="button" class="preset active" data-preset="all" aria-pressed="true">All</button>"#,
        r#"<button type="button" class="preset" data-preset="flagged" aria-pressed="false">AI flagged</button>"#,
        r#"<button type="button" class="preset" data-preset="captured" aria-pressed="false">Captured</button>"#,
        r#"<button type="button" class="preset" data-preset="held" aria-pressed="false">Held / pending</button>"#,
        r#"<button type="button" class="preset" data-preset="reconciliation" aria-pressed="false">Needs reconciliation</button>"#,
        r#"</div></div>"#,
        r#"<div class="violations"><label class="check"><input id="violations-toggle" type="checkbox" checked/><span><strong>Policy violations check</strong> — highlight captures made despite detected injection, and any audited risk override</span></label><span id="violations-summary" class="chip tone-neutral">Checking…</span></div>"#,
        r#"<div id="grid" class="grid-host" role="region" aria-label="Transactions"></div>"#,
        r#"<p id="grid-status" class="muted small" role="status">Loading transactions…</p>"#,
        r#"<noscript><p class="notice warn">The transaction grid needs JavaScript. The same data is available at <a href="/api/analytics/transactions">/api/analytics/transactions</a>.</p></noscript>"#,
        r#"</section>"#,
        r#"<p class="muted small">Grid powered by AG Grid Community (MIT). Sellers and deliveries are fictional; replay-mode rows are simulated and never provider-confirmed.</p>"#
    );
    let head = if logged_in {
        r#"<script src="/assets/vendor/ag-grid-community.min.js" defer></script><script src="/assets/intelligence.js" defer></script>"#
    } else {
        ""
    };
    shell(Shell {
        title: "Intelligence",
        eyebrow: "PORTFOLIO RISK · EVERY HOLD ACCOUNTED FOR",
        heading: "Transaction intelligence",
        subtitle: "Every authorization, capture and void — with structural verdicts, AI risk signals and provider confirmation side by side.",
        page: Page::Intelligence,
        mode: &mode,
        inference_label: &inference_label,
        logged_in,
        message: "",
        body_attrs: format!(
            r#" data-auth="{logged_in}" data-nonce="{}" data-mode="{}""#,
            esc(&nonce),
            esc(&mode)
        ),
        head: head.into(),
        content: content.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::render_intelligence;

    #[test]
    fn intelligence_page_loads_vendored_grid_with_nonce() {
        let html = render_intelligence("SANDBOX".into(), "Local".into(), "n0nce\"x".into(), true);
        assert!(html.contains(r#"data-nonce="n0nce&quot;x""#));
        assert!(html.contains(r#"src="/assets/vendor/ag-grid-community.min.js""#));
        assert!(html.contains(r#"src="/assets/intelligence.js""#));
        assert!(html.find("ag-grid-community.min.js") < html.find("/assets/intelligence.js"));
        assert!(html.contains(r#"id="grid""#) && html.contains(r#"id="quick-filter""#));
        for preset in ["flagged", "captured", "held", "reconciliation"] {
            assert!(html.contains(&format!(r#"data-preset="{preset}""#)));
        }
        assert!(html.contains(r#"href="/intelligence" aria-current="page""#));
        // No inline scripts or style attributes: the page must satisfy a strict CSP.
        assert!(!html.contains("<script>") && !html.contains(" style="));
        let logged_out = render_intelligence("SANDBOX".into(), "Local".into(), "n".into(), false);
        assert!(
            !logged_out.contains("intelligence.js") && logged_out.contains(r#"action="/login""#)
        );
    }
}
