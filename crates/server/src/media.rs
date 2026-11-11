//! Listing photos. The server fetches each photo once during discovery and serves it from its own
//! origin, so the buyer's browser never contacts a third party. Photo URLs come from untrusted
//! listings, so every fetch is hardened against SSRF: HTTPS on port 443 only, the host is resolved
//! here and every address must be public (the connection is pinned to the vetted address, so DNS
//! rebinding cannot swap it), at most two redirects with each hop re-vetted, a 5 s deadline, a
//! 2 MB streamed cap, and the declared type must match the file's magic bytes (JPEG, PNG, WebP or
//! GIF; never SVG). Any failure only means "no photo": the reason is kept on the listing.
use crate::App;
use anyhow::{Context, Result, anyhow, bail, ensure};
use futures_util::StreamExt;
use quarantinepay_core::product::{ProductTerms, is_sha256_hex};
use reqwest::Url;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

pub const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_PER_DISCOVERY: usize = 20;
const CONCURRENCY: usize = 4;
const MAX_REDIRECTS: usize = 2;
const DEADLINE: Duration = Duration::from_secs(5);
/// The only types ever stored or served. SVG is excluded: it is a document that can carry script.
pub const TYPES: [&str; 4] = ["image/jpeg", "image/png", "image/webp", "image/gif"];

/// TEST ONLY. `QP_MEDIA_ALLOW_LOOPBACK=1` lets the integration tests fetch photos from their mock
/// servers on 127.0.0.1 over plain HTTP on any port. It admits loopback addresses only (never
/// private, link-local or other ranges) and is off by default; never set it in a deployment.
fn loopback_allowed() -> bool {
    crate::config::get("QP_MEDIA_ALLOW_LOOPBACK", "") == "1"
}
/// `QP_LISTING_IMAGES=false` turns photo fetching off entirely (listings then show no photo).
fn enabled() -> bool {
    crate::config::get("QP_LISTING_IMAGES", "true") != "false"
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0 // "this network", including the unspecified address
        || a == 10
        || a == 127
        || (a == 100 && (b & 0xc0) == 64) // CGNAT 100.64.0.0/10
        || (a == 169 && b == 254) // link-local
        || (a == 172 && (b & 0xf0) == 16)
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && (c == 0 || c == 2)) // IETF assignments, TEST-NET-1
        || (a == 192 && b == 88 && c == 99) // 6to4 relay anycast
        || (a == 198 && (b & 0xfe) == 18) // benchmarking
        || (a == 198 && b == 51 && c == 100) // TEST-NET-2
        || (a == 203 && b == 0 && c == 113) // TEST-NET-3
        || a >= 224) // multicast, reserved, broadcast
}
fn embedded_v4(hi: u16, lo: u16) -> Ipv4Addr {
    Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo))
}
fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = ip.segments();
    // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) reach the embedded IPv4 address.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
        return public_v4(embedded_v4(s[6], s[7]));
    }
    if s[0] == 0x2002 {
        return public_v4(embedded_v4(s[1], s[2]));
    }
    // Only global unicast (2000::/3). That excludes ::, ::1, IPv4-compatible, ULA (fc00::/7),
    // link-local (fe80::/10), site-local, multicast (ff00::/8) and the local NAT64 prefix.
    (s[0] & 0xe000) == 0x2000
        // 2001::/23 IETF protocol assignments (Teredo, ORCHID, benchmarking) and 2001:db8::/32.
        && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
        // 3fff::/20 documentation.
        && !(s[0] == 0x3fff && s[1] < 0x1000)
}
/// True only for globally routable unicast addresses.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

/// The image type the bytes actually are, from their magic numbers.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}
/// The declared Content-Type, normalized to one of `TYPES`, or None for anything else.
pub fn declared(content_type: &str) -> Option<&'static str> {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "image/jpeg" | "image/jpg" | "image/pjpeg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        "image/webp" => Some("image/webp"),
        "image/gif" => Some("image/gif"),
        _ => None,
    }
}
/// Both the header and the bytes must agree on one allowed type.
pub fn verified_type(content_type: &str, bytes: &[u8]) -> Result<&'static str> {
    let declared = declared(content_type).ok_or_else(|| anyhow!("not an allowed image type"))?;
    let actual = sniff(bytes).ok_or_else(|| anyhow!("file is not a JPEG, PNG, WebP or GIF"))?;
    ensure!(
        declared == actual,
        "declared type does not match the file contents"
    );
    Ok(actual)
}

