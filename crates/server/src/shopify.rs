//! Read-only Shopify development-store data. Shopify checkout is independent of QuarantinePay.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::time::Duration;

const VERSION: &str = "2026-07";
const PLAN_QUERY: &str = "query { shop { plan { partnerDevelopment } } }";
const DATA_QUERY: &str = "query { products(first: 10) { nodes { id title } } orders(first: 10, query: \"test:true\") { nodes { id name test displayFinancialStatus } } }";
const MAX_BYTES: usize = 128 * 1024;

fn endpoint(domain: &str) -> Result<String> {
    let label = domain
        .strip_suffix(".myshopify.com")
        .ok_or_else(|| anyhow::anyhow!("SHOPIFY_DEV_STORE must be a myshopify.com hostname"))?;
    ensure!(
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-'),
        "invalid SHOPIFY_DEV_STORE hostname"
    );
    Ok(format!("https://{domain}/admin/api/{VERSION}/graphql.json"))
}
fn valid_development_store(value: &Value) -> Result<()> {
    ensure!(
        value.get("errors").is_none(),
        "Shopify returned GraphQL errors"
    );
    ensure!(
        value
            .pointer("/data/shop/plan/partnerDevelopment")
            .and_then(Value::as_bool)
            == Some(true),
        "Shopify did not confirm this is a partner development store"
    );
    Ok(())
}
fn safe_text<'a>(node: &'a Value, field: &str) -> Result<&'a str> {
    let value = node
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Shopify returned incomplete data"))?;
    ensure!(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "Shopify returned invalid data"
    );
    Ok(value)
}
fn bounded_result(value: &Value) -> Result<Value> {
    ensure!(
        value.get("errors").is_none(),
        "Shopify returned GraphQL errors"
    );
    let products = value
        .pointer("/data/products/nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Shopify product data missing"))?;
    let orders = value
        .pointer("/data/orders/nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Shopify order data missing"))?;
    ensure!(
        products.len() <= 10 && orders.len() <= 10,
        "Shopify response exceeds item limit"
    );
    let products: Vec<Value> = products
        .iter()
        .map(|p| Ok(json!({"id":safe_text(p,"id")?,"title":safe_text(p,"title")?})))
        .collect::<Result<_>>()?;
    let orders: Vec<Value> = orders.iter().map(|o| {
        ensure!(o.get("test").and_then(Value::as_bool) == Some(true), "Shopify returned a non-test order");
        Ok(json!({"id":safe_text(o,"id")?,"name":safe_text(o,"name")?,"test":true,"financial_status":safe_text(o,"displayFinancialStatus")?}))
    }).collect::<Result<_>>()?;
    Ok(
        json!({"source":"Shopify development store", "externally_confirmed":true, "notice":"Shopify test orders are independent of QuarantinePay settlement; no payment linkage is established.","products":products,"test_orders":orders}),
    )
}
async fn query(client: &reqwest::Client, url: &str, token: &str, graphql: &str) -> Result<Value> {
    let mut response = client
        .post(url)
        .header("X-Shopify-Access-Token", token)
        .json(&json!({"query":graphql}))
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Shopify request failed"))?;
    ensure!(
        response.status().is_success(),
        "Shopify request was rejected (HTTP {})",
        response.status().as_u16()
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_BYTES as u64),
        "Shopify response too large"
    );
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Shopify response failed"))?
    {
        ensure!(
            body.len().saturating_add(chunk.len()) <= MAX_BYTES,
            "Shopify response too large"
        );
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| anyhow::anyhow!("invalid Shopify response"))
}
/// Queries only a partner development store using a bounded, redirect-free client.
/// Requires SHOPIFY_DEV_STORE and SHOPIFY_ADMIN_ACCESS_TOKEN; no checkout or writes.
pub async fn read_dev_store() -> Result<Value> {
    let domain = std::env::var("SHOPIFY_DEV_STORE").unwrap_or_default();
    let token = std::env::var("SHOPIFY_ADMIN_ACCESS_TOKEN").unwrap_or_default();
    ensure!(
        !token.is_empty(),
        "SHOPIFY_ADMIN_ACCESS_TOKEN not configured"
    );
    let url = endpoint(&domain)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()?;
    valid_development_store(&query(&client, &url, &token, PLAN_QUERY).await?)?;
    bounded_result(&query(&client, &url, &token, DATA_QUERY).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostname_is_strict() {
        assert!(endpoint("my-dev-store.myshopify.com").is_ok());
        for bad in [
            "https://store.myshopify.com",
            "store.myshopify.com.evil.test",
            "store.myshopify.com@evil.test",
            "store.myshopify.com:443",
            "a.b.myshopify.com",
            "-store.myshopify.com",
            "STORE.myshopify.com",
            "store.myshopify.com/path",
            ".myshopify.com",
        ] {
            assert!(endpoint(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn refuses_unconfirmed_stores_and_unsafe_results() {
        assert!(
            valid_development_store(&json!({"data":{"shop":{"plan":{"partnerDevelopment":true}}}}))
                .is_ok()
        );
        for v in [
            json!({}),
            json!({"data":{"shop":{"plan":{"partnerDevelopment":false}}}}),
            json!({"errors":[{}],"data":{"shop":{"plan":{"partnerDevelopment":true}}}}),
        ] {
            assert!(valid_development_store(&v).is_err());
        }
        let mut v = json!({"data":{"products":{"nodes":[{"id":"p1","title":"Example","secret":"ignored"}]},"orders":{"nodes":[{"id":"o1","name":"#1","test":true,"displayFinancialStatus":"PAID","customer":"private"}]}}});
        let safe = bounded_result(&v).unwrap();
        assert!(safe.to_string().contains("independent"));
        assert!(!safe.to_string().contains("private"));
        v["data"]["orders"]["nodes"][0]["test"] = json!(false);
        assert!(bounded_result(&v).is_err());
        v["data"]["orders"]["nodes"] = json!(vec![json!({}); 11]);
        assert!(bounded_result(&v).is_err());
    }
}
