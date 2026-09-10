//! Diagnostic logical coverage over a declared live constructor-row universe.
//! Prefix views overlap; their native storage owners do not change.
use crate::dependency_blocks::{BlockId, DependencyBlockStore};
use egglog::{EGraph, TraceSession, Value, WriteOutcome};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
pub type FactId = usize;
pub type ViewId = usize;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FactKey {
    pub table: String,
    pub fields: Vec<Value>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoverageState {
    Covered,
    SeedOnly,
    Unobserved,
    Unsupported,
    Invalidated,
    Retired,
}
#[derive(Clone, Debug)]
pub struct Fact {
    pub id: FactId,
    pub key: FactKey,
    pub live: bool,
    /// Logical row payload only, not allocator memory or compressed bytes.
    pub payload_bytes: usize,
    /// Actual storage stays in this native function table; views never move it.
    pub native_owner: String,
    pub seeded: bool,
    pub unsupported: BTreeSet<String>,
    pub last_examined: Option<u64>,
    ever_covered: bool,
}
#[derive(Clone, Debug)]
pub struct PrefixView {
    pub id: ViewId,
    pub block: BlockId,
    /// Some(i) is a certified combined prefix; None is an entry-level view of an
    /// opaque block (no composed shortcut), covering the rows its members touched.
    pub prefix_index: Option<usize>,
    pub lhs: String,
    pub rhs: String,
    pub facts: BTreeSet<FactId>,
    pub active: bool,
    pub invalid_reason: Option<String>,
}
#[derive(Default, Debug)]
pub struct CoverageStats {
    pub live_facts: usize,
    pub covered_facts: usize,
    pub payload_bytes: usize,
    pub covered_payload_bytes: usize,
    pub active_memberships: usize,
    pub duplicated_memberships: usize,
    pub states: BTreeMap<CoverageState, usize>,
}
#[derive(Default)]
pub struct CoverageLedger {
    domain: BTreeSet<String>,
    widths: HashMap<String, usize>,
    live: HashMap<FactKey, FactId>,
    facts: Vec<Fact>,
    views: Vec<PrefixView>,
    view_keys: HashMap<(BlockId, Option<usize>), ViewId>,
    memberships: HashMap<FactId, BTreeSet<ViewId>>,
    session: Option<TraceSession>,
    epoch: u64,
    tick: u64,
}
impl CoverageLedger {
    pub fn new(functions: impl IntoIterator<Item = String>) -> Self {
        Self {
            domain: functions.into_iter().collect(),
            ..Default::default()
        }
    }
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }
    pub fn views(&self) -> &[PrefixView] {
        &self.views
    }
    pub fn state(&self, id: FactId) -> CoverageState {
        let f = &self.facts[id];
        if !f.live {
            return CoverageState::Retired;
        }
        if self
            .memberships
            .get(&id)
            .into_iter()
            .flatten()
            .any(|v| self.views[*v].active)
        {
            CoverageState::Covered
        } else if f.ever_covered {
            CoverageState::Invalidated
        } else if !f.unsupported.is_empty() {
            CoverageState::Unsupported
        } else if f.seeded {
            CoverageState::SeedOnly
        } else {
            CoverageState::Unobserved
        }
    }
    pub fn memberships(&self, id: FactId) -> Vec<ViewId> {
        self.memberships
            .get(&id)
            .into_iter()
            .flatten()
            .copied()
            .filter(|v| self.views[*v].active)
            .collect()
    }
    pub fn invalidate_view(&mut self, id: ViewId, reason: &str) {
        self.views[id].active = false;
        self.views[id].invalid_reason = Some(reason.into());
    }
    pub fn mark_examined(&mut self, id: FactId) {
        self.tick += 1;
        self.facts[id].last_examined = Some(self.tick);
    }
    pub fn uncovered(&self) -> Vec<FactId> {
        self.facts
            .iter()
            .filter(|f| f.live && self.state(f.id) != CoverageState::Covered)
            .map(|f| f.id)
            .collect()
    }
    /// Refresh the whole declared universe. This is an inventory scan, not prefix discovery.
    pub fn refresh(&mut self, eg: &EGraph) -> Result<(), String> {
        let mut inventory = Vec::new();
        let mut widths = HashMap::new();
        for name in &self.domain {
            let f = eg
                .get_function(name)
                .ok_or_else(|| format!("unknown domain function {name}"))?;
            widths.insert(name.clone(), f.schema().input.len() + 1);
            eg.function_for_each(name, |r| {
                if !r.subsumed {
                    // Keep only the schema columns (input + output); the native
                    // row carries extra trailing columns (timestamp/subsume) that
                    // are not part of the logical fact identity.
                    let n = f.schema().input.len() + 1;
                    let fields: Vec<_> = r.vals[..r.vals.len().min(n)].to_vec();
                    inventory.push(FactKey {
                        table: name.clone(),
                        fields,
                    });
                }
            })
            .map_err(|e| e.to_string())?;
        }
        self.widths = widths;
        self.epoch += 1;
        let next: HashSet<_> = inventory.iter().cloned().collect();
        let removed: Vec<_> = self
            .live
            .keys()
            .filter(|k| !next.contains(*k))
            .cloned()
            .collect();
        for key in removed {
            let id = self.live.remove(&key).unwrap();
            self.facts[id].live = false;
            for v in self.memberships.get(&id).cloned().unwrap_or_default() {
                self.invalidate_view(v, "member row retired or changed");
            }
        }
        for key in inventory {
            if self.live.contains_key(&key) {
                continue;
            }
            let id = self.facts.len();
            self.facts.push(Fact {
                id,
                native_owner: key.table.clone(),
                payload_bytes: key.fields.len() * std::mem::size_of::<Value>(),
                key: key.clone(),
                live: true,
                seeded: false,
                unsupported: BTreeSet::new(),
                last_examined: None,
                ever_covered: false,
            });
            self.live.insert(key, id);
        }
        Ok(())
    }
    fn row_id(&self, eg: &EGraph, table: &str, row: &[Value]) -> Option<FactId> {
        let n = *self.widths.get(table)?;
        if row.len() < n {
            return None;
        }
        let Some(f) = eg.get_function(table) else {
            return None;
        };
        // Canonicalize eq-sort columns so event rows recorded before a union match
        // the post-union canonical fact inventory.
        let fields: Vec<_> = row[..n]
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let sort = if i < f.schema().input.len() {
                    &f.schema().input[i]
                } else {
                    &f.schema().output
                };
                if sort.is_eq_sort() {
                    eg.get_canonical_value(v, sort)
                } else {
                    v
                }
            })
            .collect();
        self.live
            .get(&FactKey {
                table: table.into(),
                fields,
            })
            .copied()
    }
    /// Admit views only from checked block prefixes. Call after store.ingest_committed.
    pub fn sync_blocks(
        &mut self,
        eg: &EGraph,
        store: &DependencyBlockStore,
        trace: &TraceSession,
    ) -> Result<(), String> {
        if !store.uses_trace(trace) {
            return Err("block store and trace do not share a session".into());
        }
        if let Some(s) = &self.session {
            if !s.same_session(trace) {
                return Err("coverage cannot mix trace sessions".into());
            }
        } else {
            self.session = Some(trace.clone());
        }
        self.refresh(eg)?;
        let names: HashMap<_, _> = trace
            .table_names()
            .into_iter()
            .map(|(id, n)| (id, n.to_string()))
            .collect();
        let writes = trace.write_events();
        let reads = trace.row_reads();
        let mut by_match: HashMap<u64, BTreeSet<FactId>> = HashMap::new();
        let mut unresolved = HashSet::new();
        for r in &reads {
            if let Some(name) = r.table_name.as_deref() {
                if let Some(id) = self.row_id(eg, name, &r.row) {
                    by_match.entry(r.match_event_id).or_default().insert(id);
                    self.facts[id].seeded = true;
                    if let Some(reason) = store.rejected().get(&r.match_event_id) {
                        self.facts[id].unsupported.insert(reason.clone());
                    }
                } else {
                    unresolved.insert(r.match_event_id);
                }
            } else {
                unresolved.insert(r.match_event_id);
            }
        }
        for w in &writes {
            if w.outcome != WriteOutcome::Inserted {
                continue;
            }
            if let Some(name) = names.get(&w.table) {
                if let Some(id) = self.row_id(eg, name, &w.actual) {
                    by_match.entry(w.match_event_id).or_default().insert(id);
                    self.facts[id].seeded = true;
                    if let Some(reason) = store.rejected().get(&w.match_event_id) {
                        self.facts[id].unsupported.insert(reason.clone());
                    }
                } else {
                    unresolved.insert(w.match_event_id);
                }
            } else {
                unresolved.insert(w.match_event_id);
            }
        }
        for b in store.blocks() {
            if b.prefixes.is_empty() {
                // Opaque block: admit an entry-level view over the rows its members
                // touched. This observes coverage but certifies no equality shortcut.
                self.admit_view(
                    b.id,
                    None,
                    &b.entry.lhs,
                    "",
                    &b.members,
                    b.active,
                    &by_match,
                    &unresolved,
                );
            } else {
                for (pi, p) in b.prefixes.iter().enumerate() {
                    self.admit_view(
                        b.id,
                        Some(pi),
                        &p.lhs,
                        &p.rhs,
                        &p.members,
                        b.active && p.usable,
                        &by_match,
                        &unresolved,
                    );
                }
            }
        }
        Ok(())
    }

    fn admit_view(
        &mut self,
        block: BlockId,
        prefix_index: Option<usize>,
        lhs: &str,
        rhs: &str,
        members: &[u64],
        active: bool,
        by_match: &HashMap<u64, BTreeSet<FactId>>,
        unresolved: &HashSet<u64>,
    ) {
        let key = (block, prefix_index);
        let valid = active && !members.iter().any(|m| unresolved.contains(m));
        if let Some(id) = self.view_keys.get(&key).copied() {
            if !valid {
                self.invalidate_view(id, "block/prefix or a concrete row is no longer valid");
            }
            return;
        }
        if !valid {
            return;
        }
        let facts: BTreeSet<_> = members
            .iter()
            .flat_map(|m| by_match.get(m).into_iter().flatten().copied())
            .collect();
        if facts.is_empty() {
            return;
        }
        let id = self.views.len();
        self.views.push(PrefixView {
            id,
            block,
            prefix_index,
            lhs: lhs.to_string(),
            rhs: rhs.to_string(),
            facts: facts.clone(),
            active: true,
            invalid_reason: None,
        });
        self.view_keys.insert(key, id);
        for fact in facts {
            self.memberships.entry(fact).or_default().insert(id);
            self.facts[fact].ever_covered = true;
        }
    }
    pub fn marginal_new_facts(&self, view: ViewId, selected: &[ViewId]) -> usize {
        let mut covered = BTreeSet::new();
        for v in selected {
            if self.views[*v].active {
                covered.extend(
                    self.views[*v]
                        .facts
                        .iter()
                        .copied()
                        .filter(|f| self.facts[*f].live),
                );
            }
        }
        if !self.views[view].active {
            return 0;
        }
        self.views[view]
            .facts
            .iter()
            .filter(|f| self.facts[**f].live && !covered.contains(*f))
            .count()
    }
    pub fn stats(&self) -> CoverageStats {
        let mut s = CoverageStats::default();
        for f in self.facts.iter().filter(|f| f.live) {
            let state = self.state(f.id);
            *s.states.entry(state).or_default() += 1;
            s.live_facts += 1;
            s.payload_bytes += f.payload_bytes;
            let count = self.memberships(f.id).len();
            s.active_memberships += count;
            if count > 0 {
                s.covered_facts += 1;
                s.covered_payload_bytes += f.payload_bytes;
            }
            s.duplicated_memberships += count.saturating_sub(1);
        }
        s
    }
}
