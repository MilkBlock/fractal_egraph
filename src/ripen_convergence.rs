//! Run-local hash rendezvous for ripen snapshots. A hit is verified, never trusted.
//! This observer measures convergence; it does not replace native matcher history.
use crate::saturated_rule_composition::{Comparison, SaturatedRuleComposition as State, compare};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

fn hash(x: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    x.hash(&mut h);
    h.finish()
}

/// ID-independent, deliberately approximate incidence signatures. Ports, literals,
/// argument order, aliases and visibility enter the signature. Exact matching is
/// still necessary: fixed-depth refinement cannot distinguish all graphs.
fn tokens(s: &State) -> BTreeMap<u64, usize> {
    let hidden: std::collections::BTreeSet<_> = s.subsumed_rows.iter().copied().collect();
    let mut labels: Vec<_> = s
        .values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            hash((
                &v.sort,
                &v.literal,
                s.ports
                    .iter()
                    .filter(|(_, j)| **j == i)
                    .map(|(p, _)| p)
                    .collect::<Vec<_>>(),
            ))
        })
        .collect();
    for _ in 0..2 {
        let mut edges = vec![vec![]; labels.len()];
        for (i, r) in s.rows.iter().enumerate() {
            let refs: Vec<_> = r
                .args
                .iter()
                .chain(std::iter::once(&r.result))
                .copied()
                .collect();
            let aliases: Vec<_> = refs
                .iter()
                .map(|v| refs.iter().position(|x| x == v).unwrap())
                .collect();
            let row = hash((
                &r.op,
                refs.iter().map(|v| labels[*v]).collect::<Vec<_>>(),
                aliases,
                hidden.contains(&i),
            ));
            for (slot, v) in refs.iter().enumerate() {
                edges[*v].push((slot, row));
            }
        }
        labels = edges
            .into_iter()
            .enumerate()
            .map(|(i, mut e)| {
                e.sort();
                hash((labels[i], e))
            })
            .collect();
    }
    let mut counts = BTreeMap::new();
    for t in labels
        .iter()
        .map(|l| hash((0u8, l)))
        .chain(s.rows.iter().enumerate().map(|(i, r)| {
            hash((
                1u8,
                &r.op,
                r.args.iter().map(|a| labels[*a]).collect::<Vec<_>>(),
                labels[r.result],
                hidden.contains(&i),
            ))
        }))
    {
        *counts.entry(t).or_insert(0) += 1;
    }
    counts
}

