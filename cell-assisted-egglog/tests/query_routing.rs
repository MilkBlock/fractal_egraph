use cell_assisted_egglog::table_live::{Options, PortLocality, QueryRouting, Runtime};
use egglog::EGraph;
use std::path::{Path, PathBuf};
fn worker() -> Option<PathBuf> {
    std::env::var_os("CELL_MATRIX_WORKER").map(Into::into)
}
fn library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze")
}
fn setup() -> EGraph {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
 (relation Boundary (Expr i64)) (relation Offer (Expr i64)) (relation Allowed (i64))
 (function Count (Expr i64) i64 :merge (+ old new))
 (ruleset make) (ruleset consume)
 (rule ((Single x)) ((union (Get (Single x) 0) x)) :ruleset make)
 (rule ((Boundary s n) (= s (Single x)) (= x (Get s 0)) (Offer s k) (Allowed k) (>= n 0))
       ((set (Count x n) 1)) :ruleset consume :name "five-atoms")
 (let a (Leaf 1)) (let b (Leaf 2)) (let c (Leaf 3))
 (let good (Single a)) (let waiting (Single b)) (let different (Single c))
 (union (Get good 0) a) (union (Get different 0) b)
 (Offer good 1) (Offer waiting 2) (Offer different 3)
 (Allowed 1) (Allowed 2) (Allowed 3)
 (Boundary good 0) (Boundary waiting 0) (Boundary different 0)
 "#,
    )
    .unwrap();
    eg
}
#[test]
fn five_atom_seeds_are_classified_and_all_classes_remain_complete() {
    let Some(w) = worker() else { return };
    let mut reports = vec![];
    for mode in [
        QueryRouting::Direct,
        QueryRouting::Compact,
        QueryRouting::CellPriority,
    ] {
        let mut eg = setup();
        let all = eg
            .parse_program(None, "(run-schedule (seq consume make consume))")
            .unwrap();
        let consume = eg.parse_program(None, "(run consume 1)").unwrap();
        let mut r = Runtime::configured(
            eg,
            &all,
            &w,
            true,
            Options {
                analysis: Some(&library()),
                profile: true,
                query_routing: mode,
                port_locality: PortLocality::SharedJoins,
                ..Options::default()
            },
        )
        .unwrap();
        r.run(&consume).unwrap();
        r.input("(Boundary good 10) (Boundary waiting 10) (Boundary different 10)")
            .unwrap();
        r.run(&consume).unwrap();
        r.run(&all).unwrap();
        r.engine()
            .clone()
            .parse_and_run_program(
                None,
                "(check (= (Count a 10) 1)) (check (= (Count b 10) 2)) (check (= (Count c 10) 2))",
            )
            .unwrap();
        reports.push(r.report());
    }
    let w = &reports[2]["cell_work"];
    assert!(w["routed_ready"].as_u64().unwrap() > 0, "{w}");
    assert!(w["routed_suspected"].as_u64().unwrap() > 0, "{w}");
    assert!(w["routed_unmapped"].as_u64().unwrap() > 0, "{w}");
    assert!(w["routing_unique_hints"].as_u64().unwrap() > 0, "{w}");
    assert!(w["ready_reuse_seeds"].as_u64().unwrap() > 0, "{w}");
    for r in &reports {
        assert_eq!(
            r["rules"]
                .as_array()
                .unwrap()
                .iter()
                .map(|q| q["stats"]["emitted_bindings"].as_u64().unwrap())
                .sum::<u64>(),
            reports[0]["rules"]
                .as_array()
                .unwrap()
                .iter()
                .map(|q| q["stats"]["emitted_bindings"].as_u64().unwrap())
                .sum::<u64>()
        );
    }
}
#[test]
fn ambiguous_reverse_cell_location_preserves_every_binding_after_union() {
    let Some(w) = worker() else { return };
    let mut eg = setup();
    let all = eg.parse_program(None, "(run consume 1)").unwrap();
    let mut r = Runtime::configured(
        eg,
        &all,
        &w,
        true,
        Options {
            analysis: Some(&library()),
            profile: true,
            query_routing: QueryRouting::CellPriority,
            ..Options::default()
        },
    )
    .unwrap();
    r.run(&all).unwrap();
    r.input("(union (Single b) good) (Boundary good 5)")
        .unwrap();
    r.run(&all).unwrap();
    r.input("(union a b) (Boundary good 6)").unwrap();
    r.run(&all).unwrap();
    r.engine()
        .clone()
        .parse_and_run_program(None, "(check (= (Count a 6) 2))")
        .unwrap();
    let work = r.report()["cell_work"].clone();
    assert!(work["routing_ambiguous"].as_u64().unwrap() > 0, "{work}");
}
#[test]
fn queued_hints_never_survive_subsumption_or_native_row_recycling() {
    let Some(w) = worker() else { return };
    let mut eg = setup();
    let all = eg.parse_program(None, "(run consume 1)").unwrap();
    let mut r = Runtime::configured(
        eg,
        &all,
        &w,
        true,
        Options {
            analysis: Some(&library()),
            profile: true,
            query_routing: QueryRouting::CellPriority,
            ..Options::default()
        },
    )
    .unwrap();
    r.run(&all).unwrap();
    r.input("(subsume (Get good 0)) (Boundary good 1)").unwrap();
    r.run(&all).unwrap();
    for n in 2..40 {
        r.input(&format!(
            "(delete (Boundary waiting 0)) (Boundary waiting {n})"
        ))
        .unwrap();
        r.run(&all).unwrap();
    }
    let old = r.report();
    r.run(&all).unwrap();
    assert_eq!(
        old["runtime"]["dispatches"],
        r.report()["runtime"]["dispatches"]
    );
}

