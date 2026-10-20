use cell_assisted_egglog::table_live::Runtime;
use egglog::EGraph;
fn check(runtime: &Runtime, code: &str) {
    runtime
        .engine()
        .clone()
        .parse_and_run_program(None, code)
        .unwrap();
}
fn worker() -> Option<std::path::PathBuf> {
    std::env::var_os("CELL_MATRIX_WORKER").map(Into::into)
}
#[test]
fn computed_keys_and_late_aliases_use_the_complete_native_frontier() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype E (N i64)) (relation Input (E i64)) (relation At (E i64 E)) (relation Hit (E E))
      (ruleset s)
      (rule ((Input ctx i) (At ctx (+ i 1) out) (!= ctx out)) ((Hit ctx out)) :ruleset s)
      (Input (N 0) 8) (At (N 0) 9 (N 9)) (At (N 0) 10 (N 10))
      (Input (N 1) 9223372036854775807) (At (N 1) -9223372036854775808 (N 2))
    "#,
    )
    .unwrap();
    let commands = eg.parse_program(None, "(run s 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands).unwrap();
    check(
        &r,
        "(check (Hit (N 0) (N 9))) (fail (check (Hit (N 0) (N 10)))) (fail (check (Hit (N 1) (N 2))))",
    );
    r.input("(Input (N 0) 9) (union (N 0) (N 10))").unwrap();
    r.run(&commands).unwrap();
    check(&r, "(fail (check (Hit (N 0) (N 10))))");
}
#[test]
fn remainder_and_derived_guards_match_exact_signed_i64_semantics() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (relation Input (i64 i64)) (relation Result (i64 i64 i64)) (ruleset s)
      (rule ((Input a b) (= r (% a b)) (!= r 0)) ((Result a b r)) :ruleset s)
    "#,
    )
    .unwrap();
    for a in [i64::MIN, i64::MAX, -5, -4, -1, 0, 1, 4, 5] {
        for b in [i64::MIN, i64::MAX, -4, -1, 0, 1, 4] {
            eg.parse_and_run_program(None, &format!("(Input {a} {b})"))
                .unwrap();
        }
    }
    let commands = eg.parse_program(None, "(run s 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands).unwrap();
    check(
        &r,
        "(check (Result -5 4 -1)) (fail (check (Result -9223372036854775808 -1 0)))",
    );
}
#[test]
fn combined_run_globals_and_subsume_keep_one_frozen_boundary() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype E (N i64)) (let G (N 0))
      (relation P (E)) (relation Q (E)) (relation R (E)) (relation Global (E E))
      (function Count () i64 :merge (+ old new))
      (ruleset a) (ruleset b) (unstable-combined-ruleset ab a b)
      (rule () ((set (Count) 1)) :ruleset a)
      (rule ((P x)) ((Q x) (subsume (P x))) :ruleset a)
      (rule ((Q x)) ((R x) (Global x G)) :ruleset b)
      (P (N 1))
    "#,
    )
    .unwrap();
    let commands = eg.parse_program(None, "(run ab 1) (run a 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands[..1]).unwrap();
    check(
        &r,
        "(check (Q (N 1))) (fail (check (R (N 1)))) (check (= (Count) 1))",
    );
    r.run(&commands).unwrap();
    check(&r, "(check (Global (N 1) (N 0))) (check (= (Count) 1))");
    r.input("(union (N -1) (N 0)) (P (N 2))").unwrap();
    r.run(&commands).unwrap();
    r.run(&commands).unwrap();
    check(&r, "(check (Global (N 2) (N -1))) (check (= (Count) 1))");
}
#[test]
fn empty_graph_unit_query_fires_once() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,"(function Count () i64 :merge (+ old new)) (ruleset s) (rule () ((set (Count) 1)) :ruleset s)").unwrap();
    let commands = eg.parse_program(None, "(run s 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands).unwrap();
    r.run(&commands).unwrap();
    check(&r, "(check (= (Count) 1))");
}
