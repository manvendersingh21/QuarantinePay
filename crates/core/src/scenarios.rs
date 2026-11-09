use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::Verdict;

/// Fictional, server-priced offer. Payment is simulated unless a sandbox is explicitly configured.
#[derive(Clone, Debug, Serialize)]
pub struct Scenario {
    pub id: &'static str,
    pub seller: &'static str,
    pub item: &'static str,
    pub amount_minor: u64,
    pub terms: &'static str,
    pub review_required: bool,
    pub deadline_seconds: u64,
}

/// The complete acceptance agreement is included in the signed intent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScenarioTerms {
    pub id: String,
    pub seller: String,
    pub item: String,
    pub amount_minor: u64,
    pub terms: String,
    pub review_required: bool,
    pub deadline_seconds: u64,
}
impl Scenario {
    pub fn signed_terms(&self) -> ScenarioTerms {
        ScenarioTerms {
            id: self.id.into(),
            seller: self.seller.into(),
            item: self.item.into(),
            amount_minor: self.amount_minor,
            terms: self.terms.into(),
            review_required: self.review_required,
            deadline_seconds: self.deadline_seconds,
        }
    }
}
impl ScenarioTerms {
    pub fn digest(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(self).expect("serializable terms"),
        ))
    }
}
pub fn scenarios() -> Vec<Scenario> {
    [
        ("dataset", "Alder Data", "City mobility CSV", "CSV with id, date and value columns and at least two complete rows.", false),
        ("report", "Northstar Research", "Market report", "A named text report with nonempty Executive summary and Findings sections; buyer reviews quality.", true),
        ("api-data", "Harbor API", "Weather response", "A simulated response with at least two records, each with an id and numeric temperature.", false),
        ("freelance", "Lantern Studio", "Design milestone", "A deliverable file and description; buyer approves or rejects the milestone.", true),
        ("physical", "Cedar Supply", "Desk lamp", "Simulated delivered shipment and tracking evidence; buyer reviews the item. Delivery evidence does not prove quality.", true),
    ].into_iter().map(|(id,seller,item,terms,review_required)| Scenario { id,seller,item,amount_minor:100,terms,review_required,deadline_seconds:120 }).collect()
}
pub fn scenario(id: &str) -> Option<Scenario> {
    scenarios().into_iter().find(|s| s.id == id)
}

// Parse CSV without interpreting its contents. Quoted cells and CRLF are supported.
fn csv_rows(input: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                cell.push(c);
            }
        } else {
            match c {
                '"' if cell.is_empty() && !closed => quoted = true,
                ',' => {
                    row.push(std::mem::take(&mut cell));
                    closed = false;
                }
                '\n' | '\r' => {
                    if c == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                    closed = false;
                }
                _ if !closed && c != '"' => cell.push(c),
                _ => anyhow::bail!("invalid CSV quoting"),
            }
        }
    }
    ensure!(!quoted, "unterminated CSV quote");
    if !row.is_empty() || !cell.is_empty() || closed {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}
