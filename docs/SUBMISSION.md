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
- `docs/YOUTUBE_DESCRIPTION.md`, `docs/quarantinepay-demo.en.srt`, and `docs/quarantinepay-thumbnail.png`: upload description draft, English captions, and thumbnail for the local recording.

## Track eligibility

The target is the PayPal Grand Prize plus **Best Use of Channel3**, contingent on a live API import and a demo showing that integration. The official rules permit a project to win at most one Grand Prize plus one Sponsor Prize, or one Honorable Mention plus one Sponsor Prize; winning every sponsor track is not possible. Do not claim Channel3 eligibility until a real import is exercised. AG Grid/AG Studio, Bryntum, APIMatic, and Render are not integrated, so do not claim those sponsor prizes. Sponsor tools are optional for the main PayPal submission.

The official submission deadline is **November 12, 2026 at 12:00 p.m. Pacific Time**. Required materials include a working demo, public open-source repository with a license, and a demonstration video under three minutes that is publicly visible on YouTube. An unlisted YouTube video does not meet the published visibility requirement. Confirm the [current official rules](https://paypalaihackathon.devpost.com/rules) before submitting, since Devpost says the rules can change.

## Publication checklist

Review the finished video for credentials, personal information, and third-party copyrighted material; keep it under three minutes. Upload the reviewed video as **public** on YouTube, then provide its link with the public repository and working-demo instructions in the Devpost submission. Public repository visibility, YouTube upload, and Devpost publication are release steps separate from the local application build.
