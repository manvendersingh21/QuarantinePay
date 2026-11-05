mod config;
mod db;
mod integrations;
mod paypal;
mod worker;
use axum::{
    Form, Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response, Sse, sse::Event},
    routing::{get, post},
};
use config::Config;
use db::Order;
use quarantinepay_core::{Intent, SignedIntent, policy_digest};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{convert::Infallible, sync::Arc};
use subtle::ConstantTimeEq;
use uuid::Uuid;
#[derive(Clone)]
pub struct App {
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub client: reqwest::Client,
    pub paypal: paypal::PayPal,
}
struct Error(StatusCode, String);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        tracing::warn!(error=%e,"request failed");
        Self(StatusCode::BAD_REQUEST, e.to_string())
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error=%e,"database error");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "database operation failed".into(),
        )
    }
}
type Api<T> = Result<T, Error>;
fn equal(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}
fn session_message(app: &App, expires: i64) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    format!(
        "qp-session-v1:demo-buyer:{expires}:{}",
        hex::encode(Sha256::digest(app.config.password.as_bytes()))
    )
    .into_bytes()
}
fn session(app: &App) -> String {
    use ed25519_dalek::Signer;
    let expires = chrono::Utc::now().timestamp() + 28800;
    format!(
        "{expires}.{}",
        hex::encode(
            app.config
                .key
                .sign(&session_message(app, expires))
                .to_bytes()
        )
    )
}
fn valid_session(app: &App, token: &str) -> bool {
    let Some((expires, signature)) = token.split_once('.') else {
        return false;
    };
    let Ok(expires) = expires.parse::<i64>() else {
        return false;
    };
    let now = chrono::Utc::now().timestamp();
    if expires <= now || expires > now + 28800 {
        return false;
    }
    let Ok(bytes) = hex::decode(signature) else {
        return false;
    };
    let Ok(signature) = ed25519_dalek::Signature::from_slice(&bytes) else {
        return false;
    };
    app.config
        .key
        .verifying_key()
        .verify_strict(&session_message(app, expires), &signature)
        .is_ok()
}
fn authenticated(app: &App, h: &HeaderMap, write: bool) -> Api<()> {
    if h.get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.strip_prefix("Bearer ")
                .is_some_and(|s| equal(s, &app.config.password))
        })
    {
        return Ok(());
    }
    let cookie = h
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let valid = cookie
        .split(';')
        .filter_map(|c| c.trim().strip_prefix("qp_session="))
        .any(|v| valid_session(app, v));
    if !valid {
        return Err(Error(StatusCode::UNAUTHORIZED, "sign in required".into()));
    }
    if write
        && h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(app.config.base.as_str())
    {
        return Err(Error(StatusCode::FORBIDDEN, "origin rejected".into()));
    }
    Ok(())
}
#[derive(Deserialize)]
struct Login {
    password: String,
}
async fn login(State(app): State<App>, h: HeaderMap, Form(body): Form<Login>) -> Api<Response> {
    if h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(app.config.base.as_str()) {
        return Err(Error(StatusCode::FORBIDDEN, "origin rejected".into()));
    }
    if !equal(&body.password, &app.config.password) {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        return Err(Error(StatusCode::UNAUTHORIZED, "invalid password".into()));
    }
    let secure = if app.config.base.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    Ok((
        [(
            header::SET_COOKIE,
            format!(
                "qp_session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=28800{secure}",
                session(&app)
            ),
        )],
        Redirect::to("/"),
    )
        .into_response())
}
#[derive(Deserialize, Default)]
struct PageQuery {
    message: Option<String>,
}
async fn dashboard(
    State(app): State<App>,
    h: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Api<Html<String>> {
    let logged = authenticated(&app, &h, false).is_ok();
    let orders = if logged {
        sqlx::query_as::<_, Order>(
            "SELECT * FROM orders WHERE buyer='demo-buyer' ORDER BY updated_at DESC LIMIT 30",
        )
        .fetch_all(&app.pool)
        .await?
        .into_iter()
        .map(|o| serde_json::to_value(o).unwrap())
        .collect()
    } else {
        Vec::new()
    };
    let events = if logged {
        sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'kind',kind,'detail',detail,'at',created_at) FROM events WHERE buyer='demo-buyer' ORDER BY id DESC LIMIT 30").fetch_all(&app.pool).await?
    } else {
        Vec::new()
    };
    Ok(Html(quarantinepay_ui::render(
        app.config.mode.to_uppercase(),
        orders,
        events,
        logged,
        q.message.unwrap_or_default(),
    )))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Confirmation {
    sku: String,
    merchant_id: String,
    amount_minor: u64,
    currency: String,
    confirmed: bool,
}
async fn mint(app: &App, input: Confirmation) -> Api<SignedIntent> {
    if !input.confirmed
        || input.sku != "invoice-fixture-v1"
        || input.merchant_id != app.config.merchant
        || input.amount_minor != 100
        || input.currency != "USD"
        || app.config.merchant.is_empty()
    {
        return Err(Error(
            StatusCode::BAD_REQUEST,
            "confirm the supported SKU, allowlisted merchant, USD 1.00 price".into(),
        ));
    }
    let intent = Intent {
        version: 1,
        id: Uuid::new_v4(),
        buyer: "demo-buyer".into(),
        sku: input.sku,
        merchant_id: input.merchant_id,
        amount_minor: input.amount_minor,
        currency: input.currency,
        expires_at: chrono::Utc::now().timestamp() + 900,
        policy_digest: policy_digest(),
    };
    let signed = SignedIntent::sign(intent, &app.config.key)?;
    sqlx::query("INSERT INTO intents(id,buyer,token) VALUES($1,$2,$3)")
        .bind(signed.intent.id)
        .bind(&signed.intent.buyer)
        .bind(serde_json::to_value(&signed).unwrap())
        .execute(&app.pool)
        .await?;
    Ok(signed)
}
async fn intents(
    State(app): State<App>,
    h: HeaderMap,
    Json(input): Json<Confirmation>,
) -> Api<Json<SignedIntent>> {
    authenticated(&app, &h, true)?;
    Ok(Json(mint(&app, input).await?))
}
async fn checkout_inner(app: &App, signed: SignedIntent) -> Api<Uuid> {
    if let Err(e) = signed.verify(
        &app.config.key.verifying_key(),
        "demo-buyer",
        chrono::Utc::now().timestamp(),
    ) {
        db::event(
            &app.pool,
            None,
            "intent_rejected",
            json!({"reason":"signature, binding, ownership or expiry rejected"}),
        )
        .await?;
        return Err(e.into());
    }
    if signed.intent.merchant_id != app.config.merchant {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "merchant not allowlisted".into(),
        ));
    }
    let mut tx = app.pool.begin().await?;
    let consumed=sqlx::query("UPDATE intents SET used=true WHERE id=$1 AND buyer='demo-buyer' AND token=$2 AND used=false").bind(signed.intent.id).bind(serde_json::to_value(&signed).unwrap()).execute(&mut *tx).await?;
    if consumed.rows_affected() != 1 {
        return Err(Error(
            StatusCode::CONFLICT,
            "intent already used or unknown".into(),
        ));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO orders(id,buyer,intent,state,operation,request_id) VALUES($1,'demo-buyer',$2,'create_pending','create',$3)").bind(id).bind(serde_json::to_value(&signed).unwrap()).bind(Uuid::new_v4()).execute(&mut *tx).await?;
    db::event_tx(
        &mut tx,
        Some(id),
        "intent_bound",
        json!({"amount_minor":100,"currency":"USD","mode":app.config.mode}),
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}
async fn checkout(
    State(app): State<App>,
    h: HeaderMap,
    Json(signed): Json<SignedIntent>,
) -> Api<Json<Value>> {
    authenticated(&app, &h, true)?;
    Ok(Json(
        json!({"id":checkout_inner(&app,signed).await?,"state":"create_pending"}),
    ))
}
async fn authorize_inner(app: &App, id: Uuid) -> Api<()> {
    let result=sqlx::query("UPDATE orders SET state='authorize_pending',operation='authorize',request_id=$2,updated_at=now() WHERE id=$1 AND buyer='demo-buyer' AND state='approval_pending'").bind(id).bind(Uuid::new_v4()).execute(&app.pool).await?;
    if result.rows_affected() != 1 {
        return Err(Error(
            StatusCode::CONFLICT,
            "order is not awaiting approval".into(),
        ));
    }
    Ok(())
}
async fn authorize(State(app): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Api<Json<Value>> {
    authenticated(&app, &h, true)?;
    authorize_inner(&app, id).await?;
    Ok(Json(json!({"state":"authorize_pending"})))
}
async fn deliver_inner(app: &App, id: Uuid, artifact: Value) -> Api<()> {
    if serde_json::to_vec(&artifact).unwrap().len() > 65536 {
        return Err(Error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "artifact too large".into(),
        ));
    }
    let result=sqlx::query("UPDATE orders SET artifact=$2,state='quarantined',updated_at=now() WHERE id=$1 AND buyer='demo-buyer' AND state='authorized' AND deadline>now()").bind(id).bind(artifact).execute(&app.pool).await?;
    if result.rows_affected() != 1 {
        return Err(Error(
            StatusCode::CONFLICT,
            "order cannot accept delivery".into(),
        ));
    }
    Ok(())
}
async fn deliver(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(artifact): Json<Value>,
) -> Api<Json<Value>> {
    authenticated(&app, &h, true)?;
    deliver_inner(&app, id, artifact).await?;
    Ok(Json(json!({"state":"quarantined"})))
}
async fn order(State(app): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Api<Json<Order>> {
    authenticated(&app, &h, false)?;
    let o = sqlx::query_as("SELECT * FROM orders WHERE id=$1 AND buyer='demo-buyer'")
        .bind(id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(|| Error(StatusCode::NOT_FOUND, "order not found".into()))?;
    Ok(Json(o))
}
async fn ui_purchase(State(app): State<App>, h: HeaderMap) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    let signed = mint(
        &app,
        Confirmation {
            sku: "invoice-fixture-v1".into(),
            merchant_id: app.config.merchant.clone(),
            amount_minor: 100,
            currency: "USD".into(),
            confirmed: true,
        },
    )
    .await?;
    checkout_inner(&app, signed).await?;
    Ok(Redirect::to("/"))
}
async fn ui_authorize(State(app): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    authorize_inner(&app, id).await?;
    Ok(Redirect::to("/"))
}
#[derive(Deserialize)]
struct DeliveryForm {
    fixture: String,
}
async fn ui_deliver(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<DeliveryForm>,
) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    let artifact = if form.fixture == "pass" {
        include_str!("../../../fixtures/pass.json")
    } else {
        include_str!("../../../fixtures/fail.json")
    };
    deliver_inner(&app, id, serde_json::from_str(artifact).unwrap()).await?;
    Ok(Redirect::to("/"))
}
#[derive(Deserialize)]
struct Search {
    query: String,
}
async fn catalog(
    State(app): State<App>,
    h: HeaderMap,
    Query(q): Query<Search>,
) -> Api<Json<Value>> {
    authenticated(&app, &h, false)?;
    if q.query.len() > 200 {
        return Err(Error(StatusCode::BAD_REQUEST, "query too long".into()));
    }
    Ok(Json(integrations::catalog(&app, &q.query).await?))
}
async fn agent(State(app): State<App>, h: HeaderMap, Json(q): Json<Search>) -> Api<Json<Value>> {
    authenticated(&app, &h, true)?;
    if q.query.len() > 2000 {
        return Err(Error(StatusCode::BAD_REQUEST, "query too long".into()));
    }
    Ok(Json(integrations::agent(&app, &q.query).await?))
}
async fn ui_agent(State(app): State<App>, h: HeaderMap, Form(q): Form<Search>) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    if q.query.len() > 2000 {
        return Err(Error(StatusCode::BAD_REQUEST, "query too long".into()));
    }
    let value = integrations::agent(&app, &q.query).await?;
    db::event(&app.pool, None, "muse_recommendation", value).await?;
    Ok(Redirect::to("/"))
}
async fn ui_catalog(State(app): State<App>, h: HeaderMap, Form(q): Form<Search>) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    if q.query.len() > 200 {
        return Err(Error(StatusCode::BAD_REQUEST, "query too long".into()));
    }
    let value = integrations::catalog(&app, &q.query).await?;
    db::event(&app.pool, None, "catalog_results", value).await?;
    Ok(Redirect::to("/"))
}
async fn ui_redteam(State(app): State<App>, h: HeaderMap) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    let mut signed = mint(
        &app,
        Confirmation {
            sku: "invoice-fixture-v1".into(),
            merchant_id: app.config.merchant.clone(),
            amount_minor: 100,
            currency: "USD".into(),
            confirmed: true,
        },
    )
    .await?;
    signed.intent.merchant_id = "attacker-merchant".into();
    if checkout_inner(&app, signed).await.is_ok() {
        return Err(Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "security probe unexpectedly accepted".into(),
        ));
    }
    Ok(Redirect::to("/"))
}
async fn webhook(
    State(app): State<App>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> Api<Json<Value>> {
    app.paypal.verify_webhook(&h, &body).await?;
    let id = body["id"]
        .as_str()
        .ok_or_else(|| Error(StatusCode::BAD_REQUEST, "missing webhook ID".into()))?;
    // A webhook is a reconciliation hint, never a buyer command or a settlement authority.
    sqlx::query("INSERT INTO webhook_receipts(id) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(id)
        .execute(&app.pool)
        .await?;
    Ok(Json(json!({"received":true})))
}
#[derive(Deserialize)]
struct EventCursor {
    after: Option<i64>,
}
async fn events(
    State(app): State<App>,
    h: HeaderMap,
    Query(cursor): Query<EventCursor>,
) -> Api<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>> {
    authenticated(&app, &h, false)?;
    let last = cursor.after.unwrap_or(0).max(0);
    let stream = futures_util::stream::unfold((app, last), |(app, last)| async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let rows=sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'kind',kind,'order_id',order_id,'detail',detail,'at',created_at) FROM events WHERE buyer='demo-buyer' AND id>$1 ORDER BY id LIMIT 100").bind(last).fetch_all(&app.pool).await.unwrap_or_default();
        let next = rows.last().and_then(|r| r["id"].as_i64()).unwrap_or(last);
        Some((
            Ok(Event::default()
                .event("audit")
                .data(json!(rows).to_string())
                .id(next.to_string())),
            (app, next),
        ))
    });
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::load()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database)
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let app = App {
        paypal: paypal::PayPal::new(client.clone(), config.mode == "replay"),
        config: Arc::new(config),
        pool,
        client,
    };
    integrations::prepare_elastic(&app).await?;
    tokio::spawn(worker::run(app.clone()));
    let router = Router::new()
        .nest_service("/assets", tower_http::services::ServeDir::new("assets"))
        .route("/", get(dashboard))
        .route(
            "/style.css",
            get(|| async { ([(header::CONTENT_TYPE, "text/css")], quarantinepay_ui::CSS) }),
        )
        .route("/login", post(login))
        .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route(
            "/docs",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    include_str!("../../../README.md"),
                )
            }),
        )
        .route("/api/intents", post(intents))
        .route("/api/checkout", post(checkout))
        .route("/api/orders/{id}", get(order))
        .route("/api/orders/{id}/authorize", post(authorize))
        .route("/api/orders/{id}/deliveries", post(deliver))
        .route("/api/catalog/search", get(catalog))
        .route("/api/agent/runs", post(agent))
        .route("/api/events", get(events))
        .route("/api/webhooks/paypal", post(webhook))
        .route("/ui/redteam", post(ui_redteam))
        .route("/ui/purchase", post(ui_purchase))
        .route("/ui/orders/{id}/authorize", post(ui_authorize))
        .route("/ui/orders/{id}/deliveries", post(ui_deliver))
        .route("/ui/agent", post(ui_agent))
        .route("/ui/catalog", post(ui_catalog))
        .route("/paypal/return", get(|| async { Redirect::to("/") }))
        .layer(tower_http::set_header::SetResponseHeaderLayer::if_not_present(header::X_CONTENT_TYPE_OPTIONS, axum::http::HeaderValue::from_static("nosniff")))
        .layer(tower_http::set_header::SetResponseHeaderLayer::if_not_present(header::X_FRAME_OPTIONS, axum::http::HeaderValue::from_static("DENY")))
        .layer(tower_http::set_header::SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, axum::http::HeaderValue::from_static("same-origin")))
        .layer(tower_http::set_header::SetResponseHeaderLayer::if_not_present(header::CONTENT_SECURITY_POLICY, axum::http::HeaderValue::from_static("default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'")))
        .layer(DefaultBodyLimit::max(70000))
        .with_state(app.clone());
    let listener = tokio::net::TcpListener::bind(&app.config.bind).await?;
    tracing::info!(address=%app.config.bind,mode=%app.config.mode,"QuarantinePay ready; administrator password is configured or hex-encoded in .local/admin-password");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
