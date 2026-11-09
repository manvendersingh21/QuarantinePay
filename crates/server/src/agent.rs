//! Buyer-agent tasks: plan → discover → negotiate → propose. A task can only *propose*; the
//! buyer's approval of the exact contract digest is what creates a signed intent and an order.
use crate::{App, db};
use anyhow::Result;
use quarantinepay_core::PurchaseContract;
use quarantinepay_core::contract::{
    Negotiation, Objective, SignedMandate, SpendingMandate, compile_contract_under, negotiate,
    sellers,
};
use quarantinepay_core::product::{ProductTerms, compile_product_contract, negotiate_product};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn create(
    app: &App,
    request: &str,
    mandate: Option<Uuid>,
    external_discovery: bool,
) -> Result<Uuid> {
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
    sqlx::query("INSERT INTO agent_tasks(id,buyer,mode,request,state,mandate_id,external_discovery) VALUES($1,'demo-buyer',$2,$3,'planning',$4,$5)")
        .bind(id)
        .bind(&app.config.mode)
        .bind(&request)
        .bind(mandate)
        .bind(external_discovery)
        .execute(&app.pool)
        .await?;
    db::task_event(
        &app.pool,
        id,
        "agent_task_created",
        json!({"task_id":id,"request":request,"mandate_id":mandate,"mode":if mandate.is_some() {"autonomous"} else {"manual"}}),
    )
    .await?;
    let worker = app.clone();
    tokio::spawn(async move {
        if let Err(e) = run(&worker, id, &request, mandate).await {
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

/// What the agent is buying: a report from seller agents, or a discovered real listing.
enum Purchase {
    Report,
    Product(Box<ProductTerms>),
}
fn compile(
    purchase: &Purchase,
    objective: &Objective,
    negotiation: &Negotiation,
    mandate: Option<&SpendingMandate>,
) -> Result<PurchaseContract> {
    match purchase {
        Purchase::Report => compile_contract_under(objective, negotiation, mandate),
        Purchase::Product(p) => compile_product_contract(objective, p, negotiation, mandate),
    }
}

async fn run(app: &App, id: Uuid, request: &str, mandate_id: Option<Uuid>) -> Result<()> {
    let (mut objective, planner) = crate::analyst::plan(app, request).await;
    // Under a mandate the agent may never negotiate above the signed per-purchase cap.
    let mandate = match mandate_id {
        Some(mid) => {
            let signed: Option<serde_json::Value> = sqlx::query_scalar(
                "SELECT signed FROM mandates WHERE id=$1 AND buyer='demo-buyer' AND mode=$2",
            )
            .bind(mid)
            .bind(&app.config.mode)
            .fetch_optional(&app.pool)
            .await?;
            let signed: SignedMandate = serde_json::from_value(
                signed.ok_or_else(|| anyhow::anyhow!("mandate not found"))?,
            )?;
            objective.budget_minor = objective
                .budget_minor
                .min(signed.mandate.max_per_purchase_minor);
            Some(signed.mandate)
        }
        None => None,
    };
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
        json!({"task_id":id,"planner":planner["source"],"purchase_type":planner["purchase_type"],"objective":objective}),
    )
    .await?;
    let (purchase, negotiation) = if planner["purchase_type"] == "physical_product" {
        let external: bool =
            sqlx::query_scalar("SELECT external_discovery FROM agent_tasks WHERE id=$1")
                .bind(id)
                .fetch_one(&app.pool)
                .await?;
        let query = planner["product_query"].as_str().unwrap_or("").to_string();
        let found = crate::discovery::search(app, &query, objective.budget_minor, external).await?;
        sqlx::query("UPDATE agent_tasks SET discovery=$2 WHERE id=$1")
            .bind(id)
            .bind(&found.record)
            .execute(&app.pool)
            .await?;
        db::task_event(
            &app.pool,
            id,
            "agent_discovery_search",
            json!({"task_id":id,"source":found.record["source"],"query":query,"results":found.record["results"].as_array().map_or(0,|r| r.len()),"external":external}),
        )
        .await?;
        let Some(product) = found.selected else {
            let reason = found.record["note"]
                .as_str()
                .unwrap_or("no eligible listing")
                .to_string();
            sqlx::query("UPDATE agent_tasks SET state='no_deal',error=$2,updated_at=now() WHERE id=$1 AND state='negotiating'")
                .bind(id)
                .bind(&reason)
                .execute(&app.pool)
                .await?;
            db::task_event(
                &app.pool,
                id,
                "agent_no_deal",
                json!({"task_id":id,"reason":reason}),
            )
            .await?;
            return Ok(());
        };
        db::task_event(
            &app.pool,
            id,
            "agent_listing_selected",
            json!({"task_id":id,"title":product.title,"retailer":product.retailer,"listed_price_minor":product.listed_price_minor,"source":"Channel3"}),
        )
        .await?;
        let negotiation = negotiate_product(&objective, &product);
        (Purchase::Product(Box::new(product)), negotiation)
    } else {
        db::task_event(
            &app.pool,
            id,
            "agent_discovery",
            json!({"task_id":id,"sellers":sellers().iter().map(|s| json!({"id":s.id,"name":s.name,"list_minor":s.list_minor})).collect::<Vec<_>>(),"note":"Fictional seller agents"}),
        )
        .await?;
        (Purchase::Report, negotiate(&objective))
    };
    db::task_event(
        &app.pool,
        id,
        "agent_negotiation",
        json!({"task_id":id,"messages":negotiation.transcript.len(),"offers":negotiation.offers,"rationale":negotiation.rationale}),
    )
    .await?;
    if let Some(m) = &mandate
        && let Ok(contract) = compile(&purchase, &objective, &negotiation, Some(m))
    {
        let (approved, autonomy, order) =
            crate::autonomous_checkout(app, id, mandate_id.unwrap_or_default(), &contract).await?;
        db::task_event(
            &app.pool,
            id,
            "mandate_policy_check",
            json!({"task_id":id,"decision":autonomy["decision"],"reasons":autonomy["reasons"],"remaining_minor":autonomy["remaining_minor"]}),
        )
        .await?;
        if approved {
            sqlx::query("UPDATE agent_tasks SET state='purchasing',negotiation=$2,proposal=$3,proposal_digest=$4,order_id=$5,autonomy=$6,updated_at=now() WHERE id=$1 AND state='negotiating'")
                .bind(id)
                .bind(serde_json::to_value(&negotiation)?)
                .bind(serde_json::to_value(&contract)?)
                .bind(contract.digest())
                .bind(order)
                .bind(&autonomy)
                .execute(&app.pool)
                .await?;
            return Ok(());
        }
        // Outside the mandate: fall back to a normal proposal that needs the buyer.
        sqlx::query("UPDATE agent_tasks SET autonomy=$2 WHERE id=$1")
            .bind(id)
            .bind(&autonomy)
            .execute(&app.pool)
            .await?;
    }
    match compile(&purchase, &objective, &negotiation, None) {
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

const TASK_JSON: &str = "SELECT jsonb_build_object('id',t.id,'request',t.request,'state',t.state,'planner',t.planner,'objective',t.objective,'negotiation',t.negotiation,'proposal',t.proposal,'proposal_digest',t.proposal_digest,'order_id',t.order_id,'error',t.error,'mandate_id',t.mandate_id,'autonomy',t.autonomy,'discovery',t.discovery,'external_discovery',t.external_discovery,'created_at',t.created_at,'updated_at',t.updated_at,'mode',t.mode,'order',(SELECT to_jsonb(o) FROM orders o WHERE o.id=t.order_id AND o.mode=t.mode)) FROM agent_tasks t";

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
    Ok(sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object('id',t.id,'request',t.request,'state',COALESCE((SELECT o.state FROM orders o WHERE o.id=t.order_id AND o.state IN ('captured','voided','declined','reconciliation_required')),t.state),'task_state',t.state,'created_at',t.created_at,'amount_minor',(t.proposal->>'amount_minor')::bigint,'seller',t.proposal->>'seller','order_state',(SELECT state FROM orders o WHERE o.id=t.order_id),'mandate_id',t.mandate_id) FROM agent_tasks t WHERE t.buyer='demo-buyer' AND t.mode=$1 ORDER BY t.created_at DESC LIMIT 20")
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
          'autonomous', o.mandate_id IS NOT NULL,
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
