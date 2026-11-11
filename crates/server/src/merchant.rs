//! Order lifecycle with a real Shopify merchant. Runs beside the payment worker and never touches
//! PayPal: it places the merchant order once PayPal has authorized, turns the merchant's
//! fulfilment (or cancellation) into delivery evidence, and mirrors the final payment outcome
//! back onto the merchant's order.
use crate::{App, db};
use anyhow::Result;
use quarantinepay_core::SignedIntent;
use quarantinepay_core::product::SHOPIFY_SOURCE;
use serde_json::{Value, json};
use uuid::Uuid;

const SHOPIFY_ORDERS: &str = "intent->'intent'->'contract'->'product'->>'source'";

pub async fn sync(app: &App) -> Result<()> {
    if !crate::shopify::configured() {
        return Ok(());
    }
    place(app).await?;
    observe(app).await?;
    mirror(app).await?;
    Ok(())
}

async fn note_error(app: &App, id: Uuid, previous: Option<String>, error: String) -> Result<()> {
    if previous.as_deref() != Some(error.as_str()) {
        sqlx::query("UPDATE orders SET shopify_error=$2 WHERE id=$1")
            .bind(id)
            .bind(&error)
            .execute(&app.pool)
            .await?;
        db::event(
            &app.pool,
            Some(id),
            "merchant_error",
            json!({"merchant":"Shopify","error":error,"payment_effect":"none; the authorization is voided if the merchant does not fulfil before the deadline"}),
        )
        .await?;
    }
    Ok(())
}

/// PayPal authorized → place the order with the merchant.
async fn place(app: &App) -> Result<()> {
    let rows: Vec<(Uuid, Value, Option<String>, Option<String>)> = sqlx::query_as(&format!(
        "SELECT id,intent,authorization_id,shopify_error FROM orders WHERE mode=$1 AND state='authorized' AND shopify_order IS NULL AND deadline>now() AND {SHOPIFY_ORDERS}=$2 ORDER BY updated_at LIMIT 10"
    ))
    .bind(&app.config.mode)
    .bind(SHOPIFY_SOURCE)
    .fetch_all(&app.pool)
    .await?;
    for (id, intent, auth, previous) in rows {
        let signed: SignedIntent = serde_json::from_value(intent)?;
        let Some(contract) = signed.intent.contract else {
            continue;
        };
        let auth = auth.unwrap_or_default();
        match crate::shopify::place_order(id, &contract, &auth).await {
            Ok((gid, name)) => {
                let r = sqlx::query("UPDATE orders SET shopify_order=$2,shopify_name=$3,shopify_error=NULL,shopify_checked_at=now() WHERE id=$1 AND shopify_order IS NULL")
                    .bind(id)
                    .bind(&gid)
                    .bind(&name)
                    .execute(&app.pool)
                    .await?;
                if r.rows_affected() == 1 {
                    db::event(
                        &app.pool,
                        Some(id),
                        "merchant_order_placed",
                        json!({"merchant":contract.seller,"shop":contract.product.as_ref().map(|p| p.retailer.clone()),"shopify_order":gid,"name":name,"paypal_authorization":auth,"financial_status":"AUTHORIZED","note":"Placed only after PayPal authorized; funds stay held until the merchant's fulfilment is verified"}),
                    )
                    .await?;
                }
            }
            Err(e) => note_error(app, id, previous, e.to_string()).await?,
        }
    }
    Ok(())
}

/// Merchant fulfilled or cancelled → that record becomes the order's delivery evidence.
async fn observe(app: &App) -> Result<()> {
    let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(&format!(
        "SELECT id,shopify_order,shopify_error FROM orders WHERE mode=$1 AND state='authorized' AND shopify_order IS NOT NULL AND deadline>now() AND {SHOPIFY_ORDERS}=$2 AND (shopify_checked_at IS NULL OR shopify_checked_at < now() - interval '4 seconds') ORDER BY shopify_checked_at NULLS FIRST LIMIT 10"
    ))
    .bind(&app.config.mode)
    .bind(SHOPIFY_SOURCE)
    .fetch_all(&app.pool)
    .await?;
    for (id, gid, previous) in rows {
        sqlx::query("UPDATE orders SET shopify_checked_at=now() WHERE id=$1")
            .bind(id)
            .execute(&app.pool)
            .await?;
        let snapshot = match crate::shopify::snapshot(&gid).await {
            Ok(s) => s,
            Err(e) => {
                note_error(app, id, previous, e.to_string()).await?;
                continue;
            }
        };
        let o = &snapshot["order"];
        let fulfilled = o["fulfillment_status"] == "FULFILLED";
        let cancelled = !o["cancelled_at"].is_null();
        if !fulfilled && !cancelled {
            continue;
        }
        let r = sqlx::query("UPDATE orders SET artifact=$2,state='quarantined',shopify_error=NULL,updated_at=now() WHERE id=$1 AND state='authorized' AND deadline>now()")
            .bind(id)
            .bind(&snapshot)
            .execute(&app.pool)
            .await?;
        if r.rows_affected() == 1 {
            let tracking = o["fulfillments"].as_array().and_then(|f| {
                f.iter()
                    .find_map(|f| f["tracking"].as_array()?.first().cloned())
            });
            db::event(
                &app.pool,
                Some(id),
                if cancelled { "merchant_cancelled" } else { "merchant_fulfilment_observed" },
                json!({"merchant":"Shopify","shopify_order":o["name"],"fulfillment_status":o["fulfillment_status"],"cancelled_at":o["cancelled_at"],"tracking":tracking,"evidence":"fetched by QuarantinePay from the Shopify Admin API"}),
            )
            .await?;
        }
    }
    Ok(())
}

/// PayPal captured or voided → mirror it onto the merchant's order (books only, never payment).
async fn mirror(app: &App) -> Result<()> {
    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id,shopify_order,state FROM orders WHERE mode=$1 AND state IN ('captured','voided') AND shopify_order IS NOT NULL AND shopify_settlement IS NULL ORDER BY updated_at LIMIT 10",
    )
    .bind(&app.config.mode)
    .fetch_all(&app.pool)
    .await?;
    for (id, gid, state) in rows {
        let captured = state == "captured";
        let result = match crate::shopify::settle(&gid, captured).await {
            Ok(v) => v,
            Err(e) => json!({"action":"failed","error":e.to_string()}),
        };
        sqlx::query(
            "UPDATE orders SET shopify_settlement=$2 WHERE id=$1 AND shopify_settlement IS NULL",
        )
        .bind(id)
        .bind(&result)
        .execute(&app.pool)
        .await?;
        db::event(
            &app.pool,
            Some(id),
            "merchant_order_updated",
            json!({"merchant":"Shopify","shopify_order":gid,"paypal_outcome":state,"result":result,"payment_authority":false}),
        )
        .await?;
    }
    Ok(())
}
