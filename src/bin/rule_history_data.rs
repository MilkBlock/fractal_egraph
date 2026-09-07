//! Controlled ablation corpus, collected through actual egglog kernel traces.
use egglog::{EGraph, RuleActionOutcome, SerializeConfig, TraceSession};
use serde_json::json;
use std::{collections::HashSet, io::Write, time::Instant};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or("results/rule_history/data.jsonl".into());
    let mut file = std::io::BufWriter::new(std::fs::File::create(out).unwrap());
    let shapes = [
        "(Add a b)",
        "(Add a a)",
        "(Mul a b)",
        "(Mul a a)",
        "(Add (Add a b) c)",
        "(Add a (Add b c))",
        "(Mul (Mul a b) c)",
        "(Mul a (Mul b c))",
        "(Mul a (Add b c))",
        "(Mul (Add a b) c)",
        "(Add (Mul a b) (Mul a c))",
        "(Add (Mul a c) (Mul b c))",
        "(Add a (Zero))",
        "(Mul a (One))",
        "(Mul a (Add a b))",
        "(Add (Mul a a) (Mul a b))",
    ];
    let rules = r#"
      (datatype E (Var i64) (Zero) (One) (Add E E) (Mul E E))
      (ruleset comm) (ruleset assoc) (ruleset dist) (ruleset ident) (ruleset noop)
      (rule ((= root (Add x y))) ((union root (Add y x))) :ruleset comm :name "add-comm")
      (rule ((= root (Mul x y))) ((union root (Mul y x))) :ruleset comm :name "mul-comm")
      (rule ((= root (Add (Add x y) z))) ((union root (Add x (Add y z)))) :ruleset assoc :name "add-assoc")
      (rule ((= root (Mul (Mul x y) z))) ((union root (Mul x (Mul y z)))) :ruleset assoc :name "mul-assoc")
      (rule ((= root (Mul x (Add y z)))) ((union root (Add (Mul x y) (Mul x z)))) :ruleset dist :name "distribute")
      (rule ((= root (Add x (Zero)))) ((union root x)) :ruleset ident :name "add-zero")
      (rule ((= root (Mul x (One)))) ((union root x)) :ruleset ident :name "mul-one")
      (rule ((= root (Add x y))) ((union root (Add x y))) :ruleset noop :name "noop-add")
      (rule ((= root (Mul x y))) ((union root (Mul x y))) :ruleset noop :name "noop-mul")
    "#;
    let schedules = [
        ["comm", "assoc", "dist", "ident"],
        ["assoc", "dist", "comm", "ident"],
        ["dist", "comm", "assoc", "ident"],
        ["ident", "assoc", "comm", "dist"],
        ["comm", "comm", "comm", "ident"],
        ["noop", "comm", "noop", "assoc"],
    ];
    let start = Instant::now();
    let mut n = 0;
    let mut local_events = 0;
    for replica in 0..4 {
        for (shape, expr) in shapes.iter().enumerate() {
            for (schedule, rounds) in schedules.iter().enumerate() {
                let mut eg = EGraph::default();
                let t = Instant::now();
                // Each run has fresh IDs and independently renamed leaf constants.
                let base = 1000 * replica + 100 * shape + 10 * schedule;
                let setup = format!(
                    "{rules}\n(let a (Var {})) (let b (Var {})) (let c (Var {})) (let target {expr})",
                    base + 1,
                    base + 2,
                    base + 3
                );
                eg.parse_and_run_program(None, &setup).unwrap();
                let mut untraced = eg.clone();
                let sort = eg.get_sort_by_name("E").unwrap().clone();
                let target = eg.lookup_function("target", &[]).unwrap();
                let mut history = Vec::new();
                for (round, ruleset) in rounds.iter().enumerate() {
                    let trace = TraceSession::new();
                    let ts = Instant::now();
                    let report = eg.step_rules_with_trace(ruleset, &trace).unwrap();
                    let step_us = ts.elapsed().as_micros();
                    let plain_report = untraced.step_rules(ruleset).unwrap();
                    assert_eq!(report.updated, plain_report.updated);
                    if *ruleset == "noop" {
                        assert!(!report.updated, "explicit redundant rules must be no-ops");
                    }
                    let survived: HashSet<_> = trace
                        .drain_action_outcomes()
                        .into_iter()
                        .filter(|o| o.outcome == RuleActionOutcome::Survived)
                        .map(|o| o.match_event_id)
                        .collect();
                    for event in trace.drain_matches() {
                        if !survived.contains(&event.event_id) {
                            continue;
                        }
                        // Only explicitly named user variables are used. All have sort E.
                        let bindings: Vec<_> = event
                            .bindings
                            .iter()
                            .filter_map(|b| {
                                let name = b.name.as_deref()?;
                                ["root", "x", "y", "z"]
                                    .contains(&name)
                                    .then_some((name.to_owned(), b.value))
                            })
                            .collect();
                        // Lowering aliases source `root` to a generated name. Reconstruct
                        // the known LHS from retained user bindings, never generated names.
                        let get = |name: &str| {
                            eg.get_canonical_value(
                                bindings
                                    .iter()
                                    .find(|(n, _)| n == name)
                                    .unwrap_or_else(|| panic!("missing {name}: {event:?}"))
                                    .1,
                                &sort,
                            )
                        };
                        let call = |op: &str, args: &[egglog::Value]| {
                            eg.lookup_function(op, args).unwrap()
                        };
                        let root = match &*event.rule {
                            "add-comm" | "noop-add" => call("Add", &[get("x"), get("y")]),
                            "mul-comm" | "noop-mul" => call("Mul", &[get("x"), get("y")]),
                            "add-assoc" => {
                                call("Add", &[call("Add", &[get("x"), get("y")]), get("z")])
                            }
                            "mul-assoc" => {
                                call("Mul", &[call("Mul", &[get("x"), get("y")]), get("z")])
                            }
                            "distribute" => {
                                call("Mul", &[get("x"), call("Add", &[get("y"), get("z")])])
                            }
                            "add-zero" => call("Add", &[get("x"), call("Zero", &[])]),
                            "mul-one" => call("Mul", &[get("x"), call("One", &[])]),
                            _ => continue,
                        };
                        let mut bindings = bindings;
                        bindings.push(("root".into(), root));
                        history.push((round, event.rule.to_string(), root, bindings));
                    }
                    let root_id = eg.value_to_class_id(&sort, target);
                    let snapshot = eg.serialize(SerializeConfig::default());
                    assert!(snapshot.is_complete());
                    let graph = snapshot.egraph;
                    let plain_graph = untraced.serialize(SerializeConfig::default()).egraph;
                    let records = |g: &egglog::EGraph| {
                        let s = g.serialize(SerializeConfig::default()).egraph;
                        let mut rows: Vec<_> = s
                            .nodes
                            .values()
                            .map(|node| {
                                (
                                    node.eclass.to_string(),
                                    node.op.clone(),
                                    node.children
                                        .iter()
                                        .map(|id| s.nodes[id].eclass.to_string())
                                        .collect::<Vec<_>>(),
                                    node.subsumed,
                                )
                            })
                            .collect();
                        rows.sort();
                        rows
                    };
                    assert_eq!(graph.nodes.len(), plain_graph.nodes.len());
                    assert_eq!(
                        records(&eg),
                        records(&untraced),
                        "trace must preserve database state"
                    );
                    let nodes: Vec<_> = graph.nodes.values().filter(|node| node.eclass == root_id)
                        .map(|node| json!({"op":node.op, "children":node.children.iter().map(|id| graph.nodes[id].eclass.to_string()).collect::<Vec<_>>()})).collect();
                    let mut child_ops = std::collections::BTreeMap::<String, Vec<String>>::new();
                    for node in graph.nodes.values() {
                        child_ops
                            .entry(node.eclass.to_string())
                            .or_default()
                            .push(node.op.clone());
                    }
                    for ops in child_ops.values_mut() {
                        ops.sort();
                        ops.dedup();
                    }
                    // Re-canonicalize historical roots after merges; include ancestor-class history.
                    let local: Vec<_> = history.iter().filter(|(_,_,root,_)| eg.value_to_class_id(&sort,*root)==root_id)
                        .map(|(step,rule,_,bindings)| json!({"round":step,"rule":rule,"bindings":bindings.iter().map(|(name,value)| (name.clone(),eg.value_to_class_id(&sort,*value).to_string())).collect::<std::collections::BTreeMap<_,_>>()})).collect();
                    local_events += local.len();
                    writeln!(file,"{}",json!({"replica":replica,"shape":shape,"schedule":schedule,"round":round,"root":root_id.to_string(),"nodes":nodes,"child_ops":child_ops,"history":local,"updated":report.updated,"trace_verified":true,"step_us":step_us,"run_elapsed_us":t.elapsed().as_micros()})).unwrap();
                    n += 1;
                }
            }
        }
    }
    assert!(
        local_events > 0,
        "empty histories cannot support an ablation"
    );
    eprintln!(
        "wrote {n} snapshots in {:.3}s",
        start.elapsed().as_secs_f64()
    );
}
