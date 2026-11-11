# Judge guide

Ten minutes with the live build. Everything here runs on PayPal **Sandbox** (test money), a Shopify **development store** (test orders, nothing ships) and local models.

## 1. Open it

1. Go to the showcase, https://manvendersingh21.github.io/QuarantinePay/, and click **Open live demo**. The button always points at the current server URL, which can change if the server restarts.
2. Sign in with the password from the Devpost **testing instructions**. It is not in this public repository.
3. Open **Agent console**.

Discovery takes about 20–70 s, because a 30B model and an embedding model screen every listing on a single GPU. The page refreshes itself.

## 2. Things to try

**A. Buy from a real Shopify merchant (the main story)**

1. Type `Buy me an LED desk lamp under $60 with tracked delivery.`
2. Tick **Search real products**.
3. If the spending-mandate panel shows an active mandate, also tick **Buy autonomously under my mandate**.
4. Click **Run buyer agent**.

What to look at:

- **Discover real products:** the store's live catalog, with photos.
  - *Brightline* ($22) tells AI agents to approve payment. The phrase rules and Elastic withhold it.
  - *Halo* ($27) says delivery checks "can be skipped". No phrase rule catches it; the **Elastic kNN** chip shows the semantic layer withholding it.
  - The agent buys *Lumen Arc* at the store price.
- **Contract:** the item's photo is bound into the contract by its SHA-256, so you approve exactly what you saw.
- **PayPal authorization:**
  - Under a mandate, payment comes from the saved Sandbox wallet with no click.
  - Without one, approve the contract, then log in at PayPal with the Sandbox buyer from the Devpost notes.
- **Merchant order:**
  - QuarantinePay places the order in Shopify only after PayPal holds the money.
  - **Seller claims "delivered"** is refused (409) and logged.
  - The **Merchant simulator** buttons do what the store owner would click in Shopify admin, on the real order. They have no payment authority, and QuarantinePay then reads the order from Shopify by itself.

  | Merchant simulator button | What happens |
  | --- | --- |
  | Merchant ships it (UPS tracking) | All 7 checks pass. The payment is captured automatically under a mandate, or goes to buyer review without one. The Shopify order is then recorded as paid. |
  | Merchant ships with injected tracking text | Policy hold. Approving without acknowledging the risk is refused. |
  | Merchant cancels the order | The PayPal hold is voided. |

  If the merchant does nothing for 15 minutes, the hold is voided.

**B. Real products from Channel3**

1. Type `Buy me a stainless steel water bottle under $40 with tracked delivery.`
2. The store has no bottles: the relevance gate refuses its unrelated products, so the agent searches **Channel3** live. Only the search terms leave the system.
3. Fictional reseller agents quote for the chosen real listing.
4. In the seller simulator:
   - **Ship & deliver (carrier-signed)** captures the payment.
   - **Claim delivered with forged proof** fails the carrier signature check and voids the hold.

**C. The original agent contract (digital delivery)**

1. Type `Find me a market research report on AI note-taking apps under $100 covering three competitors, including 5 credible sources`.
2. Watch the negotiation, then deliver the report **with injected instructions**. The local AI analyst and the rules both flag it; capture is blocked until you reject it or explicitly override.

**D. Intelligence**

The **Intelligence** page (AG Grid) lists every purchase, its state, and its policy flags.

## 3. Mandates

A mandate lets the agent buy without a click, within a per-purchase cap and a total budget. To create one:

1. In the spending-mandate panel, choose limits, then **Sign spending mandate**.
2. Approve the wallet once at PayPal Sandbox with the Sandbox buyer from the Devpost notes.
3. Back in the console, click **I approved — activate mandate**. The USD 1.00 verification hold is voided at once.

**Revoke mandate** stops it immediately.

## 4. What is real and what is simulated

| Real | Simulated |
| --- | --- |
| PayPal Sandbox authorize, capture, void and wallet vaulting, confirmed by PayPal's API | The reseller agents and the carrier in Channel3 purchases (labelled in the UI) |
| The Shopify development store's catalog, orders, fulfilments, payment records and cancellations | The merchant simulator acting as the store owner (labelled; it has no payment authority) |
| Channel3 search results and photos | The digital "report" sellers in story C |
| Local Qwen3-30B screening and analysis; Elastic kNN over local embeddings | |

A Shopify fulfilment is the merchant's own record, which QuarantinePay reads directly. That is stronger than a seller's claim, but it is not carrier proof. Screening can only add caution; it cannot guarantee it catches every attack.

## 5. If the live demo is down

The machine may have restarted. Two options:

- **Run it yourself.** Replay mode needs only Rust and PostgreSQL: see [Quickstart](../README.md#quickstart).
- **Watch the recordings.** See the [demo video](https://manvendersingh21.github.io/QuarantinePay/quarantinepay-demo.mp4) and the live evidence in [VALIDATION.md](VALIDATION.md).

The operator brings the demo back with `scripts/demo-up.sh`.
