//! Requires PostgreSQL. Each run creates and removes its own isolated schema.
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
async fn wait(client: &Client, base: &str, id: &str, wanted: &str) -> Value {
    for _ in 0..40 {
        let v: Value = client
            .get(format!("{base}/api/orders/{id}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if v["state"] == wanted {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("order did not reach {wanted}")
}
async fn mint(client: &Client, base: &str) -> Value {
    client.post(format!("{base}/api/intents")).json(&json!({"sku":"invoice-fixture-v1","merchant_id":"replay-merchant","amount_minor":100,"currency":"USD","confirmed":true})).send().await.unwrap().error_for_status().unwrap().json().await.unwrap()
}
async fn create(client: &Client, base: &str) -> String {
    let token = mint(client, base).await;
    let v: Value = client
        .post(format!("{base}/api/checkout"))
        .json(&token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = v["id"].as_str().unwrap().to_string();
    wait(client, base, &id, "approval_pending").await;
    client
        .post(format!("{base}/api/orders/{id}/authorize"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    wait(client, base, &id, "authorized").await;
    id
}
#[tokio::test]
#[ignore = "requires running PostgreSQL; cargo build --workspace first"]
async fn payment_security_and_recovery() {
    dotenvy::dotenv().ok();
    let database = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let admin = sqlx::PgPool::connect(&database).await.unwrap();
    let schema = format!("qp_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let mut uri = reqwest::Url::parse(&database).unwrap();
    uri.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let pool = sqlx::PgPool::connect(uri.as_str()).await.unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let base = format!("http://127.0.0.1:{port}");
    let token = "quarantinepay-test-password-not-a-real-secret";
    let start = || {
        Process(
            Command::new(env!("CARGO_BIN_EXE_quarantinepay-server"))
                .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
                .env("DATABASE_URL", uri.as_str())
                .env("QP_MODE", "replay")
                .env("PAYPAL_MERCHANT_ID", "replay-merchant")
                .env("QP_PUBLIC_BASE", &base)
                .env("QP_BIND", format!("127.0.0.1:{port}"))
                .env("QP_ADMIN_PASSWORD", token)
                .env("QP_VERIFIER_MODE", "trusted-process")
                .env("ELASTIC_URL", "")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    };
    let mut process = Some(start());
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Authorization", format!("Bearer {token}").parse().unwrap());
    let client = Client::builder().default_headers(headers).build().unwrap();
    for _ in 0..60 {
        if client.get(format!("{base}/health")).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let anonymous = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    assert_eq!(
        anonymous
            .post(format!("{base}/api/checkout"))
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        anonymous
            .get(format!("{base}/api/orders/{}", Uuid::new_v4()))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let signed = mint(&client, &base).await;
    let mut tampered = signed.clone();
    tampered["intent"]["merchant_id"] = json!("attacker");
    assert_eq!(
        client
            .post(format!("{base}/api/checkout"))
            .json(&tampered)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        client
            .post(format!("{base}/api/checkout"))
            .json(&signed)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .post(format!("{base}/api/checkout"))
            .json(&signed)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    // A mode change cannot consume an intent issued in another environment.
    let cross_mode = mint(&client, &base).await;
    let cross_id = Uuid::parse_str(cross_mode["intent"]["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE intents SET mode='sandbox' WHERE id=$1")
        .bind(cross_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        client
            .post(format!("{base}/api/checkout"))
            .json(&cross_mode)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    // The replay service must neither display nor process Sandbox orders.
    let hidden = Uuid::new_v4();
    sqlx::query("INSERT INTO orders(id,buyer,intent,state,mode,operation,request_id) SELECT $1,buyer,token,'create_pending','sandbox','create',$2 FROM intents WHERE id=$3").bind(hidden).bind(Uuid::new_v4()).bind(cross_id).execute(&pool).await.unwrap();
    assert_eq!(
        client
            .get(format!("{base}/api/orders/{hidden}"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let state: String = sqlx::query_scalar("SELECT state FROM orders WHERE id=$1")
        .bind(hidden)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "create_pending");
    let passing = create(&client, &base).await;
    let pass: Value = serde_json::from_str(include_str!("../../../fixtures/pass.json")).unwrap();
    client
        .post(format!("{base}/api/orders/{passing}/deliveries"))
        .json(&pass)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        client
            .post(format!("{base}/api/orders/{passing}/deliveries"))
            .json(&pass)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    wait(&client, &base, &passing, "captured").await;
    let failing = create(&client, &base).await;
    let fail: Value = serde_json::from_str(include_str!("../../../fixtures/fail.json")).unwrap();
    client
        .post(format!("{base}/api/orders/{failing}/deliveries"))
        .json(&fail)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    wait(&client, &base, &failing, "voided").await;
    let expired = create(&client, &base).await;
    sqlx::query("UPDATE orders SET deadline=now()-interval '1 second' WHERE id=$1")
        .bind(Uuid::parse_str(&expired).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    wait(&client, &base, &expired, "voided").await;
    // Restart with an authorized order and an elapsed deadline: durable work must resume.
    let recovered = create(&client, &base).await;
    drop(process.take());
    sqlx::query("UPDATE orders SET deadline=now()-interval '1 second' WHERE id=$1")
        .bind(Uuid::parse_str(&recovered).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    process = Some(start());
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    wait(&client, &base, &recovered, "voided").await;
    // Browser login cookie must not authorize a cross-origin mutation.
    let login = anonymous
        .post(format!("{base}/login"))
        .header("Origin", &base)
        .form(&[("password", token)])
        .send()
        .await
        .unwrap();
    let cookie = login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert_eq!(
        anonymous
            .get(format!("{base}/api/orders/{recovered}"))
            .header("Cookie", cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let expired_cookie = format!("qp_session=0.{}", cookie.split_once('.').unwrap().1);
    assert_eq!(
        anonymous
            .get(format!("{base}/api/orders/{recovered}"))
            .header("Cookie", expired_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        anonymous
            .post(format!("{base}/ui/purchase"))
            .header("Cookie", cookie)
            .header("Origin", "https://attacker.invalid")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert!(
        !client
            .get(format!("{base}/"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
            .contains("attacker-merchant</")
    );
    drop(process);
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
