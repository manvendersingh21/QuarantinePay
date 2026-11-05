# Three-minute demo

- **0:00–0:20:** Show mode badge. Explain that agents recommend purchases, while the buyer approves an immutable payment intent.
- **0:20–0:45:** Click “Simulate merchant substitution.” Security activity shows intent rejection. No order or PayPal request is created.
- **0:45–1:35:** Confirm a USD 1.00 intent, approve with the Sandbox buyer, authorize, and deliver the failing fixture. Show verification failure followed by provider-confirmed void.
- **1:35–2:25:** Repeat with the passing fixture. Show the countdown, passing verdict, and provider-confirmed capture. Open PayPal Sandbox to corroborate the transaction ID.
- **2:25–2:45:** Ask the local Qwen model for a recommendation and search the local catalog. Show the private-inference badge and the GPU runtime status. If demonstrating Channel3, explicitly switch out of private catalog mode and label external discovery.
- **2:45–2:55:** Show audit history and the repository's Postman collection.

Use the phrasing “AI reasoning and delivery verification stay on our server; PayPal handles the approved payment.” Avoid claiming that all data stays on the host.

Use replay mode for rehearsal and clearly identify it. Record real Sandbox payment evidence only after buyer approval. Avoid claiming escrow, instant production release, or complete injection detection. Do not show credentials, browser session cookies, or personal account information on video.

Submission: choose applicable tracks, verify the current official rules, provide repository access, upload an unlisted video under three minutes, and submit through the user's Devpost account. These account and video actions are not automated by the application.
