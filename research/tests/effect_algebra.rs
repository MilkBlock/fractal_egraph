use egg_layout::{binding_program, effect_program};
#[path = "support/effect_fixtures.rs"]
mod fixtures;
use fixtures::{FixtureOps, port, swap_interface};
// Native tests and a finite closure over fixed-interface monotone effect summaries.
use egg_layout::{
    binding_program::Term,
    effect_program::{Fact, Summary},
};
use egglog::{EGraph, Value};
use serde_json::json;
use std::collections::BTreeMap;
fn term_text(t: &Term) -> String {
    if let Some(i) = t.op.strip_prefix("in:") {
        return format!("(Atom {i})");
    }
    format!(
        "({} {})",
        t.op,
        t.args.iter().map(term_text).collect::<Vec<_>>().join(" ")
    )
}
fn lookup(t: &Term, eg: &EGraph) -> Option<Value> {
    if let Some(i) = t.op.strip_prefix("in:") {
        return eg.lookup_function("Atom", &[eg.base_to_value(i.parse::<i64>().ok()?)]);
    }
    let args = t
        .args
        .iter()
        .map(|a| lookup(a, eg))
        .collect::<Option<Vec<_>>>()?;
    eg.lookup_function(&t.op, &args)
}
fn native(steps: &[&str], growth: bool) -> (Summary, serde_json::Value) {
    let entry = if growth {
        BTreeMap::from([("cursor".into(), port(0))])
    } else {
        swap_interface()
    };
    let mut summary = Summary::identity(entry.clone());
    if growth {
        summary
            .requires
            .insert(Fact::Relation("Seen".into(), vec![port(0)]));
    }
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,"(datatype Math (Atom i64) (Add Math Math) (Step Math)) (relation Seen (Math)) (Atom 0) (Atom 1) (Atom 2) (Atom 3)").unwrap();
    if growth {
        eg.parse_and_run_program(None, "(Seen (Atom 0))").unwrap();
    } else {
        for t in entry.values() {
            eg.parse_and_run_program(None, &term_text(t)).unwrap();
        }
    }
    let mut updates = 0;
    for (i, step) in steps.iter().enumerate() {
        let ruleset = format!("stage_{i}");
        let command;
        if *step == "grow" {
            command = format!(
                "(ruleset {ruleset}) (rule ((Seen x)) ((Seen (Step x))) :ruleset {ruleset})"
            );
            summary = summary
                .then(&Summary::grow(entry.clone(), "cursor").unwrap())
                .unwrap();
        } else {
            let before = summary.exit[*step].clone();
            let after = Term::new(
                "Math",
                "Add",
                vec![before.args[1].clone(), before.args[0].clone()],
            );
            command = format!(
                "(ruleset {ruleset}) (rule ((= r {})) ((union r {})) :ruleset {ruleset})",
                term_text(&before),
                term_text(&after)
            );
            summary = summary
                .then(&Summary::swap(entry.clone(), step).unwrap())
                .unwrap();
        }
        eg.parse_and_run_program(None, &command).unwrap();
        updates += usize::from(eg.step_rules(&ruleset).unwrap().updated);
        let facts = summary.all_facts();
        for name in ["Add", "Step", "Seen"] {
            let expected = facts
                .iter()
                .filter(|f| match f {
                    Fact::Node(t) => t.op == name,
                    Fact::Relation(n, _) => n == name,
                })
                .count();
            assert_eq!(eg.get_size(name), expected, "step {i}, table {name}");
        }
        for fact in &facts {
            match fact {
                Fact::Node(t) => {
                    assert!(lookup(t, &eg).is_some());
                }
                Fact::Relation(n, a) => {
                    let keys = a
                        .iter()
                        .map(|a| lookup(a, &eg).unwrap())
                        .collect::<Vec<_>>();
                    assert!(eg.lookup_function(n, &keys).is_some());
                }
            }
        }
        let sort = eg.get_sort_by_name("Math").unwrap();
        let nodes: Vec<_> = facts
            .iter()
            .filter_map(|f| if let Fact::Node(t) = f { Some(t) } else { None })
            .collect();
        for a in &nodes {
            for b in &nodes {
                let va = eg.get_canonical_value(lookup(a, &eg).unwrap(), sort);
                let vb = eg.get_canonical_value(lookup(b, &eg).unwrap(), sort);
                assert_eq!(
                    va == vb,
                    summary.equivalent_terms(a, b),
                    "unexpected native equality"
                );
            }
        }
    }
    let report = json!({"steps":steps,"updated_iterations":updates,"native_rows":{"Add":eg.get_size("Add"),"Step":eg.get_size("Step"),"Seen":eg.get_size("Seen")},"summary":summary.json(),"verification":"after every step, fact counts, individual facts and represented equality partition match native egglog"});
    (summary, report)
}
fn run() -> serde_json::Value {
    let mut cases = Vec::new();
    let mut swaps = Vec::new();
    for n in 0..=8 {
        let (s, r) = native(&vec!["left"; n], false);
        swaps.push(s);
        cases.push(r);
    }
    assert!(swaps[3].same_effect_and_focus(&swaps[1]));
    assert!(swaps[4].same_effect_and_focus(&swaps[2]));
    assert!(!swaps[2].same_effect_and_focus(&swaps[0]));
    for (n, s) in swaps.iter().enumerate() {
        let representative = if n == 0 {
            0
        } else if n % 2 == 1 {
            1
        } else {
            2
        };
        assert!(s.same_effect_and_focus(&swaps[representative]));
    }
    let (mixed, r) = native(&["left", "left", "right", "right"], false);
    cases.push(r);
    assert!(!mixed.same_effect_and_focus(&swaps[2]));
    let (g2, r) = native(&["grow"; 2], true);
    cases.push(r);
    let (g4, r) = native(&["grow"; 4], true);
    cases.push(r);
    assert!(!g2.same_effect_and_focus(&g4));
    let finite = egg_layout::effect_program::closure(swaps[0].clone(), &[swaps[1].clone()], 10);
    assert!(finite.closed);
    assert_eq!(finite.states.len(), 3);
    let transitions: Vec<_> = finite
        .transitions
        .iter()
        .map(|(a, _, b)| json!({"from":a,"operation":"S","to":b}))
        .collect();
    let pair = egg_layout::effect_program::closure(
        Summary::identity(swap_interface()),
        &[
            Summary::swap(swap_interface(), "left").unwrap(),
            Summary::swap(swap_interface(), "right").unwrap(),
        ],
        16,
    );
    assert!(pair.closed);
    assert_eq!(pair.states.len(), 9);
    let mut words = 0;
    for len in 0..=4 {
        for bits in 0..(1usize << len) {
            let word: Vec<_> = (0..len)
                .map(|i| {
                    if bits & (1 << i) == 0 {
                        "left"
                    } else {
                        "right"
                    }
                })
                .collect();
            let (s, _) = native(&word, false);
            assert!(
                pair.states
                    .iter()
                    .any(|candidate| candidate.same_effect_and_focus(&s))
            );
            words += 1;
        }
    }
    let states = finite.states;
    let table: Vec<Vec<usize>> = states
        .iter()
        .map(|a| {
            states
                .iter()
                .map(|b| {
                    let c = a.then(b).unwrap();
                    states
                        .iter()
                        .position(|s| s.same_effect_and_focus(&c))
                        .unwrap()
                })
                .collect()
        })
        .collect();
    for a in 0..3 {
        for b in 0..3 {
            for c in 0..3 {
                assert_eq!(table[table[a][b]][c], table[a][table[b][c]]);
            }
        }
    }

    json!({"absorption":{"E_then_E_full":swaps[2].absorbs(&swaps[2]).unwrap(),"E_then_S_full":swaps[2].absorbs(&swaps[1]).unwrap(),"E_then_S_graph_only":swaps[2].absorbs_graph(&swaps[1]).unwrap()},"two_position_closure":{"closed":pair.closed,"states":pair.states.len(),"transitions":pair.transitions,"native_words_checked_through_length_four":words},"scope":"fixed selected Add nodes, positive constructor/Seen facts and union effects; actual native validation, not an optimized egglog scheduler","observations":{"focus":"selected enode syntax, not just its eclass","ignored":["timestamps","provenance multiplicity","match counts"],"excluded":["deletions","I/O","custom merge effects","unmodeled interleaved rules"]},"finite_closure":{"state_names":["Id","S","E"],"multiplication_table":table,"states":states.iter().map(Summary::json).collect::<Vec<_>>(),"transitions":transitions,"laws":["S^3 = S","S^4 = S^2","E = S^2; E^2 = E","S^2 != Id on the minimally seeded graph"],"proof":"symbolic simultaneous port substitution plus exact set/equality-closure normalization; the three-state transition table gives the inductive recurrence for all n"},"native_cases":cases,"limitations":"Normalization is sufficient but not complete: it does not quotient arbitrary nested facts by congruence, nor infer recursive families for unbounded effects. Per-step counts are not an end-to-end performance claim."})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_effect_laws_and_counterexamples() {
        let r = run();
        assert_eq!(r["finite_closure"]["states"].as_array().unwrap().len(), 3);
        if let Ok(path) = std::env::var("EFFECT_ALGEBRA_REPORT") {
            std::fs::write(path, serde_json::to_string_pretty(&r).unwrap()).unwrap();
        }
    }
}
