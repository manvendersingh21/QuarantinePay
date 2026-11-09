# Validation: agent-contract flow on live PayPal Sandbox (2026-11-09)

Run with `scripts/sandbox-demo.cjs` driving the agent console in headless Chromium, with `QP_MODE=sandbox` and the local `qwen3:30b-instruct` model on a Tesla V100. Sellers and deliveries are fictional; the payments are real PayPal **Sandbox** transactions (test money) to the configured Sandbox merchant. Both stories completed in **167 seconds** of wall-clock time, including model latency and PayPal buyer approval.

| Story | Agent and AI | QuarantinePay result | PayPal Sandbox (fresh API read after the run) |
| --- | --- | --- | --- |
| Injected instructions in a structurally valid report | Model planner. Negotiation selected Quillfire Analytics at USD 48.60 (Northstar USD 67.60; Alder declined scope). Analyst: risk `high`, `recommended_action=reject`; rules (3 findings) and the model (grounded quote) both flagged the injected text. | 8/8 structural checks passed, `policy_hold=true`; approving without acknowledgement returned 409; buyer rejected; state `voided`, USD 0.00 captured | Order `8W039741W8964200U` (AUTHORIZE). Authorization `0N41668771224050H`: **VOIDED**, USD 48.60 |
| Compliant report | Model planner, same contract terms. Analyst: risk `low`, `recommended_action=approve`, no policy hold. | Buyer approved; state `captured` | Order `4AS35513NJ991493X`. Authorization `5XT97367RM571352F`: **CAPTURED**. Capture `2P0047510S322170S`: **COMPLETED**, USD 48.60, `final_capture=true` |

PayPal's `custom_id` on both authorizations equals the ID of the Ed25519-signed intent that embeds the buyer-approved contract (`77756ae9-…` and `b17913d4-…`, SKU `agent-contract-v1`).

An earlier API-driven run the same day also confirmed the same paths: order `3AP08680KJ959245D` / authorization `8YF390610S925722S` (injected delivery, API approval refused with 409, rejected) is **VOIDED** at PayPal, and authorization `4EP33861T7135031R` / capture `1C626993184055022` is **CAPTURED / COMPLETED**. In that run the analyst recommended `reject` for the earlier, less specific compliant fixture because of quality concerns. That was advisory only: no policy hold was set, and the buyer approved the capture. The compliant fixture was then made more specific.

**Failed void on live Sandbox.** To force a void that PayPal must refuse, an agent purchase (order `2fbdf252-…`, authorization `4SM79001VH418320T`, USD 48.60) was taken to buyer review with an injected delivery under a policy hold. The authorization was then captured out of band, directly through the PayPal API (capture `6GA98102D8926882A` COMPLETED), as a merchant might from the PayPal dashboard. When the buyer rejected the delivery, QuarantinePay requested the void, read PayPal's authorization state (`CAPTURED`), recorded `provider_state_conflict` with `claimed_success: false`, and moved the order to `reconciliation_required`. It did not show `voided`, and the console reads "Reconciliation required … no success is claimed" ([screenshot](evidence/5-failed-void-reconciliation.png)). A direct void of that authorization returned PayPal's own refusal, `422 UNPROCESSABLE_ENTITY / PREVIOUSLY_CAPTURED`, and a fresh read still shows `CAPTURED`. Definitive provider refusals (400/404/422) now also stop retries immediately with a `provider_refused` event. Permission refusals (401/403) keep their existing `provider_permission_required` handling.

**Narrated demo video** (`demo/quarantinepay-agent-demo.mp4`, 2:22). It was recorded on live Sandbox, with each shot captured as a separate browser recording and the waits for the model and for PayPal approval cut. Injected delivery: PayPal order `0WH547023E882723F`, authorization `6J16865256880614R`, **VOIDED** at PayPal. Compliant delivery: order `6CP14149978177609`, authorization `4SK8208872604480X`, **CAPTURED** (capture `8PK860870V8793209`). Both were confirmed by fresh PayPal API reads after recording. The narration is synthesized with Microsoft Edge neural TTS (`en-US-AvaNeural`).

**Live credential refusal on the void path.** This ran in an isolated schema on a second instance. An agent order was authorized with the correct credentials (authorization `9GW695555N976531G`) and taken to buyer review. The instance was then restarted with a deliberately wrong PayPal client secret, and the buyer rejected. PayPal refused the void with a real `401 invalid_client` (debug ID `f164647b1c55e`). QuarantinePay recorded `provider_permission_required`, moved the order to `reconciliation_required`, and kept the authorization ID, the `void` operation and its request ID. A fresh PayPal read showed the authorization still `CREATED`, so nothing was claimed. The operator then voided it with the correct credentials (HTTP 204), and PayPal confirms `VOIDED`. A live **403** needs a second Sandbox merchant, which was not available; 401 and 403 share the same code path, and the 403 case is covered by the mocked-provider test.

