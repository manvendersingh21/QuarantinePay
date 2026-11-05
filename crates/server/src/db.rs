use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Order {
    pub id: Uuid,
    pub buyer: String,
    pub intent: Value,
    pub state: String,
    pub mode: String,
    pub paypal_order: Option<String>,
    pub authorization_id: Option<String>,
    pub approval_url: Option<String>,
    pub deadline: Option<chrono::DateTime<chrono::Utc>>,
    pub operation: Option<String>,
    pub request_id: Option<Uuid>,
    pub artifact: Option<Value>,
    pub verification: Option<Value>,
    pub provider_result: Option<Value>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
pub async fn event(pool: &PgPool, id: Option<Uuid>, kind: &str, detail: Value) -> Result<()> {
    sqlx::query("INSERT INTO events(order_id,buyer,kind,detail) VALUES($1,'demo-buyer',$2,$3)")
        .bind(id)
        .bind(kind)
        .bind(detail)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn event_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Option<Uuid>,
    kind: &str,
    detail: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO events(order_id,buyer,kind,detail) VALUES($1,'demo-buyer',$2,$3)")
        .bind(id)
        .bind(kind)
        .bind(detail)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
