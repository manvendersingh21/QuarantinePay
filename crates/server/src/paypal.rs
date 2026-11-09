use crate::config::get;
use anyhow::{Result, ensure};
use quarantinepay_core::SignedIntent;
use reqwest::Client;
use serde_json::{Value, json};

/// Structured, sanitized provider errors keep permission failures distinct from unknown outcomes.
#[derive(Debug)]
pub(crate) struct PayPalFailure {
    pub status: u16,
    pub name: String,
    pub issues: Vec<String>,
    pub debug_id: Option<String>,
}
impl PayPalFailure {
    pub fn requires_credentials(&self) -> bool {
        matches!(self.status, 401 | 403)
    }
    /// A refusal that repeating the same request cannot change (e.g. 422 for an operation the
    /// authorization no longer permits). Timeouts, conflicts, rate limits and 5xx stay retryable.
    pub fn definitive(&self) -> bool {
        matches!(self.status, 400 | 401 | 403 | 404 | 422)
    }
}
/// The provider reports an authorization state in which the requested operation is not permitted,
/// for example a void requested for an authorization PayPal says is already captured or expired.
#[derive(Debug)]
pub(crate) struct ProviderStateConflict {
    pub operation: &'static str,
    pub provider_status: String,
}
impl std::fmt::Display for ProviderStateConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PayPal reports the authorization as {}; {} is not permitted",
            self.provider_status, self.operation
        )
    }
}
impl std::error::Error for ProviderStateConflict {}
impl std::fmt::Display for PayPalFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PayPal returned {} ({}): {}; debug_id={}",
            self.status,
            self.name,
            self.issues.join(", "),
            self.debug_id.as_deref().unwrap_or("unavailable")
        )
    }
}
impl std::error::Error for PayPalFailure {}
fn safe_code(value: &Value) -> String {
    value
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 100
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .unwrap_or("UNKNOWN")
        .into()
}
async fn decode_response(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    if !status.is_success() {
        let header_debug = response
            .headers()
            .get("paypal-debug-id")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        let error: Value = response.json().await.unwrap_or(Value::Null);
        let debug_id = error["debug_id"]
            .as_str()
            .map(str::to_owned)
            .or(header_debug)
            .filter(|s| {
                s.len() <= 100
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            });
        let name = safe_code(
            error
                .get("name")
                .or_else(|| error.get("error"))
                .unwrap_or(&Value::Null),
        );
        let issues = error["details"]
            .as_array()
            .map(|a| a.iter().map(|d| safe_code(&d["issue"])).collect())
            .unwrap_or_default();
        return Err(PayPalFailure {
            status: status.as_u16(),
            name,
            issues,
            debug_id,
        }
        .into());
    }
    if status == reqwest::StatusCode::NO_CONTENT {
        return Ok(json!({"status":"VOIDED"}));
    }
    Ok(response.json().await?)
}

