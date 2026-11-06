# Three-minute demo

- **0:00–0:20:** Show mode badge. Explain that agents recommend purchases, while the buyer approves an immutable payment intent.
- **0:20–0:45:** Click “Simulate merchant substitution.” Security activity shows intent rejection. No order or PayPal request is created.
- **0:45–1:35:** Confirm a USD 1.00 intent, approve with the Sandbox buyer, authorize, and deliver the failing fixture. Show verification failure followed by provider-confirmed void.
- **1:35–2:25:** Repeat with the passing fixture. Show the countdown, passing verdict, and provider-confirmed capture. Open PayPal Sandbox to corroborate the transaction ID.
- **2:14–2:26:** Summarize the buyer approval and payment boundary.
- **2:26–2:46:** Show the live Channel3 API verification slate: 8 public listings received, all 8 screened and stored, none withheld, and 5 returned from the local mirror. State that buyer searches remain local and imported listings are discovery-only. The slate summarizes a real import but does not show the operator clicking the UI control.

Use the phrasing “AI reasoning and delivery verification stay on our server; PayPal handles the approved payment.” Avoid claiming that all data stays on the host.

Use replay mode for rehearsal and clearly identify it. Record real Sandbox payment evidence only after buyer approval. Avoid claiming escrow, instant production release, or complete injection detection. Do not show credentials, browser session cookies, or personal account information on video.

Submission: target the Grand Prize and, only after proving live Channel3 usage, the Channel3 Sponsor Prize. The official rules cap wins at one Sponsor Prize per project. Upload a video under three minutes publicly to YouTube (unlisted visibility does not qualify), then submit its link and the public repository through the user's Devpost account. These account and video actions are not automated by the application. See the [official rules](https://paypalaihackathon.devpost.com/rules).

The narrated local recording is `.local/quarantinepay-demo-channel3-fast.mp4`: 165.6 seconds, H.264 video and AAC audio. It ends with a designed proof slate based on the live Channel3 response and local-mirror search; it does not show the operator using the import UI. PayPal credential and login screens are excluded. The file is kept outside Git until publication.

The [description draft](YOUTUBE_DESCRIPTION.md), English [caption file](quarantinepay-demo.en.srt), and white-background [thumbnail PNG](quarantinepay-thumbnail.png) with its editable [SVG source](quarantinepay-thumbnail.svg) are checked in. The captions cover the 165.6-second recording and were validated against the narration for exact text, monotonic timestamps, and YouTube's two-line caption layout.
