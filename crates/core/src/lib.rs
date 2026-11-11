use anyhow::{Result, bail, ensure};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
pub mod contract;
pub mod product;
mod scenarios;
pub use contract::{CONTRACT_SKU, PurchaseContract};
pub use scenarios::{
    Scenario, ScenarioTerms, scenario, scenario_fixture, scenarios, verify_scenario,
};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<ScenarioTerms>,
    /// Agent-negotiated, buyer-approved contract. Absent on catalog and legacy intents, so their
    /// signed bytes are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<PurchaseContract>,
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
        if let Some(contract) = &self.intent.contract {
            ensure!(
                self.intent.scenario.is_none(),
                "an intent cannot carry both catalog and contract terms"
            );
            contract.validate()?;
            ensure!(
                self.intent.sku == CONTRACT_SKU
                    && self.intent.amount_minor == contract.amount_minor
                    && self.intent.currency == contract.currency,
                "contract price or SKU mismatch"
            );
            ensure!(
                self.intent.policy_digest == contract.digest(),
                "contract policy mismatch"
            );
        } else if let Some(terms) = &self.intent.scenario {
            let offer =
                scenario(&terms.id).ok_or_else(|| anyhow::anyhow!("unsupported scenario"))?;
            ensure!(*terms == offer.signed_terms(), "scenario terms mismatch");
            ensure!(
                self.intent.sku == terms.id
                    && self.intent.amount_minor == terms.amount_minor
                    && self.intent.currency == "USD",
                "scenario price or SKU mismatch"
            );
            ensure!(
                self.intent.policy_digest == terms.digest(),
                "scenario policy mismatch"
            );
        } else {
            ensure!(self.intent.sku == "invoice-fixture-v1", "unsupported SKU");
            ensure!(
                self.intent.currency == "USD" && self.intent.amount_minor == 100,
                "price mismatch"
            );
            ensure!(
                self.intent.policy_digest == policy_digest(),
                "policy mismatch"
            );
        }
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
            scenario: None,
            contract: None,
        }
    }
    #[test]
    fn contract_intents_are_signed_and_policy_bound() {
        let key = SigningKey::from_bytes(&[11; 32]);
        let objective = contract::parse_objective(
            "Market research report on robotics under $100, three competitors, 5 sources",
        )
        .bounded()
        .unwrap();
        let c = contract::compile_contract(&objective, &contract::negotiate(&objective)).unwrap();
        let mut value = intent();
        value.sku = CONTRACT_SKU.into();
        value.amount_minor = c.amount_minor;
        value.policy_digest = c.digest();
        value.contract = Some(c);
        let signed = SignedIntent::sign(value, &key).unwrap();
        signed.verify(&key.verifying_key(), "buyer", 100).unwrap();
        // Weakening the signed contract breaks the signature.
        let mut altered = signed.clone();
        altered.intent.contract.as_mut().unwrap().minimum_sources = 1;
        assert!(altered.verify(&key.verifying_key(), "buyer", 100).is_err());
        // Even a validly re-signed weaker contract fails policy binding unless digest and price match.
        let resigned = SignedIntent::sign(altered.intent.clone(), &key).unwrap();
        assert!(resigned.verify(&key.verifying_key(), "buyer", 100).is_err());
        // An over-ceiling contract cannot be signed into a valid intent at all.
        let mut over = altered.intent;
        let oc = over.contract.as_mut().unwrap();
        oc.minimum_sources = 5;
        oc.budget_minor = 90_000;
        oc.amount_minor = 80_000;
        over.amount_minor = 80_000;
        over.policy_digest = over.contract.as_ref().unwrap().digest();
        let signed_over = SignedIntent::sign(over, &key).unwrap();
        assert!(
            signed_over
                .verify(&key.verifying_key(), "buyer", 100)
                .is_err()
        );
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
    fn scenario_terms_are_signed_and_catalog_bound() {
        let key = SigningKey::from_bytes(&[9; 32]);
        for offer in scenarios() {
            let terms = offer.signed_terms();
            let mut value = intent();
            value.sku = offer.id.into();
            value.policy_digest = terms.digest();
            value.scenario = Some(terms);
            let signed = SignedIntent::sign(value, &key).unwrap();
            signed.verify(&key.verifying_key(), "buyer", 100).unwrap();
            let mut altered = signed.clone();
            altered
                .intent
                .scenario
                .as_mut()
                .unwrap()
                .terms
                .push_str(" changed");
            assert!(altered.verify(&key.verifying_key(), "buyer", 100).is_err());
            // A valid signature over substituted terms must also fail the allowlist.
            let resigned = SignedIntent::sign(altered.intent, &key).unwrap();
            assert!(resigned.verify(&key.verifying_key(), "buyer", 100).is_err());
        }
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
