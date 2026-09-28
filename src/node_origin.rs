//! Attribute live e-nodes to the rule application that produced them.
//!
//! Counting *live* rows, not write events, is what makes merge deduplication
//! implicit: a row that a union merged away is simply not live any more, so it
//! cannot be counted twice and needs no separate correction. Rows with no
//! retained producer - initial inputs, rows rewritten by a rebuild, or any row
//! from a run that did not record provenance - are reported separately instead of
//! being folded into either bucket.
//!
//! Buckets are decided by rule name. The adaptive runtime names every rule it
//! generates `adaptive_{baked|source}_{id}` (see `reuse_adaptive::rule_program`),
//! so a name carrying that prefix is a RipSat closure rule and any other name is
//! an original source rule. This is a naming contract of this repository, not an
//! inference about the rule's text.
use crate::pipeline::Result;
use egglog::EGraph;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Prefix the adaptive runtime gives every rule it generates.
pub const RIPSAT_PREFIX: &str = "adaptive_";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeBuckets {
    /// Written by an original source rule application.
    pub seminaive: usize,
    /// Written by a RipSat closure rule application.
    pub ripsat: usize,
    /// No retained producer: input, rebuilt, or provenance was off.
    pub unattributed: usize,
}

impl NodeBuckets {
    fn add(&mut self, other: NodeBuckets) {
        self.seminaive += other.seminaive;
        self.ripsat += other.ripsat;
        self.unattributed += other.unattributed;
    }
    pub fn total(&self) -> usize {
        self.seminaive + self.ripsat + self.unattributed
    }
    fn bucket(&mut self, index: usize) {
        match index {
            0 => self.seminaive += 1,
            1 => self.ripsat += 1,
            _ => self.unattributed += 1,
        }
    }
}

/// Attribute every live constructor row of `eg`.
///
/// `rules` maps a match event id to the rule name that produced it. A caller that
/// drains its trace each round must accumulate this map as it goes: a drained
/// batch no longer answers `matches()`. `unattributed` is split by cause so a
/// missing map cannot hide behind a missing producer.
pub fn attribute(eg: &EGraph, rules: &BTreeMap<u64, String>) -> Result<Value> {
    let mut no_producer = 0usize;
    let mut unknown_match = 0usize;
    let mut totals = NodeBuckets::default();
    let mut per_function: BTreeMap<String, NodeBuckets> = BTreeMap::new();
    let mut constructors = 0usize;
    for name in eg.get_function_names() {
        let Some(function) = eg.get_function(&name) else {
            continue;
        };
        if !function.is_constructor() {
            continue;
        }
        constructors += 1;
        let arity = function.schema().input.len();
        let mut buckets = NodeBuckets::default();
        eg.function_for_each(&name, |row| {
            let key = &row.vals[..arity.min(row.vals.len())];
            let index = match eg.row_producer_row(&name, key, &row.vals) {
                Some((match_id, _)) => match rules.get(&match_id) {
                    Some(rule) if rule.starts_with(RIPSAT_PREFIX) => 1,
                    Some(_) => 0,
                    // A producer exists but its match is not in the rule map.
                    None => {
                        unknown_match += 1;
                        2
                    }
                },
                None => {
                    no_producer += 1;
                    2
                }
            };
            buckets.bucket(index);
        })?;
        totals.add(buckets);
        if buckets.total() > 0 {
            per_function.insert(name, buckets);
        }
    }
    Ok(json!({
        "total": totals.total(),
        "seminaive": totals.seminaive,
        "ripsat": totals.ripsat,
        "unattributed": totals.unattributed,
        "constructors": constructors,
        "union_events": eg.native_union_count(),
        "no_producer": no_producer,
        "unknown_match": unknown_match,
        "rules_known": rules.len(),
        "provenance": if rules.is_empty() {
            "no rule map was retained: producers cannot be named"
        } else {
            "per live row, by producing rule name"
        },
        "scope": "live rows only, so rows removed by merge are never counted; unattributed covers inputs, rebuilt rows and rows whose producer is not in the trace",
        "by_function": per_function.into_iter().map(|(name, b)| json!({"function":name,"total":b.total(),"seminaive":b.seminaive,"ripsat":b.ripsat,"unattributed":b.unattributed})).collect::<Vec<_>>(),
    }))
}
