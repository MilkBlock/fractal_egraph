//! Sieve-style semijoin summaries over COMPLETE visible virtual tables.
//! A filter is a necessary condition, never a proof that a binding exists.
//! Interned by table/columns/dependencies, not ruleset or source variable names.
use rustc_hash::FxHashSet as HashSet;
use rustc_hash::FxHashMap as HashMap;
use crate::{
    graph::{Dependency, Graph},
    program::{Arg, Call, Program, Result},
    query::QueryIndexes,
};
use std::collections::{BTreeMap, VecDeque};
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Guard {
    cols: Vec<usize>,
    child: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Spec {
    table: usize,
    cols: Vec<usize>,
    guards: Vec<Guard>,
}
#[derive(Default)]
struct Summary {
    built: bool,
    keys: HashSet<Vec<u64>>,
    bloom: Bloom,
}
#[derive(Default)]
struct Bloom {
    words: Vec<u64>,
    collide: bool,
}
fn hash(xs: &[u64]) -> u64 {
    hash_projection(xs.iter().copied(), xs.len())
}
fn hash_projection(xs: impl Iterator<Item = u64>, len: usize) -> u64 {
    let mut h = rustc_hash::FxHasher::default();
    len.hash(&mut h);
    for x in xs {
        x.hash(&mut h)
    }
    h.finish()
}
impl Bloom {
    fn location_hash(&self, h: u64) -> (usize, u64) {
        let h = if self.collide { 0 } else { h };
        let mask = (1 << ((h >> 32) & 63)) | (1 << ((h >> 38) & 63)) | (1 << ((h >> 44) & 63));
        ((h as usize) & (self.words.len() - 1), mask)
    }
    #[cfg(test)]
    fn contains(&self, xs: &[u64]) -> bool {
        if self.words.is_empty() {
            return false;
        }
        let (i, m) = self.location_hash(hash(xs));
        self.words[i] & m == m
    }
    fn contains_projection(&self, keys: &[u64], cols: &[usize]) -> bool {
        if self.words.is_empty() {
            return false;
        }
        let (i, m) = self.location_hash(hash_projection(cols.iter().map(|c| keys[*c]), cols.len()));
        self.words[i] & m == m
    }
    fn insert(&mut self, xs: &[u64]) {
        let (i, m) = self.location_hash(hash(xs));
        self.words[i] |= m;
    }
    fn grow(&mut self, keys: &HashSet<Vec<u64>>, collide: bool) {
        self.collide = collide;
        let size = (keys.len().saturating_mul(16).div_ceil(64))
            .max(1)
            .next_power_of_two();
        if self.words.len() >= size {
            return;
        }
        self.words = vec![0; size];
        for k in keys {
            self.insert(k)
        }
    }
}
#[derive(Default, serde::Serialize)]
pub struct Stats {
    pub builds: u64,
    pub rule_uses: u64,
    pub reused_summaries: u64,
    pub build_rows: u64,
    pub update_rows: u64,
    pub dependency_rows: u64,
    pub added_keys: u64,
    pub probes: u64,
    pub rejected: u64,
    pub empty_join_skips: u64,
    pub per_rule: Vec<(u64, u64)>,
}
pub struct Sieve {
    pub enabled: bool,
    specs: Vec<Spec>,
    states: Vec<Summary>,
    by_table: Vec<Vec<usize>>,
    parents: Vec<Vec<(usize, usize)>>,
    // Incoming necessary conditions for each (rule, atom).
    rules: Vec<Vec<Vec<Guard>>>,
    active: Vec<bool>,
    collide: bool,
    pub stats: Stats,
}
impl Sieve {
    #[cfg(feature = "family-frontiers")]
    pub(crate) fn survey_guards(&self, rule: usize, pivot: usize) -> Vec<(Vec<usize>, usize)> {
        self.rules[rule][pivot]
            .iter()
            .map(|g| (g.cols.clone(), g.child))
            .collect()
    }

    pub fn new(program: &Program) -> Self {
        let enabled = true;
        let mut s = Self {
            enabled,
            specs: vec![],
            states: vec![],
            by_table: vec![vec![]; program.tables.len()],
            parents: vec![],
            rules: vec![],
            active: vec![],
            collide: std::env::var("CELL_CPU_SIEVE_COLLISIONS").as_deref() == Ok("1"),
            stats: Stats {
                per_rule: vec![(0, 0); program.rules.len()],
                ..Stats::default()
            },
        };
        let mut intern = HashMap::default();
        for rule in &program.rules {
            let q = &rule.query;
            let mut targets = vec![vec![]; q.len()];
            if enabled {
                // One deterministic spanning forest; never enumerate paths or
                // combinations. O(atoms^2) planning, <=2*(atoms-1) messages.
                let slots = q
                    .iter()
                    .map(|a| {
                        a.args
                            .iter()
                            .enumerate()
                            .filter_map(|(i, a)| match a {
                                Arg::Slot(s) => Some((*s, i)),
                                _ => None,
                            })
                            .fold(BTreeMap::new(), |mut m, (s, c)| {
                                m.entry(s).or_insert(c);
                                m
                            })
                    })
                    .collect::<Vec<_>>();
                let mut edges = vec![];
                for a in 0..q.len() {
                    if !matches!(q[a].call, Call::Table(_)) {
                        continue;
                    }
                    for b in a + 1..q.len() {
                        if !matches!(q[b].call, Call::Table(_)) {
                            continue;
                        }
                        let count = slots[a].keys().filter(|s| slots[b].contains_key(s)).count();
                        if count > 0 {
                            edges.push((std::cmp::Reverse(count), a, b))
                        }
                    }
                }
                edges.sort();
                let mut groups = (0..q.len()).collect::<Vec<_>>();
                let mut tree = vec![vec![]; q.len()];
                for (_, a, b) in edges {
                    if groups[a] != groups[b] {
                        let (old, new) = (groups[b], groups[a]);
                        for g in &mut groups {
                            if *g == old {
                                *g = new
                            }
                        }
                        tree[a].push(b);
                        tree[b].push(a);
                    }
                }
                fn message(
                    s: &mut Sieve,
                    intern: &mut HashMap<Spec, usize>,
                    q: &[crate::program::Atom],
                    slots: &[BTreeMap<usize, usize>],
                    tree: &[Vec<usize>],
                    from: usize,
                    to: usize,
                    memo: &mut HashMap<(usize, usize), (usize, Vec<usize>)>,
                ) -> (usize, Vec<usize>) {
                    if let Some(v) = memo.get(&(from, to)) {
                        return v.clone();
                    }
                    // Order by sender columns, independent of slot numbering.
                    let mut shared = slots[from]
                        .iter()
                        .filter_map(|(v, c)| slots[to].get(v).map(|d| (*c, *d)))
                        .collect::<Vec<_>>();
                    shared.sort();
                    let cols = shared.iter().map(|(c, _)| *c).collect();
                    let receiver = shared.iter().map(|(_, d)| *d).collect::<Vec<_>>();
                    let mut guards = tree[from]
                        .iter()
                        .filter(|n| **n != to)
                        .map(|n| {
                            let (child, cols) = message(s, intern, q, slots, tree, *n, from, memo);
                            Guard { cols, child }
                        })
                        .collect::<Vec<_>>();
                    guards.sort();
                    guards.dedup();
                    let Call::Table(table) = q[from].call else {
                        unreachable!()
                    };
                    let spec = Spec {
                        table,
                        cols,
                        guards,
                    };
                    let id = if let Some(id) = intern.get(&spec) {
                        *id
                    } else {
                        let id = s.specs.len();
                        intern.insert(spec.clone(), id);
                        s.by_table[table].push(id);
                        s.specs.push(spec);
                        s.states.push(Summary::default());
                        s.parents.push(vec![]);
                        id
                    };
                    memo.insert((from, to), (id, receiver.clone()));
                    (id, receiver)
                }
                let mut memo = HashMap::default();
                for to in 0..q.len() {
                    for from in &tree[to] {
                        let (child, cols) =
                            message(&mut s, &mut intern, q, &slots, &tree, *from, to, &mut memo);
                        // A bare table membership is ordinary index filtering, not
                        // new fragment reuse. Only install genuinely joined summaries.
                        if !s.specs[child].guards.is_empty() {
                            targets[to].push(Guard { cols, child })
                        }
                    }
                }
            }
            s.rules.push(targets);
        }
        for (p, spec) in s.specs.iter().enumerate() {
            for (g, guard) in spec.guards.iter().enumerate() {
                s.parents[guard.child].push((p, g));
            }
        }
        s.active = vec![false; s.rules.len()];
        s
    }
    fn admit(&mut self, id: usize, keys: &[u64]) -> Option<Vec<u64>> {
        let spec = &self.specs[id];
        for g in &spec.guards {
            if !self.states[g.child]
                .bloom
                .contains_projection(keys, &g.cols)
            {
                return None;
            }
        }
        let k = spec.cols.iter().map(|c| keys[*c]).collect::<Vec<_>>();
        let state = &mut self.states[id];
        // Must track keys, NOT whether OR changed any Bloom bit. A new key can
        // collide with old bits and STILL needs upstream propagation.
        if !state.keys.insert(k.clone()) {
            return None;
        }
        state.bloom.grow(&state.keys, self.collide);
        state.bloom.insert(&k);
        self.stats.added_keys += 1;
        Some(k)
    }
    fn ensure(&mut self, id: usize, g: &Graph, index: &mut QueryIndexes) -> Result<()> {
        if self.states[id].built {
            self.stats.reused_summaries += 1;
            return Ok(());
        }
        let spec = self.specs[id].clone();
        for guard in &spec.guards {
            self.ensure(guard.child, g, index)?
        }
        index.ensure(g, spec.table)?;
        for w in index.indexed_witnesses(spec.table) {
            self.stats.build_rows += 1;
            self.admit(id, index.hashes(spec.table, w).unwrap());
        }
        self.states[id].built = true;
        self.stats.builds += 1;
        Ok(())
    }
    pub fn prepare(&mut self, rid: usize, g: &Graph, index: &mut QueryIndexes) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        self.stats.rule_uses += 1;
        if self.active[rid] {
            self.stats.reused_summaries += 1;
            return Ok(());
        }
        let targets = self.rules[rid].clone();
        for guards in targets {
            for guard in guards {
                self.ensure(guard.child, g, index)?
            }
        }
        // A receiver may not otherwise be indexed (e.g. delta root). Ensure it
        // has canonical column hashes before using its filters.
        for (atom, guards) in self.rules[rid].iter().enumerate() {
            if !guards.is_empty() {
                if let Call::Table(t) = g.program.rules[rid].query[atom].call {
                    index.ensure(g, t)?
                }
            }
        }
        self.active[rid] = true;
        Ok(())
    }
    /// Each incoming guard is necessary for the whole conjunctive query.
    /// An empty, built summary refutes every pivot in this snapshot. The next
    /// scheduled Run checks again after publication has propagated new keys.
    pub(crate) fn empty_join(&mut self, rid: usize) -> bool {
        let empty = self.rules[rid].iter().flatten().any(|guard| {
            let summary = &self.states[guard.child];
            summary.built && summary.keys.is_empty()
        });
        self.stats.empty_join_skips += u64::from(empty);
        empty
    }
    pub fn allow(
        &mut self,
        rid: usize,
        atom: usize,
        table: usize,
        w: Dependency,
        index: &QueryIndexes,
    ) -> bool {
        if !self.enabled {
            return true;
        }
        let guards = &self.rules[rid][atom];
        if guards.is_empty() {
            return true;
        }
        let Some(keys) = index.hashes(table, w) else {
            return true;
        }; // exact matcher handles hidden/stale rows
        for guard in guards {
            self.stats.probes += 1;
            self.stats.per_rule[rid].0 += 1;
            if !self.states[guard.child]
                .bloom
                .contains_projection(keys, &guard.cols)
            {
                self.stats.rejected += 1;
                self.stats.per_rule[rid].1 += 1;
                return false;
            }
        }
        true
    }
    pub fn publish(&mut self, index: &mut QueryIndexes) {
        let mut changes = std::mem::take(&mut index.changes);
        changes.sort_unstable();
        if !self.enabled {
            return;
        }
        let mut queue = VecDeque::new();
        for (w, table) in changes {
            if let Some(keys) = index.hashes(table, w) {
                for i in 0..self.by_table[table].len() {
                    let id = self.by_table[table][i];
                    if self.states[id].built {
                        self.stats.update_rows += 1;
                        if let Some(k) = self.admit(id, keys) {
                            queue.push_back((id, k))
                        }
                    }
                }
            }
        }
        while let Some((child, keys)) = queue.pop_front() {
            for i in 0..self.parents[child].len() {
                let (parent, gid) = self.parents[child][i];
                if self.states[parent].built {
                    let spec = &self.specs[parent];
                    let table = spec.table;
                    let rows = index.hash_candidates(table, &spec.guards[gid].cols, &keys);
                    for w in rows {
                        self.stats.dependency_rows += 1;
                        if let Some(k) = self.admit(parent, index.hashes(table, w).unwrap()) {
                            queue.push_back((parent, k))
                        }
                    }
                }
            }
        }
    }
    pub fn invalidate(&mut self) {
        for state in &mut self.states {
            *state = Summary::default()
        }
        self.active.fill(false);
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"specifications":self.specs.len(),"active_summaries":self.states.iter().filter(|s|s.built).count(),"stored_keys":self.states.iter().map(|s|s.keys.len()).sum::<usize>(),"bloom_bytes":self.states.iter().map(|s|s.bloom.words.len()*8).sum::<usize>(),"key_payload_bytes":self.states.iter().flat_map(|s|s.keys.iter()).map(|k|k.len()*8).sum::<usize>(),"stats":self.stats,"scope":"complete visible-table semijoin summaries; not Cell-only proof credit"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bloom_growth_never_loses_insertions() {
        let mut bloom = Bloom::default();
        let mut keys = HashSet::default();
        for i in 0..200 {
            let k = vec![i, i * 17];
            keys.insert(k.clone());
            bloom.grow(&keys, false);
            bloom.insert(&k);
            for old in &keys {
                assert!(bloom.contains(old));
                assert!(bloom.contains_projection(&[999, old[1], old[0]], &[2, 1]));
            }
        }
    }
    #[test]
    fn new_key_propagates_even_if_or_did_not_change_bits() {
        let mut s = Sieve {
            enabled: true,
            specs: vec![Spec {
                table: 0,
                cols: vec![0],
                guards: vec![],
            }],
            states: vec![Summary::default()],
            by_table: vec![vec![0]],
            parents: vec![vec![]],
            rules: vec![],
            active: vec![],
            collide: true,
            stats: Stats::default(),
        };
        assert_eq!(s.admit(0, &[7]), Some(vec![7]));
        let bits = s.states[0].bloom.words.clone();
        assert_eq!(s.admit(0, &[8]), Some(vec![8]));
        assert_eq!(bits, s.states[0].bloom.words);
        assert_eq!(s.admit(0, &[8]), None);
    }
}
