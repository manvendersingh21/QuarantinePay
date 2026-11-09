# QuarantinePay: PRD

## 1. What it is

A Rust hackathon prototype. A buyer agrees to terms, the money is only authorized (held), the seller delivers, the delivery is checked, and only then is the payment captured. If the check fails, the buyer rejects, or the deadline passes, the authorization is voided.

Tagline: **Verify delivery. Then settle.**

It is a demo, not a marketplace and not escrow. Sellers, goods and deliveries are fictional. In the default mode no real money moves.

## 2. Problem

Paying before you know the goods are right is risky, and this is worse for AI agents buying on a user's behalf. The idea is to separate "approve payment" from "settle payment" and put an automatic check, or a human review, in between.

## 3. Users

| User | Role |
| --- | --- |
| Administrator / demo buyer | The only logged-in user. Picks a scenario, confirms terms, authorizes payment, reviews delivery. |
| Fictional sellers | Simulated. Their deliveries are submitted through the UI or API. |
| Hackathon judges | Watch the demo and read the docs. |

Single admin, single demo buyer. No multi-user accounts.

## 4. Scenarios

All cost USD 1.00 and have a 120-second deadline (shortened for the demo).

| Scenario | Seller / item | Passes if | Buyer review |
| --- | --- | --- | --- |
| Dataset | Alder Data, City mobility CSV | CSV has columns `id`, `date`, `value` and at least 2 complete rows | No |
| Report | Northstar Research, Market report | Named Markdown report with nonempty `Executive summary` and `Findings` | Yes |
| API data | Harbor API, Weather response | At least 2 records, each with nonempty `id` and numeric `temperature` | No |
| Freelance milestone | Lantern Studio, Design milestone | File name and description present | Yes |
| Physical goods | Cedar Supply, Desk lamp | Simulated "delivered" tracking evidence | Yes |

Defined in `crates/core/src/scenarios.rs`.

## 5. Core flow

1. Pick a scenario and inspect its terms.
2. Server creates a **signed intent** (terms + acceptance policy + expiry).
3. Buyer confirms. The intent is used once (`POST /api/checkout`).
4. Buyer authorizes the payment (hold, not capture).
5. Seller submits a delivery.
6. Verifier checks it. Structure only, nothing is executed.
7. If the scenario needs review, buyer approves or rejects.
8. Outcome: **capture** (settle) or **void** (release the hold).

### Order states

`authorize_pending` → `authorized` → `capture_pending` → `captured`
`void_pending` → `voided`
`reconciliation_required` is used when the payment result is uncertain.

A failed check, a buyer rejection, or a missed deadline all lead to void.

### Demo payment outcomes (replay mode)

Chosen when creating the order: success, simulated decline, or ambiguous capture. Ambiguous ends in `reconciliation_required`, never a claimed success.

## 6. Functional requirements

- **Catalog:** five fixed scenarios, served at `GET /api/scenarios`.
- **Signed intents:** signature, expiry, one-time use, and mode isolation (replay vs sandbox).
- **Payments:** durable payment operations in Postgres, stable request IDs for retries, uncertain results stay pending or go to reconciliation.
- **Verification:** separate verifier binary, bounded JSON parsing, no executing submitted assets.
- **Buyer review:** required for report, milestone, physical goods, after the structural checks pass.
- **Deadlines:** accelerated 120 s window, missed deadline voids.
- **Audit log:** every event stored, streamed at `GET /api/events?after=<id>`.
- **UI:** native HTML forms served by the server. Optional Rust/WASM adds live refresh and countdowns.
- **Reset:** `/ui/reset` to rerun demos. Old orders and audit events stay in the DB.
- **Auth:** admin password; API uses `Authorization: Bearer <QP_ADMIN_PASSWORD>`. Browser uses HttpOnly SameSite cookie plus Origin check on mutations.

## 7. Modes and integrations

| Piece | Status | Notes |
| --- | --- | --- |
| Replay mode (`QP_MODE=replay`) | Default | Everything simulated locally. No external account needed. |
| PayPal Sandbox (`QP_MODE=sandbox`) | Optional | Never production. Needs client ID/secret, merchant ID, HTTPS `QP_PUBLIC_BASE`, webhook at `/api/webhooks/paypal`. Only real provider confirmation counts as confirmed. |
| Shopify dev store | Optional, read-only | `GET /api/shopify/dev-store`. Requires a partner development store, reads up to 10 products and 10 test orders. Does not create orders or connect to settlement. **Not tested live** (no credentials). |
| Channel3 catalog | Optional | Product discovery import only. Discovery data, no payment authority. |
| Local LLM (Qwen3-30B via Ollama) | Optional | Assistant and listing injection screening. Not needed for scenarios, no payment authority. |
| Elasticsearch | Optional | Loopback only, vector screening. |
| eBay, Square | Not built | Only researched in `docs/DEVELOPER_PROGRAMS.md`. |

