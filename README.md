# QuarantinePay

**Verify delivery. Then settle.** QuarantinePay is a Rust hackathon demonstration of buyer-approved terms, delivery checks, and payment settlement. Five fictional scenarios cover a dataset, report, API data, freelance milestone, and physical goods. The default replay mode simulates payment activity locally; no real funds move. The storefront, sellers, goods, delivery events, and accelerated deadlines are fictional in every mode.

## Run locally

Requires stable Rust and PostgreSQL 16 or later. The core journeys do not require a model download, GPU, external account, or payment credentials.

1. Create a PostgreSQL database and set `DATABASE_URL`. Copy `.env.example` to `.env` only if you do not already have a credential file.
2. Set `QP_MODE=replay` (the default) for simulated payments. Set `QP_ADMIN_PASSWORD` to a random value of at least 24 characters, or let the server generate one in `.local/admin-password`.
3. Run `cargo build --workspace` and then `RUST_LOG=info cargo run -p quarantinepay-server` from the repository root. Open `http://localhost:3000` and sign in as the administrator.
4. If no password was configured, read the generated password locally with `od -An -v -tx1 .local/admin-password | tr -d ' \n'`. Never share or commit it.

The UI uses native HTML forms. The optional Rust/WASM module adds live refresh and countdowns; use `make build` to build browser assets if the WASM target and matching `wasm-bindgen-cli` are installed. Without it, refresh the page manually. Each new purchase is a repeatable demonstration, while previous orders and audit events remain in the database.

Choose a scenario, inspect its terms, confirm the intent, authorize the simulated payment, and submit a passing or failing delivery. Report, milestone, and physical goods require buyer review after their structural checks pass. Rejecting a delivery requests a void; failing a check or missing the accelerated deadline also leads to a void. Physical delivery evidence is simulated and does not prove product quality. See the [demo guide](docs/DEMO.md).

## Fictional catalog

All five items cost USD 1.00 and use a 120-second accelerated delivery window.

| Scenario | Seller and item | Acceptance terms |
| --- | --- | --- |
| Dataset | Alder Data · City mobility CSV | CSV columns `id`, `date`, and `value`, with at least two complete rows. |
| Report | Northstar Research · Market report | A named Markdown report with nonempty `Executive summary` and `Findings` sections, followed by buyer review. |
| API data | Harbor API · Weather response | At least two records with a nonempty `id` and numeric `temperature`. |
| Freelance milestone | Lantern Studio · Design milestone | File and description, followed by buyer review. |
| Physical goods | Cedar Supply · Desk lamp | Simulated delivered tracking evidence, followed by buyer review; evidence does not prove product quality. |

## Payment and integration boundaries

Replay results are simulated, including authorization and settlement; they are not externally confirmed. Select a simulated decline or ambiguous capture when creating an order. An ambiguous capture ends in `reconciliation_required`, not a claimed success. PayPal Sandbox is optional and never uses production endpoints. Set `QP_MODE=sandbox`, `PAYPAL_CLIENT_ID`, `PAYPAL_CLIENT_SECRET`, and `PAYPAL_MERCHANT_ID` to values from the same Sandbox business account. The merchant ID is its 13-character account identifier. Configure an HTTPS `QP_PUBLIC_BASE`, register `/api/webhooks/paypal`, and set `PAYPAL_WEBHOOK_ID`. Use a separate Sandbox buyer account for approval. Open the approval link and then confirm authorization in QuarantinePay. Call a payment externally confirmed only when the provider has actually confirmed the result; uncertainty requires reconciliation. Goods, seller, and delivery events remain fictional in Sandbox mode.

Never enter card data into QuarantinePay. Provider-hosted Sandbox checkout is separate; consult the provider's documented test values and supported triggers. The [developer program research](docs/DEVELOPER_PROGRAMS.md) describes Shopify, eBay, Square, PayPal, and their access requirements. No eBay or Square adapter is included.

The optional read-only Shopify connector requires a user-controlled partner development store. Set `SHOPIFY_DEV_STORE` to its lowercase `<store>.myshopify.com` hostname and `SHOPIFY_ADMIN_ACCESS_TOKEN` to an Admin API token with `read_products` and `read_orders` scopes. With the administrator bearer credential, request `GET /api/shopify/dev-store`. The connector first requires Shopify to confirm `shop.plan.partnerDevelopment`, then returns up to ten products and ten test orders; it rejects non-test order data. It does not create orders, connect Shopify checkout to QuarantinePay settlement, or prove a payment occurred. The connector has not been externally tested in this environment because credentials were unavailable. If unconfigured, use the fictional local storefront. See [Shopify development stores](https://shopify.dev/docs/apps/build/stores/development-stores) and [Shop plan fields](https://shopify.dev/docs/api/admin-graphql/latest/objects/shopplan).

Optional local inference and Channel3 catalog screening remain separate from settlement; the local model is not required for the scenarios and has no payment authority. If enabling optional inference, consult `.env.example` and [privacy boundaries](docs/PRIVACY.md). Imported third-party catalog data is discovery-only.

## Security and verification

The administrator confirms signed purchase terms. Signatures, expiry, one-use intent consumption, and mode isolation protect the purchase boundary. The application preserves durable payment operations and uses stable request IDs for retries; an uncertain result must remain pending or require reconciliation. A verifier checks submitted delivery data without executing it. The report checks submitted Markdown text for named headings and nonempty sections, but does not verify its claims. The milestone example contains a file name and description rather than uploaded file contents. Deterministic structure and completeness checks cannot establish truth or subjective quality, and physical delivery evidence cannot prove condition. A hold is not escrow.

This is a single-administrator, single-demo-buyer prototype, not a production marketplace. Browser sessions use HttpOnly, SameSite cookies and Origin checks on mutations. Keep passwords, signing keys, credentials, and database data out of Git. Back up `.local/signing-key` if orders need to survive deployment migration. A model or listing screening cannot guarantee detection of prompt injection.

## Tests and API

Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`. The PostgreSQL lifecycle integration test is ignored by default: build the workspace, set `DATABASE_URL`, and run `cargo test -p quarantinepay-server --test lifecycle -- --ignored`. It creates an isolated schema. Optional external integration checks require separately configured Sandbox credentials; a local run does not establish external confirmation.

API requests use `Authorization: Bearer <QP_ADMIN_PASSWORD>`. The catalog is available at `GET /api/scenarios`. Signed intents are created at `POST /api/intents`, then submitted unchanged to `POST /api/checkout`; deliveries go to `POST /api/orders/{id}/deliveries`, reviews to `POST /api/orders/{id}/review`, and order state is available at `GET /api/orders/{id}`. `GET /api/events?after=<event-id>` streams durable audit events. See [OpenAPI](docs/openapi.json) for scenario, review, and Shopify endpoints. The older [Postman collection](docs/QuarantinePay.postman_collection.json) describes the previous invoice-only flow.

## License

MIT. See [LICENSE](LICENSE).
