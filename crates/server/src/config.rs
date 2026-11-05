use anyhow::{Context, Result, ensure};
use ed25519_dalek::SigningKey;
use rand::RngCore;
use std::{env, path::PathBuf};
#[derive(Clone)]
pub struct Config {
    pub mode: String,
    pub base: String,
    pub bind: String,
    pub database: String,
    pub password: String,
    pub key: SigningKey,
    pub merchant: String,
    pub verifier: String,
    pub verifier_mode: String,
}
pub fn get(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}
fn local_secret(name: &str, size: usize) -> Result<Vec<u8>> {
    let path = PathBuf::from(".local").join(name);
    match std::fs::read(&path) {
        Ok(v) => Ok(v),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut bytes = vec![0; size];
            rand::thread_rng().fill_bytes(&mut bytes);
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(&bytes)?;
            Ok(bytes)
        }
        Err(e) => Err(e.into()),
    }
}
impl Config {
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();
        std::fs::create_dir_all(".local")?;
        let mode = get("QP_MODE", "replay");
        ensure!(
            ["replay", "sandbox"].contains(&mode.as_str()),
            "QP_MODE must be replay or sandbox; production payments are disabled"
        );
        let key: [u8; 32] = local_secret("signing-key", 32)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid signing-key length"))?;
        let password = match env::var("QP_ADMIN_PASSWORD").ok().filter(|s| !s.is_empty()) {
            Some(s) => s,
            None => hex::encode(local_secret("admin-password", 32)?),
        };
        ensure!(
            password.len() >= 24,
            "QP_ADMIN_PASSWORD must be at least 24 characters"
        );
        let merchant = get(
            "PAYPAL_MERCHANT_ID",
            if mode == "replay" {
                "replay-merchant"
            } else {
                ""
            },
        );
        if mode == "sandbox" {
            for name in [
                "PAYPAL_CLIENT_ID",
                "PAYPAL_CLIENT_SECRET",
                "PAYPAL_MERCHANT_ID",
            ] {
                ensure!(
                    !env::var(name).unwrap_or_default().is_empty(),
                    "missing {name}"
                );
            }
        }
        if mode == "sandbox" {
            ensure!(
                merchant.len() == 13 && merchant.bytes().all(|b| b.is_ascii_alphanumeric()),
                "PAYPAL_MERCHANT_ID must be the 13-character merchant ID of the Sandbox business account"
            );
        }
        let verifier_mode = get("QP_VERIFIER_MODE", "trusted-process");
        ensure!(
            ["docker", "trusted-process"].contains(&verifier_mode.as_str()),
            "invalid verifier mode"
        );
        Ok(Self {
            mode,
            base: get("QP_PUBLIC_BASE", "http://localhost:3000"),
            bind: get("QP_BIND", "127.0.0.1:3000"),
            database: env::var("DATABASE_URL").context("DATABASE_URL required")?,
            password,
            key: SigningKey::from_bytes(&key),
            merchant,
            verifier: get("QP_VERIFIER_BIN", "target/debug/quarantinepay-verifier"),
            verifier_mode,
        })
    }
}
