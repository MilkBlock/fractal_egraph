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
    pub prefix_index: usize,
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
    view_keys: HashMap<(BlockId, usize), ViewId>,
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
                    inventory.push(FactKey {
                        table: name.clone(),
                        fields: r.vals.to_vec(),
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
    fn row_id(&self, table: &str, row: &[Value]) -> Option<FactId> {
        let n = *self.widths.get(table)?;
        if row.len() < n {
            return None;
        }
        self.live
            .get(&FactKey {
                table: table.into(),
                fields: row[..n].to_vec(),
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
                if let Some(id) = self.row_id(name, &r.row) {
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
                if let Some(id) = self.row_id(name, &w.actual) {
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
            for (pi, p) in b.prefixes.iter().enumerate() {
                let key = (b.id, pi);
                let valid =
                    b.active && p.usable && !p.members.iter().any(|m| unresolved.contains(m));
                if let Some(id) = self.view_keys.get(&key).copied() {
                    if !valid {
                        self.invalidate_view(
                            id,
                            "block/prefix or a concrete row is no longer valid",
                        );
                    }
                    continue;
                }
                if !valid {
                    continue;
                }
                let facts: BTreeSet<_> = p
                    .members
                    .iter()
                    .flat_map(|m| by_match.get(m).into_iter().flatten().copied())
                    .collect();
                if facts.is_empty() {
                    continue;
                }
                let id = self.views.len();
                self.views.push(PrefixView {
                    id,
                    block: b.id,
                    prefix_index: pi,
                    lhs: p.lhs.clone(),
                    rhs: p.rhs.clone(),
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
        }
        Ok(())
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
