mod agent;
mod analyst;
mod catalog_cache;
mod config;
mod db;
mod inference;
mod integrations;
mod paypal;
mod shopify;
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
use quarantinepay_core::{
    CONTRACT_SKU, Intent, PurchaseContract, SignedIntent, contract::contract_fixture,
    policy_digest, scenario, scenario_fixture,
};
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
            "SELECT * FROM orders WHERE buyer='demo-buyer' AND mode=$1 ORDER BY updated_at DESC LIMIT 30",
        )
        .bind(&app.config.mode)
        .fetch_all(&app.pool)
        .await?
        .into_iter()
        .map(|o| serde_json::to_value(o).unwrap())
        .collect()
    } else {
        Vec::new()
    };
    let events = if logged {
        sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'kind',kind,'detail',detail,'at',created_at) FROM events WHERE buyer='demo-buyer' AND (order_id IS NULL OR order_id IN (SELECT id FROM orders WHERE mode=$1)) AND (task_id IS NULL OR task_id IN (SELECT id FROM agent_tasks WHERE mode=$1)) ORDER BY id DESC LIMIT 30").bind(&app.config.mode).fetch_all(&app.pool).await?
    } else {
        Vec::new()
    };
    Ok(Html(quarantinepay_ui::render(
        app.config.mode.to_uppercase(),
        app.config.inference.label().into(),
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
    let selected = scenario(&input.sku);
    if !input.confirmed
        || (selected.is_none() && input.sku != "invoice-fixture-v1")
        || input.merchant_id != app.config.merchant
        || input.amount_minor != selected.as_ref().map_or(100, |s| s.amount_minor)
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
        policy_digest: selected
            .as_ref()
            .map_or_else(policy_digest, |s| s.signed_terms().digest()),
        scenario: selected.map(|s| s.signed_terms()),
        contract: None,
    };
    let signed = SignedIntent::sign(intent, &app.config.key)?;
    sqlx::query("INSERT INTO intents(id,buyer,token,mode) VALUES($1,$2,$3,$4)")
        .bind(signed.intent.id)
        .bind(&signed.intent.buyer)
        .bind(serde_json::to_value(&signed).unwrap())
        .bind(&app.config.mode)
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
async fn checkout_inner(app: &App, signed: SignedIntent, outcome: &str) -> Api<Uuid> {
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
    let consumed=sqlx::query("UPDATE intents SET used=true WHERE id=$1 AND buyer='demo-buyer' AND token=$2 AND used=false AND mode=$3").bind(signed.intent.id).bind(serde_json::to_value(&signed).unwrap()).bind(&app.config.mode).execute(&mut *tx).await?;
    if consumed.rows_affected() != 1 {
        return Err(Error(
            StatusCode::CONFLICT,
            "intent already used or unknown".into(),
        ));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO orders(id,buyer,intent,state,operation,request_id,mode,simulation_outcome) VALUES($1,'demo-buyer',$2,'create_pending','create',$3,$4,$5)").bind(id).bind(serde_json::to_value(&signed).unwrap()).bind(Uuid::new_v4()).bind(&app.config.mode).bind(outcome).execute(&mut *tx).await?;
    db::event_tx(
        &mut tx,
        Some(id),
        "intent_bound",
        json!({"amount_minor":signed.intent.amount_minor,"currency":"USD","mode":app.config.mode,"scenario":signed.intent.scenario}),
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
        json!({"id":checkout_inner(&app,signed,"approve").await?,"state":"create_pending"}),
    ))
}
async fn authorize_inner(app: &App, id: Uuid) -> Api<()> {
    let result=sqlx::query("UPDATE orders SET state='authorize_pending',operation='authorize',request_id=$2,updated_at=now() WHERE id=$1 AND buyer='demo-buyer' AND state='approval_pending' AND mode=$3 AND (intent->'intent'->>'expires_at')::bigint>extract(epoch FROM now())").bind(id).bind(Uuid::new_v4()).bind(&app.config.mode).execute(&app.pool).await?;
    if result.rows_affected() != 1 {
        return Err(Error(
            StatusCode::CONFLICT,
            "order is not awaiting approval or its purchase intent has expired".into(),
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
    let result=sqlx::query("UPDATE orders SET artifact=$2,state='quarantined',updated_at=now() WHERE id=$1 AND buyer='demo-buyer' AND state='authorized' AND deadline>now() AND mode=$3").bind(id).bind(artifact).bind(&app.config.mode).execute(&app.pool).await?;
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
    let o = sqlx::query_as("SELECT * FROM orders WHERE id=$1 AND buyer='demo-buyer' AND mode=$2")
        .bind(id)
        .bind(&app.config.mode)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(|| Error(StatusCode::NOT_FOUND, "order not found".into()))?;
    Ok(Json(o))
}
#[derive(Deserialize)]
struct PurchaseForm {
    sku: String,
    #[serde(default)]
    outcome: String,
}
async fn ui_purchase(
    State(app): State<App>,
    h: HeaderMap,
    Form(form): Form<PurchaseForm>,
) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    let selected = scenario(&form.sku)
        .ok_or_else(|| Error(StatusCode::BAD_REQUEST, "unknown scenario".into()))?;
    if !["", "approve", "decline", "ambiguous"].contains(&form.outcome.as_str())
        || (app.config.mode != "replay" && !["", "approve"].contains(&form.outcome.as_str()))
    {
        return Err(Error(
            StatusCode::BAD_REQUEST,
            "invalid simulated payment outcome".into(),
        ));
    }
    let signed = mint(
        &app,
        Confirmation {
            sku: form.sku,
            merchant_id: app.config.merchant.clone(),
            amount_minor: selected.amount_minor,
            currency: "USD".into(),
            confirmed: true,
        },
    )
    .await?;
    let outcome = if form.outcome.is_empty() {
        "approve"
    } else {
        &form.outcome
    };
    checkout_inner(&app, signed, outcome).await?;
    Ok(Redirect::to("/"))
}
async fn ui_authorize(State(app): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    authorize_inner(&app, id).await?;
    Ok(back(&app, &h))
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
    if !["pass", "fail", "malicious"].contains(&form.fixture.as_str()) {
        return Err(Error(StatusCode::BAD_REQUEST, "unknown fixture".into()));
    }
    let order: Order =
        sqlx::query_as("SELECT * FROM orders WHERE id=$1 AND buyer='demo-buyer' AND mode=$2")
            .bind(id)
            .bind(&app.config.mode)
            .fetch_optional(&app.pool)
            .await?
            .ok_or_else(|| Error(StatusCode::NOT_FOUND, "order not found".into()))?;
    let signed: SignedIntent = serde_json::from_value(order.intent).map_err(anyhow::Error::from)?;
    let artifact = if let Some(contract) = &signed.intent.contract {
        contract_fixture(contract, &form.fixture)
    } else if form.fixture == "malicious" {
        return Err(Error(
            StatusCode::BAD_REQUEST,
            "the injected-instruction example applies to agent contracts".into(),
        ));
    } else if let Some(terms) = signed.intent.scenario {
        scenario_fixture(&terms.id, form.fixture == "pass")
    } else {
        serde_json::from_str(if form.fixture == "pass" {
            include_str!("../../../fixtures/pass.json")
        } else {
            include_str!("../../../fixtures/fail.json")
        })
        .unwrap()
    };
    deliver_inner(&app, id, artifact).await?;
    db::event(
        &app.pool,
        Some(id),
        "seller_delivery",
        json!({"simulated":true,"example":form.fixture}),
    )
    .await?;
    Ok(back(&app, &h))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    decision: String,
    #[serde(default)]
    acknowledge_risk: bool,
}
async fn review_inner(app: &App, id: Uuid, decision: &str, acknowledge_risk: bool) -> Api<()> {
    if !["approve", "reject"].contains(&decision) {
        return Err(Error(
            StatusCode::BAD_REQUEST,
            "decision must be approve or reject".into(),
        ));
    }
    let capture = decision == "approve";
    let mut tx = app.pool.begin().await?;
    // A policy hold set by deterministic rules or the analyst blocks capture unless the buyer
    // explicitly overrides it. The model itself can never lift a hold.
    let hold: Option<Option<Value>> = sqlx::query_scalar(
        "SELECT analysis FROM orders WHERE id=$1 AND buyer='demo-buyer' AND mode=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(&app.config.mode)
    .fetch_optional(&mut *tx)
    .await?;
    let policy_hold = hold.flatten().is_some_and(|a| a["policy_hold"] == true);
    if capture && policy_hold && !acknowledge_risk {
        return Err(Error(
            StatusCode::CONFLICT,
            "policy hold: untrusted instructions were detected in this delivery; reject it, or acknowledge the risk to override".into(),
        ));
    }
    let risk_override = capture && policy_hold;
    let result = sqlx::query("UPDATE orders SET state=$2, operation=$3, request_id=$4, review_decision=$5, risk_override=$7, updated_at=now() WHERE id=$1 AND buyer='demo-buyer' AND mode=$6 AND state='buyer_review' AND deadline>now() AND (intent->'intent'->>'expires_at')::bigint>extract(epoch FROM now())")
        .bind(id).bind(if capture { "capture_pending" } else { "void_pending" }).bind(if capture { "capture" } else { "void" }).bind(Uuid::new_v4()).bind(decision).bind(&app.config.mode).bind(risk_override).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(Error(
            StatusCode::CONFLICT,
            "order is not awaiting a timely buyer review".into(),
        ));
    }
    db::event_tx(
        &mut tx,
        Some(id),
        "buyer_review",
        json!({"decision":decision,"risk_override":risk_override}),
    )
    .await?;
    if risk_override {
        db::event_tx(
            &mut tx,
            Some(id),
            "risk_override",
            json!({"reason":"Buyer acknowledged flagged content and overrode the policy hold"}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
async fn review(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<Review>,
) -> Api<Json<Value>> {
    authenticated(&app, &h, true)?;
    review_inner(&app, id, &input.decision, input.acknowledge_risk).await?;
    Ok(Json(json!({"decision":input.decision})))
}
async fn ui_review(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Form(input): Form<Review>,
) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    review_inner(&app, id, &input.decision, input.acknowledge_risk).await?;
    Ok(back(&app, &h))
}
async fn shopify_dev_store(State(app): State<App>, h: HeaderMap) -> Api<Json<Value>> {
    authenticated(&app, &h, false)?;
    Ok(Json(shopify::read_dev_store().await?))
}
async fn ui_reset(State(app): State<App>, h: HeaderMap) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    if app.config.mode != "replay" {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "reset is only available in simulation mode".into(),
        ));
    }
    let mut tx = app.pool.begin().await?;
    sqlx::query("DELETE FROM events WHERE order_id IN (SELECT id FROM orders WHERE mode='replay' AND buyer='demo-buyer') OR task_id IN (SELECT id FROM agent_tasks WHERE mode='replay' AND buyer='demo-buyer')").execute(&mut *tx).await?;
    sqlx::query("DELETE FROM agent_tasks WHERE mode='replay' AND buyer='demo-buyer'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM orders WHERE mode='replay' AND buyer='demo-buyer'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM intents WHERE mode='replay' AND buyer='demo-buyer'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
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
    db::event(&app.pool, None, "agent_recommendation", value).await?;
    Ok(Redirect::to("/"))
}
async fn catalog_sync(State(app): State<App>, h: HeaderMap) -> Api<Json<Value>> {
    authenticated(&app, &h, true)?;
    let result = catalog_cache::synchronize(&app).await?;
    db::event(&app.pool, None, "catalog_import", result.clone()).await?;
    Ok(Json(result))
}
async fn ui_catalog_sync(State(app): State<App>, h: HeaderMap) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    let result = match catalog_cache::synchronize(&app).await {
        Ok(result) => result,
        Err(error) => {
            let mut target = reqwest::Url::parse("http://localhost/").unwrap();
            target
                .query_pairs_mut()
                .append_pair("message", &error.to_string());
            return Ok(Redirect::to(&format!("/?{}", target.query().unwrap_or(""))));
        }
    };
    db::event(&app.pool, None, "catalog_import", result).await?;
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
    if checkout_inner(&app, signed, "approve").await.is_ok() {
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
        let rows=sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'kind',kind,'order_id',order_id,'task_id',task_id,'detail',detail,'at',created_at) FROM events WHERE buyer='demo-buyer' AND id>$1 AND (order_id IS NULL OR order_id IN (SELECT id FROM orders WHERE mode=$2)) AND (task_id IS NULL OR task_id IN (SELECT id FROM agent_tasks WHERE mode=$2)) ORDER BY id LIMIT 100").bind(last).bind(&app.config.mode).fetch_all(&app.pool).await.unwrap_or_default();
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
/// Return to the agent console when a form was posted from it; otherwise to the command center.
/// Only same-origin `/agent` paths made of safe characters are accepted (no open redirect).
fn back(app: &App, h: &HeaderMap) -> Redirect {
    let path = h
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .and_then(|r| r.strip_prefix(app.config.base.as_str()))
        .filter(|p| {
            p.starts_with("/agent")
                && p.len() <= 64
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'-')
        })
        .unwrap_or("/");
    Redirect::to(path)
}
async fn mint_contract(app: &App, contract: PurchaseContract) -> Api<SignedIntent> {
    contract.validate()?;
    if app.config.merchant.is_empty() {
        return Err(Error(
            StatusCode::BAD_REQUEST,
            "merchant not configured".into(),
        ));
    }
    let intent = Intent {
        version: 1,
        id: Uuid::new_v4(),
        buyer: "demo-buyer".into(),
        sku: CONTRACT_SKU.into(),
        merchant_id: app.config.merchant.clone(),
        amount_minor: contract.amount_minor,
        currency: contract.currency.clone(),
        // Long enough for PayPal approval, delivery, analysis and review; capture after expiry is refused.
        expires_at: chrono::Utc::now().timestamp() + 1800,
        policy_digest: contract.digest(),
        scenario: None,
        contract: Some(contract),
    };
    let signed = SignedIntent::sign(intent, &app.config.key)?;
    sqlx::query("INSERT INTO intents(id,buyer,token,mode) VALUES($1,$2,$3,$4)")
        .bind(signed.intent.id)
        .bind(&signed.intent.buyer)
        .bind(serde_json::to_value(&signed).unwrap())
        .bind(&app.config.mode)
        .execute(&app.pool)
        .await?;
    Ok(signed)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskRequest {
    request: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskApproval {
    proposal_digest: String,
    #[serde(default)]
    outcome: String,
}
/// The buyer approves one exact contract digest. Approval is single-use and re-validated.
async fn approve_task_inner(app: &App, id: Uuid, input: &TaskApproval) -> Api<Uuid> {
    let outcome = if input.outcome.is_empty() {
        "approve"
    } else {
        input.outcome.as_str()
    };
    if !["approve", "decline", "ambiguous"].contains(&outcome)
        || (app.config.mode != "replay" && outcome != "approve")
    {
        return Err(Error(
            StatusCode::BAD_REQUEST,
            "invalid simulated payment outcome".into(),
        ));
    }
    let proposal: Option<Value> = sqlx::query_scalar("UPDATE agent_tasks SET state='approved',updated_at=now() WHERE id=$1 AND buyer='demo-buyer' AND mode=$2 AND state='awaiting_approval' AND proposal_digest=$3 RETURNING proposal")
        .bind(id)
        .bind(&app.config.mode)
        .bind(&input.proposal_digest)
        .fetch_optional(&app.pool)
        .await?;
    let Some(proposal) = proposal else {
        return Err(Error(
            StatusCode::CONFLICT,
            "the task is not awaiting approval or the approved digest does not match the proposed contract".into(),
        ));
    };
    let result: Api<Uuid> = async {
        let contract: PurchaseContract =
            serde_json::from_value(proposal).map_err(anyhow::Error::from)?;
        if contract.digest() != input.proposal_digest {
            return Err(Error(
                StatusCode::CONFLICT,
                "stored proposal does not match its digest".into(),
            ));
        }
        let signed = mint_contract(app, contract).await?;
        checkout_inner(app, signed, outcome).await
    }
    .await;
    match result {
        Ok(order_id) => {
            sqlx::query("UPDATE agent_tasks SET state='purchasing',order_id=$2,updated_at=now() WHERE id=$1")
                .bind(id)
                .bind(order_id)
                .execute(&app.pool)
                .await?;
            db::task_event(
                &app.pool,
                id,
                "agent_buyer_approved",
                json!({"task_id":id,"order_id":order_id,"digest":input.proposal_digest,"signed_intent":true}),
            )
            .await?;
            Ok(order_id)
        }
        Err(e) => {
            sqlx::query(
                "UPDATE agent_tasks SET state='failed',error=$2,updated_at=now() WHERE id=$1",
            )
            .bind(id)
            .bind(&e.1)
            .execute(&app.pool)
            .await?;
            Err(e)
        }
    }
}
async fn cancel_task_inner(app: &App, id: Uuid) -> Api<()> {
    let r = sqlx::query("UPDATE agent_tasks SET state='cancelled',updated_at=now() WHERE id=$1 AND buyer='demo-buyer' AND mode=$2 AND state IN ('awaiting_approval','no_deal','failed')")
        .bind(id)
        .bind(&app.config.mode)
        .execute(&app.pool)
        .await?;
    if r.rows_affected() != 1 {
        return Err(Error(
            StatusCode::CONFLICT,
            "only an unapproved proposal can be cancelled".into(),
        ));
    }
    db::task_event(&app.pool, id, "agent_cancelled", json!({"task_id":id})).await?;
    Ok(())
}
async fn api_create_task(
    State(app): State<App>,
    h: HeaderMap,
    Json(input): Json<TaskRequest>,
) -> Api<Json<Value>> {
    authenticated(&app, &h, true)?;
    let id = agent::create(&app, &input.request).await?;
    Ok(Json(json!({"id":id,"state":"planning"})))
}
async fn api_tasks(State(app): State<App>, h: HeaderMap) -> Api<Json<Value>> {
    authenticated(&app, &h, false)?;
    Ok(Json(json!({"tasks":agent::list(&app).await?})))
}
async fn api_task(State(app): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Api<Json<Value>> {
    authenticated(&app, &h, false)?;
    agent::get(&app, id)
        .await?
        .map(Json)
        .ok_or_else(|| Error(StatusCode::NOT_FOUND, "task not found".into()))
}
async fn api_approve_task(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<TaskApproval>,
) -> Api<Json<Value>> {
    authenticated(&app, &h, true)?;
    let order_id = approve_task_inner(&app, id, &input).await?;
    Ok(Json(json!({"order_id":order_id,"state":"create_pending"})))
}
async fn api_cancel_task(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Api<Json<Value>> {
    authenticated(&app, &h, true)?;
    cancel_task_inner(&app, id).await?;
    Ok(Json(json!({"state":"cancelled"})))
}
async fn api_analytics(State(app): State<App>, h: HeaderMap) -> Api<Json<Value>> {
    authenticated(&app, &h, false)?;
    Ok(Json(agent::analytics(&app).await?))
}
async fn ui_create_task(
    State(app): State<App>,
    h: HeaderMap,
    Form(input): Form<TaskRequest>,
) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    let id = agent::create(&app, &input.request).await?;
    Ok(Redirect::to(&format!("/agent/{id}")))
}
async fn ui_approve_task(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Form(input): Form<TaskApproval>,
) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    approve_task_inner(&app, id, &input).await?;
    Ok(Redirect::to(&format!("/agent/{id}")))
}
async fn ui_cancel_task(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Api<Redirect> {
    authenticated(&app, &h, true)?;
    cancel_task_inner(&app, id).await?;
    Ok(Redirect::to(&format!("/agent/{id}")))
}
async fn agent_page_inner(
    app: &App,
    h: &HeaderMap,
    id: Option<Uuid>,
    message: String,
) -> Api<Html<String>> {
    let logged = authenticated(app, h, false).is_ok();
    let (tasks, selected, events) = if logged {
        let tasks = agent::list(app).await?;
        let selected_id = id.or_else(|| {
            tasks
                .first()
                .and_then(|t| t["id"].as_str())
                .and_then(|s| s.parse().ok())
        });
        let selected = match selected_id {
            Some(id) => agent::get(app, id).await?,
            None => None,
        };
        let events = agent::events(app, selected_id).await?;
        (tasks, selected, events)
    } else {
        (Vec::new(), None, Vec::new())
    };
    Ok(Html(quarantinepay_ui::render_agent(
        app.config.mode.to_uppercase(),
        app.config.inference.label().into(),
        tasks,
        selected,
        events,
        logged,
        message,
    )))
}
async fn agent_page(
    State(app): State<App>,
    h: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Api<Html<String>> {
    agent_page_inner(&app, &h, None, q.message.unwrap_or_default()).await
}
async fn agent_task_page(
    State(app): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Api<Html<String>> {
    agent_page_inner(&app, &h, Some(id), q.message.unwrap_or_default()).await
}
async fn intelligence_page(State(app): State<App>, h: HeaderMap) -> Response {
    let logged = authenticated(&app, &h, false).is_ok();
    let nonce = {
        use rand::RngCore;
        let mut bytes = [0u8; 18];
        rand::thread_rng().fill_bytes(&mut bytes);
        hex::encode(bytes)
    };
    let csp = format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'nonce-{nonce}'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
    );
    (
        [(header::CONTENT_SECURITY_POLICY, csp)],
        Html(quarantinepay_ui::render_intelligence(
            app.config.mode.to_uppercase(),
            app.config.inference.label().into(),
            nonce,
            logged,
        )),
    )
        .into_response()
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
    // Elastic is an optional screening aid; its absence must not take the payment gateway down.
    if let Err(e) = integrations::prepare_elastic(&app).await {
        tracing::warn!(error=%e, "Elastic screening index unavailable; continuing without it");
    }
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
        .route("/api/shopify/dev-store", get(shopify_dev_store))
        .route("/api/scenarios", get(|| async { Json(quarantinepay_core::scenarios()) }))
        .route("/api/intents", post(intents))
        .route("/api/checkout", post(checkout))
        .route("/api/orders/{id}", get(order))
        .route("/api/orders/{id}/authorize", post(authorize))
        .route("/api/orders/{id}/deliveries", post(deliver))
        .route("/api/orders/{id}/review", post(review))
        .route("/api/catalog/search", get(catalog))
        .route("/api/catalog/sync",post(catalog_sync))
        .route("/api/agent/runs", post(agent))
        .route("/api/agent/tasks", post(api_create_task).get(api_tasks))
        .route("/api/agent/tasks/{id}", get(api_task))
        .route("/api/agent/tasks/{id}/approve", post(api_approve_task))
        .route("/api/agent/tasks/{id}/cancel", post(api_cancel_task))
        .route("/api/analytics/transactions", get(api_analytics))
        .route("/agent", get(agent_page))
        .route("/agent/{id}", get(agent_task_page))
        .route("/intelligence", get(intelligence_page))
        .route("/ui/agent/tasks", post(ui_create_task))
        .route("/ui/agent/tasks/{id}/approve", post(ui_approve_task))
        .route("/ui/agent/tasks/{id}/cancel", post(ui_cancel_task))
        .route("/api/events", get(events))
        .route("/api/webhooks/paypal", post(webhook))
        .route("/ui/redteam", post(ui_redteam))
        .route("/ui/purchase", post(ui_purchase))
        .route("/ui/reset", post(ui_reset))
        .route("/ui/orders/{id}/authorize", post(ui_authorize))
        .route("/ui/orders/{id}/deliveries", post(ui_deliver))
        .route("/ui/orders/{id}/review", post(ui_review))
        .route("/ui/agent", post(ui_agent))
        .route("/ui/catalog", post(ui_catalog))
        .route("/ui/catalog/sync",post(ui_catalog_sync))
        .route("/paypal/return", get(|| async { Redirect::to("/agent") }))
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
