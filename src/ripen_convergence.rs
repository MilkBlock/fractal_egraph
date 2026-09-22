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
