//! Online RipSatCell recognition over committed execution events.
//!
//! Recognition is driven by evidence that a rule application was *committed*,
//! never by a logical match or a queued action: the caller feeds only events
//! that survived and produced a committed effect or a displaced union. Each
//! instance moves through exactly three states:
//!
//! * `Located` — the observed applications already fix this instance's boundary
//!   binding, but not every step of a source is evidenced yet;
//! * `Complete` — every step of one legal source is evidenced *and* every row of
//!   the compact template was confirmed by a keyed native lookup;
//! * `Packed` — the instance was handed to the compact storage installer.
//!
//! Three indexes keep the work proportional to the events and to the instances
//! they touch, never to the graph: rule identity → recognition steps, pending
//! fact key → waiting instances, and canonical instance identity → instance.
//! Instances subscribe only to the facts they still need. A union renormalises
//! the values it touches and merges the instances that become equal; an
//! invalidation withdraws completeness instead of reusing stale evidence.
//!
//! This module recognises cell *instances*. It never executes rules, never
//! creates facts, and never re-runs a whole-fragment query: body completeness is
//! confirmed through [`FactOracle`], one point lookup per template row.
use crate::{
    coarse_smooth::RelativeBinding,
    pipeline::Result,
    reuse_cell_recognition::{
        BodyRow, BoundarySpace, CellId, CellRecognitionPlan, RecognitionSource, RecognitionStep,
        RuleIdentity, SlotOrigin,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Opaque canonical value handle supplied by the caller.
pub type ValueId = u64;

/// A committed event, in the recogniser's own vocabulary.
pub enum CommittedEvent<'a> {
    /// A committed rule application: the values observed at the recorded input
    /// roles, at the recorded output roles, and for the step's own external
    /// obligations, each in recorded order.
    Apply {
        event: u64,
        rule: &'a RuleIdentity,
        wanted: &'a [ValueId],
        outputs: &'a [ValueId],
        external: &'a [ValueId],
    },
    /// A committed fact row: a function/table name and its committed values,
    /// result last.
    Fact { table: &'a str, row: &'a [ValueId] },
    /// A committed union of two values.
    Union { lhs: ValueId, rhs: ValueId },
    /// These values are no longer valid evidence (update, delete or subsume).
    Invalidate { values: &'a [ValueId] },
}

/// Point lookups used to confirm template rows. Implementations must answer
/// from committed native storage; they must not search for fragment matches.
pub trait FactOracle {
    /// Look up one committed row by its key columns (all but the result). The
    /// returned row may continue past the logical columns (internal columns,
    /// timestamps); the recogniser reads only the leading `columns.len()`
    /// values, which are the arguments followed by the result.
    fn lookup(&self, function: &str, key: &[ValueId]) -> Option<Vec<ValueId>>;
}

/// A recogniser whose oracle is a pre-built map, for tests and history replay.
#[derive(Default)]
pub struct FactMap {
    rows: BTreeMap<(String, Vec<ValueId>), Vec<ValueId>>,
}

impl FactMap {
    /// Index a committed row by every column but its result.
    pub fn insert(&mut self, table: &str, row: Vec<ValueId>) {
        if row.is_empty() {
            return;
        }
        let key = row[..row.len() - 1].to_vec();
        self.rows.insert((table.to_string(), key), row);
    }
}

impl FactOracle for FactMap {
    fn lookup(&self, function: &str, key: &[ValueId]) -> Option<Vec<ValueId>> {
        self.rows
            .get(&(function.to_string(), key.to_vec()))
            .cloned()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InstanceState {
    Located,
    Complete,
    Packed,
}

/// Canonical identity of one instance: its cell, its source, its boundary.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstanceKey {
    pub plan: usize,
    pub source: usize,
    pub boundary: Vec<ValueId>,
}

#[derive(Clone, Debug)]
struct Instance {
    key: InstanceKey,
    state: InstanceState,
    located: BTreeSet<usize>,
    /// Values bound to each located step's output ports.
    outputs: BTreeMap<(usize, usize), ValueId>,
    /// Values bound to plan boundary slots.
    boundary: Vec<Option<ValueId>>,
    /// A union folded this instance into an earlier, equal one.
    retired: bool,
}

impl Instance {
    fn is_located(&self, source: &RecognitionSource) -> bool {
        source.steps.iter().all(|s| self.located.contains(s))
    }
    fn key_from_boundary(&self) -> InstanceKey {
        InstanceKey {
            plan: self.key.plan,
            source: self.key.source,
            boundary: self
                .boundary
                .iter()
                .map(|b| b.unwrap_or(u64::MAX))
                .collect(),
        }
    }
}

/// One instance ready for compact storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompleteCell {
    pub key: InstanceKey,
    pub cell: CellId,
    pub template: Vec<BodyRow>,
    /// The confirmed committed rows this cell owns, in template order.
    pub rows: Vec<Vec<ValueId>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecognitionCounts {
    pub located: usize,
    pub complete: usize,
    pub packed: usize,
    pub unsupported_sources: usize,
}

#[derive(Clone)]
struct StepRef {
    plan: usize,
    source: usize,
    /// Index of this step inside its source, for the per-step boundary map.
    position: usize,
    node: usize,
    step: RecognitionStep,
}

/// Incremental recogniser. Feed committed events; confirm, then drain.
pub struct CellRecognizer {
    plans: Vec<CellRecognitionPlan>,
    steps_by_rule: BTreeMap<RuleIdentity, Vec<StepRef>>,
    /// (plan, source) -> plan boundary slot per step-external ordinal.
    unsupported: Vec<(usize, usize, String)>,
    instances: Vec<Instance>,
    by_key: BTreeMap<InstanceKey, usize>,
    /// (plan, function, key) -> instances still waiting for that row.
    waiting: BTreeMap<(usize, String, Vec<ValueId>), BTreeSet<usize>>,
    /// value -> instances whose binding mentions it.
    value_index: BTreeMap<ValueId, BTreeSet<usize>>,
    /// Union-find over observed values, for canonical identity after a union.
    parent: BTreeMap<ValueId, ValueId>,
}

impl CellRecognizer {
    pub fn new(plans: Vec<CellRecognitionPlan>) -> Self {
        let mut steps_by_rule: BTreeMap<RuleIdentity, Vec<StepRef>> = BTreeMap::new();
        let mut unsupported = vec![];
        for (plan_index, plan) in plans.iter().enumerate() {
            let boundary_len = plan.slots.boundary.len();
            for (source_index, source) in plan.sources.iter().enumerate() {
                // Every step must declare, for each of its external ordinals, one
                // instance boundary position; and every declared position must be
                // used, or two different instances could share a key. A source
                // that does not satisfy this is reported, never guessed.
                let problem = if source.external_slots.len() != source.steps.len() {
                    Some(format!(
                        "source has {} steps but {} boundary maps",
                        source.steps.len(),
                        source.external_slots.len()
                    ))
                } else if source
                    .external_slots
                    .iter()
                    .flatten()
                    .filter(|p| **p != usize::MAX)
                    .any(|p| *p >= boundary_len)
                {
                    Some("a step external maps outside the declared boundary".to_string())
                } else {
                    // A source need not use every declared position: the plan
                    // boundary is shared by all its sources. An unused position
                    // stays unbound and can never distinguish two instances of
                    // this source, because nothing binds it.
                    None
                };
                if let Some(reason) = problem {
                    unsupported.push((plan_index, source_index, reason));
                    continue;
                }
                for (position, node) in source.steps.iter().enumerate() {
                    let Some(step) = plan.graph.node(*node) else {
                        continue;
                    };
                    steps_by_rule
                        .entry(step.step.rule.clone())
                        .or_default()
                        .push(StepRef {
                            plan: plan_index,
                            source: source_index,
                            position,
                            node: *node,
                            step: step.step.clone(),
                        });
                }
            }
        }
        Self {
            plans,
            steps_by_rule,
            unsupported,
            instances: vec![],
            by_key: BTreeMap::new(),
            waiting: BTreeMap::new(),
            value_index: BTreeMap::new(),
            parent: BTreeMap::new(),
        }
    }

    pub fn unsupported_sources(&self) -> &[(usize, usize, String)] {
        &self.unsupported
    }

    fn canonical(&mut self, value: ValueId) -> ValueId {
        let mut root = value;
        while let Some(next) = self.parent.get(&root) {
            if *next == root {
                break;
            }
            root = *next;
        }
        let mut cursor = value;
        while cursor != root {
            let next = *self.parent.get(&cursor).unwrap_or(&root);
            self.parent.insert(cursor, root);
            cursor = next;
        }
        root
    }

    fn create(&mut self, plan: usize, source: usize, boundary: Vec<Option<ValueId>>) -> usize {
        let mut instance = Instance {
            key: InstanceKey {
                plan,
                source,
                boundary: vec![],
            },
            state: InstanceState::Located,
            located: BTreeSet::new(),
            outputs: BTreeMap::new(),
            boundary,
            retired: false,
        };
        instance.key = instance.key_from_boundary();
        if let Some(id) = self.by_key.get(&instance.key) {
            return *id;
        }
        let key = instance.key.clone();
        let id = self.instances.len();
        self.instances.push(instance);
        self.by_key.insert(key, id);
        id
    }

    fn index_values(&mut self, id: usize) {
        let values: BTreeSet<ValueId> = self.instances[id]
            .boundary
            .iter()
            .flatten()
            .chain(self.instances[id].outputs.values())
            .copied()
            .collect();
        for value in values {
            self.value_index.entry(value).or_default().insert(id);
        }
    }

    /// Handle one committed event.
    pub fn observe(&mut self, event: CommittedEvent<'_>) -> Result<()> {
        match event {
            CommittedEvent::Apply {
                rule,
                wanted,
                outputs,
                external,
                ..
            } => self.observe_apply(rule, wanted, outputs, external),
            CommittedEvent::Fact { table, row } => {
                self.observe_fact(table, row);
                Ok(())
            }
            CommittedEvent::Union { lhs, rhs } => {
                self.observe_union(lhs, rhs);
                Ok(())
            }
            CommittedEvent::Invalidate { values } => {
                self.observe_invalidate(values);
                Ok(())
            }
        }
    }

    fn observe_apply(
        &mut self,
        rule: &RuleIdentity,
        wanted: &[ValueId],
        outputs: &[ValueId],
        _external: &[ValueId],
    ) -> Result<()> {
        let steps = match self.steps_by_rule.get(rule) {
            Some(s) => s.clone(),
            None => return Ok(()),
        };
        // Canonicalise every observed value once, so equality after a union is
        // decided in one namespace for the whole event.
        let wanted: Vec<ValueId> = wanted.iter().map(|v| self.canonical(*v)).collect();
        let outputs: Vec<ValueId> = outputs.iter().map(|v| self.canonical(*v)).collect();
        for step_ref in steps {
            let Some(slots) = self.plans[step_ref.plan]
                .sources
                .get(step_ref.source)
                .and_then(|s| s.external_slots.get(step_ref.position))
                .cloned()
            else {
                continue;
            };
            if step_ref.step.input_count != wanted.len()
                || step_ref.step.output_count != outputs.len()
            {
                continue;
            }
            let accepted: Vec<usize> = if step_ref.step.parents.is_empty() {
                match self.root_instance(&step_ref, &slots, &wanted) {
                    Some(id) => vec![id],
                    None => vec![],
                }
            } else {
                let ids: Vec<usize> = self
                    .instances
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| {
                        !i.retired
                            && i.key.plan == step_ref.plan
                            && i.key.source == step_ref.source
                    })
                    .filter(|(_, i)| {
                        step_ref.step.parents.iter().all(|p| {
                            self.plans[step_ref.plan]
                                .sources
                                .get(step_ref.source)
                                .and_then(|s| s.steps.get(*p))
                                .is_some_and(|n| i.located.contains(n))
                        })
                    })
                    .map(|(id, _)| id)
                    .collect();
                ids.into_iter()
                    .filter(|id| self.try_extend(*id, &step_ref, &slots, &wanted, &outputs))
                    .collect()
            };
            for id in accepted {
                self.bind_outputs(id, &step_ref, &outputs);
                self.subscribe(id, step_ref.plan);
            }
        }
        Ok(())
    }

    /// A root step locates a fresh instance. Nothing is created when the observed
    /// external values contradict an existing boundary.
    fn root_instance(
        &mut self,
        step_ref: &StepRef,
        slots: &[usize],
        wanted: &[ValueId],
    ) -> Option<usize> {
        let mut boundary = vec![None; self.plans[step_ref.plan].slots.boundary.len()];
        for (input, binding) in step_ref.step.binding.iter().enumerate() {
            if let RelativeBinding::External { slot } = binding {
                let boundary_slot = *slots.get(*slot)?;
                let value = *wanted.get(input)?;
                match boundary[boundary_slot] {
                    Some(old) if old != value => return None,
                    _ => boundary[boundary_slot] = Some(value),
                }
            }
        }
        Some(self.create(step_ref.plan, step_ref.source, boundary))
    }

    /// Unify one observed application with one instance. No partial mutation:
    /// the extension is computed first and committed only if it is consistent.
    fn try_extend(
        &mut self,
        id: usize,
        step_ref: &StepRef,
        slots: &[usize],
        wanted: &[ValueId],
        outputs: &[ValueId],
    ) -> bool {
        let mut boundary = self.instances[id].boundary.clone();
        for (input, binding) in step_ref.step.binding.iter().enumerate() {
            if let RelativeBinding::External { slot } = binding {
                let Some(boundary_slot) = slots.get(*slot).copied() else {
                    return false;
                };
                let value = wanted[input];
                match boundary[boundary_slot] {
                    Some(old) if old != value => return false,
                    _ => boundary[boundary_slot] = Some(value),
                }
            }
        }
        for (input, binding) in step_ref.step.binding.iter().enumerate() {
            if let RelativeBinding::ParentPort { parent, output } = binding {
                let Some(parent_node) = self.plans[step_ref.plan]
                    .sources
                    .get(step_ref.source)
                    .and_then(|s| s.steps.get(*parent))
                else {
                    return false;
                };
                match self.instances[id].outputs.get(&(*parent_node, *output)) {
                    Some(old) if *old == wanted[input] => {}
                    _ => return false,
                }
            }
        }
        let instance = &mut self.instances[id];
        instance.boundary = boundary;
        for (port, value) in outputs.iter().enumerate() {
            instance.outputs.insert((step_ref.node, port), *value);
        }
        instance.located.insert(step_ref.node);
        let key = instance.key_from_boundary();
        if key != instance.key {
            self.by_key.remove(&instance.key.clone());
            instance.key = key.clone();
            self.by_key.insert(key, id);
        }
        true
    }

    fn bind_outputs(&mut self, id: usize, step_ref: &StepRef, outputs: &[ValueId]) {
        let instance = &mut self.instances[id];
        for (port, value) in outputs.iter().enumerate() {
            instance.outputs.insert((step_ref.node, port), *value);
        }
        instance.located.insert(step_ref.node);
        let key = instance.key_from_boundary();
        if key != instance.key {
            self.by_key.remove(&instance.key.clone());
            self.instances[id].key = key.clone();
            self.by_key.insert(key, id);
        }
        self.index_values(id);
    }

    /// Subscribe one instance to the template rows whose keys are already fixed
    /// by its boundary binding. Derived rows are resolved at confirmation time.
    fn subscribe(&mut self, id: usize, plan_index: usize) {
        let plan = &self.plans[plan_index];
        let mut pending = vec![];
        for row in &plan.slots.template {
            let mut key = Vec::with_capacity(row.columns.len().saturating_sub(1));
            let mut known = true;
            for column in row.columns.iter().take(row.columns.len().saturating_sub(1)) {
                match plan.slots.slots[*column].origin {
                    SlotOrigin::Boundary => {
                        // A library recipe's boundary positions are recognition
                        // only; a body-value column is never resolved from them.
                        if plan.slots.boundary_space != BoundarySpace::Slots {
                            known = false;
                            break;
                        }
                        let position = plan
                            .slots
                            .boundary
                            .iter()
                            .position(|s| s == column)
                            .unwrap_or(usize::MAX);
                        match self.instances[id].boundary.get(position).copied().flatten() {
                            Some(v) => key.push(v),
                            None => {
                                known = false;
                                break;
                            }
                        }
                    }
                    SlotOrigin::Literal => key.push(u64::MAX),
                    SlotOrigin::Derived => {
                        known = false;
                        break;
                    }
                }
            }
            if known {
                pending.push((row.function.clone(), key));
            }
        }
        for (function, key) in pending {
            self.waiting
                .entry((plan_index, function, key))
                .or_default()
                .insert(id);
        }
    }

    fn observe_fact(&mut self, table: &str, row: &[ValueId]) {
        if row.is_empty() {
            return;
        }
        let row: Vec<ValueId> = row.iter().map(|v| self.canonical(*v)).collect();
        let key = row[..row.len() - 1].to_vec();
        let satisfied: Vec<(usize, String, Vec<ValueId>)> = self
            .waiting
            .keys()
            .filter(|(_, function, k)| function == table && *k == key)
            .cloned()
            .collect();
        for entry in satisfied {
            self.waiting.remove(&entry);
        }
    }

    fn observe_union(&mut self, lhs: ValueId, rhs: ValueId) {
        let (lhs, rhs) = (self.canonical(lhs), self.canonical(rhs));
        if lhs == rhs {
            return;
        }
        let keep = lhs.min(rhs);
        let drop = lhs.max(rhs);
        self.parent.insert(drop, keep);
        // Renormalise every live instance onto the canonical value.
        for id in 0..self.instances.len() {
            if self.instances[id].retired {
                continue;
            }
            for value in self.instances[id].boundary.iter_mut() {
                if *value == Some(drop) {
                    *value = Some(keep);
                }
            }
            for value in self.instances[id].outputs.values_mut() {
                if *value == drop {
                    *value = keep;
                }
            }
        }
        // Rebuild the identity index. The earliest instance wins, so a merge is
        // deterministic and no located step is lost.
        let mut by_key: BTreeMap<InstanceKey, usize> = BTreeMap::new();
        let mut merges = vec![];
        for id in 0..self.instances.len() {
            if self.instances[id].retired {
                continue;
            }
            let key = self.instances[id].key_from_boundary();
            self.instances[id].key = key.clone();
            match by_key.get(&key).copied() {
                Some(first) if first != id => merges.push((first, id)),
                _ => {
                    by_key.insert(key, id);
                }
            }
        }
        for (first, duplicate) in merges {
            let located = self.instances[duplicate].located.clone();
            let outputs = self.instances[duplicate].outputs.clone();
            let duplicate_state = self.instances[duplicate].state;
            let first_state = self.instances[first].state;
            let merged_state = if first_state == InstanceState::Complete
                || duplicate_state == InstanceState::Complete
            {
                InstanceState::Complete
            } else {
                first_state
            };
            let first_instance = &mut self.instances[first];
            first_instance.located.extend(located);
            first_instance.outputs.extend(outputs);
            first_instance.state = merged_state;
            self.instances[duplicate].retired = true;
            self.instances[duplicate].located.clear();
        }
        self.by_key = by_key;
        self.value_index.clear();
        for id in 0..self.instances.len() {
            if !self.instances[id].retired {
                self.index_values(id);
            }
        }
    }

    fn observe_invalidate(&mut self, values: &[ValueId]) {
        for value in values {
            let value = self.canonical(*value);
            let affected: Vec<usize> = self
                .value_index
                .get(&value)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default();
            for id in affected {
                // Withdraw completeness: the evidence that supported it is gone,
                // so the instance must be re-confirmed rather than reused.
                if self.instances[id].state == InstanceState::Complete {
                    self.instances[id].state = InstanceState::Located;
                }
            }
        }
    }

    /// Confirm template rows by point lookup and promote instances to
    /// `Complete`. Returns how many instances became complete.
    pub fn confirm(&mut self, oracle: &impl FactOracle) -> usize {
        let mut promoted = 0;
        for id in 0..self.instances.len() {
            if self.instances[id].state == InstanceState::Packed || self.instances[id].retired {
                continue;
            }
            let plan_index = self.instances[id].key.plan;
            let Some(source) = self.plans[plan_index]
                .sources
                .get(self.instances[id].key.source)
                .cloned()
            else {
                continue;
            };
            if !self.instances[id].is_located(&source) {
                continue;
            }
            if self.rows_for(id, oracle).is_some() {
                self.instances[id].state = InstanceState::Complete;
                promoted += 1;
            }
        }
        promoted
    }

    /// Resolve every template row of one instance in dependency order, using
    /// only keyed lookups. `None` when some row is missing or unresolvable.
    fn rows_for(&self, id: usize, oracle: &impl FactOracle) -> Option<Vec<Vec<ValueId>>> {
        let instance = &self.instances[id];
        let plan = &self.plans[instance.key.plan];
        let mut slot_values: Vec<Option<ValueId>> =
            plan.slots
                .slots
                .iter()
                .enumerate()
                .map(|(i, slot)| match slot.origin {
                    SlotOrigin::Literal => Some(u64::MAX),
                    SlotOrigin::Boundary => {
                        if plan.slots.boundary_space != BoundarySpace::Slots {
                            None
                        } else {
                            plan.slots
                                .boundary
                                .iter()
                                .position(|s| *s == i)
                                .and_then(|p| instance.boundary.get(p).copied().flatten())
                        }
                    }
                    SlotOrigin::Derived => None,
                })
                .collect();
        let mut rows: Vec<Option<Vec<ValueId>>> = vec![None; plan.slots.template.len()];
        let mut pending: Vec<usize> = (0..plan.slots.template.len()).collect();
        while !pending.is_empty() {
            let mut later = vec![];
            let mut progressed = false;
            for row_index in pending {
                let row = &plan.slots.template[row_index];
                let result = row.columns.last().copied();
                // A row whose key is not fixed yet waits for another row to
                // resolve it, so template order need not be topological.
                let Some(key) = row.columns[..row.columns.len().saturating_sub(1)]
                    .iter()
                    .map(|c| slot_values[*c])
                    .collect::<Option<Vec<_>>>()
                else {
                    later.push(row_index);
                    continue;
                };
                let Some(full) = oracle.lookup(&row.function, &key) else {
                    return None;
                };
                if full.len() < row.columns.len() {
                    return None;
                }
                let logical = full[..row.columns.len()].to_vec();
                if let Some(result) = result {
                    slot_values[result] = logical.last().copied();
                }
                rows[row_index] = Some(logical);
                progressed = true;
            }
            if !progressed {
                return None;
            }
            pending = later;
        }
        rows.into_iter().collect()
    }

    /// Instances ready for compact storage, deduplicated by owned row so that
    /// overlapping cells never claim the same committed fact twice.
    pub fn complete(&self, oracle: &impl FactOracle) -> Vec<CompleteCell> {
        let mut owned: BTreeSet<(String, Vec<ValueId>)> = BTreeSet::new();
        let mut out = vec![];
        for (id, instance) in self.instances.iter().enumerate() {
            if instance.state != InstanceState::Complete || instance.retired {
                continue;
            }
            let Some(rows) = self.rows_for(id, oracle) else {
                continue;
            };
            let plan = &self.plans[instance.key.plan];
            let mut fresh = vec![];
            for (row, template) in rows.iter().zip(&plan.slots.template) {
                if owned.insert((
                    template.function.clone(),
                    row[..row.len().saturating_sub(1)].to_vec(),
                )) {
                    fresh.push(row.clone());
                }
            }
            if fresh.is_empty() {
                continue;
            }
            out.push(CompleteCell {
                key: instance.key.clone(),
                cell: plan.cell,
                template: plan.slots.template.clone(),
                rows: fresh,
            });
        }
        out
    }

    pub fn mark_packed(&mut self, keys: &[InstanceKey]) {
        for key in keys {
            if let Some(id) = self.by_key.get(key).copied() {
                self.instances[id].state = InstanceState::Packed;
            }
        }
    }

    pub fn counts(&self) -> RecognitionCounts {
        let mut counts = RecognitionCounts {
            unsupported_sources: self.unsupported.len(),
            ..Default::default()
        };
        for instance in &self.instances {
            if instance.retired {
                continue;
            }
            match instance.state {
                InstanceState::Located => counts.located += 1,
                InstanceState::Complete => counts.complete += 1,
                InstanceState::Packed => counts.packed += 1,
            }
        }
        counts
    }

    /// Row subscriptions still outstanding across all instances.
    pub fn pending_subscriptions(&self) -> usize {
        self.waiting.values().map(|s| s.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reuse_cell_recognition::{
        compile_bake, compile_library, EnvironmentId, LibraryBody, LibraryCellSource,
    };

    fn bake_plan(template: Vec<(&str, Vec<usize>)>, terms: Vec<&str>) -> CellRecognitionPlan {
        compile_bake(&[crate::reuse_cell_recognition::BakeCell {
            rule: 1,
            environment: vec!["(rule r)".into()],
            slots: terms
                .into_iter()
                .map(|t| (t.to_string(), "Math".to_string()))
                .collect(),
            template: template
                .into_iter()
                .map(|(f, c)| (f.to_string(), c))
                .collect(),
        }])
        .unwrap()
        .remove(0)
    }

    fn rule() -> RuleIdentity {
        RuleIdentity {
            environment: EnvironmentId::from_text(&["(rule r)".to_string()]),
            rule: "#1".into(),
        }
    }

    fn oracle(rows: &[(&str, Vec<ValueId>)]) -> FactMap {
        let mut map = FactMap::default();
        for (table, row) in rows {
            map.insert(table, row.clone());
        }
        map
    }

    #[test]
    fn a_trigger_alone_is_only_located() {
        let plan = bake_plan(
            vec![("Add", vec![0, 1, 2]), ("Mul", vec![2, 0, 3])],
            vec!["x", "y", "adaptive_local_0", "adaptive_local_1"],
        );
        let mut recogniser = CellRecognizer::new(vec![plan]);
        recogniser
            .observe(CommittedEvent::Apply {
                event: 1,
                rule: &rule(),
                wanted: &[10, 11],
                outputs: &[],
                external: &[10, 11],
            })
            .unwrap();
        assert_eq!(recogniser.counts().located, 1);
        let facts = oracle(&[("Add", vec![10, 11, 12])]);
        assert_eq!(recogniser.confirm(&facts), 0);
        assert!(recogniser.complete(&facts).is_empty());
    }

    #[test]
    fn a_complete_body_is_recognised_once_and_deduplicated() {
        let plan = bake_plan(
            vec![("Add", vec![0, 1, 2]), ("Mul", vec![2, 0, 3])],
            vec!["x", "y", "adaptive_local_0", "adaptive_local_1"],
        );
        let mut recogniser = CellRecognizer::new(vec![plan]);
        for event in [7u64, 7] {
            recogniser
                .observe(CommittedEvent::Apply {
                    event,
                    rule: &rule(),
                    wanted: &[10, 11],
                    outputs: &[],
                    external: &[10, 11],
                })
                .unwrap();
        }
        assert_eq!(recogniser.counts().located, 1, "a repeated apply is idempotent");
        let facts = oracle(&[("Add", vec![10, 11, 12]), ("Mul", vec![12, 10, 13])]);
        assert_eq!(recogniser.confirm(&facts), 1);
        let cells = recogniser.complete(&facts);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].rows, vec![vec![10, 11, 12], vec![12, 10, 13]]);
        // A drain is idempotent only once the caller records the installation.
        recogniser.mark_packed(&[cells[0].key.clone()]);
        assert!(recogniser.complete(&facts).is_empty());
    }

    #[test]
    fn unrelated_events_do_not_disturb_recognition() {
        let plan = bake_plan(
            vec![("Add", vec![0, 1, 2])],
            vec!["x", "y", "adaptive_local_0"],
        );
        let mut recogniser = CellRecognizer::new(vec![plan]);
        let other = RuleIdentity {
            environment: rule().environment,
            rule: "#999".into(),
        };
        recogniser
            .observe(CommittedEvent::Apply {
                event: 1,
                rule: &other,
                wanted: &[1, 2],
                outputs: &[],
                external: &[1, 2],
            })
            .unwrap();
        recogniser
            .observe(CommittedEvent::Fact {
                table: "Unrelated",
                row: &[1, 2, 3],
            })
            .unwrap();
        recogniser
            .observe(CommittedEvent::Apply {
                event: 2,
                rule: &rule(),
                wanted: &[10, 11],
                outputs: &[],
                external: &[10, 11],
            })
            .unwrap();
        let facts = oracle(&[("Add", vec![10, 11, 12])]);
        assert_eq!(recogniser.confirm(&facts), 1);
        assert_eq!(recogniser.complete(&facts).len(), 1);
    }

    #[test]
    fn union_merges_instances_that_become_equal() {
        let plan = bake_plan(
            vec![("Add", vec![0, 1, 2])],
            vec!["x", "y", "adaptive_local_0"],
        );
        let mut recogniser = CellRecognizer::new(vec![plan]);
        for (event, x) in [(1u64, 10u64), (2, 20)] {
            recogniser
                .observe(CommittedEvent::Apply {
                    event,
                    rule: &rule(),
                    wanted: &[x, 11],
                    outputs: &[],
                    external: &[x, 11],
                })
                .unwrap();
        }
        assert_eq!(recogniser.counts().located, 2);
        recogniser
            .observe(CommittedEvent::Union { lhs: 10, rhs: 20 })
            .unwrap();
        assert_eq!(recogniser.counts().located, 1, "a union merges the pair");
    }

    #[test]
    fn invalidation_withdraws_completeness() {
        let plan = bake_plan(
            vec![("Add", vec![0, 1, 2])],
            vec!["x", "y", "adaptive_local_0"],
        );
        let mut recogniser = CellRecognizer::new(vec![plan]);
        recogniser
            .observe(CommittedEvent::Apply {
                event: 1,
                rule: &rule(),
                wanted: &[10, 11],
                outputs: &[],
                external: &[10, 11],
            })
            .unwrap();
        let facts = oracle(&[("Add", vec![10, 11, 12])]);
        assert_eq!(recogniser.confirm(&facts), 1);
        assert_eq!(recogniser.counts().complete, 1);
        recogniser
            .observe(CommittedEvent::Invalidate { values: &[10] })
            .unwrap();
        assert_eq!(recogniser.counts().complete, 0);
        assert_eq!(recogniser.counts().located, 1);
        assert!(recogniser.complete(&facts).is_empty());
    }

    #[test]
    fn a_packed_instance_is_not_offered_again() {
        let plan = bake_plan(
            vec![("Add", vec![0, 1, 2])],
            vec!["x", "y", "adaptive_local_0"],
        );
        let mut recogniser = CellRecognizer::new(vec![plan]);
        recogniser
            .observe(CommittedEvent::Apply {
                event: 1,
                rule: &rule(),
                wanted: &[10, 11],
                outputs: &[],
                external: &[10, 11],
            })
            .unwrap();
        let facts = oracle(&[("Add", vec![10, 11, 12])]);
        recogniser.confirm(&facts);
        let cells = recogniser.complete(&facts);
        recogniser.mark_packed(&[cells[0].key.clone()]);
        assert_eq!(recogniser.counts().packed, 1);
        assert!(recogniser.complete(&facts).is_empty());
        assert_eq!(recogniser.confirm(&facts), 0);
    }

    fn multi_step_plan() -> CellRecognitionPlan {
        use crate::coarse_smooth::{Comb, CombKind, Effect, RelativeBinding};
        let step = |rule: &str, parents: Vec<usize>, binding: Vec<RelativeBinding>| Comb {
            kind: if parents.is_empty() {
                CombKind::CoarseRuleComposition
            } else {
                CombKind::SmoothRuleComposition
            },
            rule: rule.into(),
            input_roles: vec!["var:x".into()],
            output_roles: vec!["var:y".into()],
            parents,
            binding,
            aliases: vec![0, 1, 2],
            input_count: 1,
            output_count: 1,
            external_count: 1,
            requirements: vec![],
            effects: vec![Effect::RowFact(0)],
            external_facts: vec![],
        };
        let body = LibraryBody {
            body: 0,
            values: vec![("Math".into(), None), ("Math".into(), None)],
            rows: vec![("A".into(), vec![0, 1])],
            sources: vec![LibraryCellSource {
                enclosure: 0,
                interface: vec![0],
                recipe: vec![
                    step("r0", vec![], vec![RelativeBinding::External { slot: 0 }]),
                    step(
                        "r1",
                        vec![0],
                        vec![RelativeBinding::ParentPort {
                            parent: 0,
                            output: 0,
                        }],
                    ),
                ],
                wiring: vec![],
                boundary_body_values: vec![],
            }],
        };
        compile_library(EnvironmentId::from_text(&["(env)".into()]), &[body])
            .unwrap()
            .remove(0)
    }

    #[test]
    fn a_multi_step_source_needs_both_parent_applications() {
        let plan = multi_step_plan();
        let environment = plan.environment;
        let mut recogniser = CellRecognizer::new(vec![plan]);
        let step0 = RuleIdentity {
            environment,
            rule: "r0".into(),
        };
        let step1 = RuleIdentity {
            environment,
            rule: "r1".into(),
        };
        // The child application arrives first: it cannot locate anything yet.
        recogniser
            .observe(CommittedEvent::Apply {
                event: 1,
                rule: &step1,
                wanted: &[20],
                outputs: &[30],
                external: &[],
            })
            .unwrap();
        assert_eq!(recogniser.counts().located, 0);
        // The root application locates the instance but the source is unfinished.
        recogniser
            .observe(CommittedEvent::Apply {
                event: 2,
                rule: &step0,
                wanted: &[10],
                outputs: &[20],
                external: &[10],
            })
            .unwrap();
        let facts = oracle(&[("A", vec![10, 20])]);
        assert_eq!(recogniser.confirm(&facts), 0, "the child step is missing");
        // Replaying the child now extends the located instance.
        recogniser
            .observe(CommittedEvent::Apply {
                event: 3,
                rule: &step1,
                wanted: &[20],
                outputs: &[30],
                external: &[],
            })
            .unwrap();
        assert!(
            recogniser.unsupported_sources().is_empty(),
            "{:?}",
            recogniser.unsupported_sources()
        );
        assert_eq!(
            recogniser.counts().located,
            1,
            "both applications are evidenced"
        );
        // A library recipe is recognised from its steps, but its compact template
        // still cannot be confirmed: the artifacts do not record how recipe
        // symbols map onto stored body values, so confirmation stays closed
        // rather than resolving a body column from a recognition position.
        assert_eq!(recogniser.confirm(&facts), 0);
        assert!(recogniser.complete(&facts).is_empty());
    }

    /// A two-step library source whose boundary terms are recorded, so the
    /// boundary is a storage-slot space and both steps share one boundary port.
    fn wired_library_plan() -> CellRecognitionPlan {
        use crate::coarse_smooth::{Comb, CombKind, Effect, RelativeBinding};
        let step = |rule: &str, parents: Vec<usize>, binding: Vec<RelativeBinding>| Comb {
            kind: if parents.is_empty() {
                CombKind::CoarseRuleComposition
            } else {
                CombKind::SmoothRuleComposition
            },
            rule: rule.into(),
            input_roles: vec!["var:x".into()],
            output_roles: vec!["var:y".into()],
            parents,
            binding,
            aliases: vec![0, 1, 2],
            input_count: 1,
            output_count: 1,
            external_count: 1,
            requirements: vec![],
            effects: vec![Effect::RowFact(0)],
            external_facts: vec![],
        };
        let body = LibraryBody {
            body: 1,
            values: vec![
                ("Math".into(), None),
                ("Math".into(), None),
                ("Math".into(), None),
            ],
            rows: vec![("A".into(), vec![0, 1]), ("B".into(), vec![1, 0, 2])],
            sources: vec![LibraryCellSource {
                enclosure: 0,
                interface: vec![0],
                recipe: vec![
                    step("r0", vec![], vec![RelativeBinding::External { slot: 0 }]),
                    step("r1", vec![0], vec![RelativeBinding::External { slot: 0 }]),
                ],
                // Both steps consume canonical port 7, which denotes body value 0.
                wiring: vec![vec![Some(7)], vec![Some(7)]],
                boundary_body_values: vec![(7, 0)],
            }],
        };
        compile_library(EnvironmentId::from_text(&["(env)".into()]), &[body])
            .unwrap()
            .remove(0)
    }

    #[test]
    fn a_recorded_boundary_term_lets_a_library_cell_confirm() {
        let plan = wired_library_plan();
        assert_eq!(
            plan.slots.boundary_space,
            crate::reuse_cell_recognition::BoundarySpace::Slots
        );
        assert_eq!(plan.slots.boundary, vec![0]);
        let environment = plan.environment;
        let mut recogniser = CellRecognizer::new(vec![plan]);
        let step0 = RuleIdentity {
            environment,
            rule: "r0".into(),
        };
        let step1 = RuleIdentity {
            environment,
            rule: "r1".into(),
        };
        recogniser
            .observe(CommittedEvent::Apply {
                event: 1,
                rule: &step0,
                wanted: &[10],
                outputs: &[20],
                external: &[10],
            })
            .unwrap();
        // The second step consumes the same boundary value, so it extends the
        // same instance rather than creating a second one.
        recogniser
            .observe(CommittedEvent::Apply {
                event: 2,
                rule: &step1,
                wanted: &[10],
                outputs: &[30],
                external: &[10],
            })
            .unwrap();
        assert_eq!(recogniser.counts().located, 1);
        assert!(
            recogniser.unsupported_sources().is_empty(),
            "{:?}",
            recogniser.unsupported_sources()
        );
        let facts = oracle(&[("A", vec![10, 20]), ("B", vec![20, 10, 30])]);
        assert_eq!(recogniser.confirm(&facts), 1);
        let cells = recogniser.complete(&facts);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].rows, vec![vec![10, 20], vec![20, 10, 30]]);
    }

    #[test]
    fn a_boundary_mismatch_is_reported_not_guessed() {
        let plan = bake_plan(
            vec![("Add", vec![0, 1, 2])],
            vec!["x", "y", "adaptive_local_0"],
        );
        let mut plan = plan;
        plan.slots.boundary.pop();
        let recogniser = CellRecognizer::new(vec![plan]);
        assert_eq!(recogniser.unsupported_sources().len(), 1);
        assert_eq!(recogniser.counts().unsupported_sources, 1);
    }
}
