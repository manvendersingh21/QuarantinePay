//! Buyer-agent tasks: plan → discover → negotiate → propose. A task can only *propose*; the
//! buyer's approval of the exact contract digest is what creates a signed intent and an order.
use crate::{App, db};
use anyhow::Result;
use quarantinepay_core::contract::{compile_contract, negotiate, sellers};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn create(app: &App, request: &str) -> Result<Uuid> {
    let request: String = request
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(2000)
        .collect();
    anyhow::ensure!(
        request.trim().len() >= 8,
        "describe what the agent should buy"
    );
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_tasks(id,buyer,mode,request,state) VALUES($1,'demo-buyer',$2,$3,'planning')")
        .bind(id)
        .bind(&app.config.mode)
        .bind(&request)
        .execute(&app.pool)
        .await?;
    db::task_event(
        &app.pool,
        id,
        "agent_task_created",
        json!({"task_id":id,"request":request}),
    )
    .await?;
    let worker = app.clone();
    tokio::spawn(async move {
        if let Err(e) = run(&worker, id, &request).await {
            tracing::warn!(%id, error=%e, "agent task failed");
            let _ = sqlx::query("UPDATE agent_tasks SET state='failed',error=$2,updated_at=now() WHERE id=$1 AND state IN ('planning','negotiating')")
                .bind(id)
                .bind(e.to_string())
                .execute(&worker.pool)
                .await;
            let _ = db::task_event(
                &worker.pool,
                id,
                "agent_failed",
                json!({"task_id":id,"error":e.to_string()}),
            )
            .await;
        }
    });
    Ok(id)
}

async fn run(app: &App, id: Uuid, request: &str) -> Result<()> {
    let (objective, planner) = crate::analyst::plan(app, request).await;
    sqlx::query("UPDATE agent_tasks SET state='negotiating',planner=$2,objective=$3,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(&planner)
        .bind(serde_json::to_value(&objective)?)
        .execute(&app.pool)
        .await?;
    db::task_event(
        &app.pool,
        id,
        "agent_objective",
        json!({"task_id":id,"planner":planner["source"],"objective":objective}),
    )
    .await?;
    db::task_event(
        &app.pool,
        id,
        "agent_discovery",
        json!({"task_id":id,"sellers":sellers().iter().map(|s| json!({"id":s.id,"name":s.name,"list_minor":s.list_minor})).collect::<Vec<_>>(),"note":"Fictional seller agents"}),
    )
    .await?;
    let negotiation = negotiate(&objective);
    db::task_event(
        &app.pool,
        id,
        "agent_negotiation",
        json!({"task_id":id,"messages":negotiation.transcript.len(),"offers":negotiation.offers,"rationale":negotiation.rationale}),
    )
    .await?;
    match compile_contract(&objective, &negotiation) {
        Ok(contract) => {
            let digest = contract.digest();
            sqlx::query("UPDATE agent_tasks SET state='awaiting_approval',negotiation=$2,proposal=$3,proposal_digest=$4,updated_at=now() WHERE id=$1 AND state='negotiating'")
                .bind(id)
                .bind(serde_json::to_value(&negotiation)?)
                .bind(serde_json::to_value(&contract)?)
                .bind(&digest)
                .execute(&app.pool)
                .await?;
            db::task_event(
                &app.pool,
                id,
                "agent_contract_proposed",
                json!({"task_id":id,"seller":contract.seller,"amount_minor":contract.amount_minor,"digest":digest,"requires_buyer_approval":true}),
            )
            .await?;
        }
        Err(e) => {
            sqlx::query("UPDATE agent_tasks SET state='no_deal',negotiation=$2,error=$3,updated_at=now() WHERE id=$1 AND state='negotiating'")
                .bind(id)
                .bind(serde_json::to_value(&negotiation)?)
                .bind(e.to_string())
                .execute(&app.pool)
                .await?;
            db::task_event(
                &app.pool,
                id,
                "agent_no_deal",
                json!({"task_id":id,"reason":e.to_string()}),
            )
            .await?;
        }
    }
    Ok(())
}

const TASK_JSON: &str = "SELECT jsonb_build_object('id',t.id,'request',t.request,'state',t.state,'planner',t.planner,'objective',t.objective,'negotiation',t.negotiation,'proposal',t.proposal,'proposal_digest',t.proposal_digest,'order_id',t.order_id,'error',t.error,'created_at',t.created_at,'updated_at',t.updated_at,'mode',t.mode,'order',(SELECT to_jsonb(o) FROM orders o WHERE o.id=t.order_id AND o.mode=t.mode)) FROM agent_tasks t";

pub async fn get(app: &App, id: Uuid) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar::<_, Value>(&format!(
        "{TASK_JSON} WHERE t.id=$1 AND t.buyer='demo-buyer' AND t.mode=$2"
    ))
    .bind(id)
    .bind(&app.config.mode)
    .fetch_optional(&app.pool)
    .await?)
}
pub async fn list(app: &App) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object('id',t.id,'request',t.request,'state',COALESCE((SELECT o.state FROM orders o WHERE o.id=t.order_id AND o.state IN ('captured','voided','declined','reconciliation_required')),t.state),'task_state',t.state,'created_at',t.created_at,'amount_minor',(t.proposal->>'amount_minor')::bigint,'seller',t.proposal->>'seller','order_state',(SELECT state FROM orders o WHERE o.id=t.order_id)) FROM agent_tasks t WHERE t.buyer='demo-buyer' AND t.mode=$1 ORDER BY t.created_at DESC LIMIT 20")
        .bind(&app.config.mode)
        .fetch_all(&app.pool)
        .await?)
}
pub async fn events(app: &App, task: Option<Uuid>) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object('id',e.id,'kind',e.kind,'order_id',e.order_id,'task_id',e.task_id,'detail',e.detail,'at',e.created_at) FROM events e WHERE e.buyer='demo-buyer' AND ($1::uuid IS NULL OR e.task_id=$1 OR e.order_id=(SELECT order_id FROM agent_tasks WHERE id=$1)) AND (e.order_id IS NULL OR e.order_id IN (SELECT id FROM orders WHERE mode=$2)) AND (e.task_id IS NULL OR e.task_id IN (SELECT id FROM agent_tasks WHERE mode=$2)) ORDER BY e.id DESC LIMIT 60")
        .bind(task)
        .bind(&app.config.mode)
        .fetch_all(&app.pool)
        .await?)
}

