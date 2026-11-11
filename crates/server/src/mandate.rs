//! Buyer-signed spending mandates. A mandate is the only way an agent purchase can skip the
//! per-purchase approval click, and it is enforced deterministically before any payment effect.
use crate::{App, db};
use anyhow::{Result, ensure};
use quarantinepay_core::contract::{PurchaseContract, SignedMandate, SpendingMandate, sellers};
use serde_json::{Value, json};
use uuid::Uuid;

pub struct MandateTerms {
    pub max_per_purchase_minor: u64,
    pub max_total_minor: u64,
    pub hours: i64,
    pub auto_capture_clean: bool,
}

/// States that consume budget: anything authorized or on its way to settlement.
const COMMITTED_STATES: &str = "('create_pending','approval_pending','authorize_pending','authorized','quarantined','buyer_review','capture_pending','captured','reconciliation_required')";

pub async fn create(app: &App, terms: MandateTerms) -> Result<Value> {
    ensure!(
        (1..=168).contains(&terms.hours),
        "mandates last between 1 hour and 7 days"
    );
    let open: i64 = sqlx::query_scalar("SELECT count(*) FROM mandates WHERE buyer='demo-buyer' AND mode=$1 AND state IN ('pending_vault','active')")
        .bind(&app.config.mode)
        .fetch_one(&app.pool)
        .await?;
    ensure!(
        open == 0,
        "revoke the current mandate before signing a new one"
    );
    let now = chrono::Utc::now().timestamp();
    let id = Uuid::new_v4();
    let mandate = SpendingMandate {
        version: 1,
        id: id.to_string(),
        buyer: "demo-buyer".into(),
        currency: "USD".into(),
        max_per_purchase_minor: terms.max_per_purchase_minor,
        max_total_minor: terms.max_total_minor,
        allowed_kinds: vec!["research_report".into(), "physical_product".into()],
        allowed_sellers: sellers()
            .iter()
            .map(|s| s.id.to_string())
            .chain(
                quarantinepay_core::product::resellers()
                    .iter()
                    .map(|r| r.id.to_string()),
            )
            .chain(std::iter::once(
                quarantinepay_core::product::SHOPIFY_SELLER_ID.to_string(),
            ))
            .collect(),
        min_competitors: 3,
        min_sources: 5,
        auto_capture_clean: terms.auto_capture_clean,
        created_at: now,
        expires_at: now + terms.hours * 3600,
    };
    mandate.validate(now)?;
    let digest = mandate.digest();
    let signed = SignedMandate::sign(mandate, &app.config.key)?;
    let request_id = Uuid::new_v4();
    let setup = app
        .paypal
        .create_vault_setup(&app.config.base, &request_id.to_string(), &id.to_string())
        .await?;
    let approval = setup["links"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|l| l["rel"] == "payer-action" || l["rel"] == "approve")
        })
        .and_then(|l| l["href"].as_str())
        .ok_or_else(|| anyhow::anyhow!("PayPal did not return an approval link"))?;
    let setup_order = setup["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing PayPal order"))?;
    sqlx::query("INSERT INTO mandates(id,buyer,mode,signed,digest,state,setup_order,setup_request_id,approval_url) VALUES($1,'demo-buyer',$2,$3,$4,'pending_vault',$5,$6,$7)")
        .bind(id)
        .bind(&app.config.mode)
        .bind(serde_json::to_value(&signed)?)
        .bind(&digest)
        .bind(setup_order)
        .bind(request_id)
        .bind(approval)
        .execute(&app.pool)
        .await?;
    db::event(
        &app.pool,
        None,
        "mandate_created",
        json!({"mandate_id":id,"digest":digest,"max_per_purchase_minor":terms.max_per_purchase_minor,"max_total_minor":terms.max_total_minor,"auto_capture_clean":terms.auto_capture_clean,"expires_at":signed.mandate.expires_at}),
    )
    .await?;
    Ok(json!({"id":id,"state":"pending_vault","approval_url":approval,"digest":digest}))
}

/// After the buyer approved at PayPal: save the vault reference and void the verification hold.
pub async fn activate(app: &App, id: Uuid) -> Result<()> {
    let row: Option<(String, Option<String>, Uuid)> = sqlx::query_as("SELECT state,setup_order,setup_request_id FROM mandates WHERE id=$1 AND buyer='demo-buyer' AND mode=$2")
        .bind(id)
        .bind(&app.config.mode)
        .fetch_optional(&app.pool)
        .await?;
    let Some((state, Some(order), request_id)) = row else {
        anyhow::bail!("mandate not found");
    };
    ensure!(
        state == "pending_vault",
        "mandate is not awaiting PayPal approval"
    );
    let (vault_id, verification) = app
        .paypal
        .complete_vault_setup(&order, &format!("{request_id}-authorize"))
        .await?;
    let updated = sqlx::query("UPDATE mandates SET state='active',vault_id=$2,verification_authorization=$3,updated_at=now() WHERE id=$1 AND state='pending_vault'")
        .bind(id)
        .bind(&vault_id)
        .bind(&verification)
        .execute(&app.pool)
        .await?;
    ensure!(updated.rows_affected() == 1, "mandate changed concurrently");
    db::event(
        &app.pool,
        None,
        "mandate_activated",
        json!({"mandate_id":id,"vault":"saved","paypal_verification_authorization":verification}),
    )
    .await?;
    // The USD 1.00 verification hold is not a purchase: release it immediately.
    let voided = app
        .paypal
        .void_authorization(&verification, &format!("{request_id}-void"))
        .await;
    db::event(
        &app.pool,
        None,
        "mandate_verification_voided",
        match &voided {
            Ok(v) => json!({"mandate_id":id,"authorization":verification,"status":v["status"],"provider_confirmed":app.config.mode=="sandbox"}),
            Err(e) => json!({"mandate_id":id,"authorization":verification,"status":"VOID_FAILED","error":e.to_string(),"reconciliation_required":true}),
        },
    )
    .await?;
    Ok(())
}

