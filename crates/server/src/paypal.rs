use crate::config::get;
use anyhow::{Result, ensure};
use quarantinepay_core::SignedIntent;
use reqwest::Client;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct PayPal {
    client: Client,
    replay: bool,
}
impl PayPal {
    pub fn new(client: Client, replay: bool) -> Self {
        Self { client, replay }
    }
    async fn token(&self) -> Result<String> {
        let r = self
            .client
            .post("https://api-m.sandbox.paypal.com/v1/oauth2/token")
            .basic_auth(
                get("PAYPAL_CLIENT_ID", ""),
                Some(get("PAYPAL_CLIENT_SECRET", "")),
            )
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?;
        ensure!(
            r.status().is_success(),
            "PayPal OAuth returned {}",
            r.status()
        );
        let v: Value = r.json().await?;
        Ok(v["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing access token"))?
            .into())
    }
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        request_id: &str,
    ) -> Result<Value> {
        let mut req = self
            .client
            .request(method, format!("https://api-m.sandbox.paypal.com{path}"))
            .bearer_auth(self.token().await?)
            .header("PayPal-Request-Id", request_id)
            .header("Prefer", "return=representation");
        if let Some(body) = body {
            req = req.json(&body);
        }
        let response = req.send().await?;
        let status = response.status();
        if !status.is_success() {
            let error: Value = response.json().await.unwrap_or(Value::Null);
            // Report provider issue codes only; never log response values or buyer details.
            let code = |v: &Value| {
                v.as_str()
                    .filter(|s| {
                        s.len() <= 100
                            && s.bytes()
                                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                    })
                    .unwrap_or("UNKNOWN")
                    .to_string()
            };
            let issues = error["details"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|d| code(&d["issue"]))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            anyhow::bail!(
                "PayPal operation returned {status} ({}): {issues}",
                code(&error["name"])
            );
        }
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(json!({"status":"VOIDED"}));
        }
        Ok(response.json().await?)
    }
    pub async fn create(
        &self,
        signed: &SignedIntent,
        base: &str,
        request_id: &str,
    ) -> Result<Value> {
        if self.replay {
            return Ok(
                json!({"id":format!("REPLAY-{request_id}"),"status":"CREATED","links":[{"rel":"payer-action","href":format!("{base}/?replay_approval=required")}]}),
            );
        }
        let i = &signed.intent;
        self.call(reqwest::Method::POST,"/v2/checkout/orders",Some(json!({
   "intent":"AUTHORIZE","purchase_units":[{"reference_id":i.id.to_string(),"custom_id":i.id.to_string(),"payee":{"merchant_id":i.merchant_id},"amount":{"currency_code":i.currency,"value":format!("{}.{:02}",i.amount_minor/100,i.amount_minor%100)}}],
   "payment_source":{"paypal":{"experience_context":{"return_url":format!("{base}/paypal/return"),"cancel_url":format!("{base}/"),"user_action":"PAY_NOW"}}}
  })),request_id).await
    }
    pub async fn order(&self, id: &str) -> Result<Value> {
        self.call(
            reqwest::Method::GET,
            &format!("/v2/checkout/orders/{id}"),
            None,
            "",
        )
        .await
    }
    pub async fn authorize(&self, id: &str, request_id: &str) -> Result<Value> {
        if self.replay {
            return Ok(
                json!({"purchase_units":[{"payments":{"authorizations":[{"id":format!("REPLAY-AUTH-{request_id}"),"status":"CREATED"}]}}]}),
            );
        }
        // Approval is mandatory; an unauthorized callback alone cannot authorize a payment.
        let order = self.order(id).await?;
        if order["status"] == "COMPLETED"
            && order
                .pointer("/purchase_units/0/payments/authorizations/0/id")
                .is_some()
        {
            return Ok(order);
        }
        ensure!(
            order["status"] == "APPROVED",
            "buyer has not approved the PayPal order"
        );
        self.call(
            reqwest::Method::POST,
            &format!("/v2/checkout/orders/{id}/authorize"),
            Some(json!({})),
            request_id,
        )
        .await
    }
    pub async fn resolve(&self, id: &str, capture: bool, request_id: &str) -> Result<Value> {
        if self.replay {
            return Ok(
                json!({"id":format!("REPLAY-RESULT-{request_id}"),"status":if capture{"COMPLETED"}else{"VOIDED"}}),
            );
        }
        let auth = self
            .call(
                reqwest::Method::GET,
                &format!("/v2/payments/authorizations/{id}"),
                None,
                "",
            )
            .await?;
        let status = auth["status"].as_str().unwrap_or("");
        if capture && status == "CAPTURED" {
            return Ok(json!({"status":"COMPLETED","reconciled_authorization":auth}));
        }
        if !capture && status == "VOIDED" {
            return Ok(auth);
        }
        ensure!(
            status == "CREATED",
            "authorization cannot be resolved from {status}"
        );
        let op = if capture { "capture" } else { "void" };
        self.call(
            reqwest::Method::POST,
            &format!("/v2/payments/authorizations/{id}/{op}"),
            if capture {
                Some(json!({"final_capture":true}))
            } else {
                None
            },
            request_id,
        )
        .await
    }
    pub async fn verify_webhook(
        &self,
        headers: &axum::http::HeaderMap,
        event: &Value,
    ) -> Result<()> {
        ensure!(!self.replay, "webhooks disabled in replay mode");
        let h = |name: &str| -> Result<String> {
            Ok(headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| anyhow::anyhow!("missing webhook header"))?
                .into())
        };
        let webhook_id = get("PAYPAL_WEBHOOK_ID", "");
        ensure!(!webhook_id.is_empty(), "webhook ID not configured");
        let result=self.call(reqwest::Method::POST,"/v1/notifications/verify-webhook-signature",Some(json!({"auth_algo":h("paypal-auth-algo")?,"cert_url":h("paypal-cert-url")?,"transmission_id":h("paypal-transmission-id")?,"transmission_sig":h("paypal-transmission-sig")?,"transmission_time":h("paypal-transmission-time")?,"webhook_id":webhook_id,"webhook_event":event})),"").await?;
        ensure!(
            result["verification_status"] == "SUCCESS",
            "webhook signature rejected"
        );
        Ok(())
    }
}
