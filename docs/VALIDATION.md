# Validation

Validated in the development workspace on 2026-11-05.

## Completed

- Native Cargo workspace build and Rust/WASM browser build; generated WebAssembly bindings.
- Formatting and Clippy with warnings denied.
- Three core tests covering every signed field, buyer/expiry checks, deterministic invoice acceptance, unknown fields, and integer overflow.
- PostgreSQL integration scenario in an isolated schema: altered merchant rejection, consumed-intent replay rejection, passing delivery/capture, failing delivery/void, expiry/void, restart recovery, valid and expired browser sessions, and cross-origin mutation rejection.
- White-background dashboard checked with an emulated dark system preference: page, sidebar, cards, and inputs retain white backgrounds.
- Replay/Sandbox isolation: foreign-mode intents cannot be consumed, and foreign-mode orders are neither exposed nor processed.
- Headless Chromium dashboard check: login, merchant substitution rejection, passing capture, failing void, event-driven UI updates, desktop screenshot, and 390px mobile layout. No page errors and no horizontal overflow.
- Real PayPal Sandbox OAuth: HTTP 200; corrected merchant ID retrieved from the authenticated Sandbox business profile. Order creation returned PAYER_ACTION_REQUIRED and the configured payee.
- Real Sandbox buyer approval, authorization, passing quarantine, and capture completed through the dashboard. Fresh PayPal reads independently confirmed CAPTURED authorization, COMPLETED capture, USD 1.00, the signed intent reference, and the expected recipient.
- Failed delivery was quarantined and its void requested. PayPal returned HTTP 403 NOT_AUTHORIZED / PERMISSION_DENIED, including with an explicit JSON Content-Type. The authorization remains CREATED. The unresolved order is preserved as reconciliation_required; retries are stopped and the app is in replay mode pending credentials with void permission.
- Real Meta model listing and live recommendation: HTTP 200. Model recommendation carries no payment authority.

## Requires additional setup or manual verification

- PayPal Sandbox void: requires REST app credentials authorized to void this merchant account's authorizations. Buyer approval, authorization, and capture are verified; automated void is not yet verified. Resolve the preserved test authorization after credential setup before recording the final demo.
- PayPal signed webhook delivery: requires public HTTPS endpoint and PAYPAL_WEBHOOK_ID registration.
- Channel3 live search: requires CHANNEL3_API_KEY. Adapter follows the official v1 search endpoint but has not been exercised with a live key.
- Elasticsearch vector screening: requires an Elasticsearch endpoint/key. The optional index and deterministic feature vectors are implemented but have not been live-validated here.
- Docker builds and networkless verifier container: Docker is unavailable in this workspace. Local validation uses the trusted Rust JSON-verifier process.
- Public deployment, video upload, and Devpost submission have not been performed.

The repository's CI runs native checks, isolated PostgreSQL lifecycle validation, and a WASM compile. Browser smoke validation was performed externally with Playwright; Playwright is not a runtime dependency.
