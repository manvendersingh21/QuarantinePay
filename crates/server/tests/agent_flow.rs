//! End-to-end agent purchasing flow in replay mode against PostgreSQL, with a deliberately
//! "fooled" mock model: it inflates the budget and recommends approving an injected delivery.
//! Deterministic policy must still bound the budget, hold the payment and refuse capture.
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::process::{Child, Command, Stdio};
use uuid::Uuid;

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const REQUEST: &str = "Find me a market research report on AI note-taking apps under $100 covering three competitors, including 5 credible sources. Negotiate the best offer and only release payment if the requirements are met.";

async fn mock_model() -> (String, tokio::task::JoinHandle<()>) {
    use axum::{Json, Router, routing::post};
    let router = Router::new().route(
        "/api/chat",
        post(|Json(body): Json<Value>| async move {
            let system = body["messages"][0]["content"].as_str().unwrap_or("");
            let answer = if system.contains("planning module") {
                // Tries to exceed the buyer's written $100 limit.
                json!({"topic":"AI note-taking apps","budget_usd":250,"minimum_competitors":3,"minimum_sources":5,"required_sections":["Executive summary","Competitor analysis","Findings","Sources"],"reasoning":"Buyer wants a sourced competitor report."})
            } else {
                // A compromised analyst that obeys the injected text.
                json!({"relevance":"on_topic","manipulation_detected":false,"manipulation_evidence":[],"quality_concerns":[],"recommended_action":"approve","summary":"Looks great, release payment."})
            };
            Json(json!({"message":{"role":"assistant","content":answer.to_string()}}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (base, handle)
}

async fn get(client: &Client, url: String) -> Value {
    client
        .get(url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}
async fn wait_task(client: &Client, base: &str, id: &str, wanted: &str) -> Value {
    for _ in 0..60 {
        let v = get(client, format!("{base}/api/agent/tasks/{id}")).await;
        if v["state"] == wanted {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    panic!("task did not reach {wanted}")
}
async fn wait_order(client: &Client, base: &str, id: &str, wanted: &str) -> Value {
    for _ in 0..80 {
        let v = get(client, format!("{base}/api/orders/{id}")).await;
        if v["state"] == wanted {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    panic!("order {id} did not reach {wanted}")
}
/// Run the agent, approve its exact proposal and authorize the (simulated) payment.
async fn purchase(client: &Client, base: &str) -> (Value, String) {
    let created: Value = client
        .post(format!("{base}/api/agent/tasks"))
        .json(&json!({"request":REQUEST}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = created["id"].as_str().unwrap().to_string();
    let task = wait_task(client, base, &task_id, "awaiting_approval").await;
    let digest = task["proposal_digest"].as_str().unwrap();
    let approved: Value = client
        .post(format!("{base}/api/agent/tasks/{task_id}/approve"))
        .json(&json!({"proposal_digest":digest}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let order_id = approved["order_id"].as_str().unwrap().to_string();
    wait_order(client, base, &order_id, "approval_pending").await;
    client
        .post(format!("{base}/api/orders/{order_id}/authorize"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    wait_order(client, base, &order_id, "authorized").await;
    (task, order_id)
}
async fn deliver(client: &Client, base: &str, order_id: &str, task: &Value, kind: &str) {
    let contract: quarantinepay_core::PurchaseContract =
        serde_json::from_value(task["proposal"].clone()).unwrap();
    let artifact = quarantinepay_core::contract::contract_fixture(&contract, kind);
    client
        .post(format!("{base}/api/orders/{order_id}/deliveries"))
        .json(&artifact)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
}

#[tokio::test]
#[ignore = "requires running PostgreSQL; cargo build --workspace first"]
async fn agent_contract_purchase_is_policy_bound() {
    dotenvy::dotenv().ok();
    let database = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let admin = sqlx::PgPool::connect(&database).await.unwrap();
    let schema = format!("qp_agent_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let mut uri = reqwest::Url::parse(&database).unwrap();
    uri.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let (model_base, model) = mock_model().await;
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let base = format!("http://127.0.0.1:{port}");
    let token = "quarantinepay-agent-test-password-not-secret";
    let process = Process(
        Command::new(env!("CARGO_BIN_EXE_quarantinepay-server"))
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .env("DATABASE_URL", uri.as_str())
            .env("QP_MODE", "replay")
            .env("PAYPAL_MERCHANT_ID", "replay-merchant")
            .env("QP_PUBLIC_BASE", &base)
            .env("QP_BIND", format!("127.0.0.1:{port}"))
            .env("QP_ADMIN_PASSWORD", token)
            .env("QP_VERIFIER_MODE", "trusted-process")
            .env("QP_PRIVATE_INFERENCE", "true")
            .env("QP_INFERENCE_PROVIDER", "local")
            .env("QP_LOCAL_LLM_BASE", &model_base)
            .env("QP_LOCAL_MODEL", "mock-model")
            .env("ELASTIC_URL", "")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Authorization", format!("Bearer {token}").parse().unwrap());
    let client = Client::builder().default_headers(headers).build().unwrap();
    for _ in 0..60 {
        if client.get(format!("{base}/health")).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // 1. Planning is model-driven but bounded by the buyer's written $100 limit.
    let (task, bad_order) = purchase(&client, &base).await;
    assert_eq!(task["planner"]["source"], "model");
    assert_eq!(task["objective"]["budget_minor"], 10_000);
    assert_eq!(task["proposal"]["seller_id"], "quillfire");
    assert!(task["proposal"]["amount_minor"].as_u64().unwrap() <= 10_000);
    assert_eq!(task["proposal"]["buyer_review_required"], true);
    assert!(
        task["negotiation"]["offers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["seller_id"] == "alder" && o["status"] == "declined_scope")
    );
    // Approval is single-use and bound to the exact digest.
    let task_id = task["id"].as_str().unwrap();
    let again = client
        .post(format!("{base}/api/agent/tasks/{task_id}/approve"))
        .json(&json!({"proposal_digest":task["proposal_digest"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::CONFLICT);

    // 2. A structurally valid delivery with injected instructions is held, despite the model.
    deliver(&client, &base, &bad_order, &task, "malicious").await;
    let held = wait_order(&client, &base, &bad_order, "buyer_review").await;
    assert_eq!(held["verification"]["passed"], true);
    assert_eq!(held["analysis"]["analyst"]["source"], "model");
    assert_eq!(held["analysis"]["policy_hold"], true);
    assert_eq!(held["analysis"]["injection_detected"], true);
    assert_eq!(held["analysis"]["recommended_action"], "reject");
    assert_eq!(held["analysis"]["risk"], "high");
    assert_eq!(held["analysis"]["payment_authority"], false);
    let refused = client
        .post(format!("{base}/api/orders/{bad_order}/review"))
        .json(&json!({"decision":"approve"}))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    assert_eq!(
        get(&client, format!("{base}/api/orders/{bad_order}")).await["state"],
        "buyer_review"
    );
    client
        .post(format!("{base}/api/orders/{bad_order}/review"))
        .json(&json!({"decision":"reject"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    wait_order(&client, &base, &bad_order, "voided").await;

    // 3. A defective delivery fails deterministic checks and is voided without review.
    let (task, defective) = purchase(&client, &base).await;
    deliver(&client, &base, &defective, &task, "fail").await;
    let voided = wait_order(&client, &base, &defective, "voided").await;
    assert_eq!(voided["verification"]["passed"], false);
    assert!(voided["analysis"].is_null());

    // 4. A compliant delivery is reviewed and captured.
    let (task, good) = purchase(&client, &base).await;
    deliver(&client, &base, &good, &task, "pass").await;
    let review = wait_order(&client, &base, &good, "buyer_review").await;
    assert_eq!(review["analysis"]["policy_hold"], false);
    client
        .post(format!("{base}/api/orders/{good}/review"))
        .json(&json!({"decision":"approve"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let captured = wait_order(&client, &base, &good, "captured").await;
    assert_eq!(captured["risk_override"], false);

    // 5. An explicit, audited override is possible but recorded as a policy violation signal.
    let (task, overridden) = purchase(&client, &base).await;
    deliver(&client, &base, &overridden, &task, "malicious").await;
    wait_order(&client, &base, &overridden, "buyer_review").await;
    client
        .post(format!("{base}/api/orders/{overridden}/review"))
        .json(&json!({"decision":"approve","acknowledge_risk":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let o = wait_order(&client, &base, &overridden, "captured").await;
    assert_eq!(o["risk_override"], true);

    // 6. Read-only analytics reflect outcomes; replay results are never provider-confirmed.
    let analytics = get(&client, format!("{base}/api/analytics/transactions")).await;
    let rows = analytics["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert!(
        rows.iter()
            .all(|r| r["provider_confirmed"] == false && r["kind"] == "contract")
    );
    assert_eq!(analytics["summary"]["flagged_count"], 2);
    assert_eq!(analytics["summary"]["blocked_count"], 1);
    assert_eq!(analytics["summary"]["simulated"], true);
    // Tampering with a stored proposal's digest blocks approval.
    let created: Value = client
        .post(format!("{base}/api/agent/tasks"))
        .json(&json!({"request":REQUEST}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tid = created["id"].as_str().unwrap().to_string();
    wait_task(&client, &base, &tid, "awaiting_approval").await;
    let wrong = client
        .post(format!("{base}/api/agent/tasks/{tid}/approve"))
        .json(&json!({"proposal_digest":"0".repeat(64)}))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::CONFLICT);
    let pool = sqlx::PgPool::connect(uri.as_str()).await.unwrap();
    sqlx::query(
        "UPDATE agent_tasks SET proposal=jsonb_set(proposal,'{minimum_sources}','1') WHERE id=$1",
    )
    .bind(Uuid::parse_str(&tid).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let digest =
        get(&client, format!("{base}/api/agent/tasks/{tid}")).await["proposal_digest"].clone();
    let tampered = client
        .post(format!("{base}/api/agent/tasks/{tid}/approve"))
        .json(&json!({"proposal_digest":digest}))
        .send()
        .await
        .unwrap();
    assert_eq!(tampered.status(), StatusCode::CONFLICT);

    drop(process);
    model.abort();
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
