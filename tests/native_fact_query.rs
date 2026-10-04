use egglog::{EGraph, ast::Literal};
use std::collections::BTreeSet;

fn reference(eg: &EGraph) -> BTreeSet<Vec<egglog::Value>> {
    let mut copy = eg.clone();
    copy.parse_and_run_program(None, "(relation Probe (E)) (ruleset probe) (rule ((P x) (Q x)) ((Probe x)) :ruleset probe) (run probe 1)").unwrap();
    let mut rows = BTreeSet::new();
    copy.function_for_each("Probe", |r| {
        rows.insert(vec![r.vals[0]]);
    })
    .unwrap();
    rows
}
fn query(eg: &mut EGraph) -> egglog::CompiledFactQuery {
    eg.compile_fact_query(
        &[("E".into(), None), ("Unit".into(), Some(Literal::Unit))],
        &[("P".into(), vec![0]), ("Q".into(), vec![0])],
        &[0],
    )
    .unwrap()
}
#[test]
fn cached_join_tracks_union_insert_and_subsume_without_user_actions() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, "(datatype E (A) (B) (C)) (relation P (E)) (relation Q (E)) (relation Unexpected ()) (rule ((P x)) ((Unexpected))) (P (A)) (Q (B))").unwrap();
    let q = query(&mut eg);
    for update in [
        "",
        "(union (A) (B))",
        "(P (C)) (Q (C))",
        "(subsume (P (C)))",
    ] {
        if !update.is_empty() {
            eg.parse_and_run_program(None, update).unwrap();
        }
        let expected = reference(&eg);
        let actual: BTreeSet<_> = eg.run_fact_query(&q).unwrap().into_iter().collect();
        assert_eq!(actual, expected);
        assert_eq!(
            eg.run_fact_query(&q)
                .unwrap()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            expected
        );
        let mut count = 0;
        eg.function_for_each("Unexpected", |_| count += 1).unwrap();
        assert_eq!(count, 0, "query must not run user rules");
    }
    eg.free_fact_query(q);
}
#[test]
fn query_validates_types_and_keeps_literals_and_aliases() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (V i64) (Pair E E)) (Pair (V 1) (V 1)) (Pair (V 1) (V 2))",
    )
    .unwrap();
    let values = vec![
        ("i64".into(), Some(Literal::Int(1))),
        ("E".into(), None),
        ("E".into(), None),
    ];
    let atoms = vec![("V".into(), vec![0, 1]), ("Pair".into(), vec![1, 1, 2])];
    let q = eg.compile_fact_query(&values, &atoms, &[2]).unwrap();
    assert_eq!(eg.run_fact_query(&q).unwrap().len(), 1);
    assert!(
        eg.compile_fact_query(&values, &[("Pair".into(), vec![0, 1, 2])], &[2])
            .is_err()
    );
    eg.free_fact_query(q);
}

#[test]
#[ignore = "explicit local timing comparison"]
fn benchmark_cached_join_against_clone_and_compile() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (V i64)) (relation P (E)) (relation Q (E))",
    )
    .unwrap();
    for i in 0..200 {
        eg.parse_and_run_program(None, &format!("(P (V {i})) (Q (V {i}))"))
            .unwrap();
    }
    let expected = reference(&eg);
    let q = query(&mut eg);
    let start = std::time::Instant::now();
    for _ in 0..60 {
        assert_eq!(reference(&eg), expected);
    }
    let old = start.elapsed();
    let start = std::time::Instant::now();
    for _ in 0..60 {
        assert_eq!(
            eg.run_fact_query(&q)
                .unwrap()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            expected
        );
    }
    let new = start.elapsed();
    eprintln!("60 native joins / 200 bindings: clone+compile={old:?}, cached={new:?}");
    eg.free_fact_query(q);
}

#[test]
fn cached_join_sees_function_updates() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (A)) (function score (E) i64 :merge (max old new)) (set (score (A)) 1)",
    )
    .unwrap();
    let q = eg
        .compile_fact_query(
            &[("E".into(), None), ("i64".into(), Some(Literal::Int(1)))],
            &[("score".into(), vec![0, 1])],
            &[0],
        )
        .unwrap();
    assert_eq!(eg.run_fact_query(&q).unwrap().len(), 1);
    eg.parse_and_run_program(None, "(set (score (A)) 2)")
        .unwrap();
    assert!(eg.run_fact_query(&q).unwrap().is_empty());
    eg.free_fact_query(q);
}

