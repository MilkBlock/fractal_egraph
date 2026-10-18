use cell_assisted_egglog::table_live::Runtime;
use egglog::{EGraph, ast::Command};
use std::path::{Path, PathBuf};
fn worker() -> Option<PathBuf> {
    std::env::var_os("CELL_MATRIX_WORKER").map(Into::into)
}
fn library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze")
}
fn setup() -> (EGraph, Vec<Command>) {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
      (relation Boundary (Expr i64)) (relation Seen (Expr Expr i64))
      (relation SeenAgain (Expr Expr i64)) (ruleset consume)
      (rule ((Boundary s n) (= s (Single y)) (= x (Get s 0)) (>= n 0))
            ((Seen x y n)) :ruleset consume :name "one")
      (rule ((Boundary s n) (= s (Single y)) (= x (Get s 0)) (< n 100000))
            ((SeenAgain x y n)) :ruleset consume :name "two")
      (let x (Leaf 10)) (let y (Leaf 20)) (let s (Single x))
      (union (Get s 0) x) (Boundary s 0)
    "#,
    )
    .unwrap();
    let commands = eg.parse_program(None, "(run consume 1)").unwrap();
    (eg, commands)
}
fn check(r: &Runtime, text: &str) {
    r.engine()
        .clone()
        .parse_and_run_program(None, text)
        .unwrap();
}
#[test]
fn boundary_deltas_reuse_cell_projection_in_the_middle_of_full_queries() {
    let Some(w) = worker() else { return };
    let (eg, cmds) = setup();
    let mut r = Runtime::new_with_cells(eg, &cmds, &w, true, Some(&library()), true).unwrap();
    r.run(&cmds).unwrap();
    let before = r.report()["cell_work"]["skipped_join_probes"]
        .as_u64()
        .unwrap();
    r.input("(Boundary s 1) (Boundary s 2) (Boundary s 3)")
        .unwrap();
    r.run(&cmds).unwrap();
    r.input("(Boundary s 4)").unwrap();
    r.run(&cmds).unwrap();
    let report = r.report();
    assert!(
        report["cell_work"]["skipped_join_probes"].as_u64().unwrap() > before,
        "external-only delta must use the internal shortcut: {report}"
    );
    assert!(report["cell_work"]["certificate_reuses"].as_u64().unwrap() > 0);
    assert_eq!(r.engine().get_size("Seen"), 5);
    assert_eq!(r.engine().get_size("SeenAgain"), 5);
    // Two rules and all their seminaive anchors share the forward
    // projections; they do not each allocate an independent instance registry.
    assert_eq!(report["device"]["cell_plans"], 1);
}
#[test]
fn new_union_alternative_falls_back_without_losing_a_binding_then_recertifies() {
    let Some(w) = worker() else { return };
    let (eg, cmds) = setup();
    let mut r = Runtime::new_with_cells(eg, &cmds, &w, true, Some(&library()), true).unwrap();
    r.run(&cmds).unwrap();
    r.input("(union (Single y) s) (Boundary s 1)").unwrap();
    r.run(&cmds).unwrap();
    check(&r, "(check (Seen x x 1)) (check (Seen x y 1))");
    let report = r.report();
    // Reverse alternatives are enumerated by the ordinary query, not cached.
    assert_eq!(report["device"]["cell_plans"], 1);
    assert!(report["cell_work"]["shape_fallbacks"].as_u64().unwrap()>0);
    r.input("(union x y) (Boundary s 2)").unwrap();
    r.run(&cmds).unwrap();
    check(&r, "(check (Seen x x 2))");
}
#[test]
fn subsume_and_slot_recycling_do_not_leave_a_false_cell_guarantee() {
    let Some(w) = worker() else { return };
    let (eg, cmds) = setup();
    let mut r = Runtime::new_with_cells(eg, &cmds, &w, true, Some(&library()), true).unwrap();
    r.run(&cmds).unwrap();
    r.input("(subsume (Get s 0)) (Boundary s 1)").unwrap();
    r.run(&cmds).unwrap();
    assert_eq!(r.engine().get_size("Seen"), 1);
    assert!(r.report()["cell_work"]["invalidated_certificates"].as_u64().unwrap()>0);
    // Restore a genuinely different instance; old native row slots can be reused.
    r.input("(let t (Single y)) (union (Get t 0) y) (Boundary t 2)")
        .unwrap();
    r.run(&cmds).unwrap();
    check(&r, "(check (Seen y y 2))");
    for i in 3..30 {
        r.input(&format!("(Boundary t {i})")).unwrap();
        r.run(&cmds).unwrap();
    }
    assert!(r.report()["device"]["compactions"].as_u64().unwrap() > 0);
}
#[test]
fn unsupported_shape_remains_complete_and_numeric_guards_are_not_baked_away() {
    let Some(w) = worker() else { return };
    let (eg, cmds) = setup();
    let mut r = Runtime::new_with_cells(eg, &cmds, &w, true, Some(&library()), true).unwrap();
    r.input("(let t (Single y)) (union (Get t 0) x) (Boundary t -1) (Boundary t 100000)")
        .unwrap();
    r.run(&cmds).unwrap();
    check(&r, "(check (Seen x y 100000)) (check (SeenAgain x y -1))");
    assert_eq!(r.engine().get_size("Seen"), 2);
    assert_eq!(r.engine().get_size("SeenAgain"), 2);
}

#[test]
fn two_cell_segments_preserve_binding_backtracking_and_aliases() {
    let Some(w) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
      (relation Edge (Expr Expr)) (relation Seen (Expr Expr)) (ruleset consume)
      (rule ((Edge a b) (= a (Single x)) (= x (Get a 0))
             (= b (Single y)) (= y (Get b 0))) ((Seen x y)) :ruleset consume)
      (let x (Leaf 0)) (let y (Leaf 1))
      (let a (Single x)) (let b (Single y))
      (union (Get a 0) x) (union (Get b 0) y)
      (Edge a a) (Edge a b) (Edge b a) (Edge b b)
    "#,
    )
    .unwrap();
    let cmds = eg.parse_program(None, "(run consume 1)").unwrap();
    let mut r = Runtime::new_with_cells(eg, &cmds, &w, true, Some(&library()), true).unwrap();
    r.run(&cmds).unwrap();
    assert_eq!(r.engine().get_size("Seen"), 4);
    r.input("(union x y)").unwrap();
    r.run(&cmds).unwrap();
    assert_eq!(r.engine().get_size("Seen"), 1);
}
