# Devpost draft

**Status:** Draft; verify against the running build and new demo recording before submission.

## QuarantinePay

**Verify delivery before settlement.**

QuarantinePay demonstrates how a buyer can agree to a purchase and acceptance terms before a simulated seller submits a delivery. Five fictional journeys cover a dataset, a report, API data, a freelance milestone, and physical goods. Deterministic checks assess agreed structure and completeness. A report, milestone, and simulated physical delivery require buyer review where subjective judgment matters. The report checks submitted Markdown text for required nonempty sections; the milestone uses a simulated file name and description rather than uploaded file contents. A shipment event is not proof of product quality.

The buyer intent is signed, the lifecycle and its reasons are recorded, and an automated worker performs settlement only after the applicable checks or buyer decision. AI can assist with explanations or recommendations but has no authority to settle a payment. The default payment mode is simulated and moves no money. An optional PayPal Sandbox payment can be called provider-confirmed only after PayPal actually confirms it; the fictional goods and evidence remain simulated. The optional read-only Shopify connector can retrieve development-store products and test orders; it has not been externally tested here and does not link Shopify checkout to QuarantinePay settlement.

Built with Rust, Axum, PostgreSQL, Ed25519, and optional PayPal Sandbox. This is a hackathon prototype, not a production marketplace or escrow service. No model download is required for the local purchase journeys.

The checked-in recording demonstrates an earlier invoice flow; replace it with a current recording before claiming that it shows these five scenarios. See [DEMO.md](DEMO.md), [DEVELOPER_PROGRAMS.md](DEVELOPER_PROGRAMS.md), and the README for demonstration steps, test access requirements, and boundaries. Confirm current competition rules and any sponsor eligibility separately before submitting.