#[test]
fn compiled_check_groups_observe_action_order_and_do_not_share_results() {
    use egglog::ast::Command;
    let mut base = EGraph::default();
    base.parse_and_run_program(None, "(datatype E (A) (B))")
        .unwrap();
    let commands = base
        .parse_program(None, "(check (= (A) (B)) (= (A) (A)))")
        .unwrap();
    let Command::Check(_, facts) = &commands[0] else {
        panic!()
    };
    let check = base.compile_ground_check(facts).unwrap();
    let mut first = base.clone();
    assert!(
        first.run_ground_check(&check).is_err(),
        "must fail before union action"
    );
    assert_eq!(
        first.get_size("A"),
        0,
        "checking must not materialize facts"
    );
    first
        .parse_and_run_program(None, "(union (A) (B))")
        .unwrap();
    first.run_ground_check(&check).unwrap();
    first.run_ground_check(&check).unwrap();
    let mut other = base.clone();
    assert!(
        other.run_ground_check(&check).is_err(),
        "successful result must not leak across clones"
    );
    first.parse_and_run_program(None, "(subsume (A))").unwrap();
    first.run_ground_check(&check).unwrap();
    first.run_program(commands).unwrap(); // same visibility semantics as normal check
    base.free_ground_check(check);
    let commands = base.parse_program(None, "(check (> 2 1))").unwrap();
    let Command::Check(_, facts) = &commands[0] else {
        panic!()
    };
    let check = base.compile_ground_check(facts).unwrap();
    base.run_ground_check(&check).unwrap();
    base.free_ground_check(check);
    let commands = base.parse_program(None, "(check (= x (A)))").unwrap();
    let Command::Check(_, facts) = &commands[0] else {
        panic!()
    };
    assert!(
        base.compile_ground_check(facts).is_err(),
        "free variables cannot be batched as independent checks"
    );
}

#[test]
fn existence_and_freshness_equal_full_enumeration_for_joins() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, "(relation Edge (i64 i64))")
        .unwrap();
    for x in 0..6 {
        for y in 0..6 {
            if (x + y) % 2 == 0 {
                eg.parse_and_run_program(None, &format!("(Edge {x} {y})"))
                    .unwrap();
            }
        }
    }
    for projection in [vec![0, 1, 2], vec![0], vec![]] {
        let q = eg
            .compile_fact_query(
                &vec![("i64".into(), None); 3],
                &[
                    ("Edge".into(), vec![0, 1]),
                    ("Edge".into(), vec![1, 2]),
                    ("Edge".into(), vec![0, 2]),
                ],
                &projection,
            )
            .unwrap();
        let full: BTreeSet<_> = eg.run_fact_query(&q).unwrap().into_iter().collect();
        assert!(!full.is_empty());
        for stride in [0, 1, 2] {
            let excluded: std::collections::HashSet<_> = full
                .iter()
                .enumerate()
                .filter(|(i, _)| stride != 0 && i % stride == 0)
                .map(|(_, r)| r.clone())
                .collect();
            let (matched, fresh) = eg.run_fact_query_difference(&q, excluded.clone()).unwrap();
            assert_eq!(matched.is_some(), !full.is_empty());
            assert_eq!(fresh.is_some(), full.iter().any(|r| !excluded.contains(r)));
            assert!(full.contains(&matched.unwrap()));
            if let Some(fresh) = fresh {
                assert!(full.contains(&fresh));
                assert!(!excluded.contains(&fresh));
            }
            assert_eq!(
                eg.run_fact_query(&q)
                    .unwrap()
                    .into_iter()
                    .collect::<BTreeSet<_>>(),
                full
            );
        }
        eg.free_fact_query(q);
    }
}

#[test]
fn difference_witnesses_follow_union_and_subsume() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (A) (B) (C)) (relation P (E)) (relation Q (E)) (P (A)) (Q (B))",
    )
    .unwrap();
    let q = query(&mut eg);
    for update in [
        "",
        "(union (A) (B))",
        "(P (C)) (Q (C))",
        "(subsume (P (C)))",
    ] {
        let before = eg.run_fact_query(&q).unwrap();
        if !update.is_empty() {
            eg.parse_and_run_program(None, update).unwrap();
        }
        let sort = eg.get_arcsort_by(|s| s.name() == "E");
        let excluded: std::collections::HashSet<_> = before
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|v| eg.get_canonical_value(v, &sort))
                    .collect()
            })
            .collect();
        let full: BTreeSet<_> = eg.run_fact_query(&q).unwrap().into_iter().collect();
        let (matched, fresh) = eg.run_fact_query_difference(&q, excluded.clone()).unwrap();
        assert_eq!(matched.is_some(), !full.is_empty());
        assert_eq!(fresh.is_some(), full.iter().any(|r| !excluded.contains(r)));
    }
}

