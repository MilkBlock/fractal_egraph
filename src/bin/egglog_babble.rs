//! Run anti-unification in actual egglog, compare canonical candidates to babble.
use egglog::{EGraph, Value};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Write,
};
#[derive(Clone)]
enum Pat {
    Hole(i64, i64),
    Branch(String, Value),
}
fn pattern(
    v: Value,
    ps: &HashMap<Value, Pat>,
    lists: &HashMap<Value, Option<(Value, Value)>>,
    holes: &mut BTreeMap<(i64, i64), usize>,
) -> Json {
    match &ps[&v] {
        Pat::Hole(a, b) => {
            let n = holes.len();
            let i = *holes.entry((*a, *b)).or_insert(n);
            json!({"hole":i})
        }
        Pat::Branch(op, l) => {
            let mut args = vec![];
            let mut l = *l;
            while let Some((p, rest)) = lists[&l] {
                args.push(pattern(p, ps, lists, holes));
                l = rest;
            }
            json!({"op":op,"args":args})
        }
    }
}
fn counts(p: &Json) -> (usize, usize) {
    if p.get("hole").is_some() {
        return (0, 1);
    }
    p["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(counts)
        .fold((1, 0), |(n, h), (a, b)| (n + a, h + b))
}
fn run(f: &Json) -> Json {
    assert!(f["max_arity"].as_u64().is_some_and(|n| n <= 3));
    let nodes = f["nodes"].as_array().unwrap();
    let ids: BTreeSet<_> = nodes.iter().map(|n| n["id"].as_u64().unwrap()).collect();
    assert_eq!(
        ids.len(),
        nodes.len(),
        "multiple nodes per class are not supported yet"
    );
    let mut arities = BTreeMap::new();
    for n in nodes {
        let id = n["id"].as_u64().unwrap();
        let children = n["children"].as_array().unwrap();
        for c in children {
            let child = c.as_u64().unwrap();
            assert!(
                child < id && ids.contains(&child),
                "expected topologically ordered acyclic syntax IDs"
            );
        }
        if let Some(old) = arities.insert(n["op"].as_str().unwrap(), children.len()) {
            assert_eq!(old, children.len(), "operator must have a fixed arity");
        }
    }
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        include_str!("../../experiments/egglog_babble/anti_unify.egg"),
    )
    .unwrap();
    let mut source = format!("(Limit {})\n", f["max_arity"]);
    let mut next_list = 1usize;
    let mut intern = BTreeMap::<Vec<u64>, usize>::from([(vec![], 0)]);
    for n in f["nodes"].as_array().unwrap() {
        let children: Vec<_> = n["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap())
            .collect();
        let mut tail = 0;
        for i in (0..children.len()).rev() {
            let suffix = children[i..].to_vec();
            tail = if let Some(&id) = intern.get(&suffix) {
                id
            } else {
                let id = next_list;
                next_list += 1;
                writeln!(source, "(Cons {id} {} {tail})", children[i]).unwrap();
                intern.insert(suffix, id);
                id
            };
        }
        writeln!(source, "(Node {} {} {tail})", n["id"], n["op"]).unwrap();
    }
    eg.parse_and_run_program(None, &source).unwrap();
    source.clear();
    let (pairs, co_report) = if f.get("roots").is_some() {
        co_occurrence(&mut eg, f)
    } else {
        (
            f["pairs"].as_array().unwrap().clone(),
            json!({"mode":"explicit AU unit-test fixture"}),
        )
    };
    for (id, p) in pairs.iter().enumerate() {
        writeln!(source, "(Need {} {})", p[0], p[1]).unwrap();
        if p[2] == true {
            writeln!(source, "(Allowed {} {} {id})", p[0], p[1]).unwrap();
        } else {
            writeln!(source, "(Dead {} {})", p[0], p[1]).unwrap();
        }
    }
    eg.parse_and_run_program(None, &source).unwrap();
    eg.parse_and_run_program(None, "(run-schedule (saturate (run)))")
        .unwrap();
    let mut pats = HashMap::new();
    eg.function_for_each("Hole", |row| {
        let v = row.vals;
        pats.insert(
            v[2],
            Pat::Hole(eg.value_to_base(v[0]), eg.value_to_base(v[1])),
        );
    })
    .unwrap();
    eg.function_for_each("Branch", |row| {
        let v = row.vals;
        let op: egglog::sort::S = eg.value_to_base(v[0]);
        pats.insert(v[2], Pat::Branch(op.0, v[1]));
    })
    .unwrap();
    let mut lists = HashMap::new();
    eg.function_for_each("PNil", |r| {
        lists.insert(r.vals[0], None);
    })
    .unwrap();
    eg.function_for_each("PCons", |r| {
        let v = r.vals;
        lists.insert(v[2], Some((v[0], v[1])));
    })
    .unwrap();
    let mut candidates = BTreeSet::new();
    let mut au_rows = 0;
    eg.function_for_each("AU", |r| {
        au_rows += 1;
        let mut holes = BTreeMap::new();
        let p = pattern(r.vals[2], &pats, &lists, &mut holes);
        let (nodes, uses) = counts(&p);
        if nodes > 0 && !holes.is_empty() && (holes.len() < uses || nodes > holes.len() + 1) {
            candidates.insert(p.to_string());
        }
    })
    .unwrap();
    let expected: BTreeSet<_> = f["expected"]
        .as_array()
        .unwrap()
        .iter()
        .map(Json::to_string)
        .collect();
    eprintln!("AU complete: {} candidates", candidates.len());
    let dedup = if let Some(reference) = f.get("expected_dedup_signatures") {
        match_candidates(&mut eg, &candidates, reference)
    } else {
        Json::Null
    };
    json!({"engine":"native egglog fixed-point AU rules","input_nodes":f["nodes"].as_array().unwrap().len(),"pair_frontier":f["pairs"].as_array().unwrap().len(),"au_rows":au_rows,"actual_candidates":candidates.len(),"expected_candidates":expected.len(),"exact_candidate_parity":candidates==expected,"matching_and_dedup":dedup,"co_occurrence":co_report,
        "missing":expected.difference(&candidates).take(10).map(|s|serde_json::from_str::<Json>(s).unwrap()).collect::<Vec<_>>(),
        "extra":candidates.difference(&expected).take(10).map(|s|serde_json::from_str::<Json>(s).unwrap()).collect::<Vec<_>>(),
        "scope":"raw candidates before deduplication and library selection; finite acyclic syntax DAG; native co-occurrence when roots are supplied; not an equality-saturation or full babble replacement"})
}
fn co_occurrence(eg: &mut EGraph, f: &Json) -> (Vec<Json>, Json) {
    eg.parse_and_run_program(
        None,
        include_str!("../../experiments/egglog_babble/co_occurrence.egg"),
    )
    .unwrap();
    let nodes: BTreeMap<_, _> = f["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| (n["id"].as_u64().unwrap(), n))
        .collect();
    let mut pairs = BTreeSet::new();
    let mut pending: Vec<_> = nodes
        .iter()
        .flat_map(|(&a, x)| {
            nodes
                .iter()
                .filter(move |(_, y)| x["op"] == y["op"])
                .map(move |(&b, _)| (a, b))
        })
        .collect();
    while let Some((a, b)) = pending.pop() {
        if !pairs.insert((a, b)) {
            continue;
        }
        if nodes[&a]["op"] == nodes[&b]["op"] {
            pending.extend(
                nodes[&a]["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(nodes[&b]["children"].as_array().unwrap())
                    .map(|(a, b)| (a.as_u64().unwrap(), b.as_u64().unwrap())),
            );
        }
    }
    let mut source = String::new();
    for (pos, root) in f["roots"].as_array().unwrap().iter().enumerate() {
        assert!(nodes.contains_key(&root.as_u64().unwrap()));
        writeln!(source, "(Use -1 {pos} {root})").unwrap();
    }
    for (&id, n) in &nodes {
        for (pos, child) in n["children"].as_array().unwrap().iter().enumerate() {
            writeln!(source, "(Use {id} {pos} {child})").unwrap();
        }
    }
    for &(a, b) in &pairs {
        writeln!(source, "(CoQuery {a} {b})").unwrap();
    }
    eg.parse_and_run_program(None, &source).unwrap();
    eg.parse_and_run_program(None, "(run-schedule (saturate (run)))")
        .unwrap();
    let mut yes = BTreeSet::new();
    eg.function_for_each("CoYes", |r| {
        yes.insert((
            eg.value_to_base::<i64>(r.vals[0]) as u64,
            eg.value_to_base::<i64>(r.vals[1]) as u64,
        ));
    })
    .unwrap();
    let actual: Vec<_> = pairs
        .iter()
        .map(|&(a, b)| json!([a, b, yes.contains(&(a, b))]))
        .collect();
    let expected: BTreeSet<_> = f["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .map(Json::to_string)
        .collect();
    let actual_set: BTreeSet<_> = actual.iter().map(Json::to_string).collect();
    let report = json!({"mode":"native egglog capped occurrence analysis","frontier_from_nodes_only":true,"queried_pairs":pairs.len(),"true_pairs":yes.len(),"exact_parity":actual_set==expected});
    (actual, report)
}
fn match_candidates(eg: &mut EGraph, candidates: &BTreeSet<String>, reference: &Json) -> Json {
    eg.parse_and_run_program(None,"(sort BS) (constructor BNil () BS) (constructor BCons (i64 BS) BS) (relation Matched (i64 i64 BS)) (ruleset candidates)").unwrap();
    eg.parse_and_run_program(
        None,
        r#"
      (function NOp (i64) String :merge old)
      (function NArgs (i64) i64 :merge old)
      (function LHead (i64) i64 :merge old)
      (function LTail (i64) i64 :merge old)
      (ruleset syntax-index)
      (rule ((Node n op xs)) ((set (NOp n) op) (set (NArgs n) xs)) :ruleset syntax-index)
      (rule ((Cons xs a rest)) ((set (LHead xs) a) (set (LTail xs) rest)) :ruleset syntax-index)
      (run syntax-index 1)
    "#,
    )
    .unwrap();
    eg.parse_and_run_program(None, "(relation SubMatch (i64 i64 i64 i64 i64))")
        .unwrap();
    fn slots(holes: &BTreeSet<usize>) -> String {
        (0..3)
            .map(|i| {
                if holes.contains(&i) {
                    format!("h{i}")
                } else {
                    "-1".into()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn fragment(
        p: &Json,
        cache: &mut BTreeMap<String, (usize, BTreeSet<usize>)>,
        source: &mut String,
    ) -> (usize, BTreeSet<usize>) {
        let key = p.to_string();
        if let Some(old) = cache.get(&key) {
            return old.clone();
        }
        let args = p["args"].as_array().unwrap();
        let mut holes = BTreeSet::new();
        let mut body = vec![format!("(= {} (NOp root))", p["op"])];
        body.push(format!(
            "(= {} (NArgs root))",
            if args.is_empty() { "0" } else { "xs0" }
        ));
        for (i, a) in args.iter().enumerate() {
            let child = if let Some(h) = a.get("hole") {
                let h = h.as_u64().unwrap() as usize;
                assert!(h < 3);
                holes.insert(h);
                format!("h{h}")
            } else {
                let (id, childholes) = fragment(a, cache, source);
                holes.extend(&childholes);
                body.push(format!("(SubMatch {id} child{i} {})", slots(&childholes)));
                format!("child{i}")
            };
            body.push(format!("(= {child} (LHead xs{i}))"));
            body.push(format!(
                "(= {} (LTail xs{i}))",
                if i + 1 == args.len() {
                    "0".into()
                } else {
                    format!("xs{}", i + 1)
                }
            ));
        }
        let id = cache.len();
        cache.insert(key, (id, holes.clone()));
        writeln!(
            source,
            "(rule ({}) ((SubMatch {id} root {})) :ruleset candidates)",
            body.join(" "),
            slots(&holes)
        )
        .unwrap();
        (id, holes)
    }
    let mut source = String::new();
    let mut sizes = vec![];
    let mut fragments = BTreeMap::new();
    for (i, text) in candidates.iter().enumerate() {
        let p: Json = serde_json::from_str(text).unwrap();
        let (n, h) = counts(&p);
        sizes.push(n + h);
        let (id, holes) = fragment(&p, &mut fragments, &mut source);
        let mut args = "(BNil)".to_owned();
        for h in holes.iter().rev() {
            args = format!("(BCons h{h} {args})");
        }
        writeln!(
            source,
            "(rule ((SubMatch {id} root {})) ((Matched {i} root {args})) :ruleset candidates)",
            slots(&holes)
        )
        .unwrap();
    }
    eg.parse_and_run_program(None, &source).unwrap();
    eg.parse_and_run_program(None, "(run-schedule (saturate (run candidates)))")
        .unwrap();
    let mut lists = HashMap::new();
    eg.function_for_each("BNil", |r| {
        lists.insert(r.vals[0], None);
    })
    .unwrap();
    eg.function_for_each("BCons", |r| {
        let v = r.vals;
        lists.insert(v[2], Some((eg.value_to_base::<i64>(v[0]), v[1])));
    })
    .unwrap();
    let mut matches = vec![vec![]; sizes.len()];
    let mut rows = 0;
    eg.function_for_each("Matched", |r| {
        rows += 1;
        let id = eg.value_to_base::<i64>(r.vals[0]) as usize;
        let root = eg.value_to_base::<i64>(r.vals[1]);
        let mut v = r.vals[2];
        let mut actuals = vec![];
        while let Some((a, tail)) = lists[&v] {
            actuals.push(a);
            v = tail;
        }
        actuals.sort();
        matches[id].push((root, actuals));
    })
    .unwrap();
    let mut grouped = BTreeMap::<Vec<(i64, Vec<i64>)>, (usize, usize)>::new();
    for (id, (mut signature, size)) in matches.into_iter().zip(sizes).enumerate() {
        signature.sort();
        grouped
            .entry(signature)
            .and_modify(|s| *s = (*s).min((size, id)))
            .or_insert((size, id));
    }
    let actual: BTreeSet<_> = grouped
        .iter()
        .map(|(m, s)| json!({"matches":m,"size":s.0}).to_string())
        .collect();
    let expected: BTreeSet<_> = reference
        .as_array()
        .unwrap()
        .iter()
        .map(Json::to_string)
        .collect();
    json!({"selected_candidates":grouped.values().map(|(_,id)|serde_json::from_str::<Json>(candidates.iter().nth(*id).unwrap()).unwrap()).collect::<Vec<_>>(),"native_match_rows":rows,"actual_groups":actual.len(),"expected_groups":expected.len(),"signature_and_minimum_size_parity":actual==expected,"missing_groups":expected.difference(&actual).count(),"extra_groups":actual.difference(&expected).count(),"scope":"native egglog matching; Rust groups exact match signatures and chooses minimum pattern size; tied syntactic representatives need not be identical"})
}
fn main() {
    let a: Vec<_> = std::env::args().collect();
    assert_eq!(a.len(), 3, "egglog_babble fixture.json report.json");
    let fixture: Json = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let mut report = run(&fixture);
    if !report["matching_and_dedup"].is_null() {
        let patterns = report["matching_and_dedup"]["selected_candidates"].take();
        let path = format!("{}.candidates.json", a[2]);
        std::fs::write(&path, serde_json::to_string(&patterns).unwrap()).unwrap();
        report["candidate_file"] = json!(path);
    }
    std::fs::write(&a[2], serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("{report}");
    if fixture.get("roots").is_some() {
        assert_eq!(report["co_occurrence"]["exact_parity"], true);
    }
    assert_eq!(report["exact_candidate_parity"], true);
    if !report["matching_and_dedup"].is_null() {
        assert_eq!(
            report["matching_and_dedup"]["signature_and_minimum_size_parity"],
            true
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn co_occurrence_counts_duplicate_root_positions() {
        for (roots, yes) in [(json!([0]), false), (json!([0, 0]), true)] {
            let f = json!({"nodes":[{"id":0,"op":"leaf","children":[]}],"roots":roots,"pairs":[[0,0,yes]]});
            let (_, r) = co_occurrence(&mut EGraph::default(), &f);
            assert_eq!(r["exact_parity"], true);
        }
    }
    #[test]
    fn co_occurrence_does_not_trust_reference_flags() {
        let f = json!({"nodes":[{"id":0,"op":"leaf","children":[]}],"roots":[0,0],"pairs":[[0,0,false]]});
        let (actual, r) = co_occurrence(&mut EGraph::default(), &f);
        assert_eq!(actual, vec![json!([0, 0, true])]);
        assert_eq!(r["exact_parity"], false);
    }
    #[test]
    fn repeated_sibling_slots_count_as_two_occurrences() {
        let f = json!({"nodes":[{"id":0,"op":"leaf","children":[]},{"id":1,"op":"pair","children":[0,0]}],"roots":[1],"pairs":[[0,0,true],[1,1,false]]});
        let (_, r) = co_occurrence(&mut EGraph::default(), &f);
        assert_eq!(r["exact_parity"], true);
    }
    #[test]
    fn repeated_parent_propagates_multiplicity_and_unreachable_stays_absent() {
        let f = json!({"nodes":[{"id":0,"op":"leaf","children":[]},{"id":1,"op":"parent","children":[0]},{"id":2,"op":"unused","children":[]}],"roots":[1,1],"pairs":[[0,0,true],[1,1,true],[2,2,false]]});
        let (_, r) = co_occurrence(&mut EGraph::default(), &f);
        assert_eq!(r["exact_parity"], true);
    }
    fn fixture(allowed: bool) -> Json {
        json!({"max_arity":1,"nodes":[{"id":0,"op":"a","children":[]},{"id":1,"op":"b","children":[]},{"id":2,"op":"pair","children":[0,0]},{"id":3,"op":"pair","children":[1,1]}],
            "pairs":[[2,3,true],[0,1,allowed]],
            "expected":if allowed {vec![json!({"op":"pair","args":[{"hole":0},{"hole":0}]})]}else{vec![]}})
    }
    #[test]
    fn repeated_holes_share_identity_and_count_once() {
        assert_eq!(run(&fixture(true))["exact_candidate_parity"], true);
    }
    #[test]
    fn unavailable_child_causes_parent_hole_not_invented_structure() {
        assert_eq!(run(&fixture(false))["exact_candidate_parity"], true);
    }
    #[test]
    fn excessive_distinct_holes_fall_back_to_a_single_hole() {
        let f = json!({"max_arity":1,"nodes":[{"id":0,"op":"a","children":[]},{"id":1,"op":"b","children":[]},{"id":2,"op":"pair","children":[0,1]},{"id":3,"op":"pair","children":[1,0]}],"pairs":[[2,3,true],[0,1,true],[1,0,true]],"expected":[]});
        assert_eq!(run(&f)["exact_candidate_parity"], true);
    }
    #[test]
    fn native_matching_preserves_repeated_hole_constraint() {
        let mut f = fixture(true);
        f["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":4,"op":"pair","children":[0,1]}));
        f["expected_dedup_signatures"] = json!([{"matches":[[2,[0]],[3,[1]]],"size":3}]);
        let r = run(&f);
        assert_eq!(
            r["matching_and_dedup"]["signature_and_minimum_size_parity"],
            true
        );
        assert_eq!(r["matching_and_dedup"]["native_match_rows"], 2);
    }
    #[test]
    fn matching_signatures_deduplicate_alternative_patterns() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            include_str!("../../experiments/egglog_babble/anti_unify.egg"),
        )
        .unwrap();
        eg.parse_and_run_program(
            None,
            "(Node 0 \"a\" 0) (Node 2 \"pair\" 1) (Cons 1 0 2) (Cons 2 0 0)",
        )
        .unwrap();
        let candidates = BTreeSet::from([
            json!({"op":"pair","args":[{"hole":0},{"hole":0}]}).to_string(),
            json!({"op":"pair","args":[{"hole":0},{"op":"a","args":[]}]}).to_string(),
        ]);
        let r = match_candidates(
            &mut eg,
            &candidates,
            &json!([{"matches":[[2,[0]]],"size":3}]),
        );
        assert_eq!(r["actual_groups"], 1);
        assert_eq!(r["signature_and_minimum_size_parity"], true);
    }
}
