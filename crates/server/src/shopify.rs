//! A real Shopify merchant (partner development store only). QuarantinePay reads the merchant's
//! catalog, places the order after PayPal has authorized, and reads the merchant's fulfilment
//! record server to server. Shopify never moves money here: PayPal holds the funds, and the
//! merchant's order only mirrors that state. Every response is bounded and every write first
//! re-confirms that the store is a partner development store.
use anyhow::{Result, bail, ensure};
use quarantinepay_core::PurchaseContract;
use quarantinepay_core::product::{
    SHIP_TO_ADDRESS1, SHIP_TO_CITY, SHIP_TO_COUNTRY, SHIP_TO_PROVINCE, SHIP_TO_ZIP,
};
use serde_json::{Value, json};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const VERSION: &str = "2026-10";
const MAX_BYTES: usize = 256 * 1024;

fn store_domain() -> String {
    crate::config::get("SHOPIFY_DEV_STORE", "")
}
fn validate_domain(domain: &str) -> Result<()> {
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
    Ok(())
}
/// `SHOPIFY_ADMIN_BASE` exists only so tests can point at a local mock.
fn admin_base() -> Result<String> {
    let base = crate::config::get("SHOPIFY_ADMIN_BASE", "");
    if !base.is_empty() {
        return Ok(base.trim_end_matches('/').to_string());
    }
    let domain = store_domain();
    validate_domain(&domain)?;
    Ok(format!("https://{domain}"))
}

pub fn configured() -> bool {
    !store_domain().is_empty()
        && (!crate::config::get("SHOPIFY_ADMIN_ACCESS_TOKEN", "").is_empty()
            || (!crate::config::get("SHOPIFY_CLIENT_ID", "").is_empty()
                && !crate::config::get("SHOPIFY_CLIENT_SECRET", "").is_empty()))
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .expect("HTTP client")
    })
}

async fn bounded(mut response: reqwest::Response) -> Result<Value> {
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

fn token_cache() -> &'static Mutex<Option<(String, Instant)>> {
    static CACHE: OnceLock<Mutex<Option<(String, Instant)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// A fixed Admin API token if one is configured; otherwise the app's client-credentials grant,
/// cached until shortly before it expires (Shopify issues them for 24 hours).
async fn token() -> Result<String> {
    let fixed = crate::config::get("SHOPIFY_ADMIN_ACCESS_TOKEN", "");
    if !fixed.is_empty() {
        return Ok(fixed);
    }
    let mut cache = token_cache().lock().await;
    if let Some((t, until)) = cache.as_ref()
        && *until > Instant::now()
    {
        return Ok(t.clone());
    }
    let id = crate::config::get("SHOPIFY_CLIENT_ID", "");
    let secret = crate::config::get("SHOPIFY_CLIENT_SECRET", "");
    ensure!(
        !id.is_empty() && !secret.is_empty(),
        "Shopify app credentials not configured"
    );
    let response = client()
        .post(format!("{}/admin/oauth/access_token", admin_base()?))
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", id.as_str()),
            ("client_secret", secret.as_str()),
        ])
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Shopify token request failed"))?;
    let status = response.status();
    ensure!(
        status.is_success(),
        "Shopify refused the app's token request (HTTP {}); is the app installed on the store?",
        status.as_u16()
    );
    let v = bounded(response).await?;
    let t = v["access_token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Shopify returned no access token"))?
        .to_string();
    let ttl = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(300);
    *cache = Some((t.clone(), Instant::now() + Duration::from_secs(ttl)));
    Ok(t)
}

async fn gql(query: &str, variables: Value) -> Result<Value> {
    let body = json!({"query":query,"variables":variables});
    let url = format!("{}/admin/api/{VERSION}/graphql.json", admin_base()?);
    let mut response = None;
    for attempt in 0..2 {
        let r = client()
            .post(&url)
            .header("X-Shopify-Access-Token", token().await?)
            .json(&body)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("Shopify request failed"))?;
        // A reinstall revokes the cached token: drop it and ask for a new one, once.
        if r.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
            *token_cache().lock().await = None;
            continue;
        }
        response = Some(r);
        break;
    }
    let response = response.ok_or_else(|| anyhow::anyhow!("Shopify request failed"))?;
    ensure!(
        response.status().is_success(),
        "Shopify request was rejected (HTTP {})",
        response.status().as_u16()
    );
    let v = bounded(response).await?;
    if let Some(errors) = v["errors"].as_array().filter(|e| !e.is_empty()) {
        let msg: Vec<String> = errors
            .iter()
            .take(3)
            .map(|e| text(&e["message"], 200))
            .collect();
        bail!("Shopify: {}", msg.join("; "));
    }
    Ok(v["data"].clone())
}

