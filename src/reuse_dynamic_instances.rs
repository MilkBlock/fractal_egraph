//! Committed Cell receipts drive dynamic instance frontiers. A frontier is a set
//! of certified subclosures, not a claim that their union is globally saturated.
use super::{Bake, transitions::Compiled};
use egglog::{EGraph, Value as Native};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Runtime {
    cells: Vec<(usize, Vec<Native>)>,
    catalog: super::runtime_catalog::Catalog,
    identities: BTreeMap<(usize, Vec<Native>), usize>,
    aliases: BTreeMap<usize, usize>,
    views: BTreeMap<(usize, Vec<Native>), BTreeSet<usize>>,
    current: BTreeMap<usize, BTreeSet<usize>>,
    edges: BTreeSet<(usize, usize, usize)>,
    revisions: usize,
    union_epoch: usize,
    unresolved: usize,
    reused_receipts: usize,
    packed: BTreeSet<(usize, Vec<Native>)>,
    packed_semantic: BTreeSet<(usize, Vec<Native>)>,
}
impl Runtime {
    pub fn with_catalog(catalog: super::runtime_catalog::Catalog) -> Self {
        Self {
            catalog,
            ..Self::default()
        }
    }
    fn canonical(eg: &EGraph, bake: &Bake, rule: usize, binding: &mut [Native]) {
        for (value, (_, sort)) in binding.iter_mut().zip(&bake.macros[&rule].slots) {
            if sort == "Math" {
                *value = eg.get_canonical_value(*value, &eg.get_arcsort_by(|s| s.name() == "Math"));
            }
        }
    }
    fn index_cell(&mut self, id: usize, plans: &[Compiled]) {
        if self.aliases.get(&id).is_some_and(|owner| *owner != id) {
            return;
        }
        let (rule, binding) = &self.cells[id];
        for (rule, binding) in self.catalog.variants(*rule, binding) {
            for (p, plan) in plans
                .iter()
                .enumerate()
                .filter(|(_, p)| p.spec.source_rule == rule)
            {
                for (row, _) in &plan.rows {
                    let key = row[..row.len() - 1].iter().map(|i| binding[*i]).collect();
                    self.views.entry((p, key)).or_default().insert(id);
                }
            }
        }
    }
    fn rebuild_views(&mut self, plans: &[Compiled]) {
        self.views.clear();
        for id in 0..self.cells.len() {
            self.index_cell(id, plans);
        }
    }
    pub fn begin(&mut self, eg: &EGraph, bake: &Bake, plans: &[Compiled]) {
        let epoch = eg.native_union_count();
        if epoch == self.union_epoch {
            return;
        }
        self.union_epoch = epoch;
        self.identities.clear();
        for (id, (rule, binding)) in self.cells.iter_mut().enumerate() {
            Self::canonical(eg, bake, *rule, binding);
            let owner = *self
                .identities
                .entry(self.catalog.key(*rule, binding))
                .or_insert(id);
            self.aliases.insert(id, owner);
        }
        let old = std::mem::take(&mut self.current);
        for (id, states) in old {
            let owner = self.aliases[&id];
            self.current
                .entry(owner)
                .or_default()
                .extend(states.into_iter().map(|i| self.aliases[&i]));
        }
        self.edges = std::mem::take(&mut self.edges)
            .into_iter()
            .filter_map(|(s, t, p)| {
                let s = self.aliases[&s];
                let t = self.aliases[&t];
                (s != t).then_some((s, t, p))
            })
            .collect();
        self.packed = std::mem::take(&mut self.packed)
            .into_iter()
            .map(|(rule, mut b)| {
                Self::canonical(eg, bake, rule, &mut b);
                (rule, b)
            })
            .collect();
        self.packed_semantic = self
            .packed
            .iter()
            .map(|(r, b)| self.catalog.key(*r, b))
            .collect();
        self.rebuild_views(plans);
    }
    pub fn mark_packed(&mut self, fresh: &BTreeSet<(usize, Vec<Native>)>, minimum: usize) {
        let mut counts = BTreeMap::new();
        for (rule, _) in fresh {
            *counts.entry(*rule).or_insert(0usize) += 1;
        }
        self.packed.extend(
            fresh
                .iter()
                .filter(|(rule, _)| counts[rule] >= minimum)
                .cloned(),
        );
        self.packed_semantic.extend(
            fresh
                .iter()
                .filter(|(r, _)| counts[r] >= minimum)
                .map(|(r, b)| self.catalog.key(*r, b)),
        );
    }
    pub fn commit(
        &mut self,
        eg: &EGraph,
        bake: &Bake,
        plans: &[Compiled],
        packets: &BTreeMap<usize, Vec<Vec<Native>>>,
    ) -> BTreeSet<(usize, Vec<Native>)> {
        self.begin(eg, bake, plans);
        let mut fresh = BTreeSet::new();
        // All receipts in this batch are committed. Resolve edges after admitting
        // the whole batch, independent of rule enumeration order.
        let first_new = self.cells.len();
        let mut targets = BTreeSet::new();
        for (&rule, rows) in packets {
            for row in rows {
                let mut binding = row.clone();
                Self::canonical(eg, bake, rule, &mut binding);
                let key = self.catalog.key(rule, &binding);
                if !self.packed_semantic.contains(&key) {
                    fresh.insert((rule, binding.clone()));
                }
                let id = if let Some(id) = self.identities.get(&key) {
                    self.reused_receipts += 1;
                    *id
                } else {
                    let id = self.cells.len();
                    self.cells.push((rule, binding));
                    self.identities.insert(key, id);
                    self.current.insert(id, BTreeSet::from([id]));
                    id
                };
                targets.insert(id);
            }
        }
        for id in first_new..self.cells.len() {
            self.index_cell(id, plans);
        }
        for target in targets {
            let (rule, binding) = &self.cells[target];
            for (rule, binding) in self.catalog.variants(*rule, binding) {
                for (p, plan) in plans
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.spec.consumer_rule == rule)
                {
                    let names = plan.proof["binding_variables"].as_array();
                    let Some(names) = names else {
                        self.unresolved += 1;
                        continue;
                    };
                    let key: Option<Vec<_>> = names
                        .iter()
                        .map(|name| {
                            bake.macros[&rule]
                                .slots
                                .iter()
                                .position(|(term, _)| Some(term.as_str()) == name.as_str())
                                .map(|i| binding[i])
                        })
                        .collect();
                    let Some(key) = key else {
                        self.unresolved += 1;
                        continue;
                    };
                    for source in self.views.get(&(p, key)).into_iter().flatten() {
                        if *source == target {
                            continue;
                        }
                        let canonical_plan = plans
                            .iter()
                            .position(|candidate| {
                                candidate.relation == plan.relation
                                    && candidate.spec.consumer_rule == plan.spec.consumer_rule
                            })
                            .unwrap_or(p);
                        if self.edges.insert((*source, target, canonical_plan)) {
                            self.current.entry(*source).or_default().insert(target);
                            self.revisions += 1;
                        }
                    }
                }
            }
        }
        fresh
    }
    pub fn report(&self) -> Value {
        json!({"equivalence_catalog":self.catalog.report,"semantic_instances":self.cells.iter().enumerate().map(|(id,(r,b))|json!({"instance":id,"canonical_instance":self.aliases.get(&id).copied().unwrap_or(id),"eclass":self.catalog.key(*r,b).0})).collect::<Vec<_>>(),"instances":self.identities.len(),"instance_handles":self.cells.len(),"reused_committed_receipts":self.reused_receipts,"state_revisions":self.revisions,"unresolved_bindings":self.unresolved,
            "transitions":self.edges.iter().map(|(s,t,p)|json!({"source":s,"target":t,"plan":p})).collect::<Vec<_>>(),
            "current":self.current.iter().map(|(i,s)|json!({"instance":i,"certified_cells":s})).collect::<Vec<_>>(),
            "scope":"committed receipt frontiers and proved cut-compatible transitions; not a whole-frontier saturation certificate or unique causal provenance"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reuse_graph::adaptive::RipSatCell;
    #[test]
    fn only_installed_receipts_skip_packing_and_union_rekeys_them() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, "(datatype Math (V i64)) (let a (V 0)) (let b (V 1))")
            .unwrap();
        let mut parser = egglog::ast::Parser::default();
        let a = eg
            .eval_expr(&parser.get_expr_from_string(None, "a").unwrap())
            .unwrap()
            .1;
        let b = eg
            .eval_expr(&parser.get_expr_from_string(None, "b").unwrap())
            .unwrap()
            .1;
        let mut bake = Bake::default();
        bake.macros.insert(
            0,
            RipSatCell {
                rule: 0,
                actions: String::new(),
                rows: 0,
                classes: 1,
                proof_matches: 0,
                slots: vec![("x".into(), "Math".into())],
                template: vec![],
            },
        );
        let packets = BTreeMap::from([(0, vec![vec![a]])]);
        let mut runtime = Runtime::default();
        let fresh = runtime.commit(&eg, &bake, &[], &packets);
        runtime.mark_packed(&fresh, 2); // one instance: below packing threshold
        assert_eq!(runtime.commit(&eg, &bake, &[], &packets).len(), 1);
        runtime.mark_packed(&fresh, 1);
        assert!(runtime.commit(&eg, &bake, &[], &packets).is_empty());
        eg.parse_and_run_program(None, "(union a b)").unwrap();
        assert!(
            runtime
                .commit(&eg, &bake, &[], &BTreeMap::from([(0, vec![vec![b]])]))
                .is_empty()
        );
    }
}
