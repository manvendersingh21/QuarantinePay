# Commerce simulation work board

- Core scenario catalog, signed terms, and verification: scenarios agent
- Storefront and buyer review controls: UI agent
- Setup, demo, submission, and developer program research: documentation agent
- Order lifecycle, review authorization, payment boundaries, and verification: primary agent

Agents coordinate interfaces through the shared task messages; this board records ownership to avoid conflicting edits.

## UI agent

- Implemented five catalog-backed cards, signed terms and evidence, seller fixtures, buyer review, replay decline selection, mode labels, and reduced-motion-aware 3D card animation.
- `cargo check -p quarantinepay-ui` and `cargo test -p quarantinepay-ui` passed.

## Integration status

- Scenario lifecycle, replay decline and ambiguous results, authenticated review, reset, and migration implemented.
- Read-only Shopify development-store connector implemented; external credentials were not available for a live check.
- Browser smoke run completed all five successful local journeys at mobile width, with desktop overflow checked; PostgreSQL lifecycle, recovery, and provider-permission checks passed; replay reset was verified in the browser.

## Agentic commerce upgrade

- Contracts, negotiation, delivery checks, agent task API, worker, migration `0005`: core/server agents (`crates/**`, `migrations/**`).
- Agent console (`/agent`), intelligence grid (`/intelligence`), vendored AG Grid Community: UI agent (`crates/ui`, `assets/**`).
- Docs and deployment: documentation agent. Owns `README.md`, `docs/PRD.md`, `docs/DEMO.md`, `docs/DEPLOY.md`, `docs/openapi.json`, `docs/QuarantinePay.postman_collection.json`, `docs/SUBMISSION.md`, `docs/DEVPOST_DRAFT.md`, `render.yaml`, `.env.example`, `Dockerfile`.
- Docs describe the agent API as specified in the task brief. Live PayPal Sandbox evidence for the agent flow is **pending verification**. Report interface changes (field names, states, status codes) so the docs, OpenAPI and Postman stay accurate.