/// Every write is preceded by this check: QuarantinePay only ever acts on a development store.
async fn ensure_development_store() -> Result<()> {
    static CONFIRMED: OnceLock<()> = OnceLock::new();
    if CONFIRMED.get().is_some() {
        return Ok(());
    }
    let d = gql("{ shop { plan { partnerDevelopment } } }", json!({})).await?;
    ensure!(
        d.pointer("/shop/plan/partnerDevelopment") == Some(&json!(true)),
        "Shopify did not confirm this is a partner development store"
    );
    let _ = CONFIRMED.set(());
    Ok(())
}

fn text(v: &Value, max: usize) -> String {
    v.as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}
fn minor(decimal: &str) -> u64 {
    let (whole, frac) = decimal.split_once('.').unwrap_or((decimal, "0"));
    let frac: String = format!("{frac:0<2}").chars().take(2).collect();
    match (whole.parse::<u64>(), frac.parse::<u64>()) {
        (Ok(w), Ok(f)) => w.saturating_mul(100).saturating_add(f),
        _ => 0,
    }
}
fn decimal(minor: u64) -> String {
    format!("{}.{:02}", minor / 100, minor % 100)
}

/// Ask Shopify's image CDN for a 480 px wide rendition, so stored thumbnails stay small.
fn sized_image(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut u) if u.scheme() == "https" && u.host_str() == Some("cdn.shopify.com") => {
            let kept: Vec<(String, String)> = u
                .query_pairs()
                .filter(|(k, _)| k != "width" && k != "height")
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            u.query_pairs_mut()
                .clear()
                .extend_pairs(kept)
                .append_pair("width", "480");
            u.to_string()
        }
        _ => url.to_string(),
    }
}

pub struct Catalog {
    pub merchant: String,
    pub domain: String,
    /// Normalized like Channel3 listings so the same screening applies.
    pub listings: Vec<Value>,
}

/// Search the merchant's live catalog. Only sanitized search words are sent.
pub async fn search(terms: &str) -> Result<Catalog> {
    let words: Vec<String> = terms
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '-')
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .take(8)
        .collect();
    ensure!(!words.is_empty(), "no product search terms");
    const Q: &str = "query($q:String!){ shop{ name myshopifyDomain currencyCode plan{partnerDevelopment} } products(first:20, query:$q, sortKey:RELEVANCE){ nodes{ id title handle vendor status description(truncateAt:600) onlineStoreUrl featuredImage{ url altText } variants(first:5){ nodes{ id title price availableForSale image{ url } } } } } }";
    let mut data = gql(Q, json!({"q":format!("{} status:active", words.join(" "))})).await?;
    if data["products"]["nodes"]
        .as_array()
        .is_none_or(|n| n.is_empty())
        && words.len() > 1
    {
        data = gql(
            Q,
            json!({"q":format!("({}) status:active", words.join(" OR "))}),
        )
        .await?;
    }
    ensure!(
        data.pointer("/shop/plan/partnerDevelopment") == Some(&json!(true)),
        "Shopify did not confirm this is a partner development store"
    );
    let domain = text(&data["shop"]["myshopifyDomain"], 120);
    validate_domain(&domain)?;
    let currency = text(&data["shop"]["currencyCode"], 8);
    let mut listings = Vec::new();
    for p in data["products"]["nodes"].as_array().into_iter().flatten() {
        let handle = text(&p["handle"], 200);
        let url = Some(text(&p["onlineStoreUrl"], 1000))
            .filter(|u| u.starts_with("https://"))
            .unwrap_or_else(|| format!("https://{domain}/products/{handle}"));
        for v in p["variants"]["nodes"].as_array().into_iter().flatten() {
            let vt = text(&v["title"], 100);
            let title = if vt.is_empty() || vt == "Default Title" {
                text(&p["title"], 300)
            } else {
                format!("{} — {vt}", text(&p["title"], 200))
            };
            // The variant's own photo when it has one, else the product's featured photo.
            let image = [&v["image"]["url"], &p["featuredImage"]["url"]]
                .into_iter()
                .map(|u| text(u, 1000))
                .find(|u| !u.is_empty())
                .map(|u| sized_image(&u));
            listings.push(json!({
                "product_id": text(&v["id"], 200),
                "title": title,
                "brand": Some(text(&p["vendor"], 80)).filter(|s| !s.is_empty()),
                "retailer": domain,
                "price_minor": minor(&text(&v["price"], 20)),
                "currency": currency,
                "availability": if v["availableForSale"] == true { "InStock" } else { "OutOfStock" },
                "condition": "new",
                "listing_url": url,
                "description": text(&p["description"], 600),
                "image_url": image,
            }));
            if listings.len() >= 8 {
                break;
            }
        }
        if listings.len() >= 8 {
            break;
        }
    }
    Ok(Catalog {
        merchant: text(&data["shop"]["name"], 120),
        domain,
        listings,
    })
}

