use cell_assisted_egglog::table_live::{Options, PortLocality, Runtime};
use egglog::EGraph;
use std::path::{Path, PathBuf};
fn worker() -> Option<PathBuf> {
    std::env::var_os("CELL_MATRIX_WORKER").map(Into::into)
}
fn library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze")
}
fn environment() -> EGraph {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
      (relation Boundary (Expr i64)) (relation Other (Expr i64))
      (function Count (i64 i64) i64 :merge (+ old new))
      (ruleset make) (ruleset consume) (ruleset later)
      (rule ((Single x)) ((union (Get (Single x) 0) x)) :ruleset make)
      (rule ((Boundary s n) (= s (Single x)) (= x (Get s 0)) (>= n 0))
            ((set (Count 0 n) 1)) :ruleset consume :name "positive")
      (rule ((Boundary renamed m) (= renamed (Single value)) (= value (Get renamed 0)) (< m 10))
            ((set (Count 1 m) 1)) :ruleset consume :name "small")
      (rule ((Other t z) (= t (Single v)) (= v (Get t 0)))
            ((set (Count 2 z) 1)) :ruleset consume :name "different-tail")
      (rule ((Boundary t n) (= t (Single y)) (= y (Get t 0)))
            ((set (Count 3 n) 1)) :ruleset later :name "late")
      (let seed (Leaf 7)) (let one (Single seed))
      (Boundary one -1) (Boundary one 0) (Boundary one 10) (Other one 99)
    "#,
    )
    .unwrap();
    eg
}
#[test]
fn shared_cell_context_and_residuals_preserve_distinct_guards_and_bindings() {
    let Some(w) = worker() else { return };
    let mut reports = vec![];
    for mode in [
        PortLocality::Independent,
        PortLocality::SharedContext,
        PortLocality::SharedJoins,
    ] {
        let mut eg = environment();
        let cmds = eg
            .parse_program(None, "(run-schedule (seq consume make consume later))")
            .unwrap();
        let mut r = Runtime::configured(
            eg,
            &cmds,
            &w,
            true,
            Options {
                analysis: Some(&library()),
                profile: true,
                port_locality: mode,
                ..Options::default()
            },
        )
        .unwrap();
        r.run(&cmds).unwrap();
        r.engine().clone().parse_and_run_program(None,"(check (= (Count 0 0) 1)) (check (= (Count 0 10) 1)) (check (= (Count 1 -1) 1)) (check (= (Count 1 0) 1)) (check (= (Count 2 99) 1)) (check (= (Count 3 -1) 1))").unwrap();
        reports.push(r.report());
    }
    let work = |i: usize, k: &str| reports[i]["cell_work"][k].as_u64().unwrap();
    assert!(work(0, "port_context_words") > 0);
    assert!(
        work(1, "port_context_words") < work(0, "port_context_words"),
        "{reports:?}"
    );
    assert!(
        work(2, "port_probes") < work(1, "port_probes"),
        "{reports:?}"
    );
    assert!(work(2, "port_shared_joins_saved") > 0);
    assert_eq!(work(0, "port_bindings"), work(2, "port_bindings"));
}
#[test]
fn delayed_watermarks_union_subsume_and_compaction_keep_exact_frontiers() {
    let Some(w) = worker() else { return };
    let mut eg = environment();
    let all = eg
        .parse_program(None, "(run-schedule (seq consume make later))")
        .unwrap();
    let c = eg.parse_program(None, "(run consume 1)").unwrap();
    let m = eg.parse_program(None, "(run make 1)").unwrap();
    let l = eg.parse_program(None, "(run later 1)").unwrap();
    let mut r = Runtime::configured(
        eg,
        &all,
        &w,
        true,
        Options {
            analysis: Some(&library()),
            profile: true,
            port_locality: PortLocality::SharedJoins,
            ..Options::default()
        },
    )
    .unwrap();
    r.run(&c).unwrap();
    r.run(&m).unwrap();
    r.run(&c).unwrap();
    for n in 0..80 {
        r.input(&format!(
            "(let extra{n} (Single (Leaf {}))) (Boundary extra{n} {n})",
            n + 100
        ))
        .unwrap();
        r.run(&m).unwrap();
        r.run(&c).unwrap();
    }
    r.run(&l).unwrap();
    r.input("(union (Single (Leaf 999)) one)").unwrap();
    r.run(&all).unwrap();
    r.input("(subsume (Get one 0)) (Boundary one 12345)")
        .unwrap();
    r.run(&all).unwrap();
    let before = r.report();
    r.run(&all).unwrap();
    assert_eq!(
        before["runtime"]["dispatches"],
        r.report()["runtime"]["dispatches"]
    );
    assert!(r.report()["device"]["compactions"].as_u64().unwrap() > 0);
}

#[test]
fn consumers_with_different_native_watermarks_are_not_forced_into_one_frontier() {
    let Some(w) = worker() else { return };
    let mut eg = environment();
    for name in ["positive", "small"] {
        let sink = eg.prepare_cell_action("consume", name).unwrap();
        let query = eg.prepare_cell_query(&sink).unwrap();
        let rows = eg.run_cell_queries(&[&query]).unwrap();
        assert!(rows[0].is_empty());
        eg.apply_completed_cell_ruleset(&[(&sink, &rows[0])])
            .unwrap();
    }
    let all = eg
        .parse_program(None, "(run-schedule (seq make consume later))")
        .unwrap();
    let mut r = Runtime::configured(
        eg,
        &all,
        &w,
        true,
        Options {
            analysis: Some(&library()),
            profile: true,
            port_locality: PortLocality::SharedJoins,
            ..Options::default()
        },
    )
    .unwrap();
    r.run(&all).unwrap();
    r.engine()
        .clone()
        .parse_and_run_program(None, "(check (= (Count 0 0) 1)) (check (= (Count 1 0) 1))")
        .unwrap();
}
