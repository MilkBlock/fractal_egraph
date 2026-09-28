//! Attribute node creation to the rule application that caused it.
//!
//! Per-row provenance cannot answer "who produced this node" in a graph that
//! unions: the runtime deliberately drops a row's retained producer when a
//! rebuild rewrites that row (`PendingState::invalidate_origin`: "Rebuild still
//! invalidates changed rows; this does not assert lineage through
//! canonicalization"). Asking live rows therefore reports "no producer" for
//! almost everything, which measures nothing.
//!
//! This module instead accounts for the **committed write stream** and reconciles
//! it against the live graph:
//!
//! * `inputs` - constructor nodes present before the first traced round;
//! * `inserted` / `deduplicated` / `updated` / `unsupported` - committed writes to
//!   constructor tables, bucketed by the producing rule;
//! * `live_total` - constructor nodes measured in the final graph;
//! * `removed_or_merged = inputs + inserted - live_total` - the measured number of
//!   nodes that did not survive, i.e. what merging (and deletion) actually removed.
//!
//! Buckets are decided by rule name, using the contract
//! `reuse_adaptive::rule_program` implements: `adaptive_baked_{id}` is a rule
//! whose RHS a learned macro replaced - a RipSat closure rule - while
//! `adaptive_source_{id}` is the original source rule still executing, and any
//! other name is an original source rule as well. Note that in the adaptive
//! runner *every* rule carries the `adaptive_` prefix, so the prefix alone cannot
//! separate the two; the `baked` / `source` infix is what does. A write whose
//! match is not in the retained rule map gets its own bucket rather than being
//! folded into either side.
use crate::pipeline::Result;
use egglog::{EGraph, TraceBatch, WriteOutcome};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Prefix of every rule the adaptive runtime generates.
pub const ADAPTIVE_PREFIX: &str = "adaptive_";
/// Infix marking a rule whose RHS a learned macro replaced.
pub const BAKED_INFIX: &str = "adaptive_baked_";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Buckets([usize; 3]);

impl Buckets {
    fn add(&mut self, bucket: usize) {
        self.0[bucket] += 1;
    }
    fn total(&self) -> usize {
        self.0.iter().sum()
    }
    fn to_json(self) -> Value {
        json!({
            "seminaive": self.0[0],
            "ripsat": self.0[1],
            "unknown": self.0[2],
            "total": self.total(),
        })
    }
}

/// Committed-write accounting for one run.
#[derive(Clone, Debug, Default)]
pub struct WriteAccounting {
    inputs: usize,
    inserted: Buckets,
    deduplicated: Buckets,
    updated: Buckets,
    unsupported: Buckets,
    /// Writes that transport an earlier row version through a rebuild.
    rebuild: Buckets,
    /// Writes to constructor tables that were seen at all.
    constructor_writes: usize,
    /// Writes to non-constructor tables, counted so they are not silently mixed in.
    other_writes: usize,
}

impl WriteAccounting {
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructor nodes present before any traced rule ran.
    pub fn set_inputs(&mut self, nodes: usize) {
        self.inputs = nodes;
    }

    pub fn inputs(&self) -> usize {
        self.inputs
    }

    /// Fold one drained trace batch into the accounting.
    pub fn record(
        &mut self,
        batch: &TraceBatch,
        rules: &BTreeMap<u64, String>,
        constructor_tables: &BTreeSet<usize>,
    ) {
        for write in &batch.writes {
            if !constructor_tables.contains(&write.table_id()) {
                self.other_writes += 1;
                continue;
            }
            self.constructor_writes += 1;
            let bucket = match rules.get(&write.match_event_id) {
                Some(rule) if rule.starts_with(BAKED_INFIX) => 1,
                Some(_) => 0,
                None => 2,
            };
            match write.outcome {
                WriteOutcome::Inserted => self.inserted.add(bucket),
                WriteOutcome::Deduplicated => self.deduplicated.add(bucket),
                WriteOutcome::Updated => self.updated.add(bucket),
                WriteOutcome::Unsupported => self.unsupported.add(bucket),
            }
            if write.rebuild_of.is_some() {
                self.rebuild.add(bucket);
            }
        }
    }

    /// Reconcile the committed writes against the live graph.
    pub fn report(&self, eg: &EGraph) -> Result<Value> {
        let (live_total, by_function) = live_constructor_nodes(eg)?;
        let inserted = self.inserted.total();
        let removed_or_merged = (self.inputs + inserted).saturating_sub(live_total);
        Ok(json!({
            "live_total": live_total,
            "inputs": self.inputs,
            "inserted": self.inserted.to_json(),
            "deduplicated": self.deduplicated.to_json(),
            "updated": self.updated.to_json(),
            "unsupported": self.unsupported.to_json(),
            "rebuild_writes": self.rebuild.to_json(),
            "reconciliation": {
                "identity": "live_total = inputs + inserted - removed_or_merged",
                "removed_or_merged": removed_or_merged,
                "deduplicated_and_updated": self.deduplicated.total() + self.updated.total(),
                "note": "removed_or_merged is measured; deduplicated/updated are write outcomes. They are two views and are not asserted equal.",
            },
            "constructor_writes": self.constructor_writes,
            "other_writes": self.other_writes,
            "by_function": by_function.into_iter().map(|(name, total)| json!({"function": name, "live": total})).collect::<Vec<_>>(),
            "scope": "committed writes to constructor tables, bucketed by producing rule name; live rows measured in the final graph",
        }))
    }
}

/// Live constructor rows, total and per function.
pub fn live_constructor_nodes(eg: &EGraph) -> Result<(usize, BTreeMap<String, usize>)> {
    let mut total = 0usize;
    let mut per_function = BTreeMap::new();
    for name in eg.get_function_names() {
        let Some(function) = eg.get_function(&name) else {
            continue;
        };
        if !function.is_constructor() {
            continue;
        }
        let mut count = 0usize;
        eg.function_for_each(&name, |_| count += 1)?;
        total += count;
        if count > 0 {
            per_function.insert(name, count);
        }
    }
    Ok((total, per_function))
}