/// Resolve and vet the destination of one hop; returns the address to connect to.
async fn vet(url: &Url, loopback_ok: bool) -> Result<(String, SocketAddr, bool)> {
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "credentials in photo URL"
    );
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("photo URL has no host"))?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("photo URL has no port"))?;
    let literal = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .ok();
    let addrs: Vec<SocketAddr> = match literal {
        Some(ip) => vec![SocketAddr::new(ip, port)],
        None => tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|_| anyhow!("photo host did not resolve"))?
            .take(16)
            .collect(),
    };
    ensure!(!addrs.is_empty(), "photo host did not resolve");
    // Every answer must be acceptable, so a mixed public/private answer cannot be exploited.
    let test_hatch = loopback_ok && addrs.iter().all(|a| a.ip().is_loopback());
    ensure!(
        test_hatch || addrs.iter().all(|a| public_ip(a.ip())),
        "blocked: photo host is not a public address"
    );
    ensure!(
        url.scheme() == "https" || (test_hatch && url.scheme() == "http"),
        "blocked: photo URL is not HTTPS"
    );
    ensure!(
        port == 443 || test_hatch,
        "blocked: photo URL uses a non-standard port"
    );
    Ok((host, addrs[0], literal.is_some()))
}

/// Fetch one photo under every check. Returns (content type, bytes).
pub async fn fetch(source: &str) -> Result<(&'static str, Vec<u8>)> {
    tokio::time::timeout(DEADLINE, fetch_inner(source))
        .await
        .map_err(|_| anyhow!("timed out"))?
}
async fn fetch_inner(source: &str) -> Result<(&'static str, Vec<u8>)> {
    let started = Instant::now();
    let loopback_ok = loopback_allowed();
    let mut url = Url::parse(source).map_err(|_| anyhow!("invalid photo URL"))?;
    for hop in 0..=MAX_REDIRECTS {
        let (host, addr, literal) = vet(&url, loopback_ok).await?;
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .timeout(DEADLINE.saturating_sub(started.elapsed()))
            .user_agent("QuarantinePay-media/1");
        if !literal {
            // Connect to the vetted address; TLS still verifies the certificate for `host`.
            builder = builder.resolve(&host, addr);
        }
        let client = builder.build().context("HTTP client")?;
        let mut response = client
            .get(url.clone())
            .header(
                reqwest::header::ACCEPT,
                "image/webp,image/png,image/jpeg,image/gif;q=0.9",
            )
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    anyhow!("timed out")
                } else {
                    anyhow!("photo request failed")
                }
            })?;
        let status = response.status();
        if status.is_redirection() {
            ensure!(hop < MAX_REDIRECTS, "too many redirects");
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| anyhow!("redirect without a location"))?;
            url = url
                .join(location)
                .map_err(|_| anyhow!("invalid redirect location"))?;
            continue;
        }
        ensure!(
            status == reqwest::StatusCode::OK,
            "HTTP {}",
            status.as_u16()
        );
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        ensure!(
            declared(&content_type).is_some(),
            "not an allowed image type"
        );
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= MAX_BYTES as u64),
            "photo is larger than 2 MB"
        );
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| {
            if e.is_timeout() {
                anyhow!("timed out")
            } else {
                anyhow!("photo download failed")
            }
        })? {
            ensure!(
                body.len().saturating_add(chunk.len()) <= MAX_BYTES,
                "photo is larger than 2 MB"
            );
            body.extend_from_slice(&chunk);
        }
        let kind = verified_type(&content_type, &body)?;
        return Ok((kind, body));
    }
    bail!("too many redirects")
}

