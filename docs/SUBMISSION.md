# QuarantinePay submission draft

## Pitch

QuarantinePay makes agent-assisted purchases verifiable: the buyer confirms a signed purchase intent, PayPal authorizes a hold, and delivery verification gates capture or void. AI recommendations have no payment authority.

## What the demo proves

- A changed merchant ID fails Ed25519 verification before a PayPal order is created.
- A failing digital delivery produces a confirmed Sandbox void; a passing delivery produces a confirmed Sandbox capture.
- Intent nonces are consumed once. Expired intents cannot initiate another monetary effect, while compensating voids remain possible.
- Model prompts and delivery verification run locally. Qwen3-30B-A3B Instruct runs on a Tesla V100 with 32 GB VRAM; the local model server's cloud mode is disabled.
- A white Rust-rendered dashboard displays the authorization lifecycle, countdown, private-inference configuration, and durable audit events.

Use this privacy wording: **AI reasoning and delivery verification stay on our server. PayPal handles the approved payment.**

## Implementation

Rust application and verifier; Axum/Tokio; PostgreSQL/SQLx; Ed25519; Leptos SSR plus Rust/WASM browser behavior; local Ollama/Qwen inference; local Elasticsearch vector screening; PayPal Orders v2 and Payments v2.

The example delivery is a bounded JSON invoice fixture checked against a signed deterministic policy. This prototype does not claim to verify arbitrary software, malware, physical shipping, or all prompt injections. Authorization holds are not escrow. All payment demonstrations use Sandbox accounts.

## Judge resources

- Repository README: installation, local-model setup, Sandbox configuration, architecture, and security limits.
- `docs/QuarantinePay.postman_collection.json`: signed intent → checkout → authorization → delivery → settlement inspection.
- `docs/openapi.json`: public API schema.
- `docs/VALIDATION.md`: completed tests and live service evidence.
- `docs/PRIVACY.md`: data destinations and enforced local-inference defaults.
- `docs/DEMO.md`: demo outline and publication notes.

## Track eligibility

The current verified integration supports the main PayPal project submission. Channel3 is implemented as an explicit external-discovery option but requires a live key and actual usage before claiming that sponsor track. AG Grid/AG Studio, Bryntum, and APIMatic's TypeScript SDK are excluded by the strict Rust application requirement; do not claim those integrations. Verify the current official rules before selecting categories.

## Publication checklist

Review the finished video for credentials and personal information; keep it under three minutes. Provide the repository access required by the contest, upload the reviewed video as unlisted, then submit through the project owner's Devpost account. Public repository visibility, YouTube upload, and Devpost publication are release steps separate from the local application build.
