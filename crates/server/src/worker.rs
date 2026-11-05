use crate::{
    App,
    db::{self, Order},
};
use anyhow::{Result, ensure};
use quarantinepay_core::{SignedIntent, Verdict, policy_digest};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

async fn verify(app: &App, artifact: &Value) -> Result<Verdict> {
    let bytes = serde_json::to_vec(artifact)?;
    ensure!(bytes.len() <= 65536, "artifact too large");
    let mut command = if app.config.verifier_mode == "docker" {
        let mut c = tokio::process::Command::new("docker");
        c.args([
            "run",
            "--rm",
            "-i",
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--memory=64m",
            "--cpus=0.5",
            "--pids-limit=16",
            "--user=65532:65532",
            "quarantinepay-verifier:local",
        ]);
        c
    } else {
        tokio::process::Command::new(&app.config.verifier)
    };
    command
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("verifier stdin unavailable"))?;
    stdin.write_all(&bytes).await?;
    drop(stdin);
    let output =
        tokio::time::timeout(std::time::Duration::from_secs(5), child.wait_with_output()).await??;
    ensure!(
        output.status.success() && output.stdout.len() < 4096,
        "verifier process failed"
    );
    let verdict: Verdict = serde_json::from_slice(&output.stdout)?;
    ensure!(
        verdict.policy_digest == policy_digest(),
        "verifier policy mismatch"
    );
    Ok(verdict)
}
async fn process(app: &App, id: uuid::Uuid) -> Result<()> {
    let mut tx = app.pool.begin().await?;
    // Transaction-scoped advisory lock avoids duplicate provider effects across worker instances.
    let locked: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await?;
    if !locked {
        return Ok(());
    }
    let mut order: Order = sqlx::query_as("SELECT * FROM orders WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    ensure!(order.mode == app.config.mode, "payment mode mismatch");
    let signed: SignedIntent = serde_json::from_value(order.intent.clone())?;
    // Expiry forbids new purchases; a previously authorized payment must still be voidable after expiry.
    signed.verify(
        &app.config.key.verifying_key(),
        &order.buyer,
        chrono::Utc::now()
            .timestamp()
            .min(signed.intent.expires_at - 1),
    )?;
    ensure!(
        signed.intent.merchant_id == app.config.merchant,
        "merchant not allowlisted"
    );
    if matches!(order.state.as_str(), "authorized" | "quarantined") {
        let expired = order.deadline.is_some_and(|t| t <= chrono::Utc::now());
        if !expired && order.artifact.is_none() {
            return Ok(());
        }
        let verdict = if expired {
            json!({"passed":false,"reason":"verification deadline expired"})
        } else {
            match verify(app, order.artifact.as_ref().unwrap()).await {
                Ok(v) => serde_json::to_value(v)?,
                Err(_) => json!({"passed":false,"reason":"verifier failed or timed out"}),
            }
        };
        let capture =
            verdict["passed"] == true && order.deadline.is_some_and(|t| t > chrono::Utc::now());
        order.state = if capture {
            "capture_pending"
        } else {
            "void_pending"
        }
        .into();
        let request_id = uuid::Uuid::new_v4();
        sqlx::query("UPDATE orders SET state=$2,operation=$3,request_id=$4,verification=$5,updated_at=now() WHERE id=$1").bind(id).bind(&order.state).bind(if capture{"capture"}else{"void"}).bind(request_id).bind(&verdict).execute(&mut *tx).await?;
        // Persist the operation before any network effect. A crash resumes with the same request ID.
        db::event_tx(&mut tx, Some(id), "verification_complete", verdict).await?;
        tx.commit().await?;
        return Ok(());
    }
    if !matches!(
        order.state.as_str(),
        "create_pending" | "authorize_pending" | "capture_pending" | "void_pending"
    ) {
        return Ok(());
    }
    let request_id = order
        .request_id
        .ok_or_else(|| anyhow::anyhow!("missing request ID"))?
        .to_string();
    // Never retry beyond PayPal's minimum request-ID retention window without operator reconciliation.
    if order.updated_at < chrono::Utc::now() - chrono::Duration::minutes(5) {
        sqlx::query("UPDATE orders SET state='reconciliation_required' WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        db::event_tx(
            &mut tx,
            Some(id),
            "reconciliation_required",
            json!({"operation":order.operation}),
        )
        .await?;
        tx.commit().await?;
        return Ok(());
    }
    let result = match order.state.as_str() {
        "create_pending" => {
            app.paypal
                .create(&signed, &app.config.base, &request_id)
                .await
        }
        "authorize_pending" => {
            app.paypal
                .authorize(order.paypal_order.as_deref().unwrap_or(""), &request_id)
                .await
        }
        "capture_pending" | "void_pending" => {
            app.paypal
                .resolve(
                    order.authorization_id.as_deref().unwrap_or(""),
                    order.state == "capture_pending",
                    &request_id,
                )
                .await
        }
        _ => unreachable!(),
    };
    match result {
        Ok(value) => {
            match order.state.as_str() {
                "create_pending" => {
                    let provider_id = value["id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("missing PayPal order ID"))?;
                    let approval = value["links"]
                        .as_array()
                        .and_then(|a| {
                            a.iter()
                                .find(|v| v["rel"] == "payer-action" || v["rel"] == "approve")
                        })
                        .and_then(|v| v["href"].as_str())
                        .ok_or_else(|| anyhow::anyhow!("missing approval URL"))?;
                    sqlx::query("UPDATE orders SET state='approval_pending',paypal_order=$2,approval_url=$3,provider_result=$4,updated_at=now() WHERE id=$1").bind(id).bind(provider_id).bind(approval).bind(&value).execute(&mut *tx).await?;
                }
                "authorize_pending" => {
                    let auth = value
                        .pointer("/purchase_units/0/payments/authorizations/0")
                        .ok_or_else(|| anyhow::anyhow!("missing authorization"))?;
                    ensure!(auth["status"] == "CREATED", "authorization not confirmed");
                    let auth_id = auth["id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("missing authorization ID"))?;
                    sqlx::query("UPDATE orders SET state='authorized',authorization_id=$2,deadline=now()+interval '120 seconds',provider_result=$3,updated_at=now() WHERE id=$1").bind(id).bind(auth_id).bind(&value).execute(&mut *tx).await?;
                }
                _ => {
                    let capture = order.state == "capture_pending";
                    ensure!(
                        value["status"] == if capture { "COMPLETED" } else { "VOIDED" },
                        "provider settlement not final"
                    );
                    sqlx::query("UPDATE orders SET state=$2,provider_result=$3,updated_at=now() WHERE id=$1").bind(id).bind(if capture{"captured"}else{"voided"}).bind(&value).execute(&mut *tx).await?;
                }
            }
            db::event_tx(
                &mut tx,
                Some(id),
                "provider_confirmed",
                json!({"operation":order.operation,"mode":app.config.mode}),
            )
            .await?;
            tx.commit().await?;
        }
        Err(error) => {
            tx.commit().await?;
            tracing::warn!(%id, operation=?order.operation,error=%error,"provider outcome pending; retrying with stable request ID");
        }
    }
    Ok(())
}
pub async fn run(app: App) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        let ids=sqlx::query_scalar::<_,uuid::Uuid>("SELECT id FROM orders WHERE mode=$1 AND state IN ('create_pending','authorize_pending','authorized','quarantined','capture_pending','void_pending') ORDER BY updated_at LIMIT 100").bind(&app.config.mode).fetch_all(&app.pool).await;
        match ids {
            Ok(ids) => {
                for id in ids {
                    if let Err(e) = process(&app, id).await {
                        tracing::warn!(%id,error=%e,"worker failed; payment remains unresolved");
                    }
                }
            }
            Err(e) => tracing::error!(error=%e,"worker database failure"),
        }
    }
}