/// Fetch (or reuse a recent copy of) one photo and store it; returns its SHA-256.
async fn store(app: &App, source: &str) -> Result<String> {
    let cached: Option<String> = sqlx::query_scalar(
        "SELECT sha256 FROM listing_images WHERE source_url=$1 AND fetched_at > now() - interval '1 day' ORDER BY fetched_at DESC LIMIT 1",
    )
    .bind(source)
    .fetch_optional(&app.pool)
    .await
    .map_err(|_| anyhow!("photo cache unavailable"))?;
    if let Some(sha) = cached {
        return Ok(sha);
    }
    let (kind, bytes) = fetch(source).await?;
    let sha = hex::encode(Sha256::digest(&bytes));
    sqlx::query("INSERT INTO listing_images(sha256,content_type,bytes,source_url) VALUES($1,$2,$3,$4) ON CONFLICT (sha256) DO NOTHING")
        .bind(&sha)
        .bind(kind)
        .bind(&bytes)
        .bind(source)
        .execute(&app.pool)
        .await
        .map_err(|_| anyhow!("photo could not be stored"))?;
    Ok(sha)
}

/// A stored photo, by its SHA-256: (content type, bytes).
pub async fn load(app: &App, sha: &str) -> Result<Option<(String, Vec<u8>)>> {
    if !is_sha256_hex(sha) {
        return Ok(None);
    }
    let row: Option<(String, Vec<u8>)> =
        sqlx::query_as("SELECT content_type, bytes FROM listing_images WHERE sha256=$1")
            .bind(sha)
            .fetch_optional(&app.pool)
            .await?;
    Ok(row.filter(|(t, _)| TYPES.contains(&t.as_str())))
}

/// The main photo of a Channel3 product: `images[]` entries carry `url` and `is_main_image`.
pub fn channel3_image_url(product: &Value) -> Option<String> {
    let images = product["images"].as_array()?;
    images
        .iter()
        .find(|i| i["is_main_image"] == true)
        .or_else(|| images.first())
        .and_then(|i| i["url"].as_str())
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty() && u.len() <= 1000 && !u.chars().any(char::is_control))
}

