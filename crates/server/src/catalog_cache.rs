//! Public catalog imports are separate from private user searches.
use crate::{App, config::get};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use sqlx::PgPool;
const PROVIDER_URL: &str = "https://api.trychannel3.com/v1/search";

async fn fetch_public(
    client: &reqwest::Client,
    endpoint: &str,
    key: &str,
    public_seed: &str,
) -> Result<Vec<Value>> {
    let mut header = reqwest::header::HeaderValue::from_str(key)?;
    header.set_sensitive(true);
    let response = client
        .post(endpoint)
        .header("x-api-key", header)
        .json(&json!({"query":public_seed,"limit":8}))
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "Channel3 catalog import returned {}",
        response.status()
    );
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 2 * 1024 * 1024,
            "catalog response exceeds 2 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    let array = value
        .as_array()
        .or_else(|| value["data"].as_array())
        .or_else(|| value["products"].as_array())
        .or_else(|| value["items"].as_array())
        .ok_or_else(|| anyhow::anyhow!("Channel3 returned an unexpected catalog structure"))?;
    Ok(array.iter().take(8).cloned().collect())
}
async fn store(
    pool: &PgPool,
    products: Vec<(String, Value, String)>,
    blocked: &[String],
) -> Result<usize> {
    let count = products.len();
    let mut tx = pool.begin().await?;
    for id in blocked {
        sqlx::query("DELETE FROM catalog_products WHERE product_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    for (id, product, text) in products {
        sqlx::query("INSERT INTO catalog_products(product_id,product,search_text) VALUES($1,$2,$3) ON CONFLICT(product_id) DO UPDATE SET product=$2,search_text=$3,screened_at=now()")
            .bind(id).bind(product).bind(text).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(count)
}
pub async fn search(pool: &PgPool, query: &str) -> Result<Vec<Value>> {
    let terms = query
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|word| {
            word.len() >= 3
                && ![
                    "recommend",
                    "find",
                    "show",
                    "looking",
                    "want",
                    "need",
                    "please",
                    "the",
                    "and",
                    "for",
                    "with",
                    "this",
                    "that",
                    "can",
                    "you",
                    "what",
                    "some",
                    "good",
                ]
                .contains(&word.to_ascii_lowercase().as_str())
        })
        .take(16)
        .collect::<Vec<_>>()
        .join(" OR ");
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('product',product,'source','Channel3 local mirror','screened_at',screened_at,'purchasable_in_demo',false,'flagged',false) FROM catalog_products WHERE to_tsvector('simple',search_text) @@ websearch_to_tsquery('simple',$1) ORDER BY ts_rank(to_tsvector('simple',search_text),websearch_to_tsquery('simple',$1)) DESC,screened_at DESC LIMIT 8")
        .bind(terms).fetch_all(pool).await?)
}
pub async fn context(pool: &PgPool, query: &str) -> Result<Vec<Value>> {
    // Only the title and description that passed screening enter the model context.
    Ok(search(pool,query).await?.into_iter().take(3).map(|row|json!({"title":row["product"]["title"].as_str().unwrap_or("").chars().take(200).collect::<String>(),"description":row["product"]["description"].as_str().unwrap_or("").chars().take(1200).collect::<String>(),"source":"Channel3 local mirror","purchasable_in_demo":false})).collect())
}
pub async fn synchronize(app: &App) -> Result<Value> {
    let key = get("CHANNEL3_API_KEY", "");
    ensure!(
        !key.is_empty(),
        "CHANNEL3_API_KEY is required to import the public catalog"
    );
    // This seed comes only from operator configuration, never a user request body or search.
    let seed = get("QP_CATALOG_SEED_QUERY", "developer tools");
    ensure!(
        !seed.trim().is_empty() && seed.len() <= 200,
        "public catalog seed must be 1–200 characters"
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let products = fetch_public(&client, PROVIDER_URL, &key, &seed).await?;
    let received = products.len();
    let mut safe = Vec::new();
    let mut withheld = 0;
    let mut blocked = Vec::new();
    for product in products {
        let Some(id) = product["id"]
            .as_str()
            .or_else(|| product["product_id"].as_str())
            .filter(|id| !id.is_empty() && id.len() <= 200)
            .map(str::to_owned)
        else {
            withheld += 1;
            continue;
        };
        let text = format!(
            "{} {}",
            product["title"].as_str().unwrap_or(""),
            product["description"].as_str().unwrap_or("")
        );
        if text.trim().is_empty()
            || text.len() > 6000
            || serde_json::to_vec(&product)?.len() > 65536
        {
            withheld += 1;
            blocked.push(id);
            continue;
        }
        if crate::integrations::screen(app, &text).await? {
            withheld += 1;
            blocked.push(id);
            continue;
        }
        safe.push((id, product, text));
    }
    let stored = store(&app.pool, safe, &blocked).await?;
    Ok(
        json!({"source":"Channel3","received":received,"stored":stored,"withheld":withheld,"private_search":true,"notice":"Only the configured public seed was sent to Channel3. User searches stay in the local mirror. Imported items are discovery-only; merchant pricing and availability can change."}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn public_import_sends_only_the_explicit_seed() {
        let router=axum::Router::new().route("/search",axum::routing::post(|axum::Json(body):axum::Json<Value>|async move{
        assert_eq!(body,json!({"query":"developer tools","limit":8}));
        axum::Json(json!({"data":[{"id":"one","title":"Developer keyboard","description":"Compact keyboard"}]}))
    }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let items = fetch_public(
            &client,
            &format!("http://{address}/search"),
            "fake-test-key",
            "developer tools",
        )
        .await
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "one");
        task.abort();
    }
    #[tokio::test]
    #[ignore = "requires PostgreSQL; creates an isolated schema"]
    async fn local_mirror_search_uses_screened_snapshot() {
        dotenvy::dotenv().ok();
        let database = std::env::var("DATABASE_URL").unwrap();
        let admin = PgPool::connect(&database).await.unwrap();
        let schema = format!("qp_catalog_test_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let mut uri = reqwest::Url::parse(&database).unwrap();
        uri.query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        let pool = PgPool::connect(uri.as_str()).await.unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        store(
            &pool,
            vec![(
                "one".into(),
                json!({"title":"Developer keyboard","description":"Compact typing accessory"}),
                "Developer keyboard Compact typing accessory".into(),
            )],
            &[],
        )
        .await
        .unwrap();
        let rows = search(&pool, "keyboard").await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["source"], "Channel3 local mirror");
        assert_eq!(rows[0]["purchasable_in_demo"], false);
        assert_eq!(
            search(&pool, "recommend a keyboard").await.unwrap().len(),
            1
        );
        assert_eq!(
            context(&pool, "keyboard").await.unwrap()[0]["title"],
            "Developer keyboard"
        );
        assert!(
            search(&pool, "private unmatched query")
                .await
                .unwrap()
                .is_empty()
        );
        store(&pool, Vec::new(), &["one".into()]).await.unwrap();
        assert!(
            search(&pool, "keyboard").await.unwrap().is_empty(),
            "a newly withheld product must be evicted from the mirror"
        );
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
    }
}