#[test]
fn no_cell_catalog_still_routes_every_supported_query_without_dropping_unknowns() {
    let Some(w) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,"(datatype E (V i64)) (relation P (E)) (relation Q (E)) (ruleset r) (rule ((P x)) ((Q x)) :ruleset r) (P (V 1))").unwrap();
    let cmds = eg.parse_program(None, "(run r 1)").unwrap();
    let mut r = Runtime::configured(
        eg,
        &cmds,
        &w,
        true,
        Options {
            query_routing: QueryRouting::CellPriority,
            profile: true,
            ..Options::default()
        },
    )
    .unwrap();
    r.run(&cmds).unwrap();
    r.engine()
        .clone()
        .parse_and_run_program(None, "(check (Q (V 1)))")
        .unwrap();
    assert_eq!(r.report()["cell_work"]["routed_ready"], 0);
    assert!(r.report()["cell_work"]["routed_unmapped"].as_u64().unwrap() > 0);
}

#[test]
fn a_cell_certificate_delegates_internal_delta_seeds_before_deep_matching() {
    let Some(w) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
 (relation Boundary (Expr i64)) (relation Seen (Expr i64))
 (ruleset make) (ruleset use)
 (rule ((Single x)) ((union (Get (Single x) 0) x)) :ruleset make)
 (rule ((Boundary s n) (= s (Single x)) (= x (Get s 0))) ((Seen x n)) :ruleset use)
 (Boundary (Single (Leaf 1)) 5)
 "#,
    )
    .unwrap();
    let cmds = eg
        .parse_program(None, "(run-schedule (seq use make use))")
        .unwrap();
    let mut r = Runtime::configured(
        eg,
        &cmds,
        &w,
        true,
        Options {
            analysis: Some(&library()),
            profile: true,
            query_routing: QueryRouting::CellPriority,
            ..Options::default()
        },
    )
    .unwrap();
    r.run(&cmds).unwrap();
    let work = r.report()["cell_work"].clone();
    assert!(
        work["routing_delegated_to_port"].as_u64().unwrap() > 0,
        "{work}"
    );
    assert_eq!(work["port_bindings"], 1);
    assert_eq!(r.engine().get_size("Seen"), 1);
}

#[test]
fn absent_locator_is_rejected_only_for_this_snapshot_and_union_can_enable_it() {
    let Some(w) = worker() else { return };
    let mut eg = setup();
    let cmds = eg.parse_program(None, "(run consume 1)").unwrap();
    let mut r = Runtime::configured(
        eg,
        &cmds,
        &w,
        true,
        Options {
            analysis: Some(&library()),
            profile: true,
            query_routing: QueryRouting::CellPriority,
            ..Options::default()
        },
    )
    .unwrap();
    r.run(&cmds).unwrap();
    r.input("(Boundary (Leaf 999) 99)").unwrap();
    r.run(&cmds).unwrap();
    assert!(
        r.report()["cell_work"]["routing_no_next_atom"]
            .as_u64()
            .unwrap()
            > 0
    );
    r.input("(union (Leaf 999) good)").unwrap();
    r.run(&cmds).unwrap();
    r.engine()
        .clone()
        .parse_and_run_program(None, "(check (= (Count a 99) 1))")
        .unwrap();
}
