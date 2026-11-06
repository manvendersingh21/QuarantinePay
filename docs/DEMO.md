# Three-minute demo

- **0:00–0:16:** Introduce the signed buyer intent and verification window.
- **0:16–0:42:** Explain local AI and the privacy boundary.
- **0:42–0:56:** Show merchant substitution being rejected.
- **0:56–1:47:** Show a failing delivery and PayPal Sandbox void.
- **1:47–2:37:** Show a passing delivery and PayPal Sandbox capture.
- **2:37–2:59:** Show the verified Channel3 API result slate and local-mirror search counts. The slate summarizes a real import; it does not show the operator clicking the UI control.

Use the phrasing “AI reasoning and delivery verification stay on our server; PayPal handles the approved payment.” Avoid claiming that all data stays on the host.

Use replay mode for rehearsal and clearly identify it. Record real Sandbox payment evidence only after buyer approval. Avoid claiming escrow, instant production release, or complete injection detection. Do not show credentials, browser session cookies, or personal account information on video.

Submission: target the Grand Prize and, only after proving live Channel3 usage, the Channel3 Sponsor Prize. The official rules cap wins at one Sponsor Prize per project. Upload a video under three minutes publicly to YouTube (unlisted visibility does not qualify), then submit its link and the public repository through the user's Devpost account. These account and video actions are not automated by the application. See the [official rules](https://paypalaihackathon.devpost.com/rules).

The submission recording is [`demo/quarantinepay-contest.mp4`](../demo/quarantinepay-contest.mp4): approximately 178.7 seconds, H.264 video and AAC audio. It ends with a designed proof slate based on the live Channel3 response and local-mirror search; it does not show the operator using the import UI. PayPal credential and login screens are excluded. The video is included in the public repository.

The [description draft](YOUTUBE_DESCRIPTION.md), English [caption file](quarantinepay-demo.en.srt), and white-background [thumbnail PNG](quarantinepay-thumbnail.png) with its editable [SVG source](quarantinepay-thumbnail.svg) are checked in. The captions cover the 178.7-second recording and were validated against the narration for exact text, monotonic timestamps, and YouTube's two-line caption layout.

The full version is available at [`demo/quarantinepay-full.mp4`](../demo/quarantinepay-full.mp4) (194.8 seconds), with [matching captions](../demo/quarantinepay-full.en.srt). The contest version uses the same slower pace and omits the repeated closing summary to remain under three minutes.

The videos use Microsoft Ava neural narration generated from the public demo script through Microsoft’s online speech service. This is separate from the application’s locally hosted inference.