#[derive(Default)]
pub struct Fingerprint {
    counts: BTreeMap<u64, usize>,
    xor: u64,
}
impl Fingerprint {
    /// Update XOR contributions for changed signature multiplicities. Counts avoid
    /// cancelling two equal records. Snapshot extraction/refinement is NOT O(delta).
    fn update(&mut self, next: BTreeMap<u64, usize>) -> usize {
        let mut changed = 0;
        for (&t, &n) in &self.counts {
            if next.get(&t) != Some(&n) {
                self.xor ^= hash((t, n));
                changed += 1;
            }
        }
        for (&t, &n) in &next {
            if self.counts.get(&t) != Some(&n) {
                self.xor ^= hash((t, n));
                changed += 1;
            }
        }
        self.counts = next;
        changed
    }
}
struct Entry {
    contract: String,
    run: String,
    round: usize,
    state: State,
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub snapshots: usize,
    pub signature_seconds: f64,
    pub comparison_seconds: f64,
    pub hash_hits: usize,
    pub verified_hits: usize,
    pub collisions_or_unknown: usize,
    pub capacity_misses: usize,
    pub changed_contributions: usize,
}
pub struct Index {
    entries: HashMap<(u64, u64), Entry>,
    pub stats: Stats,
    capacity: usize,
    budget: usize,
}
impl Index {
    pub fn new(capacity: usize, budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            stats: Stats::default(),
            capacity,
            budget,
        }
    }
    pub fn observe(
        &mut self,
        run: &str,
        round: usize,
        state: State,
        contract: &str,
        fp: &mut Fingerprint,
    ) -> Value {
        self.stats.snapshots += 1;
        let started = std::time::Instant::now();
        self.stats.changed_contributions += fp.update(tokens(&state));
        let key = (hash((state.scope.to_string(), contract)), fp.xor);
        self.stats.signature_seconds += started.elapsed().as_secs_f64();
        self.intern(run, round, state, contract, key)
    }
    pub fn observe_packets(
        &mut self,
        run: &str,
        round: usize,
        p: &Packets,
        contract: &str,
    ) -> Value {
        self.stats.snapshots += 1;
        let key = (hash((p.scope.to_string(), contract)), p.fp.xor);
        if self.entries.get(&key).is_some_and(|e| e.run == run) {
            return json!({"status":"same_run"});
        }
        self.intern(run, round, p.snapshot(), contract, key)
    }
    fn intern(
        &mut self,
        run: &str,
        round: usize,
        state: State,
        contract: &str,
        key: (u64, u64),
    ) -> Value {
        if let Some(old) = self.entries.get(&key) {
            if old.run == run {
                return json!({"status":"same_run"});
            }
            self.stats.hash_hits += 1;
            if old.contract != contract {
                self.stats.collisions_or_unknown += 1;
                return json!({"status":"collision_or_unknown"});
            }
            let started = std::time::Instant::now();
            let comparison = compare(&state, &old.state, self.budget);
            self.stats.comparison_seconds += started.elapsed().as_secs_f64();
            if let Ok(Comparison::Equivalent { value_map, .. }) = comparison {
                self.stats.verified_hits += 1;
                return json!({"status":"verified","run":run,"round":round,"prior_run":old.run,"prior_round":old.round,"value_map":value_map});
            }
            self.stats.collisions_or_unknown += 1;
            // No bucket scan or representative selection. Collisions lose recall.
            return json!({"status":"collision_or_unknown"});
        }
        if self.entries.len() < self.capacity {
            self.entries.insert(
                key,
                Entry {
                    contract: contract.into(),
                    run: run.into(),
                    round,
                    state,
                },
            );
        } else {
            self.stats.capacity_misses += 1;
        }
        json!({"status":"miss"})
    }
    pub fn report(&self) -> Value {
        json!({"stats":self.stats,"entries":self.entries.len(),"capacity":self.capacity,"mode":"observer; native execution and history retained"})
    }
}

