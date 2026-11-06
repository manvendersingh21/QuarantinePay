# YouTube upload draft

**Title**

QuarantinePay — Secure Agentic Checkout with PayPal Sandbox

**Description**

QuarantinePay puts a verifiable boundary between an AI recommendation and a payment: the buyer approves a signed purchase intent, PayPal Sandbox authorizes the payment, and deterministic delivery checks gate capture or void.

This demo shows merchant-substitution rejection, a failed delivery followed by a provider-confirmed void, and a passing delivery followed by a provider-confirmed capture. The assistant runs locally on a Tesla V100 GPU and has no payment tools or credentials. The checked-in demo purchase is a USD 1.00 JSON invoice fixture.

All payment activity shown is PayPal Sandbox activity; no real funds are transferred. A PayPal authorization hold is not escrow and release timing depends on PayPal and the funding source. Product recommendations do not initiate payments. The model, prompts, and deterministic delivery verifier run on the application host; PayPal receives the payment fields needed to process an approved transaction.

Source code, setup instructions, API collection, and validation notes: https://github.com/manvendersingh21/QuarantinePay

Chapters
0:00 Introduction
0:16 Local AI and privacy boundary
0:42 Signed intent red-team check
0:56 Failed delivery and PayPal Sandbox void
1:47 Verified delivery and PayPal Sandbox capture
2:37 Channel3 live import results

This video is a product demonstration, not financial advice or a claim that the system detects every prompt injection.

**This recording includes a closing result slate from the verified live Channel3 import:** 8 listings received, screened, and stored; 0 withheld; 5 results returned from the local mirror. The slate summarizes the API result and does not show the operator UI interaction. Imported listings are discovery-only. Set YouTube visibility to **Public** for the PayPal AI Hackathon submission.
