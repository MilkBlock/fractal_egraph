//! Online normalization of prepared CS compositions. No candidate discovery or ripen here.
//! Opaque inputs are alpha-normalized using ordered interfaces. Positive exact
//! sequential prefixes can be factored; other compositions retain full preparations.
use crate::pipeline::Result;
use egglog::{EGraph, Value};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};

#[path = "cell_graft_interfaces.rs"]
mod graft;
#[path = "cell_binding_normalize.rs"]
mod normalize;

const DSL: &str = r#"
(datatype*
 (CellParents (NoParents) (Parent Cell CellParents))
 (Cell (CoarseApply CellParents i64)))
"#;

pub(super) struct Registry {
    eg: EGraph,
    values: BTreeMap<usize, Value>,
    origins: BTreeMap<usize, usize>,
    programs: BTreeMap<usize, Json>,
    interfaces: BTreeMap<usize, Json>,
    prepared: BTreeMap<usize, graft::Prepared>,
    grafts: BTreeMap<(usize, Vec<usize>), Json>,
    checked_grafts: BTreeSet<(usize, Vec<usize>)>,
    completed: BTreeSet<usize>,
    apply_entries: BTreeMap<usize, usize>,
    seen_windows: usize,
    factored: usize,
    contracts: BTreeMap<String, usize>,
    nodes: Vec<Json>,
    certificates: BTreeSet<(usize, usize)>,
    unions: usize,
    normalized: usize,
    normalization_fallbacks: usize,
}
impl Registry {
    pub fn new() -> Result<Self> {
        let mut eg = EGraph::default();
        let cmds = eg.parse_program(None, DSL)?;
        eg.run_program(cmds)?;
        Ok(Self {
            eg,
            values: BTreeMap::new(),
            origins: BTreeMap::new(),
            programs: BTreeMap::new(),
            interfaces: BTreeMap::new(),
            prepared: BTreeMap::new(),
            grafts: BTreeMap::new(),
            checked_grafts: BTreeSet::new(),
            completed: BTreeSet::new(),
            apply_entries: BTreeMap::new(),
            seen_windows: 0,
            factored: 0,
            contracts: BTreeMap::new(),
            nodes: vec![],
            certificates: BTreeSet::new(),
            unions: 0,
            normalized: 0,
            normalization_fallbacks: 0,
        })
    }
    pub fn contains(&self, unit: usize) -> bool {
        self.values.contains_key(&unit)
    }
    pub fn register(
        &mut self,
        unit: usize,
        parents: &[usize],
        origin: &Json,
        source: &str,
        validation: &str,
    ) -> Result<()> {
        if self.values.contains_key(&unit) {
            return Ok(());
        }
        // Parents are real registered CS units, not arbitrary intersecting fragments.
        // Retain original coordinate contracts for unsupported normalization cases.
        // Supported cases compare executable preparations, not history provenance.
        let parent_contracts = parents
            .iter()
            .map(|p| {
                self.origins
                    .get(p)
                    .cloned()
                    .ok_or_else(|| format!("unregistered Cell parent {p}"))
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let fallback = json!({"recipe":origin["reuse_recipe"], "interface":origin["reuse_interface_terms"], "source":source,"validation":validation,"parents":parent_contracts});
        let normalized =
            if std::env::var("EGG_LAYOUT_CELL_BINDING_NORMALIZATION").as_deref() == Ok("0") {
                Err("normalization disabled for ablation".into())
            } else {
                normalize::contract(origin, source, validation)
            };
        let descriptor = match normalized {
            Ok(key) => {
                self.normalized += 1;
                key
            }
            Err(_) => {
                self.normalization_fallbacks += 1;
                fallback
            }
        };
        if let Some(prepared) = graft::Prepared::new(origin, source, validation) {
            self.prepared.insert(unit, prepared);
        }
        self.interfaces.insert(unit, json!({"parameter_marker":origin["parameter_marker"],"reuse_interface_terms":origin["reuse_interface_terms"],"symbolic_values":origin["symbolic_values"]}));
        self.programs.insert(unit, descriptor.clone());
        let parent_programs: Vec<_> = parents
            .iter()
            .filter_map(|p| self.programs.get(p).zip(self.interfaces.get(p)))
            .collect();
        let residual = normalize::residual(&parent_programs, &descriptor, origin).or_else(|| {
            if parents.len() == 1 {
                self.programs
                    .get(&parents[0])
                    .and_then(|p| normalize::suffix(p, &descriptor))
            } else {
                None
            }
        });
        let descriptor = if let Some(residual) = residual {
            self.factored += 1;
            residual
        } else {
            descriptor
        };
        let key = serde_json::to_string(&descriptor)?;
        let next = self.contracts.len();
        let contract = *self.contracts.entry(key).or_insert(next);
        let parent_expr = parents
            .iter()
            .rev()
            .fold("(NoParents)".to_string(), |tail, p| {
                format!("(Parent cell_{p} {tail})")
            });
        let commands = self.eg.parse_program(
            None,
            &format!("(let cell_{unit} (CoarseApply {parent_expr} {contract}))"),
        )?;
        self.eg.run_program(commands)?;
        let expression =
            egglog::ast::Parser::default().get_expr_from_string(None, &format!("cell_{unit}"))?;
        let value = self.eg.eval_expr(&expression)?.1;
        self.values.insert(unit, value);
        self.origins.insert(unit, contract);
        self.nodes
            .push(json!({"unit":unit,"parents":parents,"contract":contract,
                "history_parents":parents,"producer_parents":origin["composition"]["producer_parts"],
                "context_parents":origin["composition"]["context_parts"],
                "parent_scope":"observed history; not a necessary execution dependency"}));
        self.consider_graft(unit, parents.to_vec());
        Ok(())
    }
    fn consider_graft(&mut self, target: usize, mut parents: Vec<usize>) {
        parents.sort_unstable();
        parents.dedup();
        if parents.is_empty()
            || parents.contains(&target)
            || !self.checked_grafts.insert((target, parents.clone()))
        {
            return;
        }
        let Some(child) = self.prepared.get(&target) else {
            return;
        };
        let Some(sources) = parents
            .iter()
            .map(|p| self.prepared.get(p).map(|s| (*p, s)))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        if let Some(proof) = child.extension(&sources) {
            self.grafts.insert(
                (target, parents.clone()),
                json!({"target":target,"parents":parents,"proof":proof}),
            );
        }
    }
    /// Inspect only newly observed windows. All recorded producer antecedents
    /// must participate; no arbitrary parent subsets or graph-wide pair search.
    pub fn connect_windows(&mut self, windows: &[super::super::cs::Window]) {
        for w in &windows[self.seen_windows..] {
            if w.kind == "apply_entry" {
                self.apply_entries.insert(w.consumer, w.unit);
            }
            if w.kind != "producer_consumer" {
                continue;
            }
            let parents: Option<Vec<_>> = w
                .members
                .iter()
                .filter(|r| **r != w.consumer)
                .map(|r| self.apply_entries.get(r).copied())
                .collect();
            if let Some(parents) = parents {
                self.consider_graft(w.unit, parents);
            }
        }
        self.seen_windows = windows.len();
    }
    /// Only the completed, exact catalog can call this; weak/reentry results never do.
    /// Family IDs are local to a catalog refresh, so representatives are rebuilt each time.
    pub fn admit_catalog(&mut self, catalog: &Json) -> Result<()> {
        let mut representatives = BTreeMap::new();
        let mut commands = String::new();
        for trigger in catalog["triggers"].as_array().into_iter().flatten() {
            let (Some(unit), Some(family)) = (
                trigger["binding_origin"]["candidate_id"].as_u64(),
                trigger["saturated_rule_composition"].as_u64(),
            ) else {
                continue;
            };
            let unit = unit as usize;
            if !self.values.contains_key(&unit) {
                continue;
            }
            self.completed.insert(unit);
            if let Some(&owner) = representatives.get(&family) {
                let pair = (unit.min(owner), unit.max(owner));
                if unit != owner && !self.certificates.contains(&pair) {
                    commands.push_str(&format!("(union cell_{owner} cell_{unit})\n"));
                }
            } else {
                representatives.insert(family, unit);
            }
        }
        if !commands.is_empty() {
            let parsed = self.eg.parse_program(None, &commands)?;
            self.eg.run_program(parsed)?; // native rebuild propagates congruence
        }
        // Record certificates only after successful execution.
        representatives.clear();
        for trigger in catalog["triggers"].as_array().into_iter().flatten() {
            let (Some(unit), Some(family)) = (
                trigger["binding_origin"]["candidate_id"].as_u64(),
                trigger["saturated_rule_composition"].as_u64(),
            ) else {
                continue;
            };
            let unit = unit as usize;
            if !self.values.contains_key(&unit) {
                continue;
            }
            if let Some(&owner) = representatives.get(&family) {
                if unit != owner && self.certificates.insert((unit.min(owner), unit.max(owner))) {
                    self.unions += 1;
                }
            } else {
                representatives.insert(family, unit);
            }
        }
        Ok(())
    }
    pub fn report(&self) -> Json {
        let sort = self.eg.get_arcsort_by(|s| s.name() == "Cell");
        let mut classes = BTreeMap::new();
        let units: Vec<_> = self
            .values
            .iter()
            .map(|(unit, value)| {
                let canonical = self.eg.get_canonical_value(*value, &sort);
                let next = classes.len();
                let class = *classes.entry(canonical).or_insert(next);
                json!({"unit":unit,"eclass":class})
            })
            .collect();
        let grafts: Vec<_> = self
            .grafts
            .iter()
            .filter(|((target, parents), _)| {
                self.completed.contains(target)
                    && parents.iter().all(|p| self.completed.contains(p))
            })
            .map(|(_, edge)| edge)
            .collect();
        json!({"graft_interfaces":grafts,"graft_interface_scope":"conditional replay extension, independent of historical edges; only exact completed endpoint certificates", "registered_compositions":self.values.len(),"eclasses":classes.len(),"contracts":self.contracts.len(),"normalized_bindings":self.normalized,"factored_successors":self.factored,"normalization_fallbacks":self.normalization_fallbacks,"certified_unions":self.unions,"nodes":self.nodes,"units":units,"scope":"automatic prepared CS registration; exact completed catalog unions; conservative full binding contracts; no ripen skipped"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interface_waits_for_both_exact_endpoints_and_is_idempotent() {
        let mut r = Registry::new().unwrap();
        let origin = |members| {
            json!({"parameter_marker":"Input","members":members,
            "symbolic_values":[{"token":1,"term":"(Input 0)","sort":"T"}],
            "reuse_interface_terms":{"lower":["(Input 0)"],"upper":[]},"staged_injections":[]})
        };
        let base = "(datatype T (Input i64) (A T) (B T)) (A (Input 0))";
        r.register(0, &[], &origin(vec![0]), base, "").unwrap();
        r.register(
            1,
            &[0],
            &origin(vec![0, 1]),
            &format!("{base} (B (Input 0))"),
            "",
        )
        .unwrap();
        assert_eq!(r.report()["graft_interfaces"], json!([]));
        r.admit_catalog(&json!({"triggers":[{"binding_origin":{"candidate_id":1},"saturated_rule_composition":1}]})).unwrap();
        assert_eq!(r.report()["graft_interfaces"], json!([]));
        let cat = json!({"triggers":[{"binding_origin":{"candidate_id":0},"saturated_rule_composition":0},{"binding_origin":{"candidate_id":1},"saturated_rule_composition":1}]});
        r.admit_catalog(&cat).unwrap();
        assert_eq!(r.report()["graft_interfaces"].as_array().unwrap().len(), 1);
        r.consider_graft(1, vec![0]);
        r.admit_catalog(&cat).unwrap();
        assert_eq!(r.report()["graft_interfaces"].as_array().unwrap().len(), 1);
        assert_ne!(
            r.report()["units"][0]["eclass"],
            r.report()["units"][1]["eclass"]
        );
    }
    #[test]
    fn renamed_successors_merge_after_parent_certificate() {
        let mut r = Registry::new().unwrap();
        let origin = |slot| json!({"parameter_marker":"RipenInput","reuse_interface_terms":{"lower":[format!("(RipenInput {slot})")],"upper":[]}});
        for (unit, parents, slot, source) in [
            (0, vec![], 7, "(A (RipenInput 7))"),
            (1, vec![], 41, "(A (RipenInput 41)) (B (RipenInput 41))"),
            (2, vec![0], 7, "(A (RipenInput 7)) (C (RipenInput 7))"),
            (
                3,
                vec![1],
                41,
                "(A (RipenInput 41)) (B (RipenInput 41)) (C (RipenInput 41))",
            ),
        ] {
            r.register(unit, &parents, &origin(slot), source, source)
                .unwrap();
        }
        let before = r.report();
        assert_eq!(
            before["nodes"][2]["contract"],
            before["nodes"][3]["contract"]
        );
        assert_ne!(before["units"][2]["eclass"], before["units"][3]["eclass"]);
        r.admit_catalog(&json!({"triggers":[
            {"binding_origin":{"candidate_id":0},"saturated_rule_composition":0},
            {"binding_origin":{"candidate_id":1},"saturated_rule_composition":0}]}))
            .unwrap();
        let after = r.report();
        assert_eq!(after["units"][2]["eclass"], after["units"][3]["eclass"]);
        assert_eq!(after["certified_unions"], 1);
        assert_eq!(after["factored_successors"], 2);
    }
    #[test]
    fn catalog_refresh_is_idempotent_and_preserves_multi_parent_bindings() {
        let mut r = Registry::new().unwrap();
        let origin = json!({"reuse_recipe":["fixture"],"reuse_interface_terms":{"lower":["x"],"upper":["y"]}});
        r.register(0, &[], &origin, "left", "checks").unwrap();
        r.register(1, &[], &origin, "right", "checks").unwrap();
        r.register(2, &[0, 1], &origin, "joined", "checks").unwrap();
        r.register(3, &[0, 1], &origin, "joined", "different obligation")
            .unwrap();
        let catalog = |family| {
            json!({"triggers":[
            {"binding_origin":{"candidate_id":0},"saturated_rule_composition":family},
            {"binding_origin":{"candidate_id":1},"saturated_rule_composition":family}]})
        };
        r.admit_catalog(&catalog(0)).unwrap();
        r.admit_catalog(&catalog(99)).unwrap(); // IDs are refresh-local, not durable identities
        let report = r.report();
        assert_eq!(report["registered_compositions"], 4);
        assert_eq!(report["certified_unions"], 1);
        assert_eq!(report["eclasses"], 3);
        assert_eq!(report["nodes"][2]["parents"], json!([0, 1]));
        assert_ne!(report["units"][2]["eclass"], report["units"][3]["eclass"]);
        assert!(r.register(4, &[999], &origin, "bad", "").is_err());
        assert!(!r.contains(4));
    }
}
