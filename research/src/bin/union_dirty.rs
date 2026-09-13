//! Bounded dirty-port cache with real egglog matching for oversized buckets.
use egglog::{EGraph, TraceSession, Value};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Default)]
struct Bucket {
    p: BTreeSet<i64>,
    q: BTreeSet<i64>,
}
#[derive(Default)]
struct Cache {
    parents: BTreeMap<Value, Value>,
    buckets: BTreeMap<(Value, i64), Bucket>,
    ports: BTreeMap<Value, BTreeSet<i64>>,
    dirty: BTreeSet<(Value, i64)>,
    notifications: usize,
    union_ports_visited: usize,
    fact_relocations: usize,
}
impl Cache {
    fn find(&self, mut v: Value) -> Value {
        while let Some(&p) = self.parents.get(&v) {
            if p == v {
                break;
            }
            v = p;
        }
        v
    }
    fn insert(&mut self, tag: Value, k: i64, x: i64, side: bool) {
        let tag = self.find(tag);
        let b = self.buckets.entry((tag, k)).or_default();
        let added = if side { b.q.insert(x) } else { b.p.insert(x) };
        self.ports.entry(tag).or_default().insert(k);
        if added {
            self.mark(tag, k);
        }
    }
    fn mark(&mut self, tag: Value, k: i64) {
        self.notifications += 1;
        self.dirty.insert((tag, k));
    }
    fn union(&mut self, lhs: Value, rhs: Value, parent: Value) {
        let a = self.find(lhs);
        let b = self.find(rhs);
        if a == b {
            return;
        }
        let parent = self.find(parent);
        assert!(parent == a || parent == b);
        let child = if parent == a { b } else { a };
        self.parents.insert(child, parent);
        let keys = self.ports.remove(&child).unwrap_or_default();
        self.union_ports_visited += keys.len();
        for k in keys {
            let old = self.buckets.remove(&(child, k)).unwrap();
            self.fact_relocations += old.p.len() + old.q.len();
            let dst = self.buckets.entry((parent, k)).or_default();
            dst.p.extend(old.p);
            dst.q.extend(old.q);
            self.ports.entry(parent).or_default().insert(k);
            self.mark(parent, k);
        }
    }
    fn take_dirty(&mut self) -> BTreeSet<(Value, i64)> {
        std::mem::take(&mut self.dirty)
            .into_iter()
            .map(|(g, k)| (self.find(g), k))
            .collect()
    }
    fn pairs(&self) -> BTreeSet<[i64; 3]> {
        self.buckets
            .iter()
            .flat_map(|((_, k), b)| {
                b.p.iter()
                    .flat_map(move |x| b.q.iter().map(move |z| [*k, *x, *z]))
            })
            .collect()
    }
}
fn run(budget: usize) -> serde_json::Value {
    let (groups, keys, fanout) = (16i64, 16i64, 2i64);
    let setup = "(datatype Group (Tag i64)) (relation P (i64 Group i64)) (relation Q (i64 Group i64)) (relation Out (i64 i64 i64)) (relation Merge (i64 i64)) (ruleset merge) (ruleset consume) (rule ((Merge a b) (= x (Tag a)) (= y (Tag b))) ((union x y)) :ruleset merge :name \"merge\") (rule ((P k c x) (Q k c z)) ((Out k x z)) :ruleset consume :name \"consume\")";
    let setup = format!(
        "{setup} (relation Pending (i64 Group)) (ruleset fallback) (rule ((Pending k c) (P k c x) (Q k c z)) ((Out k x z)) :ruleset fallback :name \"fallback\")"
    );
    let mut optimized = EGraph::default();
    optimized.parse_and_run_program(None, &setup).unwrap();
    let mut text = String::new();
    for g in 0..groups {
        text += &format!("(Tag {g}) ");
        for k in 0..keys {
            if k % 4 != g % 4 {
                continue;
            }
            for j in 0..fanout {
                text += &format!(
                    "(P {k} (Tag {g}) {}) (Q {k} (Tag {g}) {}) ",
                    g * 10000 + k * 10 + j,
                    1000000 + g * 10000 + k * 10 + j
                );
            }
        }
    }
    optimized.parse_and_run_program(None, &text).unwrap();
    let mut ordinary = optimized.clone();
    optimized.step_rules("consume").unwrap();
    ordinary.step_rules("consume").unwrap();
    let sort = optimized.get_sort_by_name("Group").unwrap().clone();
    let tags: Vec<_> = (0..groups)
        .map(|g| {
            optimized
                .lookup_function("Tag", &[optimized.base_to_value(g)])
                .unwrap()
        })
        .collect();
    let mut cache = Cache::default();
    for g in 0..groups {
        for k in 0..keys {
            if k % 4 != g % 4 {
                continue;
            }
            for j in 0..fanout {
                cache.insert(tags[g as usize], k, g * 10000 + k * 10 + j, false);
                cache.insert(tags[g as usize], k, 1000000 + g * 10000 + k * 10 + j, true);
            }
        }
    }
    cache.take_dirty();
    cache.notifications = 0;
    let trace = TraceSession::with_dependencies();
    let mut phases = Vec::new();

    for (phase, merges, late) in [
        ("coalesced_unions", vec![(0, 4), (4, 8)], false),
        ("disjoint_ports", vec![(1, 2)], false),
        ("redundant_union", vec![(0, 8)], false),
        ("late_fact", vec![], true),
    ] {
        let previous = optimized.get_size("Out");
        let before = trace.union_events().len();
        let mut actual_unions = 0;
        for (a, b) in merges {
            let program = format!("(Merge {a} {b})");
            optimized.parse_and_run_program(None, &program).unwrap();
            ordinary.parse_and_run_program(None, &program).unwrap();
            optimized.step_rules_with_trace("merge", &trace).unwrap();
            ordinary.step_rules("merge").unwrap();
        }
        for u in trace.union_events().into_iter().skip(before) {
            if u.displaced.is_some() {
                actual_unions += 1;
                cache.union(u.lhs, u.rhs, u.canonical);
            }
        }
        if late {
            let text = "(P 0 (Tag 4) 9000000)";
            optimized.parse_and_run_program(None, text).unwrap();
            ordinary.parse_and_run_program(None, text).unwrap();
            cache.insert(tags[4], 0, 9000000, false);
        }
        let signals = std::mem::take(&mut cache.notifications);
        let pending = cache.dirty.len();
        let dirty = cache.take_dirty();
        let mut enumerated = 0;
        let mut fallbacks = 0;
        let mut fallback_matches = 0;
        let mut estimated_fallback_pairs = 0;
        let mut max_local_batch = 0;
        let mut fallback_requests = Vec::new();
        for &(g, k) in &dirty {
            let b = &cache.buckets[&(g, k)];
            let product = b.p.len().saturating_mul(b.q.len());
            if product > budget {
                fallbacks += 1;
                estimated_fallback_pairs += product;
                let representative = tags
                    .iter()
                    .position(|&t| optimized.get_canonical_value(t, &sort) == g)
                    .unwrap();
                fallback_requests.push((k, representative));
            } else {
                max_local_batch = max_local_batch.max(product);
                enumerated += product;
                for &x in &b.p {
                    for &z in &b.q {
                        optimized
                            .parse_and_run_program(None, &format!("(Out {k} {x} {z})"))
                            .unwrap();
                    }
                }
            }
        }
        if !fallback_requests.is_empty() {
            let requests = fallback_requests
                .iter()
                .map(|(k, g)| format!("(Pending {k} (Tag {g}))"))
                .collect::<Vec<_>>()
                .join(" ");
            optimized.parse_and_run_program(None, &requests).unwrap();
            let fallback_trace = TraceSession::with_dependencies();
            optimized
                .step_rules_with_trace("fallback", &fallback_trace)
                .unwrap();
            fallback_matches = fallback_trace.matches().len();
            let cleanup = fallback_requests
                .iter()
                .map(|(k, g)| format!("(delete (Pending {k} (Tag {g})))"))
                .collect::<Vec<_>>()
                .join(" ");
            optimized.parse_and_run_program(None, &cleanup).unwrap();
        }
        assert_eq!(optimized.get_size("Pending"), 0);
        let mut baseline_probe = ordinary.clone();
        let baseline_trace = TraceSession::with_dependencies();
        baseline_probe
            .step_rules_with_trace("consume", &baseline_trace)
            .unwrap();
        let ordinary_candidates = baseline_trace.matches().len();
        ordinary.step_rules("consume").unwrap();
        // Build a separate oracle from immutable original rows and native canonicalization,
        // rather than trusting the cache's union/merge implementation.
        let mut p = Vec::new();
        let mut q = Vec::new();
        for g in 0..groups {
            for k in 0..keys {
                if k % 4 != g % 4 {
                    continue;
                }
                for j in 0..fanout {
                    p.push((
                        k,
                        optimized.get_canonical_value(tags[g as usize], &sort),
                        g * 10000 + k * 10 + j,
                    ));
                    q.push((
                        k,
                        optimized.get_canonical_value(tags[g as usize], &sort),
                        1000000 + g * 10000 + k * 10 + j,
                    ));
                }
            }
        }
        if late {
            p.push((0, optimized.get_canonical_value(tags[4], &sort), 9000000));
        }
        let mut expected = BTreeSet::new();
        for &(k, g, x) in &p {
            for &(l, h, z) in &q {
                if k == l && g == h {
                    expected.insert([k, x, z]);
                }
            }
        }
        assert_eq!(cache.pairs(), expected);
        for eg in [&optimized, &ordinary] {
            assert_eq!(eg.get_size("Out"), expected.len());
            for row in &expected {
                let key: Vec<_> = row.iter().map(|&v| eg.base_to_value(v)).collect();
                assert!(eg.lookup_function("Out", &key).is_some());
            }
        }
        assert!(max_local_batch <= budget);
        phases.push(json!({"phase":phase,"ordinary_consumer_diagnostic_candidates":ordinary_candidates,"union_ports_visited":std::mem::take(&mut cache.union_ports_visited),"fact_relocations":std::mem::take(&mut cache.fact_relocations),"actual_unions":actual_unions,"dirty_signals":signals,"pending_before_canonical_dedup":pending,"dirty_buckets":dirty.len(),"all_live_buckets":cache.buckets.len(),"local_pair_visits":enumerated,"temporary_request_rows_after_flush":optimized.get_size("Pending"),"native_fallback_calls":usize::from(fallbacks>0),"fallback_buckets":fallbacks,"native_fallback_candidates":fallback_matches,"estimated_fallback_pair_space":estimated_fallback_pairs,"max_local_batch":max_local_batch,"new_outputs":expected.len()-previous,"total_outputs":expected.len(),"global_rescan_pair_space":expected.len(),"cache_storage":{"fact_references":p.len()+q.len(),"reverse_port_entries":cache.ports.values().map(BTreeSet::len).sum::<usize>(),"parent_links":cache.parents.len(),"pending_after_flush":cache.dirty.len()},"checks":"cache, isolated hybrid execution and ordinary egglog agree on every tuple"}));
    }
    json!({"budget_pairs":budget,"scope":"shared dirty-port prototype, actual native scoped matching fallback, not an integrated egglog scheduler or RSS benchmark","phases":phases})
}
fn main() {
    let result = json!({"runs":[run(usize::MAX),run(8),run(0)],"limits":"Budget caps per-bucket local enumeration work only, not memory or total phase work; shared fact index, native fallback trace/work and output storage are not capped."});
    let text = serde_json::to_string_pretty(&result).unwrap();
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(path, text).unwrap();
    } else {
        println!("{text}");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coalescing_fallback_redundancy_and_late_inputs() {
        for budget in [usize::MAX, 8, 0] {
            let x = run(budget);
            let p = x["phases"].as_array().unwrap();
            assert!(
                p[0]["dirty_signals"].as_u64().unwrap() > p[0]["dirty_buckets"].as_u64().unwrap()
            );
            assert_eq!(p[2]["dirty_buckets"], 0);
            assert_eq!(p[3]["dirty_buckets"], 1);
            if budget == 0 {
                assert_eq!(p[0]["local_pair_visits"], 0);
                assert_eq!(p[0]["native_fallback_calls"], 1);
            }
        }
    }
}
