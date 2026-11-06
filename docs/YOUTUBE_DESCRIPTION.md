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
0:14 Local AI and privacy boundary
0:36 Signed intent red-team check
0:48 Failed delivery and PayPal Sandbox void
1:31 Verified delivery and PayPal Sandbox capture
2:14 Summary

This video is a product demonstration, not financial advice or a claim that the system detects every prompt injection.

**Before publishing:** This recording predates the verified live Channel3 catalog import (8 listings screened and stored; 5 found by local search). Do not claim Channel3 sponsor-track eligibility based on this video. Record an updated demonstration that shows the import and local search, then revise the video URL and chapters. Set YouTube visibility to **Public** for the PayPal AI Hackathon submission.