/// Place the merchant order after PayPal authorized. Idempotent by the QuarantinePay order tag:
/// a retry after a crash finds the existing order instead of creating a second one.
pub async fn place_order(
    order_ref: uuid::Uuid,
    contract: &PurchaseContract,
    authorization_id: &str,
) -> Result<(String, String)> {
    let p = contract
        .product
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("not a product contract"))?;
    ensure_development_store().await?;
    let tag = format!("qp-{order_ref}");
    let existing = gql(
        "query($q:String!){ orders(first:1, query:$q){ nodes{ id name } } }",
        json!({"q":format!("tag:{tag}")}),
    )
    .await?;
    if let Some(o) = existing["orders"]["nodes"]
        .as_array()
        .and_then(|n| n.first())
    {
        return Ok((text(&o["id"], 200), text(&o["name"], 40)));
    }
    let amount = decimal(contract.amount_minor);
    let money = json!({"shopMoney":{"amount":amount,"currencyCode":"USD"}});
    let d = gql(
        "mutation($order: OrderCreateOrderInput!, $options: OrderCreateOptionsInput){ orderCreate(order:$order, options:$options){ order{ id name } userErrors{ field message } } }",
        json!({
            "order": {
                "currency": "USD",
                "test": true,
                "lineItems": [{"variantId": p.product_id, "quantity": 1, "priceSet": money, "requiresShipping": true}],
                "shippingAddress": {"firstName":"Quarantine","lastName":"Buyer","address1":SHIP_TO_ADDRESS1,"city":SHIP_TO_CITY,"provinceCode":SHIP_TO_PROVINCE,"countryCode":SHIP_TO_COUNTRY,"zip":SHIP_TO_ZIP},
                "financialStatus": "AUTHORIZED",
                "transactions": [{"kind":"AUTHORIZATION","status":"SUCCESS","gateway":"QuarantinePay (PayPal Sandbox)","authorizationCode":authorization_id,"amountSet":money}],
                "tags": ["quarantinepay", tag],
                "note": format!("Paid through QuarantinePay. PayPal authorization {authorization_id} is held until QuarantinePay verifies this order's fulfilment; it is voided if the order is cancelled or not fulfilled within {} minutes.", contract.deadline_seconds / 60),
            },
            "options": {"inventoryBehaviour":"DECREMENT_OBEYING_POLICY","sendReceipt":false,"sendFulfillmentReceipt":false}
        }),
    )
    .await?;
    let errors = d["orderCreate"]["userErrors"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ensure!(
        errors.is_empty(),
        "Shopify refused the order: {}",
        errors
            .iter()
            .map(|e| text(&e["message"], 200))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let o = &d["orderCreate"]["order"];
    let id = text(&o["id"], 200);
    ensure!(
        id.starts_with("gid://shopify/Order/"),
        "Shopify returned no order"
    );
    Ok((id, text(&o["name"], 40)))
}

/// The merchant's current record of the order, normalized and bounded. This is the only delivery
/// evidence a Shopify purchase accepts, and QuarantinePay fetches it itself.
pub async fn snapshot(order_gid: &str) -> Result<Value> {
    let d = gql(
        "query($id:ID!){ shop{ myshopifyDomain } order(id:$id){ id name tags cancelledAt displayFulfillmentStatus lineItems(first:5){ nodes{ quantity variant{ id } } } shippingAddress{ address1 city provinceCode zip countryCodeV2 } fulfillments(first:5){ status displayStatus createdAt deliveredAt trackingInfo(first:3){ number company url } } } }",
        json!({"id":order_gid}),
    )
    .await?;
    let o = &d["order"];
    ensure!(o.is_object(), "Shopify order not found");
    let a = &o["shippingAddress"];
    Ok(json!({
        "source": "shopify_admin_api",
        "shop": text(&d["shop"]["myshopifyDomain"], 120),
        "fetched_at": chrono::Utc::now(),
        "order": {
            "id": text(&o["id"], 200),
            "name": text(&o["name"], 40),
            "tags": o["tags"].as_array().into_iter().flatten().take(20).map(|t| text(t, 100)).collect::<Vec<_>>(),
            "cancelled_at": o["cancelledAt"].as_str().map(|s| text(&json!(s), 40)),
            "fulfillment_status": text(&o["displayFulfillmentStatus"], 40),
            "line_items": o["lineItems"]["nodes"].as_array().into_iter().flatten().take(5).map(|l| json!({"variant_id":text(&l["variant"]["id"],200),"quantity":l["quantity"].as_u64().unwrap_or(0)})).collect::<Vec<_>>(),
            "shipping_address": {"address1":text(&a["address1"],120),"city":text(&a["city"],80),"province_code":text(&a["provinceCode"],8),"zip":text(&a["zip"],16),"country_code":text(&a["countryCodeV2"],4)},
            "fulfillments": o["fulfillments"].as_array().into_iter().flatten().take(5).map(|f| json!({
                "status": text(&f["status"], 40),
                "display_status": text(&f["displayStatus"], 40),
                "created_at": text(&f["createdAt"], 40),
                "delivered_at": f["deliveredAt"].as_str().map(|s| text(&json!(s), 40)),
                "tracking": f["trackingInfo"].as_array().into_iter().flatten().take(3).map(|t| json!({"number":text(&t["number"],80),"company":text(&t["company"],120),"url":text(&t["url"],300)})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }
    }))
}

/// Mirror PayPal's final state onto the merchant's order. PayPal is the source of truth; this only
/// keeps the merchant's books consistent and never changes payment state.
pub async fn settle(order_gid: &str, captured: bool) -> Result<Value> {
    ensure_development_store().await?;
    if captured {
        // Record the capture against the AUTHORIZATION transaction placed with the order, so the
        // merchant's books show the same authorize -> capture sequence PayPal does.
        let d = gql(
            "query($id:ID!){ order(id:$id){ displayFinancialStatus transactions{ id kind status amountSet{ shopMoney{ amount currencyCode } } } } }",
            json!({"id":order_gid}),
        )
        .await?;
        if d["order"]["displayFinancialStatus"] == "PAID" {
            return Ok(json!({"action":"already_paid","financial_status":"PAID"}));
        }
        let auth = d["order"]["transactions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["kind"] == "AUTHORIZATION" && t["status"] == "SUCCESS")
            .ok_or_else(|| {
                anyhow::anyhow!("Shopify order has no successful authorization to capture")
            })?;
        let money = &auth["amountSet"]["shopMoney"];
        let d = gql(
            "mutation($input: OrderCaptureInput!){ orderCapture(input:$input){ transaction{ id status order{ displayFinancialStatus } } userErrors{ field message } } }",
            json!({"input":{"id":order_gid,"parentTransactionId":auth["id"],"amount":money["amount"],"currency":money["currencyCode"]}}),
        )
        .await?;
        let r = &d["orderCapture"];
        let errors = r["userErrors"].as_array().cloned().unwrap_or_default();
        ensure!(
            errors.is_empty(),
            "Shopify: {}",
            errors
                .iter()
                .map(|e| text(&e["message"], 200))
                .collect::<Vec<_>>()
                .join("; ")
        );
        Ok(json!({
            "action":"capture_recorded",
            "transaction":text(&r["transaction"]["id"],200),
            "financial_status":text(&r["transaction"]["order"]["displayFinancialStatus"],40),
        }))
    } else {
        // The merchant may have cancelled first (that is what voided the hold): nothing to mirror.
        let d = gql(
            "query($id:ID!){ order(id:$id){ cancelledAt } }",
            json!({"id":order_gid}),
        )
        .await?;
        if let Some(at) = d["order"]["cancelledAt"].as_str() {
            return Ok(json!({"action":"already_cancelled","cancelled_at":text(&json!(at),40)}));
        }
        let d = gql(
            "mutation($id:ID!){ orderCancel(orderId:$id, reason:DECLINED, restock:true, notifyCustomer:false, refundMethod:{originalPaymentMethodsRefund:false}, staffNote:\"QuarantinePay voided the PayPal authorization\"){ job{ id } orderCancelUserErrors{ field message } } }",
            json!({"id":order_gid}),
        )
        .await?;
        let errors = d["orderCancel"]["orderCancelUserErrors"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        ensure!(
            errors.is_empty(),
            "Shopify: {}",
            errors
                .iter()
                .map(|e| text(&e["message"], 200))
                .collect::<Vec<_>>()
                .join("; ")
        );
        Ok(json!({"action":"cancelled","job":text(&d["orderCancel"]["job"]["id"],200)}))
    }
}

/// Read-only summary for the dashboard's development-store panel.
pub async fn read_dev_store() -> Result<Value> {
    ensure!(configured(), "Shopify development store not configured");
    let d = gql(
        "{ shop { name myshopifyDomain plan { partnerDevelopment } } products(first: 10, query:\"status:active\") { nodes { id title } } orders(first: 10, query: \"tag:quarantinepay\", reverse:true) { nodes { id name test displayFinancialStatus displayFulfillmentStatus } } }",
        json!({}),
    )
    .await?;
    ensure!(
        d.pointer("/shop/plan/partnerDevelopment") == Some(&json!(true)),
        "Shopify did not confirm this is a partner development store"
    );
    Ok(json!({
        "source": "Shopify development store",
        "shop": text(&d["shop"]["myshopifyDomain"], 120),
        "products": d["products"]["nodes"].as_array().into_iter().flatten().map(|p| json!({"id":text(&p["id"],200),"title":text(&p["title"],256)})).collect::<Vec<_>>(),
        "quarantinepay_orders": d["orders"]["nodes"].as_array().into_iter().flatten().map(|o| json!({"id":text(&o["id"],200),"name":text(&o["name"],40),"test":o["test"],"financial_status":text(&o["displayFinancialStatus"],40),"fulfillment_status":text(&o["displayFulfillmentStatus"],40)})).collect::<Vec<_>>(),
        "notice": "Orders here were placed by QuarantinePay after PayPal authorized. PayPal holds the funds; Shopify mirrors the outcome.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostname_is_strict() {
        assert!(validate_domain("my-dev-store.myshopify.com").is_ok());
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
            assert!(validate_domain(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn shopify_cdn_images_are_resized() {
        assert_eq!(
            sized_image("https://cdn.shopify.com/s/files/1/lamp.jpg?v=17"),
            "https://cdn.shopify.com/s/files/1/lamp.jpg?v=17&width=480"
        );
        assert_eq!(
            sized_image("https://cdn.shopify.com/s/files/1/lamp.jpg?width=2048&v=1"),
            "https://cdn.shopify.com/s/files/1/lamp.jpg?v=1&width=480"
        );
        assert_eq!(
            sized_image("https://cdn.shopify.com/lamp.png"),
            "https://cdn.shopify.com/lamp.png?width=480"
        );
        // Other hosts are left alone (the media fetcher vets them).
        assert_eq!(
            sized_image("https://img.example/a.jpg?width=9"),
            "https://img.example/a.jpg?width=9"
        );
    }
    #[test]
    fn money_round_trips() {
        assert_eq!(minor("34.00"), 3_400);
        assert_eq!(minor("34.5"), 3_450);
        assert_eq!(minor("7"), 700);
        assert_eq!(minor("abc"), 0);
        assert_eq!(decimal(3_405), "34.05");
    }
}
