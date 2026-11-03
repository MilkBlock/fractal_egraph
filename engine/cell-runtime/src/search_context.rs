//! Observation at the delta-seed boundary, before any residual LHS search.
//! Stable table versions are conservative evidence of unchanged read domains,
//! not permission to replay a delta-filtered answer or skip RHS effects.
use crate::{
    graph::Graph,
    program::{Arg, Call},
    value::Value,
};
use serde::Serialize;
use serde_json::{Value as J, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

#[derive(Default, Serialize)]
pub struct Work {
    entries: u64,
    candidates: u64,
    merge_candidates: u64,
    decoded_rows: u64,
    key_reads: u64,
    bindings: u64,
    seconds: f64,
}
#[derive(Default, Serialize)]
struct Stats {
    first: Work,
    changed: Work,
    stable_same_stage: Work,
    stable_later_stage: Work,
}
struct Previous {
    versions: Vec<u64>,
    epoch: u64,
}
type SurveyKey = (usize, usize, Vec<Option<Value>>);
pub struct Ticket {
    rule: usize,
    kind: u8,
    counts: [u64; 4],
    bindings: usize,
    start: Instant,
}
#[derive(Default)]
pub struct Survey {
    pub enabled: bool,
    projected: bool,
    keys: BTreeMap<SurveyKey, Previous>,
    stats: BTreeMap<usize, Stats>,
}
impl Survey {
    pub fn new() -> Self {
        let mode = std::env::var("CELL_CPU_PRESEARCH_SURVEY").unwrap_or_else(|_| "0".into());
        Self {
            enabled: matches!(mode.as_str(), "1" | "full"),
            projected: mode != "full",
            ..Self::default()
        }
    }
    pub fn start(
        &mut self,
        graph: &Graph,
        rule: usize,
        pivot: usize,
        values: &[Option<Value>],
        merge_rows: u64,
        bindings: usize,
    ) -> Option<Ticket> {
        if !self.enabled {
            return None;
        }
        let tables = graph.program.rules[rule]
            .query
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                if i != pivot {
                    if let Call::Table(t) = a.call {
                        Some(t)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        let versions = tables
            .iter()
            .map(|t| graph.query_versions[*t])
            .collect::<Vec<_>>();
        let used = graph.program.rules[rule]
            .query
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != pivot)
            .flat_map(|(_, a)| a.args.iter())
            .filter_map(|a| if let Arg::Slot(s) = a { Some(*s) } else { None })
            .collect::<BTreeSet<_>>();
        let key = (
            rule,
            pivot,
            if self.projected {
                used.iter()
                    .filter_map(|s| values[*s].clone().map(Some))
                    .collect()
            } else {
                values.to_vec()
            },
        );
        let kind = match self.keys.get(&key) {
            None => 0,
            Some(old) if old.versions != versions => 1,
            Some(old) if old.epoch == graph.epoch => 2,
            Some(_) => 3,
        };
        self.keys.insert(
            key,
            Previous {
                versions,
                epoch: graph.epoch,
            },
        );
        Some(Ticket {
            rule,
            kind,
            counts: [
                graph.stats.residual_candidates,
                merge_rows,
                graph.stats.decoded_rows,
                graph.key_lookup_calls.get(),
            ],
            bindings,
            start: Instant::now(),
        })
    }
    pub fn finish(
        &mut self,
        ticket: Option<Ticket>,
        graph: &Graph,
        merge_rows: u64,
        bindings: usize,
    ) {
        let Some(t) = ticket else { return };
        let s = self.stats.entry(t.rule).or_default();
        let w = match t.kind {
            0 => &mut s.first,
            1 => &mut s.changed,
            2 => &mut s.stable_same_stage,
            _ => &mut s.stable_later_stage,
        };
        w.entries += 1;
        w.candidates += graph.stats.residual_candidates - t.counts[0];
        w.merge_candidates += merge_rows - t.counts[1];
        w.decoded_rows += graph.stats.decoded_rows - t.counts[2];
        w.key_reads += graph.key_lookup_calls.get() - t.counts[3];
        w.bindings += (bindings - t.bindings) as u64;
        w.seconds += t.start.elapsed().as_secs_f64();
    }
    pub fn report(&self, graph: &Graph) -> J {
        json!({"enabled":self.enabled,"projected_key":self.projected,"timing_eligible":!self.enabled,
            "scope":"delta seed already bound; projected key uses only anchor slots read by residual query; full mode keeps all slots; actual residual LHS work, including original marker consumers; table-wide versions are conservative; stable input is not cached answer credit",
            "distinct_contexts":self.keys.len(),"rules":self.stats.iter().map(|(rid,s)|json!({
                "rule":rid,"name":graph.program.templates[graph.program.rules[*rid].template].name,"work":s})).collect::<Vec<_>>()})
    }
}

// A positive answer context, not a static closure/negative impossibility proof.
// Only an ordinary search that encountered NO semi-naive exclusion may publish
// one. Incomplete searches remain normal searches; no extra search is performed.
use crate::{
    graph::Dependency,
    query::{QueryIndexes, ReadStamp},
    refutation::Key,
};
use std::{collections::HashMap, rc::Rc};
#[derive(Clone)]
pub struct Answer {
    pub values: Vec<Value>,
    // Relative residual-atom position, epoch, exact physical witness.
    pub witnesses: Vec<(usize, u64, Dependency)>,
}
struct Context {
    id: u64,
    tables: BTreeSet<usize>,
    answers: Rc<Vec<Answer>>,
    reads: Vec<ReadStamp>,
    rule: usize,
    epoch: u64,
}
#[derive(Default, Serialize)]
pub struct ReuseStats {
    pub probes: u64,
    pub hits: u64,
    pub later_stage_hits: u64,
    pub other_rule_hits: u64,
    pub recorded: u64,
    pub incomplete: u64,
    pub empty: u64,
    pub invalidated: u64,
    pub affected_context_checks: u64,
    pub leased_ranges_checked: u64,
    pub replay_rows: u64,
    pub delta_rejected_answers: u64,
    pub reused_bindings: u64,
    pub build_witness_reads: u64,
    pub observed_saved_candidates: u64,
    pub observed_saved_keys: u64,
    pub marker_anchored_hits: u64,
    pub observed_saved_candidates_by_rule: BTreeMap<usize, u64>,
    pub observed_saved_keys_by_rule: BTreeMap<usize, u64>,
    pub hits_by_rule: BTreeMap<usize, u64>,
    pub peak_contexts: usize,
    pub peak_answers: usize,
    pub peak_ranges: usize,
    pub lookup_invalidation_seconds: f64,
}
pub struct Reuse {
    pub enabled: bool,
    pub observe: bool,
    pub residual: bool,
    contexts: HashMap<Key, Context>,
    by_id: BTreeMap<u64, Key>,
    watchers: BTreeMap<usize, BTreeSet<u64>>,
    next_id: u64,
    answers: usize,
    ranges: usize,
    pub stats: ReuseStats,
}
impl Reuse {
    pub fn new() -> Self {
        let mode = std::env::var("CELL_CPU_SEARCH_CONTEXTS").unwrap_or_else(|_| "0".into());
        assert!(
            matches!(mode.as_str(), "0" | "1" | "observe" | "marker"),
            "CELL_CPU_SEARCH_CONTEXTS: expected 0, 1, observe, marker"
        );
        Self {
            enabled: mode != "0",
            observe: mode == "observe",
            residual: mode != "marker",
            contexts: HashMap::new(),
            by_id: BTreeMap::new(),
            watchers: BTreeMap::new(),
            next_id: 0,
            answers: 0,
            ranges: 0,
            stats: ReuseStats::default(),
        }
    }
    pub fn get(
        &mut self,
        key: &Key,
        rule: usize,
        graph: &Graph,
        indexes: &QueryIndexes,
    ) -> Option<Rc<Vec<Answer>>> {
        let clock = self.observe.then(Instant::now);
        self.stats.probes += 1;
        let found = self.contexts.get(key).and_then(|c| {
            c.reads
                .iter()
                .all(|r| {
                    self.stats.leased_ranges_checked += 1;
                    indexes.valid_read(graph, r)
                })
                .then_some(c)
        });
        let answer = found.map(|c| {
            self.stats.hits += 1;
            self.stats.later_stage_hits += u64::from(c.epoch != graph.epoch);
            self.stats.other_rule_hits += u64::from(c.rule != rule);
            *self.stats.hits_by_rule.entry(rule).or_default() += 1;
            c.answers.clone()
        });
        if answer.is_none() && self.remove(key) {
            self.stats.invalidated += 1;
        }
        if let Some(c) = clock {
            self.stats.lookup_invalidation_seconds += c.elapsed().as_secs_f64();
        }
        answer
    }
    fn remove(&mut self, key: &Key) -> bool {
        let Some(c) = self.contexts.remove(key) else {
            return false;
        };
        self.answers -= c.answers.len();
        self.ranges -= c.reads.len();
        self.by_id.remove(&c.id);
        for t in c.tables {
            let w = self.watchers.get_mut(&t).unwrap();
            w.remove(&c.id);
            if w.is_empty() {
                self.watchers.remove(&t);
            }
        }
        true
    }
    pub fn insert(
        &mut self,
        key: Key,
        answers: Vec<Answer>,
        reads: Vec<ReadStamp>,
        rule: usize,
        epoch: u64,
    ) {
        self.remove(&key);
        self.stats.recorded += 1;
        let tables = reads
            .iter()
            .map(|r| match r {
                ReadStamp::Table { table, .. } | ReadStamp::Bucket { table, .. } => *table,
            })
            .collect::<BTreeSet<_>>();
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("context identity exhausted");
        self.by_id.insert(id, key.clone());
        for &t in &tables {
            self.watchers.entry(t).or_default().insert(id);
        }
        self.answers += answers.len();
        self.ranges += reads.len();
        self.contexts.insert(
            key,
            Context {
                id,
                tables,
                answers: Rc::new(answers),
                reads,
                rule,
                epoch,
            },
        );
        self.stats.peak_contexts = self.stats.peak_contexts.max(self.contexts.len());
        self.stats.peak_answers = self.stats.peak_answers.max(self.answers);
        self.stats.peak_ranges = self.stats.peak_ranges.max(self.ranges);
    }
    pub fn publish(&mut self, graph: &Graph, indexes: &QueryIndexes, changed: &BTreeSet<usize>) {
        if !self.enabled {
            return;
        }
        let clock = self.observe.then(Instant::now);
        // Only contexts that read a modified table are eligible for invalidation.
        // Leases then distinguish changed buckets from unrelated rows in it.
        let affected = changed
            .iter()
            .filter_map(|t| self.watchers.get(t))
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>();
        self.stats.affected_context_checks += affected.len() as u64;
        for id in affected {
            let key = &self.by_id[&id];
            let c = &self.contexts[key];
            if !c.reads.iter().all(|r| {
                self.stats.leased_ranges_checked += 1;
                indexes.valid_read(graph, r)
            }) {
                let key = key.clone();
                self.remove(&key);
                self.stats.invalidated += 1;
            }
        }
        if let Some(c) = clock {
            self.stats.lookup_invalidation_seconds += c.elapsed().as_secs_f64();
        }
    }
    pub fn report(&self) -> J {
        json!({"enabled":self.enabled,"observe":self.observe,"residual_joins":self.residual,"stats":self.stats,
            "live_contexts":self.contexts.len(),"live_answers":self.contexts.values().map(|c|c.answers.len()).sum::<usize>(),
            "scope":"complete positive residual joins; alpha-normalized binding and exact versioned read ranges; original semi-naive partition rechecked; no negative answers or RHS skipped; not a RipSat closure certificate"})
    }
}
