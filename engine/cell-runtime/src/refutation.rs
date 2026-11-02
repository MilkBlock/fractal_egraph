//! Exact, online negative receipts for marker-anchored residual queries.
//! These are NOT RipSat completeness certificates. A receipt is admitted only
//! after an unrestricted, unsuccessful residual search, and retains every read
//! range that justified that failure. Hashes choose supersets, never truth.
use crate::{
    graph::Graph,
    query::{QueryIndexes, ReadStamp},
    value::Value,
};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Operand {
    Bound(Value),
    Variable(usize),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Operator {
    Table(usize),
    Primitive(String, String),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    // Alpha-normalized residual conjunction; RHS and ruleset do not affect
    // existence on the same immutable visible snapshot. Atom order is retained.
    pub atoms: Vec<(Operator, Vec<Operand>)>,
}
#[derive(Default)]
pub struct Evidence {
    pub reads: Vec<ReadStamp>,
    pub delta_restricted: bool,
}
struct Receipt {
    evidence: Evidence,
    candidates: u64,
}
#[derive(Default, serde::Serialize)]
pub struct Stats {
    pub lookups: u64,
    pub hits: u64,
    pub admitted: u64,
    pub invalidated: u64,
    pub delta_incomplete: u64,
    pub positive_searches: u64,
    pub certification_candidates: u64,
    /// Historical search work attached to hits; NOT measured work saved.
    pub prior_search_candidates_on_hits: u64,
    pub peak_receipts: usize,
    pub peak_read_ranges: usize,
    pub per_rule_hits: HashMap<usize, u64>,
}
pub struct Refutation {
    pub enabled: bool,
    receipts: HashMap<Key, Receipt>,
    pub stats: Stats,
}
impl Refutation {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            receipts: HashMap::new(),
            stats: Stats::default(),
        }
    }
    pub fn reject(
        &mut self,
        rule: usize,
        key: &Key,
        indexes: &QueryIndexes,
        graph: &Graph,
    ) -> bool {
        self.stats.lookups += 1;
        if let Some(r) = self.receipts.get(key) {
            if r.evidence
                .reads
                .iter()
                .all(|s| indexes.valid_read(graph, s))
            {
                self.stats.hits += 1;
                *self.stats.per_rule_hits.entry(rule).or_default() += 1;
                self.stats.prior_search_candidates_on_hits += r.candidates;
                return true;
            }
            self.receipts.remove(key);
            self.stats.invalidated += 1;
        }
        false
    }
    pub fn finish(&mut self, key: Key, evidence: Evidence, candidates: u64, found: bool) {
        self.stats.certification_candidates += candidates;
        if found {
            self.stats.positive_searches += 1;
        } else if evidence.delta_restricted {
            // "No new match in this delta partition" is not "no match".
            self.stats.delta_incomplete += 1;
        } else {
            self.receipts.insert(
                key,
                Receipt {
                    evidence,
                    candidates,
                },
            );
            self.stats.admitted += 1;
            self.stats.peak_receipts = self.stats.peak_receipts.max(self.receipts.len());
            self.stats.peak_read_ranges = self
                .stats
                .peak_read_ranges
                .max(self.receipts.values().map(|r| r.evidence.reads.len()).sum());
        }
    }
    pub fn publish(&mut self, indexes: &QueryIndexes, graph: &Graph) {
        let before = self.receipts.len();
        // Reclaim stale evidence at the published snapshot, including hidden
        // markers. Cached references do not pin graph frames or provider chains.
        self.receipts.retain(|_, r| {
            r.evidence
                .reads
                .iter()
                .all(|s| indexes.valid_read(graph, s))
        });
        self.stats.invalidated += (before - self.receipts.len()) as u64;
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"stats":self.stats,
            "live_receipts":self.receipts.len(),
            "live_read_ranges":self.receipts.values().map(|r|r.evidence.reads.len()).sum::<usize>(),
            "scope":"online complete residual-query negatives anchored by existing markers; not baked Cell closure or static impossibility"})
    }
}
