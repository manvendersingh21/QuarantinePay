# QuarantinePay submission draft

**QuarantinePay: the trust layer that lets AI agents safely spend money.**

QuarantinePay makes autonomous commerce safe by separating what AI agents recommend from what payment systems are allowed to execute. A buyer agent plans a purchase from a plain-English request. A local Qwen3 model does the planning, and a rule parser is the fallback. The agent then negotiates with three seller agents inside a budget it cannot raise. The outcome is a canonical contract, validated by deterministic policy. The buyer approves that exact contract by its SHA-256 digest, and it is signed into an Ed25519 purchase intent. Only then does PayPal Sandbox authorize, or hold, the funds. The seller's delivery is quarantined. Deterministic checks, injection detection and a local AI analyst examine it. Capture happens only after buyer review. A failed check, a rejection or a missed deadline voids the hold. Any embedded instruction aimed at the AI or the payment system triggers a policy hold that only the buyer can override, explicitly, and the override is audited.

The AI has no payment authority. It can propose an objective, and it can add caution by raising risk or recommending rejection. It cannot raise a budget, choose an unlisted seller, alter a signed contract, clear a failed check or a policy hold, or call PayPal.

Boundaries to state plainly:

- Sellers, goods and deliveries are fictional, and the seller agents are deterministic programs. Replay mode simulates payments.
- Only results returned by PayPal Sandbox count as confirmed. A hold is not escrow. Authorizations expire, and a void can be refused. An earlier test saw `403 PERMISSION_DENIED`, which the app records as `reconciliation_required` and never as a success.
- Structural checks establish structure and counts, not truth or quality. Injection detection cannot catch every attack.
- Channel3 is discovery-only and separate from settlement. The Shopify merchant path runs against a development store, with test orders and no goods shipped. Its fulfilment evidence is the merchant's own Shopify record, read by QuarantinePay, which is stronger than a seller's claim but is not carrier proof. It was verified live on 2026-11-10: a fulfilled order was captured, and a cancelled order and one with injected tracking text were both voided.

**Verification status:** Unit tests cover contract policy, negotiation bounds, signature and policy binding, delivery checks and injection rules. PayPal Sandbox capture and void were confirmed for an *earlier* build (see `VALIDATION.md`). The live Sandbox run of the agent-contract flow, with the local-model planner and analyst, was verified on 2026-11-09: capture `2P0047510S322170S` COMPLETED and the injected-delivery authorization `0N41668771224050H` VOIDED, both by fresh PayPal API reads (`docs/VALIDATION.md`). The Render Blueprint has not been deployed.

Built with Rust, Axum, PostgreSQL, PayPal Orders v2 Sandbox, Ollama Qwen3, AG Grid Community, leptos and WASM.

See the [README](../README.md), [DEMO.md](DEMO.md) and [DEPLOY.md](DEPLOY.md). Verify every claim against the running build before publication.
