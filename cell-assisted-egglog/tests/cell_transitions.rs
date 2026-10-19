use cell_assisted_egglog::table_live::Runtime;
use egglog::EGraph;
use std::path::{Path, PathBuf};
fn worker() -> Option<PathBuf> {
    std::env::var_os("CELL_MATRIX_WORKER").map(Into::into)
}
fn library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze")
}
fn check(r: &Runtime, src: &str) {
    r.engine().clone().parse_and_run_program(None, src).unwrap();
}

#[test]
fn native_rhs_commit_completes_a_waiting_cell_and_drives_its_consumer() {
    let Some(w) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
 (relation Boundary (Expr i64)) (relation Seen (Expr i64))
 (ruleset make) (ruleset consume)
 (rule ((Single x)) ((union (Get (Single x) 0) x)) :ruleset make)
 (rule ((Boundary s n) (= s (Single x)) (= x (Get s 0)) (>= n 0)) ((Seen x n)) :ruleset consume)
 (let x (Leaf 7)) (let s (Single x)) (Boundary s 42) (Boundary s -1)
 "#,
    )
    .unwrap();
    eg.parse_and_run_program(
        None,
        &(0..64)
            .map(|n| format!("(Boundary s {n})"))
            .collect::<String>(),
    )
    .unwrap();
    let cmds = eg
        .parse_program(None, "(run-schedule (seq consume make consume))")
        .unwrap();
    let mut r = Runtime::new_with_cells(eg, &cmds, &w, true, Some(&library()), true).unwrap();
    r.run(&cmds).unwrap();
    check(&r, "(check (Seen x 42))");
    assert_eq!(r.engine().get_size("Seen"), 64);
    let stats = r.report();
    assert!(
        stats["cell_work"]["missing_key_waits"].as_u64().unwrap() > 0,
        "{stats}"
    );
    assert!(
        stats["cell_work"]["port_bindings"].as_u64().unwrap() > 0,
        "the Cell port, not a repeated full lookup, must drive the late match: {stats}"
    );
    assert_eq!(stats["cell_work"]["port_bindings"], 64);
    assert!(stats["device"]["retries"].as_u64().unwrap() > 0);
    assert!(stats["cell_work"]["delegated_branches"].as_u64().unwrap() > 0);
}

fn partial() -> EGraph {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
 (relation Boundary (Expr i64))
 (function Count (i64) i64 :merge (+ old new))
 (function Pulse (i64) i64 :merge new)
 (ruleset consume) (ruleset tick)
 (rule ((= x (Get o n)) (= s (Single x)) (= x (Get s 0)) (Boundary o m))
       ((set (Count m) 1)) :ruleset consume)
 (rule ((= n (Pulse 0))) ((set (Pulse 0) n)) :ruleset tick)
 (let x (Leaf 7)) (let o (Leaf 8)) (let s (Single x))
 (union (Get o 3) x) (Boundary o 99)
 (set (Pulse 0) 0)
 "#,
    )
    .unwrap();
    eg
}
#[test]
fn missing_suffix_resumes_from_retained_binding_without_replaying_prefix() {
    let Some(w) = worker() else { return };
    let mut eg = partial();
    let cmds = eg.parse_program(None, "(run consume 1)").unwrap();
    let mut r = Runtime::new_with_cells(eg, &cmds, &w, true, Some(&library()), true).unwrap();
    r.run(&cmds).unwrap();
    assert_eq!(r.engine().get_size("Count"), 0);
    r.input("(union (Get s 0) x)").unwrap();
    r.run(&cmds).unwrap();
    check(&r, "(check (= (Count 99) 1))");
    let stats = r.report();
    assert!(
        stats["cell_work"]["prefix_rows_reused"].as_u64().unwrap() > 0,
        "{stats}"
    );
    assert!(
        stats["cell_work"]["port_bindings"].as_u64().unwrap() > 0,
        "{stats}"
    );
    assert!(
        stats["cell_work"]["direct_row_reuses"].as_u64().unwrap() > 0,
        "{stats}"
    );
    r.run(&cmds).unwrap();
    check(&r, "(check (= (Count 99) 1))");
}
#[test]
fn watch_and_port_compaction_preserve_a_delayed_consumer_without_history_replay() {
    let Some(w) = worker() else { return };
    let mut eg = partial();
    eg.parse_and_run_program(None, "(union (Get s 0) x)")
        .unwrap();
    let all = eg
        .parse_program(None, "(run-schedule (seq consume tick))")
        .unwrap();
    let consume = eg.parse_program(None, "(run consume 1)").unwrap();
    let tick = eg.parse_program(None, "(run tick 1)").unwrap();
    let mut r = Runtime::new_with_cells(eg, &all, &w, true, Some(&library()), true).unwrap();
    r.run(&consume).unwrap();
    let rounds = r.report()["device"]["watch_capacity"]
        .as_u64()
        .unwrap()
        .max(r.report()["device"]["port_capacity"].as_u64().unwrap())
        * 2;
    for i in 1..rounds {
        r.input(&format!(
            "(delete (Get o 3)) (union (Get o 3) x) (set (Pulse 0) {i})"
        ))
        .unwrap();
        r.run(&tick).unwrap();
    }
    r.run(&consume).unwrap();
    check(&r, "(check (= (Count 99) 2))");
    r.run(&all).unwrap();
    check(&r, "(check (= (Count 99) 2))");
    let stats = r.report();
    assert!(
        stats["device"]["watch_compactions"].as_u64().unwrap() > 1,
        "{stats}"
    );
    assert!(
        stats["device"]["port_compactions"].as_u64().unwrap() > 1,
        "{stats}"
    );
    assert!(
        stats["device"]["watch_count"].as_u64().unwrap()
            <= stats["device"]["watch_capacity"].as_u64().unwrap(),
        "an exactly full dependency buffer is valid; publication must not overflow"
    );
}

#[test]
fn one_ready_cell_grafts_a_child_cell_and_the_child_drives_the_next_stage() {
    let Some(w) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
      (relation Boundary (Expr i64)) (relation Reached (Expr))
      (ruleset make) (ruleset graft) (ruleset finish)
      (rule ((Single x)) ((union (Get (Single x) 0) x)) :ruleset make)
      (rule ((Boundary s n) (= s (Single x)) (= x (Get s 0)) (< n 2))
            ((Boundary (Single s) (+ n 1))) :ruleset graft)
      (rule ((Boundary s 2) (= s (Single x)) (= x (Get s 0)))
            ((Reached s)) :ruleset finish)
      (Boundary (Single (Leaf 7)) 0)
    "#,
    )
    .unwrap();
    let cmds = eg
        .parse_program(None, "(run-schedule (saturate (seq graft make finish)))")
        .unwrap();
    let mut r = Runtime::new_with_cells(eg, &cmds, &w, true, Some(&library()), true).unwrap();
    r.run(&cmds).unwrap();
    check(&r, "(check (Reached (Single (Single (Single (Leaf 7))))))");
    assert_eq!(r.engine().get_size("Boundary"), 3);
    assert_eq!(r.engine().get_size("Reached"), 1);
    let report = r.report();
    assert!(report["cell_work"]["ports_published"].as_u64().unwrap() >= 3);
    assert!(report["cell_work"]["port_bindings"].as_u64().unwrap() > 0);
    let before = report["runtime"]["dispatches"].clone();
    r.run(&cmds).unwrap();
    assert_eq!(r.report()["runtime"]["dispatches"], before);
}