#[test]
fn source_witness_plan_keeps_actual_producers_through_a_summary() {
    use egglog::{
        TraceSession, WriteOutcome,
        ast::{Expr, Span},
    };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,"(datatype E (A)) (relation Seed (E)) (relation P (E)) (relation Q (E)) (relation Summary (E)) (relation Out (E)) (ruleset make) (ruleset fill) (ruleset consume) (rule ((Seed x)) ((P x) (Q x)) :ruleset make :name \"producer\") (rule ((P x) (Q x)) ((Summary x)) :ruleset fill) (rule ((Summary x)) ((Out x)) :ruleset consume :name \"consumer\") (Seed (A))").unwrap();
    let trace = TraceSession::with_dependencies();
    let run = eg.parse_program(None, "(run make 1)").unwrap();
    eg.run_program_with_trace(run, &trace).unwrap();
    let produced = trace.drain_completed();
    let ids: BTreeSet<_> = produced
        .writes
        .iter()
        .filter(|w| w.outcome == WriteOutcome::Inserted)
        .map(|w| w.event_id)
        .collect();
    eg.parse_and_run_program(None, "(run fill 1)").unwrap();
    let mut parser = egglog::ast::Parser::default();
    let x = parser.get_expr_from_string(None, "x").unwrap();
    eg.register_source_read_plan(
        &trace,
        "consumer",
        "Summary",
        vec![
            ("P".into(), vec![x.clone()], Some("original:P".into())),
            ("Q".into(), vec![x], Some("original:Q".into())),
        ],
        vec!["x".into()],
    )
    .unwrap();
    let run = eg.parse_program(None, "(run consume 1)").unwrap();
    eg.run_program_with_trace(run, &trace).unwrap();
    let b = trace.drain_completed();
    let m = b
        .matches
        .iter()
        .find(|m| m.rule.as_ref() == "consumer")
        .unwrap();
    assert!(m.physical_witness_complete);
    let reads: Vec<_> = b
        .reads
        .iter()
        .filter(|r| r.match_event_id == m.event_id && !r.planner_summary)
        .collect();
    assert_eq!(reads.len(), 2);
    assert!(
        reads
            .iter()
            .all(|r| r.producer_write_event_id.is_some_and(|i| ids.contains(&i)))
    );
    assert!(
        reads
            .iter()
            .all(|r| r.source_span.as_deref().unwrap().starts_with("original:"))
    );
    assert_eq!(b.reads.iter().filter(|r| r.planner_summary).count(), 1);
    // A missing source binding must not fabricate a complete proof.
    let missing = Expr::Var(
        Span::Rust(std::sync::Arc::new(egglog::ast::RustSpan {
            file: file!(),
            line: line!(),
            column: column!(),
        })),
        "missing".into(),
    );
    eg.register_source_read_plan(
        &trace,
        "consumer",
        "Summary",
        vec![("Q".into(), vec![missing], None)],
        vec!["x".into()],
    )
    .unwrap();
    eg.parse_and_run_program(None, "(datatype F (Unused)) (Seed (A))")
        .unwrap();
    // A naive diagnostic consumer guarantees a second match even with no delta.
    eg.parse_and_run_program(None,"(ruleset diagnose) (rule ((Summary x)) ((Out x)) :ruleset diagnose :name \"diagnose\" :naive)").unwrap();
    let missing = parser.get_expr_from_string(None, "missing").unwrap();
    eg.register_source_read_plan(
        &trace,
        "diagnose",
        "Summary",
        vec![("Q".into(), vec![missing], None)],
        vec!["x".into()],
    )
    .unwrap();
    let run = eg.parse_program(None, "(run diagnose 1)").unwrap();
    eg.run_program_with_trace(run, &trace).unwrap();
    let missing = trace.drain_completed();
    assert!(!missing.matches.is_empty());
    assert!(missing.matches.iter().all(|m| !m.physical_witness_complete));
}

#[test]
fn repeated_ground_subterms_remain_read_only_and_observe_updates() {
    use egglog::ast::Command;
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (A) (Pair E E)) (function score (E) i64 :merge new)",
    )
    .unwrap();
    let mut term = "(A)".to_string();
    for _ in 0..8 {
        term = format!("(Pair {term} {term})");
    }
    let cmds = eg
        .parse_program(
            None,
            &format!("(check (= (score {term}) 7) (= {term} {term}))"),
        )
        .unwrap();
    let Command::Check(_, facts) = &cmds[0] else {
        panic!()
    };
    let check = eg.compile_ground_check(facts).unwrap();
    assert!(eg.run_ground_check(&check).is_err());
    assert_eq!(eg.get_size("Pair"), 0);
    eg.parse_and_run_program(None, &format!("(set (score {term}) 7)"))
        .unwrap();
    eg.run_ground_check(&check).unwrap();
    eg.parse_and_run_program(None, &format!("(set (score {term}) 9)"))
        .unwrap();
    assert!(eg.run_ground_check(&check).is_err());
}