fn nonempty(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}
pub fn verify_scenario(id: &str, artifact: &Value) -> Verdict {
    let result = (|| -> Result<()> {
        ensure!(
            serde_json::to_vec(artifact)?.len() <= 65536,
            "artifact exceeds 64 KiB"
        );
        match id {
            "dataset" => {
                let csv = artifact
                    .get("csv")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("CSV file missing"))?;
                let rows = csv_rows(csv)?;
                ensure!(rows.len() >= 3, "at least two data rows required");
                let header = &rows[0];
                for name in ["id", "date", "value"] {
                    ensure!(
                        header.iter().filter(|s| s.as_str() == name).count() == 1,
                        "required column {name} missing or duplicated"
                    );
                }
                ensure!(
                    rows[1..]
                        .iter()
                        .all(|r| r.len() == header.len() && r.iter().all(|s| !s.trim().is_empty())),
                    "rows must have complete columns"
                );
            }
            "report" => {
                ensure!(nonempty(artifact.get("file")), "report file missing");
                let content = artifact
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("report text missing"))?;
                for required in ["Executive summary", "Findings"] {
                    let mut lines = content.lines();
                    let heading = format!("# {required}");
                    ensure!(
                        lines.by_ref().any(|line| line.trim() == heading),
                        "required section {required} missing"
                    );
                    ensure!(
                        lines
                            .take_while(|line| !line.trim_start().starts_with('#'))
                            .any(|line| !line.trim().is_empty()),
                        "section {required} is empty"
                    );
                }
            }
            "api-data" => {
                let records = artifact
                    .get("records")
                    .and_then(Value::as_array)
                    .ok_or_else(|| anyhow::anyhow!("records missing"))?;
                ensure!(records.len() >= 2, "at least two records required");
                ensure!(
                    records.iter().all(|r| nonempty(r.get("id"))
                        && r.get("temperature").is_some_and(Value::is_number)),
                    "each record requires an id and numeric temperature"
                );
            }
            "freelance" => {
                ensure!(
                    nonempty(artifact.get("file")) && nonempty(artifact.get("description")),
                    "file and description required"
                );
            }
            "physical" => {
                ensure!(
                    artifact.get("status").and_then(Value::as_str) == Some("delivered")
                        && nonempty(artifact.get("tracking")),
                    "simulated delivered status and tracking evidence required"
                );
            }
            _ => anyhow::bail!("unknown scenario"),
        }
        Ok(())
    })();
    Verdict {
        passed: result.is_ok(),
        policy_digest: scenario(id)
            .map(|s| s.signed_terms().digest())
            .unwrap_or_default(),
        reason: result.err().map(|e| e.to_string()).unwrap_or_else(|| {
            if scenario(id).is_some_and(|s| s.review_required) {
                "structural checks passed; buyer review required".into()
            } else {
                "all deterministic checks passed".into()
            }
        }),
    }
}
pub fn scenario_fixture(id: &str, pass: bool) -> Value {
    match (id, pass) {
        ("dataset", true) => json!({"csv":"id,date,value\n1,2026-01-01,10\n2,2026-01-02,12\n"}),
        ("dataset", false) => json!({"csv":"id,date\n1,2026-01-01\n"}),
        ("report", true) => {
            json!({"file":"market-report.md", "content":"# Executive summary\nDemand increased.\n# Findings\nTwo markets grew.\n"})
        }
        ("report", false) => {
            json!({"file":"market-report.md", "content":"# Introduction\nAn incomplete report.\n"})
        }
        ("api-data", true) => {
            json!({"records":[{"id":"a","temperature":20},{"id":"b","temperature":21}]})
        }
        ("api-data", false) => json!({"records":[{"id":"a"}]}),
        ("freelance", true) => json!({"file":"design.zip","description":"First milestone"}),
        ("freelance", false) => json!({"file":"","description":""}),
        ("physical", true) => json!({"status":"delivered","tracking":"SIM-12345"}),
        ("physical", false) => json!({"status":"in transit","tracking":"SIM-12345"}),
        _ => json!({}),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_examples_cover_pass_and_fail() {
        for s in scenarios() {
            assert!(
                verify_scenario(s.id, &scenario_fixture(s.id, true)).passed,
                "{}",
                s.id
            );
            assert!(
                !verify_scenario(s.id, &scenario_fixture(s.id, false)).passed,
                "{}",
                s.id
            );
        }
    }
    #[test]
    fn report_checks_text_not_claimed_section_names() {
        assert!(
            !verify_scenario(
                "report",
                &json!({"file":"report.md", "sections":["Executive summary", "Findings"]})
            )
            .passed
        );
        assert!(
            !verify_scenario(
                "report",
                &json!({"file":"report.md", "content":"# Executive summary\n# Findings\nResults"})
            )
            .passed
        );
        assert!(!verify_scenario("report", &json!({"file":"report.md", "content":"Executive summary and Findings are mentioned here"})).passed);
    }
    #[test]
    fn csv_validates_quoting_and_row_shape() {
        assert!(
            verify_scenario(
                "dataset",
                &json!({"csv":"id,date,value\r\n1,2026-01-01,\"10,000\"\r\n2,2026-01-02,5"})
            )
            .passed
        );
        assert!(
            !verify_scenario("dataset", &json!({"csv":"id,date,value\n1,2,\"3\n2,3,4"})).passed
        );
        assert!(!verify_scenario("dataset", &json!({"csv":"id,date,value\n1,2,3\n2,3"})).passed);
    }
}