#[derive(Clone)]
pub struct PayPal {
    client: Client,
    replay: bool,
    base: String,
    client_id: String,
    client_secret: String,
}
impl PayPal {
    pub fn new(client: Client, replay: bool) -> Self {
        Self {
            client,
            replay,
            base: "https://api-m.sandbox.paypal.com".into(),
            client_id: get("PAYPAL_CLIENT_ID", ""),
            client_secret: get("PAYPAL_CLIENT_SECRET", ""),
        }
    }
    #[cfg(test)]
    pub(crate) fn for_test(client: Client, base: String) -> Self {
        Self {
            client,
            replay: false,
            base,
            client_id: "test-client".into(),
            client_secret: "test-secret".into(),
        }
    }
    async fn token(&self) -> Result<String> {
        let r = self
            .client
            .post(format!("{}/v1/oauth2/token", self.base))
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?;
        let v = decode_response(r).await?;
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
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.token().await?)
            .header("Content-Type", "application/json")
            .header("Prefer", "return=representation");
        if !request_id.is_empty() {
            req = req.header("PayPal-Request-Id", request_id);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        decode_response(req.send().await?).await
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
        // Authorization CAPTURED is not proof that the individual capture settled.
        // Replay the exact persisted capture request ID and use its actual status.
        if !capture && status == "VOIDED" {
            return Ok(auth);
        }
        if matches!(
            status,
            "VOIDED" | "EXPIRED" | "DENIED" | "CAPTURED" | "PARTIALLY_CAPTURED"
        ) && !(capture && status == "CAPTURED")
        {
            return Err(ProviderStateConflict {
                operation: if capture { "capture" } else { "void" },
                provider_status: safe_code(&auth["status"]),
            }
            .into());
        }
        ensure!(
            status == "CREATED" || (capture && status == "CAPTURED"),
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

#[cfg(test)]
pub(crate) async fn mock_resolution_failure(
    status: u16,
) -> (
    PayPal,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    tokio::task::JoinHandle<()>,
) {
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let router=Router::new()
        .route("/v1/oauth2/token",post(||async{Json(json!({"access_token":"fake-test-token"}))}))
        .route("/v2/payments/authorizations/{id}",get(||async{Json(json!({"status":"CREATED"}))}))
        .route("/v2/payments/authorizations/{id}/void",post(move |headers:axum::http::HeaderMap|{
            let count=count.clone();async move {
                assert_eq!(headers["Content-Type"],"application/json");
                assert!(!headers["PayPal-Request-Id"].is_empty());
                count.fetch_add(1,Ordering::SeqCst);
                (axum::http::StatusCode::from_u16(status).unwrap(),Json(json!({"name":"NOT_AUTHORIZED","debug_id":"test-debug-403","details":[{"issue":"PERMISSION_DENIED","description":"DO_NOT_LOG_PRIVATE_RESPONSE_VALUES"}]})))
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (PayPal::for_test(Client::new(), base), attempts, handle)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn void_of_captured_authorization_is_a_state_conflict_not_a_success() {
        use axum::{
            Json, Router,
            routing::{get, post},
        };
        let router = Router::new()
            .route(
                "/v1/oauth2/token",
                post(|| async { Json(json!({"access_token":"fake"})) }),
            )
            .route(
                "/v2/payments/authorizations/{id}",
                get(|| async { Json(json!({"status":"CAPTURED"})) }),
            )
            .route(
                "/v2/payments/authorizations/{id}/void",
                post(|| async {
                    // Reaching this would surface as a 418 provider failure, not a conflict.
                    axum::http::StatusCode::IM_A_TEAPOT
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let paypal = PayPal::for_test(Client::new(), format!("http://{address}"));
        let error = paypal.resolve("auth", false, "req").await.unwrap_err();
        let conflict = error.downcast_ref::<ProviderStateConflict>().unwrap();
        assert_eq!(conflict.provider_status, "CAPTURED");
        assert_eq!(conflict.operation, "void");
        server.abort();
    }
    #[tokio::test]
    async fn captured_authorization_does_not_fabricate_completed_capture() {
        use axum::{
            Json, Router,
            routing::{get, post},
        };
        let router = Router::new()
            .route(
                "/v1/oauth2/token",
                post(|| async { Json(json!({"access_token":"fake"})) }),
            )
            .route(
                "/v2/payments/authorizations/test-auth",
                get(|| async { Json(json!({"status":"CAPTURED"})) }),
            )
            .route(
                "/v2/payments/authorizations/test-auth/capture",
                post(|headers: axum::http::HeaderMap| async move {
                    assert_eq!(headers["PayPal-Request-Id"], "original-request");
                    Json(json!({"id":"test-capture","status":"PENDING"}))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let paypal = PayPal::for_test(Client::new(), format!("http://{address}"));
        let value = paypal
            .resolve("test-auth", true, "original-request")
            .await
            .unwrap();
        assert_eq!(value["status"], "PENDING");
        assert_eq!(value["id"], "test-capture");
        server.abort();
    }
    #[tokio::test]
    async fn provider_errors_keep_permission_classification_and_redact_response_values() {
        for (status, needs_credentials) in [(401, true), (403, true), (429, false), (500, false)] {
            let (paypal, attempts, server) = mock_resolution_failure(status).await;
            let error = paypal
                .resolve("test-authorization", false, "stable-test-request")
                .await
                .unwrap_err();
            let failure = error.downcast_ref::<PayPalFailure>().unwrap();
            assert_eq!(failure.status, status);
            assert_eq!(failure.requires_credentials(), needs_credentials);
            assert_eq!(failure.debug_id.as_deref(), Some("test-debug-403"));
            assert!(
                !error
                    .to_string()
                    .contains("DO_NOT_LOG_PRIVATE_RESPONSE_VALUES")
            );
            assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
            server.abort();
        }
    }
}
