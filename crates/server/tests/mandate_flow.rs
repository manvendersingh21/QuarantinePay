//! Autonomous purchasing under a buyer-signed spending mandate (replay mode, PostgreSQL).
//! The mock model is deliberately fooled: it recommends approving the injected report.
//! The mandate must still bound budget, and capture must still require clean, independent checks.
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
const REQUEST: &str = "Find me a market research report on AI note-taking apps under $100 covering three competitors, including 5 credible sources.";

async fn mock_model() -> (String, tokio::task::JoinHandle<()>) {
    use axum::{Json, Router, routing::post};
    let router = Router::new().route(
        "/api/chat",
        post(|Json(body): Json<Value>| async move {
            let system = body["messages"][0]["content"].as_str().unwrap_or("");
            let answer = if system.contains("planning module") {
                json!({"topic":"AI note-taking apps","budget_usd":100,"minimum_competitors":3,"minimum_sources":5,"required_sections":["Executive summary","Competitor analysis","Sources"],"reasoning":"Sourced competitor report."})
            } else {
                json!({"relevance":"on_topic","manipulation_detected":false,"manipulation_evidence":[],"quality_concerns":[],"recommended_action":"approve","summary":"Looks good."})
            };
            Json(json!({"message":{"role":"assistant","content":answer.to_string()}}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    (
        base,
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }),
    )
}
async fn get(c: &Client, url: String) -> Value {
    c.get(url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}
async fn post(c: &Client, url: String, body: Value) -> reqwest::Response {
    c.post(url).json(&body).send().await.unwrap()
}
async fn wait_task(c: &Client, base: &str, id: &str, pred: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..80 {
        let v = get(c, format!("{base}/api/agent/tasks/{id}")).await;
        if pred(&v) {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    panic!("task {id} did not reach the expected state")
}
async fn wait_order(c: &Client, base: &str, id: &str, wanted: &str) -> Value {
    for _ in 0..80 {
        let v = get(c, format!("{base}/api/orders/{id}")).await;
        if v["state"] == wanted {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    panic!("order {id} did not reach {wanted}")
}
async fn task(c: &Client, base: &str, mandate: &str) -> Value {
    let created: Value = post(
        c,
        format!("{base}/api/agent/tasks"),
        json!({"request":REQUEST,"mandate_id":mandate}),
    )
    .await
    .error_for_status()
    .unwrap()
    .json()
    .await
    .unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    wait_task(c, base, &id, |t| {
        !["planning", "negotiating"].contains(&t["state"].as_str().unwrap_or(""))
    })
    .await
}
async fn deliver(c: &Client, base: &str, t: &Value, kind: &str) {
    let contract: quarantinepay_core::PurchaseContract =
        serde_json::from_value(t["proposal"].clone()).unwrap();
    let order = t["order_id"].as_str().unwrap();
    post(
        c,
        format!("{base}/api/orders/{order}/deliveries"),
        quarantinepay_core::contract::contract_fixture(&contract, kind),
    )
    .await
    .error_for_status()
    .unwrap();
}

#[tokio::test]
#[ignore = "requires running PostgreSQL; cargo build --workspace first"]
async fn mandate_purchases_are_autonomous_but_bounded() {
    dotenvy::dotenv().ok();
    let database = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let admin = sqlx::PgPool::connect(&database).await.unwrap();
    let schema = format!("qp_mandate_test_{}", Uuid::new_v4().simple());
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
    let token = "quarantinepay-mandate-test-password-xyz";
    let process = Process(
        Command::new(env!("CARGO_BIN_EXE_quarantinepay-server"))
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .env("DATABASE_URL", uri.as_str())
            .env("QP_MODE", "replay")
            .env("PAYPAL_MERCHANT_ID", "replay-merchant")
            .env("QP_PUBLIC_BASE", &base)
            .env("QP_BIND", format!("127.0.0.1:{port}"))
            .env("QP_ADMIN_PASSWORD", token)
            .env("QP_PRIVATE_INFERENCE", "true")
            .env("QP_INFERENCE_PROVIDER", "local")
            .env("QP_LOCAL_LLM_BASE", &model_base)
            .env("QP_LOCAL_MODEL", "mock-model")
            .env("ELASTIC_URL", "")
            .env("SHOPIFY_DEV_STORE", "")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Authorization", format!("Bearer {token}").parse().unwrap());
    let c = Client::builder().default_headers(headers).build().unwrap();
    for _ in 0..60 {
        if c.get(format!("{base}/health")).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // 1. Sign and activate a mandate: USD 60 per purchase, USD 100 total, auto-capture clean.
    let m: Value = post(
        &c,
        format!("{base}/api/mandates"),
        json!({"max_per_purchase_usd":60,"max_total_usd":100,"hours":24,"auto_capture_clean":true}),
    )
    .await
    .error_for_status()
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(m["state"], "pending_vault");
    let mid = m["id"].as_str().unwrap().to_string();
    // Not yet active: no autonomous purchase possible.
    let t0 = task(&c, &base, &mid).await;
    assert_eq!(t0["state"], "awaiting_approval");
    assert_eq!(t0["autonomy"]["decision"], "outside_mandate");
    post(&c, format!("{base}/api/mandates/{mid}/activate"), json!({}))
        .await
        .error_for_status()
        .unwrap();
    let listed = get(&c, format!("{base}/api/mandates")).await;
    assert_eq!(listed["mandates"][0]["state"], "active");
    assert_eq!(listed["mandates"][0]["vault_saved"], true);
    // Only one open mandate at a time.
    assert_eq!(
        post(
            &c,
            format!("{base}/api/mandates"),
            json!({"max_per_purchase_usd":10,"max_total_usd":10,"hours":1})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    // 2. Autonomous purchase: no approval call, no authorize call; clean delivery auto-captures.
    let t1 = task(&c, &base, &mid).await;
    assert_eq!(t1["state"], "purchasing", "{t1}");
    assert_eq!(t1["autonomy"]["decision"], "auto_approved");
    assert_eq!(t1["proposal"]["buyer_review_required"], false);
    assert_eq!(t1["proposal"]["mandate_id"], mid);
    let o1 = t1["order_id"].as_str().unwrap().to_string();
    let authorized = wait_order(&c, &base, &o1, "authorized").await;
    assert!(
        authorized["paypal_order"]
            .as_str()
            .unwrap()
            .starts_with("REPLAY-MIT-")
    );
    deliver(&c, &base, &t1, "pass").await;
    let captured = wait_order(&c, &base, &o1, "captured").await;
    assert!(
        captured["review_decision"].is_null(),
        "no human decision was needed"
    );

    // 3. Injected delivery under the mandate: the fooled model says approve, but the rules hold it.
    let t2 = task(&c, &base, &mid).await;
    assert_eq!(t2["state"], "purchasing");
    let o2 = t2["order_id"].as_str().unwrap().to_string();
    wait_order(&c, &base, &o2, "authorized").await;
    deliver(&c, &base, &t2, "malicious").await;
    let held = wait_order(&c, &base, &o2, "buyer_review").await;
    assert_eq!(held["analysis"]["policy_hold"], true);

    // 4. Budget: two purchases committed (~USD 94.50 of 100); a third purchase must ask the buyer.
    let t3 = task(&c, &base, &mid).await;
    assert_eq!(t3["state"], "awaiting_approval");
    assert!(
        t3["autonomy"]["reasons"]
            .to_string()
            .contains("remaining mandate budget"),
        "{}",
        t3["autonomy"]
    );
    assert!(t3["proposal"]["mandate_id"].is_null());
    assert_eq!(t3["proposal"]["buyer_review_required"], true);

    // The held order still cannot be approved without acknowledgement; reject voids it.
    assert_eq!(
        post(
            &c,
            format!("{base}/api/orders/{o2}/review"),
            json!({"decision":"approve"})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    post(
        &c,
        format!("{base}/api/orders/{o2}/review"),
        json!({"decision":"reject"}),
    )
    .await
    .error_for_status()
    .unwrap();
    wait_order(&c, &base, &o2, "voided").await;
    // Voided holds release budget.
    let listed = get(&c, format!("{base}/api/mandates")).await;
    assert_eq!(
        listed["mandates"][0]["committed_minor"], t1["proposal"]["amount_minor"],
        "only the captured purchase still counts"
    );
    // The mandate cap bounded negotiation: the agent never agreed above USD 60.
    assert!(t1["proposal"]["amount_minor"].as_u64().unwrap() <= 6_000);

    // 5. A tampered mandate (raised budget in storage) fails signature verification.
    let pool = sqlx::PgPool::connect(uri.as_str()).await.unwrap();
    sqlx::query("UPDATE mandates SET signed=jsonb_set(signed,'{mandate,max_total_minor}','50000') WHERE id=$1")
        .bind(Uuid::parse_str(&mid).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let t4 = task(&c, &base, &mid).await;
    assert_eq!(t4["state"], "awaiting_approval");
    assert!(
        t4["autonomy"]["reasons"].to_string().contains("signature"),
        "{}",
        t4["autonomy"]
    );
    sqlx::query("UPDATE mandates SET signed=jsonb_set(signed,'{mandate,max_total_minor}','10000') WHERE id=$1")
        .bind(Uuid::parse_str(&mid).unwrap())
        .execute(&pool)
        .await
        .unwrap();

    // 6. Revocation stops autonomy immediately.
    post(&c, format!("{base}/api/mandates/{mid}/revoke"), json!({}))
        .await
        .error_for_status()
        .unwrap();
    let t5 = task(&c, &base, &mid).await;
    assert_eq!(t5["state"], "awaiting_approval");
    assert!(
        t5["autonomy"]["reasons"].to_string().contains("revoked"),
        "{}",
        t5["autonomy"]
    );

    // 7. Manual tasks (no mandate) are unchanged.
    let created: Value = post(
        &c,
        format!("{base}/api/agent/tasks"),
        json!({"request":REQUEST}),
    )
    .await
    .json()
    .await
    .unwrap();
    let t6 = wait_task(&c, &base, created["id"].as_str().unwrap(), |t| {
        t["state"] == "awaiting_approval"
    })
    .await;
    assert!(t6["autonomy"].is_null());
    assert_eq!(t6["proposal"]["buyer_review_required"], true);

    drop(process);
    model.abort();
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
