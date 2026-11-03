//! Union causes and exact, publication-scoped two-atom Spine entry programs.
//! Causes explain a selected equality proof, not a shortest syntactic distance.
use crate::{
    graph::{FactRef, Graph, Row},
    program::{Arg, Call, Program, Result},
    value::Value,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Cause {
    Apply {
        rule: usize,
        frame: u32,
        op: usize,
    },
    Congruence {
        left: FactRef,
        right: FactRef,
        parents: Vec<usize>,
        complete: bool,
    },
    LocalCongruence,
    External,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub epoch: u64,
    pub left: u64,
    pub right: u64,
    pub representative: u64,
    pub requested: [u64; 2],
    pub canonicalization_parents: Vec<usize>,
    pub canonicalization_complete: bool,
    pub depth: Option<u32>,
    pub cause: Cause,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Lineage {
    pub events: Vec<Event>,
    pub parent_causes: BTreeMap<u64, usize>,
}
impl Lineage {
    pub fn validate(&self, parents: &BTreeMap<u64, u64>) -> Result<()> {
        for (i, e) in self.events.iter().enumerate() {
            if e.left == e.right || e.representative != e.left.min(e.right) {
                return Err("invalid union lineage endpoints".into());
            }
            if e.canonicalization_parents.iter().any(|p| *p >= i) {
                return Err("forward union canonicalization cause".into());
            }
            let local = match &e.cause {
                Cause::Apply { .. } => Some(0),
                Cause::Congruence {
                    parents, complete, ..
                } => {
                    if parents.iter().any(|p| *p >= i) {
                        return Err("cyclic or forward union cause".into());
                    }
                    if *complete && !parents.is_empty() {
                        parents
                            .iter()
                            .map(|p| self.events[*p].depth)
                            .collect::<Option<Vec<_>>>()
                            .map(|ds| 1 + ds.into_iter().max().unwrap())
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let canonical = if e.canonicalization_complete {
                e.canonicalization_parents
                    .iter()
                    .map(|p| self.events[*p].depth)
                    .collect::<Option<Vec<_>>>()
                    .map(|ds| ds.into_iter().max().unwrap_or(0))
            } else {
                None
            };
            let expected = local.zip(canonical).map(|(a, b)| a.max(b));
            if e.depth != expected {
                return Err("union cause depth differs".into());
            }
        }
        for (&displaced, &i) in &self.parent_causes {
            let e = self.events.get(i).ok_or("missing union cause")?;
            if displaced != e.left.max(e.right)
                || parents.get(&displaced) != Some(&e.representative)
            {
                return Err("union cause/parent edge differs".into());
            }
        }
        Ok(())
    }
    pub fn record(
        &mut self,
        epoch: u64,
        requested: [u64; 2],
        a: u64,
        b: u64,
        cause: Cause,
        uf: &BTreeMap<u64, u64>,
    ) {
        let mut canonicalization_parents = BTreeSet::new();
        let mut canonicalization_complete = true;
        for (raw, canonical) in requested.into_iter().zip([a, b]) {
            if raw == canonical {
                continue;
            }
            if let Some(ids) = self.explain(raw, canonical, uf) {
                canonicalization_parents.extend(ids)
            } else {
                canonicalization_complete = false
            }
        }
        let local = match &cause {
            Cause::Apply { .. } => Some(0),
            Cause::Congruence {
                parents,
                complete: true,
                ..
            } if !parents.is_empty() => parents
                .iter()
                .map(|p| self.events[*p].depth)
                .collect::<Option<Vec<_>>>()
                .map(|ds| 1 + ds.into_iter().max().unwrap()),
            _ => None,
        };
        let canonical = if canonicalization_complete {
            canonicalization_parents
                .iter()
                .map(|p| self.events[*p].depth)
                .collect::<Option<Vec<_>>>()
                .map(|ds| ds.into_iter().max().unwrap_or(0))
        } else {
            None
        };
        let depth = local.zip(canonical).map(|(a, b)| a.max(b));
        let id = self.events.len();
        self.parent_causes.insert(a.max(b), id);
        self.events.push(Event {
            epoch,
            left: a,
            right: b,
            representative: a.min(b),
            requested,
            canonicalization_parents: canonicalization_parents.into_iter().collect(),
            canonicalization_complete,
            depth,
            cause,
        });
    }
    pub fn explain(&self, a: u64, b: u64, parents: &BTreeMap<u64, u64>) -> Option<Vec<usize>> {
        let mut path = vec![];
        let mut seen = BTreeMap::new();
        let mut x = a;
        loop {
            seen.insert(x, path.len());
            let Some(&next) = parents.get(&x) else { break };
            path.push(*self.parent_causes.get(&x)?);
            x = next;
        }
        let mut other = vec![];
        let mut x = b;
        loop {
            if let Some(&at) = seen.get(&x) {
                path.truncate(at);
                path.extend(other);
                path.sort_unstable();
                path.dedup();
                return Some(path);
            }
            other.push(*self.parent_causes.get(&x)?);
            x = *parents.get(&x)?;
        }
    }
    pub fn congruence(
        &self,
        left: FactRef,
        right: FactRef,
        a: &[Value],
        b: &[Value],
        parents: &BTreeMap<u64, u64>,
    ) -> Cause {
        let mut support = BTreeSet::new();
        let mut complete = a.len() == b.len();
        for (a, b) in a.iter().zip(b) {
            if a == b {
                continue;
            }
            match (a, b) {
                (Value::Eq(a), Value::Eq(b)) => {
                    if let Some(ids) = self.explain(*a, *b, parents) {
                        support.extend(ids)
                    } else {
                        complete = false
                    }
                }
                // Container canonicalization is deliberately not assigned a
                // scalar-congruence proof or a fabricated depth.
                _ => complete = false,
            }
        }
        Cause::Congruence {
            left,
            right,
            parents: support.into_iter().collect(),
            complete,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub rule: usize,
    pub slot: usize,
    pub atoms: [usize; 2],
    pub columns: [usize; 2],
}
pub fn compile(p: &Program) -> Vec<Entry> {
    let mut result = vec![];
    for (rid, r) in p.rules.iter().enumerate() {
        let atoms = r
            .query
            .iter()
            .enumerate()
            .filter_map(|(i, a)| matches!(a.call, Call::Table(_)).then_some(i))
            .collect::<Vec<_>>();
        if atoms.len() != 2 {
            continue;
        }
        let mut positions = [BTreeMap::new(), BTreeMap::new()];
        let mut admitted = true;
        for (j, &i) in atoms.iter().enumerate() {
            let a = &r.query[i];
            let Call::Table(t) = a.call else {
                unreachable!()
            };
            if !p.tables[t].constructor {
                admitted = false;
                break;
            }
            for (col, arg) in a.args.iter().enumerate() {
                if let Arg::Slot(s) = arg
                    && p.tables[t].schema[col]["kind"] == "eq"
                {
                    positions[j].entry(*s).or_insert(col);
                }
            }
        }
        if admitted {
            for (&slot, &col) in &positions[0] {
                if let Some(&other) = positions[1].get(&slot) {
                    result.push(Entry {
                        rule: rid,
                        slot,
                        atoms: [atoms[0], atoms[1]],
                        columns: [col, other],
                    });
                }
            }
        }
    }
    result
}
struct Neighborhood {
    versions: BTreeMap<usize, u64>,
    rows: BTreeMap<(usize, usize), Vec<Row>>,
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub published_neighborhoods: u64,
    pub context_row_reads: u64,
    pub routed_seeds: u64,
    pub candidate_pairs: u64,
    pub bindings: u64,
    pub bindings_by_rule: BTreeMap<usize, u64>,
    pub fallback_seeds: u64,
    pub empty_context_seeds: u64,
    pub expired_contexts: u64,
    pub expired_table_domains: u64,
}
pub struct Grafts {
    pub enabled: bool,
    entries: BTreeMap<(usize, usize), Vec<Entry>>,
    tables: BTreeSet<usize>,
    contexts: BTreeMap<u64, Neighborhood>,
    subscribers: BTreeMap<usize, BTreeSet<u64>>,
    pub stats: Stats,
}
impl Grafts {
    pub fn new(p: &Program, enabled: bool) -> Self {
        let mut entries: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let mut tables = BTreeSet::new();
        for e in compile(p) {
            for &atom in &e.atoms {
                entries.entry((e.rule, atom)).or_default().push(e.clone());
                if let Call::Table(t) = p.rules[e.rule].query[atom].call {
                    tables.insert(t);
                }
            }
        }
        Self {
            enabled,
            entries,
            tables,
            contexts: BTreeMap::new(),
            subscribers: BTreeMap::new(),
            stats: Stats::default(),
        }
    }
    pub fn publish(&mut self, g: &Graph, roots: BTreeSet<u64>) -> Result<()> {
        if !self.enabled || self.tables.is_empty() {
            return Ok(());
        }
        // Visit only domains whose tables changed, not every live neighborhood
        // at every Run. Empty domains subscribe too, so insertions cannot hide.
        for t in g.query_changes.values().copied().collect::<BTreeSet<_>>() {
            for id in self.subscribers.remove(&t).unwrap_or_default() {
                let Some(c) = self.contexts.get_mut(&id) else {
                    continue;
                };
                if c.versions.remove(&t).is_some() {
                    self.stats.expired_table_domains += 1;
                }
                c.rows.retain(|(table, _), _| *table != t);
                if c.versions.is_empty() {
                    self.contexts.remove(&id);
                    self.stats.expired_contexts += 1;
                }
            }
        }
        for &id in &roots {
            if crate::value::root(&g.parents, id) != id {
                self.remove_context(id);
            }
        }
        for root in roots
            .into_iter()
            .map(|r| crate::value::root(&g.parents, r))
            .collect::<BTreeSet<_>>()
        {
            let mut rows: BTreeMap<_, Vec<_>> = BTreeMap::new();
            for row in g.equality_neighbors(root, &self.tables)? {
                self.stats.context_row_reads += 1;
                for (col, v) in row.values.iter().enumerate() {
                    if *v == Value::Eq(root) {
                        rows.entry((row.table, col)).or_default().push(row.clone());
                    }
                }
            }
            self.remove_context(root);
            self.contexts.insert(
                root,
                Neighborhood {
                    rows,
                    versions: self
                        .tables
                        .iter()
                        .map(|t| (*t, g.query_versions[*t]))
                        .collect(),
                },
            );
            for t in &self.tables {
                self.subscribers.entry(*t).or_default().insert(root);
            }
            self.stats.published_neighborhoods += 1;
        }
        Ok(())
    }
    fn remove_context(&mut self, id: u64) {
        if let Some(c) = self.contexts.remove(&id) {
            for t in c.versions.keys() {
                if let Some(xs) = self.subscribers.get_mut(t) {
                    xs.remove(&id);
                }
            }
            self.stats.expired_contexts += 1;
        }
    }
    pub fn route(
        &mut self,
        g: &Graph,
        rid: usize,
        pivot: usize,
        seed: &Row,
    ) -> Option<(usize, Vec<Row>)> {
        if !self.enabled {
            return None;
        }
        for entry in self.entries.get(&(rid, pivot)).into_iter().flatten() {
            let side = usize::from(entry.atoms[1] == pivot);
            let other = 1 - side;
            let Value::Eq(root) = seed.values[entry.columns[side]] else {
                continue;
            };
            let Some(c) = self.contexts.get(&root) else {
                continue;
            };
            if !entry
                .atoms
                .iter()
                .all(|i| match g.program.rules[rid].query[*i].call {
                    Call::Table(t) => c.versions.get(&t) == Some(&g.query_versions[t]),
                    _ => false,
                })
            {
                continue;
            }
            let Call::Table(t) = g.program.rules[rid].query[entry.atoms[other]].call else {
                unreachable!()
            };
            self.stats.routed_seeds += 1;
            let rows = c
                .rows
                .get(&(t, entry.columns[other]))
                .cloned()
                .unwrap_or_default();
            if rows.is_empty() {
                self.stats.empty_context_seeds += 1;
            }
            return Some((entry.atoms[other], rows));
        }
        self.stats.fallback_seeds += 1;
        None
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"compiled_entries":self.entries.len(),"live_contexts":self.contexts.len(),"stats":self.stats,
            "scope":"exact two-constructor Spine joins supplied by complete published union neighborhoods; ordinary matcher fallback"})
    }
}