/// Read-only analytics rows for the intelligence grid. No mutation is reachable from here.
pub async fn analytics(app: &App) -> Result<Value> {
    let rows = sqlx::query_scalar::<_, Value>(r#"
        SELECT jsonb_build_object(
          'id', o.id, 'created_at', o.created_at, 'updated_at', o.updated_at,
          'item', COALESCE(o.intent->'intent'->'contract'->>'item', o.intent->'intent'->'scenario'->>'item', 'Legacy invoice artifact'),
          'seller', COALESCE(o.intent->'intent'->'contract'->>'seller', o.intent->'intent'->'scenario'->>'seller', 'Legacy merchant'),
          'amount_minor', (o.intent->'intent'->>'amount_minor')::bigint,
          'currency', o.intent->'intent'->>'currency',
          'state', o.state, 'mode', o.mode,
          'provider_confirmed', o.mode = 'sandbox' AND o.state IN ('approval_pending','authorized','quarantined','buyer_review','captured','voided'),
          'paypal_order', o.paypal_order, 'authorization_id', o.authorization_id,
          'structural_passed', (o.verification->>'passed')::boolean,
          'risk', o.analysis->>'risk',
          'injection_detected', COALESCE((o.analysis->>'injection_detected')::boolean, false),
          'recommended_action', o.analysis->>'recommended_action',
          'review_decision', o.review_decision, 'risk_override', o.risk_override,
          'task_id', (SELECT t.id FROM agent_tasks t WHERE t.order_id = o.id),
          'kind', CASE WHEN o.intent->'intent' ? 'contract' THEN 'contract' WHEN o.intent->'intent' ? 'scenario' THEN 'catalog' ELSE 'legacy' END)
        FROM orders o WHERE o.buyer='demo-buyer' AND o.mode=$1 ORDER BY o.created_at DESC LIMIT 500"#)
        .bind(&app.config.mode)
        .fetch_all(&app.pool)
        .await?;
    let amount = |r: &Value| r["amount_minor"].as_u64().unwrap_or(0);
    let held_states = [
        "authorized",
        "quarantined",
        "buyer_review",
        "capture_pending",
        "void_pending",
    ];
    let state = |r: &Value| r["state"].as_str().unwrap_or("").to_string();
    let authorized_minor: u64 = rows
        .iter()
        .filter(|r| r["authorization_id"].is_string())
        .map(amount)
        .sum();
    let captured_minor: u64 = rows
        .iter()
        .filter(|r| state(r) == "captured")
        .map(amount)
        .sum();
    let held_minor: u64 = rows
        .iter()
        .filter(|r| held_states.contains(&state(r).as_str()))
        .map(amount)
        .sum();
    let summary = json!({
        "authorized_minor": authorized_minor,
        "captured_minor": captured_minor,
        "held_minor": held_minor,
        "voided_count": rows.iter().filter(|r| state(r) == "voided").count(),
        "blocked_count": rows.iter().filter(|r| r["injection_detected"] == true && state(r) != "captured").count(),
        "flagged_count": rows.iter().filter(|r| r["injection_detected"] == true).count(),
        "reconciliation_count": rows.iter().filter(|r| state(r) == "reconciliation_required").count(),
        "orders": rows.len(),
        "mode": app.config.mode,
        "simulated": app.config.mode == "replay",
    });
    Ok(json!({"rows": rows, "summary": summary}))
}
