# Devpost submission draft

**Status:** Draft only. Submit through the project owner's Devpost account after the final video and integration checks are complete.

## Project name

QuarantinePay

## Tagline

Verify the delivery before the agent's payment settles.

## Short description

QuarantinePay binds a buyer's approval to an exact purchase, holds PayPal Sandbox authorization during delivery verification, and captures or voids only after a deterministic result.

## Project description

Agent recommendations and payment authority should not share the same trust boundary. A tool call can change its merchant or amount after a buyer thinks they have approved a purchase, and a successful payment does not prove that the promised digital delivery is correct.

QuarantinePay is a Rust payment gateway that binds a buyer, allowlisted product, merchant, amount, currency, expiration, and acceptance policy into an Ed25519-signed intent. A recommendation model cannot change those fields or call payment operations. After the buyer approves the PayPal Sandbox order, the backend authorizes it and places the digital delivery in quarantine. A bounded Rust verifier checks the submitted JSON invoice against the signed deterministic policy. Passing delivery queues capture; a failed check or deadline queues void. The worker persists idempotency identifiers and verifies the final state with PayPal before recording settlement.

The demo shows a merchant-substitution attempt rejected before order creation, a failing invoice followed by a PayPal-confirmed void, and a passing invoice followed by a PayPal-confirmed capture. The assistant runs Qwen3-30B-A3B locally on a Tesla V100 GPU and has no payment tools or credentials. An operator can import a fixed public catalog seed from Channel3; returned listings are screened locally and stored in a PostgreSQL mirror, while buyer searches stay local. The live import returned eight listings, all eight passed screening, and a follow-up local search found five. Imported listings are discovery-only and cannot alter checkout. User prompts and delivery verification stay on the application host; PayPal receives the approved payment fields required to process the transaction. All payment activity in the demo is Sandbox activity. An authorization hold is not escrow, and the prototype verifies only the bounded invoice fixture rather than arbitrary software or physical delivery.

## How we built it

Rust with Axum and Tokio for the API and worker; Ed25519-signed immutable purchase intents; PostgreSQL and SQLx for durable payment state and audit events; a deterministic Rust invoice verifier; Leptos server-rendered UI with an optional Rust/WASM refresh layer; local Ollama inference with Qwen3-30B-A3B; and PayPal Orders v2 / Payments v2 Sandbox APIs. The Postman collection documents the authorize, verify, capture/void lifecycle.

## What was challenging

Payment APIs can return ambiguous outcomes, and a retry must not create a second charge or lose the authorization needed for a compensating void. QuarantinePay records a stable request identifier before each provider effect, reconciles provider state, and stops retrying permission failures while preserving the authorization for operator review. The other difficult boundary was keeping model output away from payment authority: the model can recommend, while the gateway independently validates the buyer-bound signed fields.

## AI use

Qwen3-30B-A3B Instruct runs locally through Ollama on the host GPU for product recommendations and supporting listing-screening judgments. The model has no PayPal credentials or capture/void tools; model output cannot initiate settlement. There is no cloud fallback. The signed intent and deterministic verifier, not the model, govern payment effects.

Development was AI-assisted with Codex. Claude Opus 5.5 and Gemini 3.8 Flash through Agy reviewed source snapshots at the user's request; those snapshots excluded `.env` and `.local`. These reviewer models are not part of the running product. Listing screening is a defense-in-depth signal, not a guarantee that every injection is caught.

## Built with

Rust, PayPal Sandbox, Qwen3-30B-A3B, Ollama, PostgreSQL, Ed25519, Axum, Tokio, Leptos, WebAssembly, Elasticsearch, Postman.

## Public assets and testing

- Repository: https://github.com/manvendersingh21/QuarantinePay
- Demo video: **add the public YouTube URL after upload**. The updated local recording is 165.6 seconds and includes a proof slate from the verified live Channel3 import and local search. It summarizes the results but does not show the operator import UI interaction.
- Setup and test steps: `README.md`
- API requests: `docs/QuarantinePay.postman_collection.json`
- API schema: `docs/openapi.json`
- Demo outline, captions, description, and thumbnail: `docs/DEMO.md`, `docs/quarantinepay-demo.en.srt`, `docs/YOUTUBE_DESCRIPTION.md`, and `docs/quarantinepay-thumbnail.png`
- Privacy boundaries and known limitations: `docs/PRIVACY.md`

## Track selection checklist

- PayPal Grand Prize: the application uses PayPal Sandbox centrally and demonstrates buyer approval, authorization, and verified capture/void.
- Best Use of Channel3: **live API import verified (8 listings screened and stored; 5 found in local search). The updated local demo shows a result slate, not the operator UI flow; publish it and review the track requirements before claiming this prize.**
- AG Grid, APIMatic, Bryntum, and Render: not integrated; do not claim these sponsor prizes.
- A project may win at most one sponsor prize, alongside a Grand Prize or an Honorable Mention. See the [official rules](https://paypalaihackathon.devpost.com/rules).

## Before submission

1. Publish the updated 165.6-second recording publicly to YouTube and replace the video URL placeholder. The recording shows the verified Channel3 results slate; it does not show the operator UI interaction.
2. Check every submission statement against the build and public recording.
3. Submit via the project owner's Devpost account before the published deadline.

The Git history shows the project was created on November 5, 2026, during the published submission period. Recheck the [official rules](https://paypalaihackathon.devpost.com/rules) before submitting in case they change.