/// Mutable flat facts with reverse incidence. Union only revisits incident facts.
pub struct Packets {
    scope: Value,
    values: Vec<crate::saturated_rule_composition::Vertex>,
    parent: Vec<usize>,
    ports: BTreeMap<String, usize>,
    rows: std::collections::BTreeSet<crate::saturated_rule_composition::Row>,
    keys: BTreeMap<(String, Vec<usize>), usize>,
    incident: Vec<std::collections::BTreeSet<crate::saturated_rule_composition::Row>>,
    fp: Fingerprint,
    pub changes: usize,
}
impl Packets {
    pub fn new(s: &State) -> Self {
        let mut p = Self {
            scope: s.scope.clone(),
            values: s.values.clone(),
            parent: (0..s.values.len()).collect(),
            ports: s.ports.clone(),
            rows: Default::default(),
            keys: Default::default(),
            incident: vec![Default::default(); s.values.len()],
            fp: Default::default(),
            changes: 0,
        };
        for i in 0..p.values.len() {
            p.adjust(p.vertex_token(i), true);
        }
        for r in &s.rows {
            p.ensure(r.clone());
        }
        p
    }
    pub fn root(&self, mut i: usize) -> usize {
        while self.parent[i] != i {
            i = self.parent[i];
        }
        i
    }
    fn label(&self, i: usize) -> u64 {
        hash((
            &self.values[i].sort,
            &self.values[i].literal,
            self.ports
                .iter()
                .filter(|(_, v)| **v == i)
                .map(|(k, _)| k)
                .collect::<Vec<_>>(),
        ))
    }
    fn vertex_token(&self, i: usize) -> u64 {
        hash((0u8, self.label(i)))
    }
    fn row_token(&self, r: &crate::saturated_rule_composition::Row) -> u64 {
        let ids: Vec<_> = r
            .args
            .iter()
            .chain(std::iter::once(&r.result))
            .copied()
            .collect();
        hash((
            1u8,
            &r.op,
            ids.iter().map(|i| self.label(*i)).collect::<Vec<_>>(),
            ids.iter()
                .map(|i| ids.iter().position(|v| v == i).unwrap())
                .collect::<Vec<_>>(),
        ))
    }
    fn adjust(&mut self, t: u64, add: bool) {
        let n = self.fp.counts.get(&t).copied().unwrap_or(0);
        if n > 0 {
            self.fp.xor ^= hash((t, n));
        }
        let next = if add {
            n + 1
        } else {
            n.checked_sub(1).expect("present contribution")
        };
        if next > 0 {
            self.fp.xor ^= hash((t, next));
            self.fp.counts.insert(t, next);
        } else {
            self.fp.counts.remove(&t);
        }
        self.changes += 1;
    }
    pub fn vertex(&mut self, v: crate::saturated_rule_composition::Vertex) -> usize {
        let i = self.values.len();
        self.values.push(v);
        self.parent.push(i);
        self.incident.push(Default::default());
        self.adjust(self.vertex_token(i), true);
        i
    }
    pub fn ensure(&mut self, mut r: crate::saturated_rule_composition::Row) {
        r.args = r.args.iter().map(|i| self.root(*i)).collect();
        r.result = self.root(r.result);
        let key = (r.op.clone(), r.args.clone());
        if let Some(&other) = self.keys.get(&key) {
            if self.root(other) != r.result {
                // Constructor functionality: native rebuild may merge parent
                // results without a rule-caused UnionEvent. Close that congruence.
                self.union(other, r.result);
                self.ensure(r);
                return;
            }
        }
        self.keys.insert(key, r.result);
        if self.rows.insert(r.clone()) {
            self.adjust(self.row_token(&r), true);
            for i in r.args.iter().chain(std::iter::once(&r.result)) {
                self.incident[*i].insert(r.clone());
            }
        }
    }
    pub fn union(&mut self, a: usize, b: usize) {
        let mut a = self.root(a);
        let mut b = self.root(b);
        if a == b {
            return;
        }
        assert_eq!(self.values[a], self.values[b]);
        if self.incident[a].len() < self.incident[b].len() {
            std::mem::swap(&mut a, &mut b);
        }
        let affected: std::collections::BTreeSet<_> =
            self.incident[a].union(&self.incident[b]).cloned().collect();
        for r in &affected {
            self.adjust(self.row_token(r), false);
            self.rows.remove(r);
            self.keys.remove(&(r.op.clone(), r.args.clone()));
            for i in r.args.iter().chain(std::iter::once(&r.result)) {
                self.incident[*i].remove(r);
            }
        }
        self.adjust(self.vertex_token(a), false);
        self.adjust(self.vertex_token(b), false);
        self.parent[b] = a;
        for v in self.ports.values_mut() {
            if *v == b {
                *v = a;
            }
        }
        self.adjust(self.vertex_token(a), true);
        for r in affected {
            self.ensure(r);
        }
    }
    pub fn snapshot(&self) -> State {
        let mut ids = vec![0; self.values.len()];
        let mut values = vec![];
        for i in 0..self.values.len() {
            if self.root(i) == i {
                ids[i] = values.len();
                values.push(self.values[i].clone());
            }
        }
        State {
            version: 2,
            scope: self.scope.clone(),
            local_ids: vec![],
            values,
            rows: self
                .rows
                .iter()
                .map(|r| crate::saturated_rule_composition::Row {
                    op: r.op.clone(),
                    args: r.args.iter().map(|i| ids[*i]).collect(),
                    result: ids[r.result],
                })
                .collect(),
            subsumed_rows: vec![],
            ports: self
                .ports
                .iter()
                .map(|(k, i)| (k.clone(), ids[*i]))
                .collect(),
        }
    }
}
