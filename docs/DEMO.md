# Three-minute demo

- **0:00–0:20:** Show mode badge. Explain that agents recommend purchases, while the buyer approves an immutable payment intent.
- **0:20–0:45:** Click “Simulate merchant substitution.” Security activity shows intent rejection. No order or PayPal request is created.
- **0:45–1:35:** Confirm a USD 1.00 intent, approve with the Sandbox buyer, authorize, and deliver the failing fixture. Show verification failure followed by provider-confirmed void.
- **1:35–2:25:** Repeat with the passing fixture. Show the countdown, passing verdict, and provider-confirmed capture. Open PayPal Sandbox to corroborate the transaction ID.
- **2:25–2:45:** Ask the local Qwen model for a recommendation and search the local catalog. Show the private-inference badge and GPU runtime status. If a Channel3 key is configured and a live import has been verified, show the operator importing the fixed public seed, then search the local mirror; state that buyer searches remain local and imported items are discovery-only.
- **2:45–2:55:** Show audit history and the repository's Postman collection.

Use the phrasing “AI reasoning and delivery verification stay on our server; PayPal handles the approved payment.” Avoid claiming that all data stays on the host.

Use replay mode for rehearsal and clearly identify it. Record real Sandbox payment evidence only after buyer approval. Avoid claiming escrow, instant production release, or complete injection detection. Do not show credentials, browser session cookies, or personal account information on video.

Submission: target the Grand Prize and, only after proving live Channel3 usage, the Channel3 Sponsor Prize. The official rules cap wins at one Sponsor Prize per project. Upload a video under three minutes publicly to YouTube (unlisted visibility does not qualify), then submit its link and the public repository through the user's Devpost account. These account and video actions are not automated by the application. See the [official rules](https://paypalaihackathon.devpost.com/rules).

A narrated local recording was generated at `.local/quarantinepay-demo.mp4` in the validated workspace: 146.2 seconds, H.264 video and AAC audio. Only the application page is recorded; PayPal credential and login screens are excluded. The file is kept outside Git until publication.