pub async fn revoke(app: &App, id: Uuid) -> Result<()> {
    let r = sqlx::query("UPDATE mandates SET state='revoked',revoked_at=now(),updated_at=now() WHERE id=$1 AND buyer='demo-buyer' AND mode=$2 AND state IN ('pending_vault','active')")
        .bind(id)
        .bind(&app.config.mode)
        .execute(&app.pool)
        .await?;
    ensure!(r.rows_affected() == 1, "mandate is not active");
    db::event(&app.pool, None, "mandate_revoked", json!({"mandate_id":id})).await?;
    Ok(())
}

fn committed_sql() -> String {
    format!(
        "COALESCE((SELECT sum((o.intent->'intent'->>'amount_minor')::bigint)::bigint FROM orders o WHERE o.mandate_id=m.id AND o.state IN {COMMITTED_STATES}),0)::bigint"
    )
}

pub async fn list(app: &App) -> Result<Vec<Value>> {
    let rows = sqlx::query_scalar::<_, Value>(&format!(
        "SELECT jsonb_build_object('id',m.id,'state',CASE WHEN m.state='active' AND (m.signed->'mandate'->>'expires_at')::bigint <= extract(epoch FROM now()) THEN 'expired' ELSE m.state END,'digest',m.digest,'approval_url',CASE WHEN m.state='pending_vault' THEN m.approval_url END,'mandate',m.signed->'mandate','committed_minor',{c},'remaining_minor',GREATEST((m.signed->'mandate'->>'max_total_minor')::bigint - {c},0),'purchases',(SELECT count(*) FROM orders o WHERE o.mandate_id=m.id),'vault_saved',m.vault_id IS NOT NULL,'created_at',m.created_at,'revoked_at',m.revoked_at) FROM mandates m WHERE m.buyer='demo-buyer' AND m.mode=$1 ORDER BY m.created_at DESC LIMIT 5",
        c = committed_sql()
    ))
    .bind(&app.config.mode)
    .fetch_all(&app.pool)
    .await?;
    Ok(rows)
}

pub struct Decision {
    pub approved: bool,
    pub record: Value,
}
/// Deterministic mandate check under a row lock, so concurrent agent tasks cannot overspend.
pub async fn check(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    app: &App,
    id: Uuid,
    contract: &PurchaseContract,
) -> Result<Decision> {
    let row: Option<(Value, String, Option<String>)> = sqlx::query_as(
        "SELECT signed,state,vault_id FROM mandates WHERE id=$1 AND buyer='demo-buyer' AND mode=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(&app.config.mode)
    .fetch_optional(&mut **tx)
    .await?;
    let now = chrono::Utc::now().timestamp();
    let mut reasons = Vec::new();
    let mut committed = 0u64;
    let mut max_total = 0u64;
    match row {
        None => reasons.push("mandate not found".to_string()),
        Some((signed, state, vault)) => {
            let signed: SignedMandate = serde_json::from_value(signed)?;
            if let Err(e) = signed.verify(&app.config.key.verifying_key(), "demo-buyer", now) {
                reasons.push(format!("mandate signature or validity rejected: {e}"));
            }
            if state != "active" {
                reasons.push(format!("mandate is {state}"));
            }
            if vault.is_none() {
                reasons.push("no saved PayPal wallet".into());
            }
            committed = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT {} FROM mandates m WHERE m.id=$1",
                committed_sql()
            ))
            .bind(id)
            .fetch_one(&mut **tx)
            .await? as u64;
            max_total = signed.mandate.max_total_minor;
            reasons.extend(signed.mandate.permits(contract, committed, now));
        }
    }
    let approved = reasons.is_empty();
    let checked = vec![
        "Mandate signature valid and unexpired".to_string(),
        "Seller and purchase type allowed".to_string(),
        format!(
            "Price USD {:.2} within the per-purchase cap",
            contract.amount_minor as f64 / 100.0
        ),
        "Cumulative budget not exceeded".to_string(),
        if contract.product.is_some() {
            format!(
                "Exact real listing \"{}\" and tracked delivery to the buyer",
                contract.item
            )
        } else {
            "Scope meets the mandate's minimum (competitors, sources)".to_string()
        },
        "PayPal wallet saved for merchant-initiated payment".to_string(),
    ];
    Ok(Decision {
        approved,
        record: json!({
            "decision": if approved { "auto_approved" } else { "outside_mandate" },
            "reasons": reasons,
            "checked": if approved { checked } else { Vec::new() },
            "committed_minor": committed,
            "remaining_minor": max_total.saturating_sub(committed),
        }),
    })
}

pub async fn vault_for_order(app: &App, mandate: Uuid) -> Result<String> {
    let row: Option<(Value, String, Option<String>)> = sqlx::query_as(
        "SELECT signed,state,vault_id FROM mandates WHERE id=$1 AND buyer='demo-buyer' AND mode=$2",
    )
    .bind(mandate)
    .bind(&app.config.mode)
    .fetch_optional(&app.pool)
    .await?;
    let (signed, state, vault) = row.ok_or_else(|| anyhow::anyhow!("mandate not found"))?;
    let signed: SignedMandate = serde_json::from_value(signed)?;
    signed.verify(
        &app.config.key.verifying_key(),
        "demo-buyer",
        chrono::Utc::now().timestamp(),
    )?;
    ensure!(
        state == "active",
        "mandate is {state}; no autonomous payment"
    );
    vault.ok_or_else(|| anyhow::anyhow!("no saved PayPal wallet"))
}
