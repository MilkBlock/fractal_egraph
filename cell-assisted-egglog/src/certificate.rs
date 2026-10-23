//! Common certificate/marker vocabulary. A document is evidence, not executable
//! permission: each backend constructs it only after its native/static checker.
//! Deserializing user JSON never creates a live saturation authority.
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    CompleteQuery,
    CandidateQuery,
    FiniteClosure,
    GuardedClosure,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub method: String,
    pub strength: Strength,
    pub rulesets: Vec<String>,
    pub contract: Json,
    pub evidence: Json,
}
impl Document {
    pub fn finite(ruleset: &str, body: Json, evidence: Json) -> Self {
        Self {
            method: "native-ripsat-and-delta-replay/v1".into(),
            strength: Strength::FiniteClosure,
            rulesets: vec![ruleset.into()],
            contract: body,
            evidence,
        }
    }
    pub fn guarded(method: &str, rulesets: Vec<String>, contract: Json, evidence: Json) -> Self {
        Self {
            method: method.into(),
            strength: Strength::GuardedClosure,
            rulesets,
            contract,
            evidence,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub certificate: usize,
    pub anchor: String,
    pub slots: Vec<(String, String)>,
    pub validity: Vec<String>,
}
#[derive(Clone, Default, Serialize)]
pub struct Book {
    pub documents: Vec<Document>,
    pub markers: Vec<Marker>,
    #[serde(skip)]
    environment: Option<u64>,
}
impl Book {
    /// Trusted checker boundary, not an unchecked file loader. Native replay,
    /// complete rule-shape validation or source query validation precedes this.
    pub fn checked(&mut self, document: Document) -> usize {
        let id = self.documents.len();
        self.documents.push(document);
        id
    }
    pub fn marker(
        &mut self,
        certificate: usize,
        anchor: String,
        slots: Vec<(String, String)>,
        validity: Vec<String>,
    ) -> Result<usize> {
        if certificate >= self.documents.len() {
            return Err("marker has no checked certificate".into());
        }
        let id = self.markers.len();
        self.markers.push(Marker {
            certificate,
            anchor,
            slots,
            validity,
        });
        Ok(id)
    }
    pub fn bind_environment(&mut self, eg: &egglog::EGraph) {
        self.environment = Some(eg.cell_rule_environment_stamp());
    }
    pub fn current(&self, eg: &egglog::EGraph) -> bool {
        self.environment == Some(eg.cell_rule_environment_stamp())
    }
    pub fn permits(&self, id: usize, strength: Strength, eg: &egglog::EGraph) -> bool {
        self.current(eg)
            && self
                .documents
                .get(id)
                .is_some_and(|c| c.strength == strength)
    }
    pub fn report(&self) -> Json {
        json!({"schema":"ripsat-certificates/v1","certificates":self.documents,"markers":self.markers,
        "scope":"Checked proof methods share one marker vocabulary. Query certificates do not authorize saturation. Runtime aliases, visibility and numeric guards still require their versioned witnesses."})
    }
}
