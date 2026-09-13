//! Ablation 1: reuse rule-match substitutions to recognize rooted prefixes.
//! Deliberately no block partitioner or Zobrist history yet.
#[path = "slotted_baseline/alloc.rs"]
mod alloc;
use egglog::{EGraph, RuleActionOutcome, RuleMatchEvent, TraceSession, Value};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
    time::Instant,
};
const LHS: &str = "(Add (Mul x y) (Neg x))";
const RHS: &str = "(Add (Neg x) (Mul x y))";
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Prefix {
    schema: u8,
    root: Value,
    ports: Vec<Value>,
}
fn canon(eg: &EGraph, v: Value) -> Value {
    eg.get_canonical_value(v, eg.get_sort_by_name("E").unwrap())
}
fn lookup(eg: &EGraph, op: &str, args: &[Value]) -> Value {
    canon(
        eg,
        eg.lookup_function(op, args)
            .expect("post-step constructor must exist"),
    )
}
fn detectors(set: &str, local: bool) -> String {
    let mut text = format!("(ruleset {set})\n");
    for (name, pat) in [("shallow", "(Add x y)"), ("lhs", LHS), ("rhs", RHS)] {
        let anchor = if local { "(Target root)" } else { "" };
        text += &format!(
            "(rule ({anchor} (= root {pat})) ((union root {pat})) :ruleset {set} :name \"{name}\")\n"
        );
    }
    text
}
fn setup(active: usize, old_rhs: usize, noise: usize, redundant: bool) -> (EGraph, Vec<Value>) {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, &format!("(datatype E (Var i64) (Add E E) (Mul E E) (Neg E) (Box E))\n(relation Target (E))\n(ruleset source)\n(rewrite {LHS} {RHS} :ruleset source :name \"source\")")).unwrap();
    for (set, local) in [("detect", true), ("global", false)] {
        eg.parse_and_run_program(None, &detectors(set, local))
            .unwrap();
    }
    let mut roots = Vec::new();
    for i in 0..active + old_rhs + noise {
        let x = format!("(Var {})", 2 * i + 1);
        let y = format!("(Var {})", 2 * i + 2);
        let l = format!("(Add (Mul {x} {y}) (Neg {x}))");
        let r = format!("(Add (Neg {x}) (Mul {x} {y}))");
        let text = if i < active {
            l.clone()
        } else if i < active + old_rhs {
            r.clone()
        } else {
            format!("(Add {x} {y})")
        };
        let e = eg.parser.get_expr_from_string(None, &text).unwrap();
        let root = eg.eval_expr(&e).unwrap().1;
        if i < active {
            roots.push(root);
            eg.parse_and_run_program(None, &format!("(Target {l})"))
                .unwrap();
            if redundant {
                eg.parse_and_run_program(None, &format!("(union {l} {r})"))
                    .unwrap();
            }
        }
    }
    (eg, roots)
}
fn survived(trace: &TraceSession) -> Vec<RuleMatchEvent> {
    let keep: HashSet<_> = trace
        .drain_action_outcomes()
        .into_iter()
        .filter(|o| o.outcome == RuleActionOutcome::Survived)
        .map(|o| o.match_event_id)
        .collect();
    trace
        .drain_matches()
        .into_iter()
        .filter(|e| keep.contains(&e.event_id))
        .collect()
}
fn materialize(eg: &EGraph, events: &[RuleMatchEvent], source: bool) -> HashSet<Prefix> {
    let mut out = HashSet::new();
    for e in events {
        let b: HashMap<_, _> = e
            .bindings
            .iter()
            .filter_map(|b| {
                b.name
                    .as_deref()
                    .filter(|n| ["x", "y"].contains(n))
                    .map(|n| (n, canon(eg, b.value)))
            })
            .collect();
        let x = *b.get("x").expect("fixture requires retained x");
        let y = *b.get("y").expect("fixture requires retained y");
        if e.rule.as_ref() == "shallow" {
            out.insert(Prefix {
                schema: 0,
                root: lookup(eg, "Add", &[x, y]),
                ports: vec![x, y],
            });
            continue;
        }
        let m = lookup(eg, "Mul", &[x, y]);
        let n = lookup(eg, "Neg", &[x]);
        for rhs in [false, true] {
            if !source && rhs != (e.rule.as_ref() == "rhs") {
                continue;
            }
            let ports = if rhs { vec![n, m] } else { vec![m, n] };
            let root = lookup(eg, "Add", &ports);
            out.insert(Prefix {
                schema: 0,
                root,
                ports,
            }); // depth 1: ordered boundary ports
            out.insert(Prefix {
                schema: if rhs { 2 } else { 1 },
                root,
                ports: if rhs { vec![x, x, y] } else { vec![x, y, x] },
            }); // depth 2: retain x sharing
        }
    }
    out
}
fn reference(eg: &mut EGraph, ruleset: &str) -> (Vec<RuleMatchEvent>, bool) {
    let t = TraceSession::new();
    let report = eg.step_rules_with_trace(ruleset, &t).unwrap();
    (survived(&t), report.updated)
}
fn run(mode: &str, case: &str, active: usize, noise: usize) -> serde_json::Value {
    assert!(["rematch", "global_rematch", "witness", "source_only"].contains(&mode));
    let old = if case == "preexisting" { active } else { 0 };
    let active = if case == "inactive" { 0 } else { active };
    let (mut eg, roots) = setup(active, old, noise, case == "redundant");
    let setup_live = alloc::LIVE.load(Ordering::Relaxed);
    alloc::PEAK.store(setup_live, Ordering::Relaxed);
    let start = Instant::now();
    let trace = TraceSession::new();
    let report = if mode == "witness" {
        eg.step_rules_with_trace("source", &trace)
    } else {
        eg.step_rules("source")
    }
    .unwrap();
    let source_ns = start.elapsed().as_nanos();
    let t = Instant::now();
    let events = if mode == "rematch" || mode == "global_rematch" {
        let (e, changed) = reference(
            &mut eg,
            if mode == "rematch" {
                "detect"
            } else {
                "global"
            },
        );
        assert!(!changed);
        e
    } else if mode == "witness" {
        survived(&trace)
    } else {
        vec![]
    };
    let found = if mode == "source_only" {
        HashSet::new()
    } else {
        materialize(&eg, &events, mode == "witness")
    };
    let recognize_ns = t.elapsed().as_nanos();
    let measured_ns = start.elapsed().as_nanos();
    let phase_peak = alloc::PEAK
        .load(Ordering::Relaxed)
        .saturating_sub(setup_live);
    let phase_retained = alloc::LIVE.load(Ordering::Relaxed) as i64 - setup_live as i64;
    let touched: HashSet<_> = roots.iter().map(|r| canon(&eg, *r)).collect();
    // Fresh detector rules prevent a previous seminaive query from hiding old matches.
    eg.parse_and_run_program(None, &detectors("verify", false))
        .unwrap();
    let oracle = TraceSession::new();
    assert!(!eg.step_rules_with_trace("verify", &oracle).unwrap().updated);
    let all = materialize(&eg, &survived(&oracle), false);
    let expected = if mode == "witness" || mode == "rematch" {
        all.iter()
            .filter(|p| touched.contains(&p.root))
            .cloned()
            .collect::<HashSet<_>>()
    } else {
        all.clone()
    };
    if mode != "source_only" {
        assert_eq!(found, expected);
        assert!(found.is_subset(&all));
    }
    if case == "redundant" || case == "inactive" {
        assert!(!report.updated);
    }
    json!({"mode":mode,"case":case,"active_roots":active,"preexisting_rhs_roots":old,"unrelated_roots":noise,"source_updated":report.updated,"source_ns":source_ns,"recognize_ns":recognize_ns,"measured_ns":measured_ns,"trace_events_used":events.len(),"recognized_prefixes":found.len(),"global_prefixes":all.len(),"expected_event_prefixes":all.iter().filter(|p|touched.contains(&p.root)).count(),"global_coverage":if all.is_empty(){1.0}else{found.len() as f64/all.len() as f64},"phase_peak_requested_bytes":phase_peak,"phase_retained_delta_bytes":phase_retained,"exact_verified":mode!="source_only"})
}
fn main() {
    let a: Vec<_> = std::env::args().collect();
    let mode = a.get(1).expect("rematch|witness|source_only");
    let case = a.get(2).expect("fresh|preexisting|redundant|inactive");
    assert!(["fresh", "preexisting", "redundant", "inactive"].contains(&case.as_str()));
    let n: usize = a.get(3).expect("active count").parse().unwrap();
    let noise: usize = a.get(4).expect("unrelated count").parse().unwrap();
    println!("{}", run(mode, case, n, noise));
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefix_ports_follow_tree_positions_and_preserve_sharing() {
        let (mut eg, _) = setup(1, 0, 0, false);
        let trace = TraceSession::new();
        eg.step_rules_with_trace("source", &trace).unwrap();
        let prefixes = materialize(&eg, &survived(&trace), true);
        let lhs = prefixes.iter().find(|p| p.schema == 1).unwrap();
        let rhs = prefixes.iter().find(|p| p.schema == 2).unwrap();
        assert_eq!(lhs.ports[0], lhs.ports[2]);
        assert_ne!(lhs.ports[0], lhs.ports[1]);
        assert_eq!(rhs.ports[0], rhs.ports[1]);
        assert_ne!(rhs.ports[0], rhs.ports[2]);
    }
    #[test]
    fn same_class_can_have_prefixes_not_covered_by_this_witness() {
        let (mut eg, _) = setup(1, 0, 0, false);
        eg.parse_and_run_program(
            None,
            "(union (Add (Mul (Var 1) (Var 2)) (Neg (Var 1))) (Add (Var 99) (Var 100)))",
        )
        .unwrap();
        let trace = TraceSession::new();
        eg.step_rules_with_trace("source", &trace).unwrap();
        let seen = materialize(&eg, &survived(&trace), true);
        let (events, changed) = reference(&mut eg, "detect");
        assert!(!changed);
        let actual = materialize(&eg, &events, false);
        assert!(seen.is_subset(&actual));
        assert!(seen.len() < actual.len());
    }
    #[test]
    fn witness_matches_native_rematch_on_active_prefixes() {
        for case in ["fresh", "preexisting", "redundant", "inactive"] {
            for mode in ["rematch", "witness"] {
                let r = run(mode, case, 4, 7);
                assert_eq!(r["exact_verified"], true);
            }
        }
    }
    #[test]
    fn witness_does_not_claim_complete_global_discovery() {
        let r = run("witness", "preexisting", 4, 0);
        assert!(
            r["recognized_prefixes"].as_u64().unwrap() < r["global_prefixes"].as_u64().unwrap()
        );
    }
    #[test]
    fn redundant_apply_is_still_a_witness_not_a_mutation() {
        let r = run("witness", "redundant", 4, 0);
        assert_eq!(r["source_updated"], false);
        assert!(r["recognized_prefixes"].as_u64().unwrap() > 0);
    }
}