/// After screening and eligibility: fetch the photo of every listing that has one, replace the
/// remote reference with a local `/media/<sha256>` (`image`), keep `image_url` for audit, record
/// why a photo is missing (`image_note`), and bind the selected listing's photo into its terms.
/// Listings withheld by screening tried to instruct the agent; their URLs are never fetched.
pub async fn attach(
    app: &App,
    listings: &[Value],
    results: &mut [Value],
    selected: Option<&mut ProductTerms>,
) {
    let urls: HashMap<&str, &str> = listings
        .iter()
        .filter_map(|l| Some((l["product_id"].as_str()?, l["image_url"].as_str()?)))
        .filter(|(_, u)| !u.is_empty())
        .collect();
    let on = enabled();
    let mut jobs: Vec<(usize, String)> = Vec::new();
    for (i, r) in results.iter_mut().enumerate() {
        let Some(url) = r["product_id"].as_str().and_then(|id| urls.get(id)) else {
            continue;
        };
        let url = url.to_string();
        r["image_url"] = json!(url);
        let note = if !on {
            Some("listing photos are turned off")
        } else if r["screening"] == "withheld" {
            Some("not fetched: listing withheld by screening")
        } else if jobs.len() >= MAX_PER_DISCOVERY {
            Some("not fetched: photo limit for this search reached")
        } else {
            None
        };
        match note {
            Some(n) => r["image_note"] = json!(n),
            None => jobs.push((i, url)),
        }
    }
    let mut unique: Vec<String> = jobs.iter().map(|(_, u)| u.clone()).collect();
    unique.sort();
    unique.dedup();
    let fetched: HashMap<String, Result<String, String>> = futures_util::stream::iter(unique)
        .map(|u| async move {
            let r = store(app, &u).await.map_err(|e| e.to_string());
            if let Err(e) = &r {
                tracing::info!(reason=%e, "listing photo not used");
            }
            (u, r)
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    for (i, url) in jobs {
        match fetched.get(&url) {
            Some(Ok(sha)) => results[i]["image"] = json!(format!("/media/{sha}")),
            Some(Err(e)) => results[i]["image_note"] = json!(e),
            None => {}
        }
    }
    if let Some(p) = selected {
        p.image_sha256 = results
            .iter()
            .find(|r| r["product_id"].as_str() == Some(p.product_id.as_str()))
            .and_then(|r| r["image"].as_str())
            .and_then(|i| i.strip_prefix("/media/"))
            .filter(|s| is_sha256_hex(s))
            .map(str::to_string);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_pass() {
        for ok in [
            "93.184.216.34",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "172.15.255.255",
            "172.32.0.1",
            "2606:4700::1111",
            "2a00:1450:4001::200e",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:0808:0808::1",
        ] {
            assert!(public_ip(ok.parse().unwrap()), "{ok} should be public");
        }
        for bad in [
            "0.0.0.0",
            "0.1.2.3",
            "127.0.0.1",
            "127.255.255.254",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254", // cloud metadata
            "100.64.0.1",      // CGNAT
            "100.127.255.255",
            "192.0.0.8",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.7",
            "203.0.113.9",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::127.0.0.1", // IPv4-compatible
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "2001::1", // Teredo
            "2001:10::1",
            "3fff::1",
            "::ffff:127.0.0.1", // IPv4-mapped loopback
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:100.64.0.1",
            "64:ff9b::7f00:1",    // NAT64 to loopback
            "64:ff9b::a9fe:a9fe", // NAT64 to metadata
            "64:ff9b:1::1",       // local-use NAT64
            "2002:7f00:0001::1",  // 6to4 to loopback
            "2002:c0a8:0101::1",  // 6to4 to 192.168.1.1
            "100::1",             // discard-only
        ] {
            assert!(!public_ip(bad.parse().unwrap()), "{bad} should be blocked");
        }
    }

    #[test]
    fn type_must_match_magic_bytes() {
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0, 0x10];
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        let gif = b"GIF89a\x01\0\x01\0";
        let webp = b"RIFF\x24\0\0\0WEBPVP8 ";
        assert_eq!(verified_type("image/jpeg", &jpeg).unwrap(), "image/jpeg");
        assert_eq!(verified_type("image/jpg", &jpeg).unwrap(), "image/jpeg");
        assert_eq!(
            verified_type("IMAGE/PNG; charset=x", png).unwrap(),
            "image/png"
        );
        assert_eq!(verified_type("image/gif", gif).unwrap(), "image/gif");
        assert_eq!(verified_type("image/webp", webp).unwrap(), "image/webp");
        // Mismatch, SVG, HTML disguised as an image, and missing or unknown types are refused.
        assert!(verified_type("image/png", &jpeg).is_err());
        assert!(
            verified_type(
                "image/svg+xml",
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
            )
            .is_err()
        );
        assert!(verified_type("image/png", b"<html><script>alert(1)</script>").is_err());
        assert!(verified_type("text/html", png).is_err());
        assert!(verified_type("", png).is_err());
        assert!(verified_type("image/webp", b"RIFF\0\0\0\0WAVEfmt ").is_err());
        assert!(verified_type("image/jpeg", &[]).is_err());
        assert_eq!(sniff(b"<svg"), None);
    }

    #[tokio::test]
    async fn fetch_refuses_unsafe_urls_before_connecting() {
        for (url, why) in [
            ("http://93.184.216.34/a.png", "HTTPS"),
            ("https://127.0.0.1/a.png", "public"),
            ("https://[::1]/a.png", "public"),
            ("https://169.254.169.254/latest/meta-data", "public"),
            ("https://[::ffff:10.0.0.1]/a.png", "public"),
            ("https://93.184.216.34:8443/a.png", "port"),
            ("https://user:pw@93.184.216.34/a.png", "credentials"),
            ("ftp://93.184.216.34/a.png", "HTTPS"),
            ("not a url", "invalid"),
        ] {
            let e = fetch(url).await.unwrap_err().to_string();
            assert!(e.contains(why), "{url}: {e}");
        }
    }

    #[test]
    fn channel3_main_image_is_preferred() {
        let p = json!({"images":[{"url":"https://cdn.example/a","is_main_image":false},{"url":"https://cdn.example/b","is_main_image":true}]});
        assert_eq!(
            channel3_image_url(&p).as_deref(),
            Some("https://cdn.example/b")
        );
        let p = json!({"images":[{"url":"https://cdn.example/a"}]});
        assert_eq!(
            channel3_image_url(&p).as_deref(),
            Some("https://cdn.example/a")
        );
        assert_eq!(channel3_image_url(&json!({"images":[]})), None);
        assert_eq!(channel3_image_url(&json!({})), None);
    }
}
