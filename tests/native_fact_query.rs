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