## 8. Architecture

Cargo workspace:

| Crate | Purpose |
| --- | --- |
| `core` | Intents, signing, scenarios, verdict types |
| `server` | Axum web server, DB, worker, PayPal, Shopify, catalog, inference |
| `verifier` | Standalone delivery checker |
| `ui` | HTML rendering |
| `browser` | Optional WASM module |

Storage: PostgreSQL 16+. Migrations 0001–0004 (init, payment modes, catalog cache, scenario review).

Server files: `main.rs` (routes), `db.rs`, `worker.rs` (background payment/deadline work), `paypal.rs`, `shopify.rs`, `catalog_cache.rs`, `inference.rs`, `integrations.rs`, `config.rs`.

## 9. API summary

| Endpoint | Purpose |
| --- | --- |
| `GET /api/scenarios` | Catalog |
| `POST /api/intents` | Create signed intent |
| `POST /api/checkout` | Submit intent unchanged |
| `POST /api/orders/{id}/authorize` | Authorize payment |
| `POST /api/orders/{id}/deliveries` | Submit delivery |
| `POST /api/orders/{id}/review` | Buyer approve / reject |
| `GET /api/orders/{id}` | Order state |
| `GET /api/events` | Audit stream |
| `GET /api/catalog/search`, `POST /api/catalog/sync` | Catalog import |
| `POST /api/agent/runs` | Assistant run |
| `GET /api/shopify/dev-store` | Read-only Shopify check |
| `POST /api/webhooks/paypal` | PayPal webhook |
| `GET /health` | Health |

Full spec: `docs/openapi.json`.

## 10. Non-goals

- Not escrow. A hold is not escrow.
- Not a production marketplace. One admin, one buyer.
- No card data ever entered into the app.
- Checks cannot prove truth or quality. The report check only looks for headings and nonempty sections. The milestone has a file name and description, not file contents. Physical evidence cannot prove product condition.
- AI cannot move money.
- Prompt injection screening cannot be guaranteed.
- Shopify is not linked to settlement.

## 11. Security

- Signed, expiring, single-use intents.
- Mode isolation so replay orders can't settle in sandbox.
- Verifier never executes submitted data.
- Secrets (`.env`, `.local/`, signing key) are git-ignored. Back up `.local/signing-key` if orders must survive a migration.
- Privacy default: AI reasoning and delivery verification stay on the server; PayPal only gets approved payment fields (`docs/PRIVACY.md`).

## 12. Setup

1. Rust stable and PostgreSQL 16+.
2. Set `DATABASE_URL`, `QP_MODE=replay`, `QP_ADMIN_PASSWORD` (24+ chars, or one is generated in `.local/admin-password`).
3. `cargo build --workspace`, then `RUST_LOG=info cargo run -p quarantinepay-server`.
4. Open `http://localhost:3000` and log in.
5. Optional: `make build` for the WASM assets.

## 13. Testing

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Postgres lifecycle test (ignored by default): `cargo test -p quarantinepay-server --test lifecycle -- --ignored` with `DATABASE_URL` set. It uses an isolated schema.

Status noted in the board: all five local journeys passed in a browser smoke run; Postgres lifecycle, recovery and provider-permission checks passed.

## 14. Known gaps

- Existing demo video and screenshots predate the five scenarios and need re-recording.
- Shopify connector untested against a real dev store.
- PayPal Sandbox not covered by local tests; needs your own Sandbox credentials.
- `docs/QuarantinePay.postman_collection.json` still describes the old invoice-only flow.
- eBay and Square adapters don't exist.

## 15. Docs map

| File | Contents |
| --- | --- |
| `README.md` | Setup, boundaries, API |
| `docs/DEMO.md` | Demo script |
| `docs/SUBMISSION.md`, `DEVPOST_DRAFT.md`, `YOUTUBE_DESCRIPTION.md` | Submission text |
| `docs/DEVELOPER_PROGRAMS.md` | Shopify, eBay, Square, PayPal research |
| `docs/PRIVACY.md` | Where data goes |
| `docs/VALIDATION.md` | Validation notes |
| `docs/openapi.json` | API spec |
