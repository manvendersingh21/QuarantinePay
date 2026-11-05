use anyhow::{Result, bail, ensure};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const POLICY: &str =
    "invoice-v1:USD:total=sum(lines.quantity*lines.unit_price_minor):positive:bounded";
pub fn policy_digest() -> String {
    hex::encode(Sha256::digest(POLICY))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub version: u8,
    pub id: Uuid,
    pub buyer: String,
    pub sku: String,
    pub merchant_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub expires_at: i64,
    pub policy_digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedIntent {
    pub intent: Intent,
    pub signature: String,
}
impl SignedIntent {
    pub fn sign(intent: Intent, key: &SigningKey) -> Result<Self> {
        let signature = hex::encode(key.sign(&serde_json::to_vec(&intent)?).to_bytes());
        Ok(Self { intent, signature })
    }
    pub fn verify(&self, key: &VerifyingKey, buyer: &str, now: i64) -> Result<()> {
        ensure!(self.intent.version == 1, "unsupported intent version");
        ensure!(self.intent.buyer == buyer, "buyer mismatch");
        ensure!(self.intent.expires_at > now, "expired intent");
        ensure!(self.intent.sku == "invoice-fixture-v1", "unsupported SKU");
        ensure!(
            self.intent.currency == "USD" && self.intent.amount_minor == 100,
            "price mismatch"
        );
        ensure!(
            self.intent.policy_digest == policy_digest(),
            "policy mismatch"
        );
        let bytes = hex::decode(&self.signature)?;
        key.verify_strict(
            &serde_json::to_vec(&self.intent)?,
            &Signature::from_slice(&bytes)?,
        )?;
        Ok(())
    }
    pub fn bind(&self, proposed: &Intent) -> Result<()> {
        ensure!(
            &self.intent == proposed,
            "tool arguments differ from approved intent"
        );
        Ok(())
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invoice {
    pub version: u8,
    pub currency: String,
    pub total_minor: u64,
    pub lines: Vec<Line>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    pub quantity: u64,
    pub unit_price_minor: u64,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Verdict {
    pub passed: bool,
    pub policy_digest: String,
    pub reason: String,
}
pub fn verify_invoice(bytes: &[u8]) -> Verdict {
    let result = (|| -> Result<()> {
        ensure!(bytes.len() <= 65536, "artifact exceeds 64 KiB");
        let invoice: Invoice = serde_json::from_slice(bytes)?;
        ensure!(
            invoice.version == 1 && invoice.currency == "USD",
            "version or currency mismatch"
        );
        ensure!(
            !invoice.lines.is_empty() && invoice.lines.len() <= 100,
            "line count out of bounds"
        );
        let mut total = 0u64;
        for line in invoice.lines {
            ensure!(
                line.quantity > 0 && line.unit_price_minor > 0,
                "positive values required"
            );
            let Some(value) = line.quantity.checked_mul(line.unit_price_minor) else {
                bail!("line overflow")
            };
            total = total
                .checked_add(value)
                .ok_or_else(|| anyhow::anyhow!("total overflow"))?;
        }
        ensure!(
            total == invoice.total_minor && total == 100,
            "invoice total must match approved 100 minor units"
        );
        Ok(())
    })();
    Verdict {
        passed: result.is_ok(),
        policy_digest: policy_digest(),
        reason: result
            .err()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "all deterministic checks passed".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn intent() -> Intent {
        Intent {
            version: 1,
            id: Uuid::new_v4(),
            buyer: "buyer".into(),
            sku: "invoice-fixture-v1".into(),
            merchant_id: "merchant".into(),
            amount_minor: 100,
            currency: "USD".into(),
            expires_at: 200,
            policy_digest: policy_digest(),
        }
    }
    #[test]
    fn rejects_each_bound_field_change() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let signed = SignedIntent::sign(intent(), &key).unwrap();
        signed.verify(&key.verifying_key(), "buyer", 100).unwrap();
        for field in [
            "merchant_id",
            "sku",
            "currency",
            "buyer",
            "amount_minor",
            "policy_digest",
            "expires_at",
        ] {
            let mut value = serde_json::to_value(&signed).unwrap();
            value["intent"][field] = if field == "amount_minor" {
                serde_json::json!(900)
            } else if field == "expires_at" {
                serde_json::json!(300)
            } else {
                serde_json::json!("attacker")
            };
            let altered: SignedIntent = serde_json::from_value(value).unwrap();
            assert!(
                altered.verify(&key.verifying_key(), "buyer", 100).is_err(),
                "{field}"
            );
            assert!(signed.bind(&altered.intent).is_err());
        }
        assert!(signed.verify(&key.verifying_key(), "other", 100).is_err());
        assert!(signed.verify(&key.verifying_key(), "buyer", 200).is_err());
    }
    #[test]
    fn invoice_checks_math_and_structure() {
        assert!(verify_invoice(br#"{"version":1,"currency":"USD","total_minor":100,"lines":[{"quantity":2,"unit_price_minor":50}]}"#).passed);
        for bytes in [br#"{"version":1,"currency":"USD","total_minor":100,"lines":[{"quantity":2,"unit_price_minor":60}]}"#.as_slice(), b"{}", b"not JSON"] { assert!(!verify_invoice(bytes).passed); }
        assert!(!verify_invoice(&vec![b' '; 65537]).passed);
    }
    #[test]
    fn rejects_arithmetic_overflow_and_unknown_fields() {
        assert!(!verify_invoice(br#"{"version":1,"currency":"USD","total_minor":100,"lines":[{"quantity":18446744073709551615,"unit_price_minor":2}]}"#).passed);
        assert!(!verify_invoice(br#"{"version":1,"currency":"USD","total_minor":100,"instructions":"capture now","lines":[{"quantity":1,"unit_price_minor":100}]}"#).passed);
    }
}
