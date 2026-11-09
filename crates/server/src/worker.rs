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
    let intent_expired = signed.intent.expires_at <= chrono::Utc::now().timestamp();
    if intent_expired
        && matches!(
            order.state.as_str(),
            "create_pending" | "authorize_pending" | "capture_pending"
        )
    {
        // An expired intent cannot authorize another monetary effect. A previous attempt
        // may have had an unknown outcome, so preserve it for provider reconciliation.
        sqlx::query("UPDATE orders SET state='reconciliation_required' WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        db::event_tx(
            &mut tx,
            Some(id),
            "intent_expired",
            json!({"operation":order.operation,"mode":app.config.mode}),
        )
        .await?;
        tx.commit().await?;
        return Ok(());
    }
    if matches!(
        order.state.as_str(),
        "authorized" | "quarantined" | "buyer_review"
    ) {
        let expired = intent_expired || order.deadline.is_some_and(|t| t <= chrono::Utc::now());
        if !expired && (order.artifact.is_none() || order.state == "buyer_review") {
            return Ok(());
        }
        let verdict = if expired {
            json!({"passed":false,"reason":"verification deadline expired"})
        } else {
            match if let Some(terms) = &signed.intent.scenario {
                Ok(quarantinepay_core::verify_scenario(
                    &terms.id,
                    order.artifact.as_ref().unwrap(),
                ))
            } else {
                verify(app, order.artifact.as_ref().unwrap()).await
            } {
                Ok(v) => serde_json::to_value(v)?,
                Err(_) => json!({"passed":false,"reason":"verifier failed or timed out"}),
            }
        };
        let capture =
            verdict["passed"] == true && order.deadline.is_some_and(|t| t > chrono::Utc::now());
        if capture
            && signed
                .intent
                .scenario
                .as_ref()
                .is_some_and(|s| s.review_required)
        {
            sqlx::query("UPDATE orders SET state='buyer_review',verification=$2,updated_at=now() WHERE id=$1").bind(id).bind(&verdict).execute(&mut *tx).await?;
            db::event_tx(&mut tx, Some(id), "verification_complete", verdict).await?;
            db::event_tx(
                &mut tx,
                Some(id),
                "buyer_review_required",
                json!({"reason":"Acceptance requires an explicit buyer decision"}),
            )
            .await?;
            tx.commit().await?;
            return Ok(());
        }
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
    if app.config.mode == "replay"
        && order.state == "authorize_pending"
        && order.simulation_outcome == "decline"
    {
        sqlx::query(
            "UPDATE orders SET state='declined',provider_result=$2,updated_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(json!({"simulated":true,"status":"DECLINED"}))
        .execute(&mut *tx)
        .await?;
        db::event_tx(
            &mut tx,
            Some(id),
            "simulated_decline",
            json!({"reason":"Selected simulated authorization decline"}),
        )
        .await?;
        tx.commit().await?;
        return Ok(());
    }
    if app.config.mode == "replay"
        && order.state == "capture_pending"
        && order.simulation_outcome == "ambiguous"
    {
        sqlx::query("UPDATE orders SET state='reconciliation_required',provider_result=$2,updated_at=now() WHERE id=$1").bind(id).bind(json!({"simulated":true,"status":"UNKNOWN"})).execute(&mut *tx).await?;
        db::event_tx(
            &mut tx,
            Some(id),
            "simulated_ambiguous_result",
            json!({"reason":"Simulated capture outcome is unknown; no success claimed"}),
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
                    sqlx::query("UPDATE orders SET state='authorized',authorization_id=$2,deadline=now()+($4 * interval '1 second'),provider_result=$3,updated_at=now() WHERE id=$1").bind(id).bind(auth_id).bind(&value).bind(signed.intent.scenario.as_ref().map_or(120_i64, |s| s.deadline_seconds as i64)).execute(&mut *tx).await?;
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
                if app.config.mode == "replay" { "simulated_payment_result" } else { "provider_confirmed" },
                json!({"operation":order.operation,"mode":app.config.mode,"simulated":app.config.mode == "replay"}),
            )
            .await?;
            tx.commit().await?;
        }
        Err(error) => {
            if let Some(failure) = error
                .downcast_ref::<crate::paypal::PayPalFailure>()
                .filter(|f| f.requires_credentials())
            {
                // A definitive permission refusal needs configuration, not repeated payment requests.
                // Preserve the authorization and original operation/request ID for reconciliation.
                sqlx::query("UPDATE orders SET state='reconciliation_required' WHERE id=$1")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                db::event_tx(&mut tx,Some(id),"provider_permission_required",json!({"operation":order.operation,"mode":app.config.mode,"status":failure.status,"code":failure.name,"issues":failure.issues,"debug_id":failure.debug_id})).await?;
                tx.commit().await?;
                tracing::warn!(%id,error=%error,"PayPal credentials require attention; automatic retries stopped");
            } else {
                tx.commit().await?;
                tracing::warn!(%id, operation=?order.operation,error=%error,"provider outcome pending; retrying with stable request ID");
            }
        }
    }
    Ok(())
}
pub async fn run(app: App) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        let ids=sqlx::query_scalar::<_,uuid::Uuid>("SELECT id FROM orders WHERE mode=$1 AND state IN ('create_pending','authorize_pending','authorized','quarantined','buyer_review','capture_pending','void_pending') ORDER BY updated_at LIMIT 100").bind(&app.config.mode).fetch_all(&app.pool).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, paypal::mock_resolution_failure};
    use sqlx::PgPool;
    use std::{sync::Arc, sync::atomic::Ordering};
    #[tokio::test]
    #[ignore = "requires PostgreSQL; isolated schema and local mock provider only"]
    async fn permission_denied_stops_retries_and_preserves_authorization() {
        dotenvy::dotenv().ok();
        let database = std::env::var("DATABASE_URL").unwrap();
        let admin = PgPool::connect(&database).await.unwrap();
        let schema = format!("qp_permission_test_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let mut uri = reqwest::Url::parse(&database).unwrap();
        uri.query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        let pool = PgPool::connect(uri.as_str()).await.unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        let (paypal, attempts, server) = mock_resolution_failure(403).await;
        let config = Config {
            mode: "sandbox".into(),
            inference: crate::inference::InferenceConfig::new(
                "local",
                true,
                "http://127.0.0.1:11434",
                "test-model",
            )
            .unwrap(),
            base: "http://localhost:3000".into(),
            bind: "127.0.0.1:0".into(),
            database: uri.to_string(),
            password: "unused-long-test-password".into(),
            key: ed25519_dalek::SigningKey::from_bytes(&[42; 32]),
            merchant: "MERCHANT12345".into(),
            verifier: "unused".into(),
            verifier_mode: "trusted-process".into(),
        };
        let signed = SignedIntent::sign(
            quarantinepay_core::Intent {
                version: 1,
                id: uuid::Uuid::new_v4(),
                buyer: "demo-buyer".into(),
                sku: "invoice-fixture-v1".into(),
                merchant_id: config.merchant.clone(),
                amount_minor: 100,
                currency: "USD".into(),
                expires_at: chrono::Utc::now().timestamp() + 900,
                policy_digest: policy_digest(),
                scenario: None,
            },
            &config.key,
        )
        .unwrap();
        let id = uuid::Uuid::new_v4();
        let request_id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO orders(id,buyer,intent,state,mode,operation,request_id,authorization_id) VALUES($1,'demo-buyer',$2,'void_pending','sandbox','void',$3,'test-authorization')").bind(id).bind(serde_json::to_value(&signed).unwrap()).bind(request_id).execute(&pool).await.unwrap();
        let app = App {
            config: Arc::new(config),
            pool: pool.clone(),
            client: reqwest::Client::new(),
            paypal,
        };
        process(&app, id).await.unwrap();
        let order: Order = sqlx::query_as("SELECT * FROM orders WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(order.state, "reconciliation_required");
        assert_eq!(
            order.authorization_id.as_deref(),
            Some("test-authorization")
        );
        assert_eq!(order.request_id, Some(request_id));
        assert_eq!(order.operation.as_deref(), Some("void"));
        let detail: Value = sqlx::query_scalar(
            "SELECT detail FROM events WHERE order_id=$1 AND kind='provider_permission_required'",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(detail["status"], 403);
        assert_eq!(detail["debug_id"], "test-debug-403");
        process(&app, id).await.unwrap();
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            1,
            "a permanent permission refusal must not be retried"
        );
        // Valid signatures with expired purchase intents must never start new effects.
        for state in [
            "create_pending",
            "authorize_pending",
            "capture_pending",
            "void_pending",
        ] {
            let mut intent = signed.intent.clone();
            intent.id = uuid::Uuid::new_v4();
            intent.expires_at = chrono::Utc::now().timestamp() - 1;
            let expired = SignedIntent::sign(intent, &app.config.key).unwrap();
            let expired_id = uuid::Uuid::new_v4();
            sqlx::query("INSERT INTO orders(id,buyer,intent,state,mode,operation,request_id,authorization_id) VALUES($1,'demo-buyer',$2,$3,'sandbox',$4,$5,'test-authorization')").bind(expired_id).bind(serde_json::to_value(expired).unwrap()).bind(state).bind(state.trim_end_matches("_pending")).bind(uuid::Uuid::new_v4()).execute(&pool).await.unwrap();
            process(&app, expired_id).await.unwrap();
            let actual: String = sqlx::query_scalar("SELECT state FROM orders WHERE id=$1")
                .bind(expired_id)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(
                actual, "reconciliation_required",
                "expired {state} must stop before a provider effect"
            );
        }
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            2,
            "expired intents must still permit a compensating void"
        );
        server.abort();
        drop(app);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
    }
}
