# Validation

Validated in the development workspace on 2026-11-05.

## Completed

- Native Cargo workspace build and Rust/WASM browser build; generated WebAssembly bindings.
- Narrated MP4 demo generated locally, verified at 146.2 seconds with H.264/AAC. Representative frames were inspected; credential screens are excluded.
- Formatting and Clippy with warnings denied.
- Three core tests covering every signed field, buyer/expiry checks, deterministic invoice acceptance, unknown fields, and integer overflow.
- PostgreSQL integration scenario in an isolated schema: altered merchant rejection, consumed-intent replay rejection, passing delivery/capture, failing delivery/void, expiry/void, restart recovery, valid and expired browser sessions, and cross-origin mutation rejection.
- White-background dashboard checked with an emulated dark system preference: page, sidebar, cards, and inputs retain white backgrounds.
- Replay/Sandbox isolation: foreign-mode intents cannot be consumed, and foreign-mode orders are neither exposed nor processed.
- Headless Chromium dashboard check: login, merchant substitution rejection, passing capture, failing void, event-driven UI updates, desktop screenshot, and 390px mobile layout. No page errors and no horizontal overflow.
- Real PayPal Sandbox OAuth and buyer approval with credentials and merchant ID from the same business account.
- Both full Sandbox flows verified through the dashboard: passing quarantine → capture, failing quarantine → void. Fresh PayPal API reads independently confirmed CAPTURED / COMPLETED and VOIDED, USD 1.00, signed intent references, and the expected recipient.
- The earlier test authorization was voided through its original merchant dashboard. A fresh PayPal API read confirmed VOIDED, and the local ledger was reconciled.
- Elasticsearch 9.5.6 installed with its official SHA-512 checksum verified, running as an unprivileged service on 127.0.0.1:19200. The three-document attack index was created and live vector queries executed during private catalog search.
- Local Qwen3-30B-A3B Instruct downloaded and successfully queried through the Rust API. Ollama reports a 19.4 GB loaded footprint entirely on the Tesla V100 GPU with an 8,192-token context.
- Private local catalog lookup returned a screened product. The local Ollama server rejected a cloud-model request with HTTP 403 and “ollama cloud is disabled.” Unit tests reject remote model routes, cloud model tags, cloud/private provider combinations, and redirect-based prompt forwarding.
- Local mock-provider regression tests cover HTTP 401/403 versus 429/500 classification, safe error details, stable request headers, and preservation of PENDING capture status during idempotent reconciliation. An isolated PostgreSQL test proves permission refusals stop after one attempt while preserving the authorization/request ID, and expired intents cannot start new monetary effects.
- Real Meta model listing and live recommendation: HTTP 200. Model recommendation carries no payment authority.

## Requires additional setup or manual verification

- PayPal signed webhook delivery: requires public HTTPS endpoint and PAYPAL_WEBHOOK_ID registration.
- Channel3 live search: requires CHANNEL3_API_KEY and an explicit opt-out of private catalog mode. Adapter follows the official v1 search endpoint but has not been exercised with a live key.
- Docker builds and networkless verifier container: Docker is unavailable in this workspace. Local validation uses the trusted Rust JSON-verifier process.
- Public deployment, video upload, and Devpost submission have not been performed.

The repository's CI runs native checks, local network-safety tests, isolated PostgreSQL lifecycle/permission validation, and a WASM compile. Browser smoke validation was performed externally with Playwright; Playwright is not a runtime dependency.