**Dockerfile-equivalent build.** Container builds cannot run on this host: podman is installed, but the sandbox forbids the mounts needed to apply image layers. The Dockerfile's steps were replicated natively instead:
- `cargo build --release --workspace`;
- the WASM release build plus `wasm-bindgen 0.2.130`, matching the version pinned in the Dockerfile and `Cargo.lock`;
- the runtime layout (`bin/` and `assets/`), started with a cleared environment, `QP_BIND=""` and Render-style `PORT=10011`.

On a fresh schema it migrated, generated its signing key, served `/`, `/agent`, `/intelligence`, the WASM module and the vendored AG Grid, and completed a replay agent purchase through to `captured`. This is not a container build; the Docker image itself remains unbuilt.

**Public deployment.** The same build is served at https://deck-com-range-occasional.trycloudflare.com through a Cloudflare quick tunnel (`scripts/public-tunnel.sh`). `QP_DEMO_BASE=https://deck-com-range-occasional.trycloudflare.com scripts/sandbox-demo.cjs` completed both stories through the public HTTPS origin in 152 s: injected delivery, authorization `64L774879U300943H` **VOIDED**; compliant delivery, authorization `0EV5186317836644V` **CAPTURED** (capture `9MX02262AL010403A`). Both were confirmed by fresh PayPal API reads.

Screenshots: [proposal](evidence/1-agent-proposal.png), [policy hold](evidence/2-injection-policy-hold.png), [captured](evidence/3-clean-delivery-captured.png), [intelligence grid](evidence/4-intelligence-grid.png). Some full-page captures show the fixed sidebar at the scroll position where the capture was taken.

Automated checks on this build: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace`, and the PostgreSQL tests `lifecycle`, `agent_flow`, `permission_denied_stops_retries_and_preserves_authorization` and `local_mirror_search_uses_screened_snapshot` all passed. Not covered: a live 403 permission refusal (mocked test only; the live refusal tested was a provider-state conflict), webhook delivery (no public HTTPS endpoint), Docker builds and Render deployment.

---

> **Historical validation:** The evidence below covers the earlier invoice-only build. It does not establish verification of the five-scenario implementation; run the current tests separately.

# Validation

Validated in the development workspace on 2026-11-06.

## Completed

- Native Cargo workspace build and Rust/WASM browser build; generated WebAssembly bindings.
- Updated MP4 demo with Microsoft Ava neural narration verified at 178.7 seconds with H.264/AAC. The audio track was decoded and checked for speech-level signal in every section. The closing proof slate shows the live Channel3 import and local-search counts; representative frames were inspected and credential screens are excluded.
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
- Live Channel3 import verified on 2026-11-06 using the configured fixed public seed: HTTP 200, 8 received, 8 screened and stored, 0 withheld. A subsequent authenticated search returned 5 results from the local PostgreSQL mirror; every result was marked discovery-only. User search text was not sent to Channel3. Elasticsearch was unavailable during this import, so optional vector screening was disabled for this run; the configured local Qwen model performed listing screening.
- GitHub Actions passed on the catalog-mirror commit, including native build, workspace tests, payment lifecycle, permission handling, PostgreSQL mirror search, and WASM compilation: [run 42149837884](https://github.com/manvendersingh21/QuarantinePay/actions/runs/42149837884).
- Local mock-provider regression tests cover HTTP 401/403 versus 429/500 classification, safe error details, stable request headers, and preservation of PENDING capture status during idempotent reconciliation. An isolated PostgreSQL test proves permission refusals stop after one attempt while preserving the authorization/request ID, and expired intents cannot start new monetary effects.
- Real Meta model listing and live recommendation: HTTP 200. Model recommendation carries no payment authority.

## Requires additional setup or manual verification

- PayPal signed webhook delivery: requires public HTTPS endpoint and PAYPAL_WEBHOOK_ID registration.
- Channel3 live import is verified as described above. To claim the sponsor track in the submission, update the public demo to show the import and local-mirror search. User searches remain local.
- Docker builds and networkless verifier container: Docker is unavailable in this workspace. Local validation uses the trusted Rust JSON-verifier process.
- Public YouTube upload and Devpost submission have not been performed. The official rules require a publicly visible YouTube video under three minutes and cap wins at one Sponsor Prize per project; see the [official rules](https://paypalaihackathon.devpost.com/rules).

The repository's CI runs native checks, local network-safety tests, isolated PostgreSQL lifecycle/permission validation, and a WASM compile. Browser smoke validation was performed externally with Playwright; Playwright is not a runtime dependency.
