//! Residual-search ablation: learned producer pairs versus equality-aware indexes.
//! Native egglog supplies committed facts and canonicalization; search variants are offline.
use egglog::{EGraph, TraceSession, Value, WriteOutcome};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
struct Row {
    key: i64,
    payload: i64,
    tag: Value,
    source: Option<i64>,
    side: bool,
}
#[derive(Default)]
struct Search {
    visits: usize,
    guard_checks: usize,
    fast: usize,
    fallback: usize,
    outputs: BTreeSet<[i64; 3]>,
}
fn run() -> serde_json::Value {
    let (keys, groups, fanout) = (4i64, 8i64, 4i64);
    let mut eg = EGraph::default();
    let mut setup = String::from(
        "(datatype Group (Tag i64)) (relation SeedP (i64 i64 i64)) (relation SeedQ (i64 i64 i64)) (relation P (i64 Group i64)) (relation Q (i64 Group i64)) (relation Out (i64 i64 i64)) (ruleset produce) (ruleset consume)\n",
    );
    for g in 0..groups {
        setup += &format!(
            "(Tag {g})\n(rule ((SeedP k {g} x)) ((P k (Tag {g}) x)) :ruleset produce :name \"make-p-{g}\")\n(rule ((SeedQ k {g} z)) ((Q k (Tag {g}) z)) :ruleset produce :name \"make-q-{g}\")\n"
        );
    }
    setup += "(rule ((P k g x) (Q k g z)) ((Out k x z)) :ruleset consume :name \"consume\")";
    eg.parse_and_run_program(None, &setup).unwrap();
    let mut ordinary = eg.clone();
    let trace = TraceSession::with_dependencies();
    let sort = eg.get_sort_by_name("Group").unwrap().clone();
    let mut facts = Vec::<Row>::new();
    let mut manifest = Vec::<(bool, i64, i64, i64)>::new();
    let mut learned = BTreeSet::new();
    let mut phases = Vec::new();
    let mut previous_keys = BTreeMap::new();
    let mut initial_origin_keys = BTreeMap::new();
    for phase in 0..4 {
        let mut text = String::new();
        if phase <= 1 {
            let range = if phase == 0 { 0..1 } else { 1..fanout };
            for k in 0..keys {
                for g in 0..groups {
                    for j in range.clone() {
                        for side in [false, true] {
                            let payload =
                                (if side { 1_000_000 } else { 0 }) + k * 10000 + g * 100 + j;
                            text += &format!(
                                "({} {k} {g} {payload})\n",
                                if side { "SeedQ" } else { "SeedP" }
                            );
                            manifest.push((side, k, g, payload));
                        }
                    }
                }
            }
        } else if phase == 2 {
            for k in 0..keys {
                for side in [false, true] {
                    let payload = (if side { 2_000_000 } else { 3_000_000 }) + k;
                    text += &format!("({} {k} (Tag 2) {payload})\n", if side { "Q" } else { "P" });
                    manifest.push((side, k, 2, payload));
                }
            }
        } else {
            text += "(union (Tag 0) (Tag 1))";
        }
        eg.parse_and_run_program(None, &text).unwrap();
        ordinary.parse_and_run_program(None, &text).unwrap();
        if phase == 2 {
            for &(side, k, g, payload) in manifest.iter().rev().take((keys * 2) as usize) {
                let tag = eg.lookup_function("Tag", &[eg.base_to_value(g)]).unwrap();
                facts.push(Row {
                    key: k,
                    payload,
                    tag,
                    source: None,
                    side,
                });
            }
        }
        let before = trace.write_events().len();
        eg.step_rules_with_trace("produce", &trace).unwrap();
        ordinary.step_rules("produce").unwrap();
        let names: BTreeMap<_, _> = trace.table_names().into_iter().collect();
        let matches: BTreeMap<_, _> = trace
            .matches()
            .into_iter()
            .map(|m| (m.event_id, m))
            .collect();
        for w in trace
            .write_events()
            .into_iter()
            .skip(before)
            .filter(|w| w.outcome == WriteOutcome::Inserted && w.rebuild_of.is_none())
        {
            let Some(name) = names.get(&w.table) else {
                continue;
            };
            if name.as_ref() != "P" && name.as_ref() != "Q" {
                continue;
            }
            let side = name.as_ref() == "Q";
            let rule = matches[&w.match_event_id].rule.as_ref();
            let group: i64 = rule
                .strip_prefix(if side { "make-q-" } else { "make-p-" })
                .unwrap()
                .parse()
                .unwrap();
            facts.push(Row {
                key: eg.value_to_base(w.actual[0]),
                payload: eg.value_to_base(w.actual[2]),
                tag: w.actual[1],
                source: Some(group),
                side,
            });
        }
        assert_eq!(facts.len(), manifest.len());
        eg.step_rules_with_trace("consume", &trace).unwrap();
        ordinary.step_rules("consume").unwrap();
        let mut expected = BTreeSet::new();
        let canonical_group = |g| if phase == 3 && g == 1 { 0 } else { g };
        for &(sa, k, g, x) in &manifest {
            if sa {
                continue;
            }
            for &(sb, l, h, z) in &manifest {
                if sb && k == l && canonical_group(g) == canonical_group(h) {
                    expected.insert([k, x, z]);
                }
            }
        }
        for graph in [&eg, &ordinary] {
            assert_eq!(graph.get_size("Out"), expected.len());
            for tuple in &expected {
                let key: Vec<_> = tuple.iter().map(|&v| graph.base_to_value(v)).collect();
                assert!(graph.lookup_function("Out", &key).is_some());
            }
        }
        let canonical = |r: &Row| eg.get_canonical_value(r.tag, &sort);
        let mut changed = 0;
        let mut composite: BTreeMap<(i64, Value), Vec<&Row>> = BTreeMap::new();
        let mut by_source: BTreeMap<(i64, Option<i64>), Vec<&Row>> = BTreeMap::new();
        let mut by_key: BTreeMap<i64, Vec<&Row>> = BTreeMap::new();
        for r in &facts {
            let id = (r.side, r.payload);
            let now = canonical(r);
            if previous_keys.insert(id, now).is_some_and(|old| old != now) {
                changed += 1;
            }
            initial_origin_keys.entry(id).or_insert(now);
            if r.side {
                composite.entry((r.key, now)).or_default().push(r);
                by_key.entry(r.key).or_default().push(r);
                by_source.entry((r.key, r.source)).or_default().push(r);
            }
        }
        if phase == 0 {
            for p in facts.iter().filter(|r| !r.side) {
                for q in facts.iter().filter(|r| r.side) {
                    if expected.contains(&[p.key, p.payload, q.payload]) && p.key == q.key {
                        learned.insert((p.source.unwrap(), q.source.unwrap()));
                    }
                }
            }
        }
        let predicted = |p: &Row, q: &Row| match (p.source, q.source) {
            (Some(a), Some(b)) => learned.contains(&(a, b)),
            _ => false,
        };
        let mut variants = Vec::new();
        for mode in [
            "key_only",
            "canonical_composite",
            "learned_source_only",
            "learned_source_with_fallback",
            "stale_composite",
        ] {
            let mut s = Search::default();
            for p in facts.iter().filter(|r| !r.side) {
                let candidates: Vec<&Row> = match mode {
                    "canonical_composite" | "learned_source_with_fallback" => composite
                        .get(&(p.key, canonical(p)))
                        .cloned()
                        .unwrap_or_default(),
                    "learned_source_only" => p
                        .source
                        .map(|a| {
                            learned
                                .iter()
                                .filter(|(src, _)| *src == a)
                                .flat_map(|(_, b)| {
                                    by_source
                                        .get(&(p.key, Some(*b)))
                                        .into_iter()
                                        .flatten()
                                        .copied()
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    "stale_composite" => facts
                        .iter()
                        .filter(|q| {
                            q.side
                                && q.key == p.key
                                && initial_origin_keys[&(q.side, q.payload)]
                                    == initial_origin_keys[&(p.side, p.payload)]
                        })
                        .collect(),
                    _ => by_key.get(&p.key).cloned().unwrap_or_default(),
                };
                for q in candidates {
                    // Source-only uses the learned rule-pair routing before residual validation.
                    if mode == "learned_source_only" && !predicted(p, q) {
                        continue;
                    }
                    s.visits += 1;
                    if mode == "key_only"
                        || mode == "learned_source_only"
                        || mode == "stale_composite"
                    {
                        s.guard_checks += 1;
                        if canonical(p) != canonical(q) {
                            continue;
                        }
                    }
                    if mode == "learned_source_with_fallback" {
                        if predicted(p, q) {
                            s.fast += 1;
                        } else {
                            s.fallback += 1;
                        }
                    }
                    s.outputs.insert([p.key, p.payload, q.payload]);
                }
            }
            let missing = expected.difference(&s.outputs).count();
            let extra = s.outputs.difference(&expected).count();
            if [
                "key_only",
                "canonical_composite",
                "learned_source_with_fallback",
            ]
            .contains(&mode)
            {
                assert_eq!(s.outputs, expected);
            }
            variants.push(json!({"mode":mode,"candidate_pairs_after_routing":s.visits,"residual_equality_checks":s.guard_checks,"fast_pairs":s.fast,"fallback_pairs":s.fallback,"outputs":s.outputs.len(),"missing":missing,"extra":extra}));
        }
        phases.push(json!({"phase":(["train_one_per_group","held_out_fanout_four","external_facts","union_groups_0_1"][phase]),"facts":facts.len(),"expected_outputs":expected.len(),"learned_source_pairs":learned.len(),"keys_changed_by_union":changed,"variants":variants,"index_maintenance":{"implemented_strategy":"rebuild composite index per phase","rows_scanned":facts.len(),"inserted_q_rows":facts.iter().filter(|r|r.side).count()},"logical_index_entries":{"q_row_references":facts.iter().filter(|r|r.side).count(),"canonical_buckets":composite.len(),"learned_rule_pairs":learned.len(),"source_index_q_references":facts.iter().filter(|r|r.side).count(),"source_index_buckets":by_source.len()}}));
    }
    json!({"scope":"actual ordinary/traced egglog facts and canonicalization; offline residual-search ablation, not kernel acceleration", "method":"source-pair routing learned on phase 0 only; counterexamples and safe fallback evaluated on later phases", "timing":"no performance claim; operation counts only", "phases":phases})
}
fn main() {
    let result = run();
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
    fn source_routing_requires_external_and_union_fallback() {
        let data = run();
        let phases = data["phases"].as_array().unwrap();
        assert_eq!(phases[1]["variants"][2]["missing"], 0);
        assert!(phases[2]["variants"][2]["missing"].as_u64().unwrap() > 0);
        assert!(phases[3]["variants"][4]["missing"].as_u64().unwrap() > 0);
        assert!(phases[3]["keys_changed_by_union"].as_u64().unwrap() > 0);
    }
}
