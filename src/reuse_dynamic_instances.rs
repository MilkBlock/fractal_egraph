//! Committed Cell receipts drive dynamic instance frontiers. A frontier is a set
//! of certified subclosures, not a claim that their union is globally saturated.
use super::{Bake, transitions::Compiled};
use egglog::{EGraph, Value as Native};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
struct GraftBinding {
    rule: usize,
    parent_classes: Vec<usize>,
    parent_slots: Vec<Vec<(usize, usize)>>,
    target_class: usize,
    target_slots: Vec<(usize, usize, bool)>,
}

#[derive(Default)]
pub(super) struct Runtime {
    cells: Vec<(usize, Vec<Native>)>,
    catalog: super::runtime_catalog::Catalog,
    observe: bool,
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
    port_frontiers: BTreeMap<Native, BTreeSet<usize>>,
    observed_grafts: BTreeMap<(Vec<usize>, usize, usize), usize>,
    graft_bindings: Vec<Arc<GraftBinding>>,
    binding_index: BTreeMap<Arc<GraftBinding>, usize>,
}
impl Runtime {
    pub fn with_catalog(catalog: super::runtime_catalog::Catalog) -> Self {
        Self {
            observe: std::env::var("EGG_LAYOUT_OBSERVED_GRAFTS").as_deref() != Ok("0"),
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
        let mut ports = BTreeMap::<Native, BTreeSet<usize>>::new();
        let sort = eg.get_arcsort_by(|s| s.name() == "Math");
        for (port, ids) in std::mem::take(&mut self.port_frontiers) {
            ports
                .entry(eg.get_canonical_value(port, &sort))
                .or_default()
                .extend(
                    ids.into_iter()
                        .map(|i| self.aliases.get(&i).copied().unwrap_or(i)),
                );
        }
        self.port_frontiers = ports;
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
        let mut receipts = vec![];
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
                    self.cells.push((rule, binding.clone()));
                    self.identities.insert(key, id);
                    self.current.insert(id, BTreeSet::from([id]));
                    id
                };
                receipts.push((id, rule, binding));
                targets.insert(id);
            }
        }
        // Discovery uses actual committed bindings and LHS read positions, not
        // admission to the residual-query compiler. Same-batch peers consult the
        // previous frontier; no arbitrary total order is imposed on them.
        if self.observe {
            let mut pending = BTreeMap::<Native, BTreeSet<usize>>::new();
            for (target, rule, binding) in receipts {
                let parents: Vec<_> = self
                    .catalog
                    .read_slots(rule)
                    .iter()
                    .flat_map(|slot| {
                        self.port_frontiers
                            .get(&binding[*slot])
                            .into_iter()
                            .flatten()
                            .copied()
                    })
                    .filter(|id| *id != target)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let key = (parents.clone(), target, rule);
                if !parents.is_empty() && !self.observed_grafts.contains_key(&key) {
                    let mut symbols = BTreeMap::new();
                    let mut rows = vec![];
                    let mut parent_classes = vec![];
                    for parent in &parents {
                        let (family, values) = &self.cells[*parent];
                        let mut ports: Vec<_> = values
                            .iter()
                            .zip(&bake.macros[family].slots)
                            .enumerate()
                            .filter(|(_, (_, (_, sort)))| sort == "Math")
                            .map(|(slot, (v, _))| (self.catalog.shared_slot(*family, slot), *v))
                            .collect();
                        ports.sort_by_key(|(slot, _)| *slot);
                        let mut row = vec![];
                        for (slot, value) in ports {
                            let next = symbols.len();
                            let symbol = *symbols.entry(value).or_insert(next);
                            row.push((slot, symbol));
                        }
                        parent_classes.push(self.catalog.key(*family, values).0);
                        rows.push(row);
                    }
                    let mut ports: Vec<_> = binding
                        .iter()
                        .zip(&bake.macros[&rule].slots)
                        .enumerate()
                        .filter(|(_, (_, (_, sort)))| sort == "Math")
                        .map(|(slot, (v, _))| {
                            (
                                self.catalog.shared_slot(rule, slot),
                                *v,
                                self.catalog.read_slots(rule).contains(&slot),
                            )
                        })
                        .collect();
                    ports.sort_by_key(|(slot, _, _)| *slot);
                    let mut target_slots = vec![];
                    for (slot, value, read) in ports {
                        let next = symbols.len();
                        let symbol = *symbols.entry(value).or_insert(next);
                        target_slots.push((slot, symbol, read));
                    }
                    let shape = Arc::new(GraftBinding {
                        rule,
                        parent_classes,
                        parent_slots: rows,
                        target_class: self.catalog.key(rule, &binding).0,
                        target_slots,
                    });
                    let pattern = if let Some(id) = self.binding_index.get(&shape) {
                        *id
                    } else {
                        let id = self.graft_bindings.len();
                        self.graft_bindings.push(shape.clone());
                        self.binding_index.insert(shape, id);
                        id
                    };
                    self.observed_grafts.insert(key, pattern);
                    for parent in parents {
                        if self.current.entry(parent).or_default().insert(target) {
                            self.revisions += 1;
                        }
                    }
                }
                for (value, (_, sort)) in binding.iter().zip(&bake.macros[&rule].slots) {
                    if sort == "Math" {
                        pending.entry(*value).or_default().insert(target);
                    }
                }
            }
            // A target receipt certifies its own body, not the union of all source
            // bodies. Keep their certificates available until actual equality coalesces them.
            for (port, ids) in pending {
                self.port_frontiers.entry(port).or_default().extend(ids);
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
                            if self.current.entry(*source).or_default().insert(target) {
                                self.revisions += 1;
                            }
                        }
                    }
                }
            }
        }
        fresh
    }
    pub fn report(&self) -> Value {
        json!({"observed_discovery_enabled":self.observe,"observed_grafts":self.observed_grafts.iter().map(|((parents,target,rule),binding)|json!({"parents":parents,"target_cell":target,"rule":rule,"relative_binding_pattern":binding})).collect::<Vec<_>>(),"observed_graft_count":self.observed_grafts.len(),"graft_binding_patterns":self.graft_bindings,"graft_binding_pattern_count":self.graft_bindings.len(),"compiled_link_count":self.edges.len(),"discovery_scope":"committed recipe receipts and concrete LHS read slots; port-context compatibility, not unique causal provenance or proof of new row mutation","equivalence_catalog":self.catalog.report,"semantic_instances":self.cells.iter().enumerate().map(|(id,(r,b))|json!({"instance":id,"canonical_instance":self.aliases.get(&id).copied().unwrap_or(id),"eclass":self.catalog.key(*r,b).0})).collect::<Vec<_>>(),"instances":self.identities.len(),"instance_handles":self.cells.len(),"reused_committed_receipts":self.reused_receipts,"state_revisions":self.revisions,"unresolved_bindings":self.unresolved,
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
