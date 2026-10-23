//! Observed applications select programs; native typed queries establish their
//! complete interfaces. Numeric variables stay slots, never training literals.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub ruleset: String,
    pub rule: String,
    pub root: String,
}
impl Query {
    pub(super) fn spec(&self) -> boundary::Spec {
        boundary::Spec {
            ruleset: self.ruleset.clone(),
            rule: self.rule.clone(),
            root: self.root.clone(),
            plan: None,
        }
    }
}

pub(super) fn discover(
    eg: &mut EGraph,
    evidence: &Json,
    reserved: &BTreeSet<(String, String)>,
) -> (Vec<Query>, Vec<Json>) {
    let mut queries = vec![];
    let mut rejected = vec![];
    for witness in evidence["rules"].as_array().into_iter().flatten() {
        if witness["records"].as_u64().unwrap_or(0) == 0 {
            continue;
        }
        let (Some(set), Some(rule)) = (witness["ruleset"].as_str(), witness["rule"].as_str())
        else {
            continue;
        };
        if reserved.contains(&(set.into(), rule.into())) {
            continue;
        }
        let mut reasons = vec![];
        let mut found = None;
        // Recorded reads determine the anchors, in their source-position order.
        // The compiler keeps the entire LHS, including unobserved alternatives.
        for root in witness["read_tables"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
        {
            if !eg.get_function(root).is_some_and(|f| f.is_constructor()) {
                continue;
            }
            let result = (|| -> Result<()> {
                let sink = boundary::prepare(eg, set, rule)?;
                // Reject unsupported event-mirror columns per query, before
                // one container port could prevent exporting other programs.
                for sort in sink.atoms.iter().flat_map(|a| &a.sorts) {
                    eg.cell_sort_is_equality(sort)?;
                }
                let tables = (0..sink.atoms.len()).collect();
                boundary::compile(eg, sink, tables, root)?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    found = Some(Query {
                        ruleset: set.into(),
                        rule: rule.into(),
                        root: root.into(),
                    });
                    break;
                }
                Err(e) => reasons.push(e.to_string()),
            }
        }
        if let Some(q) = found {
            queries.push(q);
        } else {
            rejected.push(json!({"ruleset":set,"rule":rule,"reasons":reasons,
            "fallback":"original native query; no history-based exclusion"}));
        }
    }
    (queries, rejected)
}

impl Runtime {
    /// All selected bindings and all residual rules execute at one original
    /// Run boundary. This is not a local saturation or speculative recurrence.
    pub(super) fn run_boundary(&mut self, set: &str) -> Result<egglog_reports::RunReport> {
        self.sync()?;
        let started = std::time::Instant::now();
        let ids = self
            .boundary
            .get(set)
            .cloned()
            .ok_or("missing boundary programs")?;
        let matches = ids
            .iter()
            .map(|id| self.boundary_matches(*id))
            .collect::<Result<Vec<_>>>()?;
        let batches = ids
            .iter()
            .zip(&matches)
            .map(|(id, b)| (&self.kernels[*id].sink, b.as_slice()))
            .collect::<Vec<_>>();
        self.stats.frontier_seconds += started.elapsed().as_secs_f64();
        let started = std::time::Instant::now();
        let report = self.eg.step_cell_mixed_candidates(set, &batches)?;
        self.stats.effect_seconds += started.elapsed().as_secs_f64();
        self.stats.ruleset_runs += 1;
        self.sync()?;
        Ok(report)
    }
}
