use cell_assisted_egglog::{Part, Session, environment, numeric_native::NumericSession};
use std::path::Path;
fn setup(extension: &str) -> Option<NumericSession> {
    let worker = std::env::var_os("CELL_MATRIX_WORKER")?;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    let mut source = Session::with_extension(
        &dir,
        "",
        "cost-improve",
        vec!["single-get".into()],
        vec![
            Part {
                atoms: vec![0],
                cell: true,
            },
            Part {
                atoms: vec![1],
                cell: false,
            },
            Part {
                atoms: vec![2],
                cell: false,
            },
        ],
        extension,
    )
    .unwrap();
    source.set_audit(true);
    let env = format!("{}\n{extension}", environment(&dir).unwrap().0);
    Some(NumericSession::new(source, &env, Path::new(&worker)).unwrap())
}
fn extension() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("matrix-numeric/consumer.egg"),
    )
    .unwrap()
}
fn rounds(s: &mut NumericSession) {
    for _ in 0..4 {
        s.round().unwrap();
    }
}
fn check(s: &NumericSession, text: &str) {
    s.engine()
        .clone()
        .parse_and_run_program(None, text)
        .unwrap();
}
#[test]
fn actual_cell_numeric_guard_and_colliding_merge_match_native() {
    let ext = extension();
    let Some(mut s) = setup(&ext) else { return };
    let input = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("matrix-numeric/input.egg"),
    )
    .unwrap();
    s.input(&input).unwrap();
    rounds(&mut s);
    check(
        &s,
        "(check (= (Best (Leaf 200)) 9999999999999999)) (check (= (Best (Leaf 201)) 2)) (check (= (Best (Leaf 202)) 1))",
    );
    assert_eq!(s.stats.accepted_bindings, 3);
    assert_eq!(s.stats.native_sets, 2);
    assert!(s.stats.audit_uncovered.iter().all(|v| *v == 0));
    let before = s.stats.device["execution_counts"].clone();
    let writes = s.stats.native_sets;
    s.round().unwrap();
    assert_eq!(before, s.stats.device["execution_counts"]);
    assert_eq!(s.stats.native_sets, writes);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    let mut native = egglog::EGraph::default();
    native
        .parse_and_run_program(
            None,
            &format!(
                "{}\n{ext}\n{input}\n(run-schedule (saturate (run)))",
                environment(&dir).unwrap().0
            ),
        )
        .unwrap();
    for k in [200, 201, 202] {
        let e = egglog::ast::Parser::default()
            .get_expr_from_string(None, &format!("(Best (Leaf {k}))"))
            .unwrap();
        let a = native.eval_expr(&e).unwrap().1;
        let mut actual = s.engine().clone();
        let b = actual.eval_expr(&e).unwrap().1;
        assert_eq!(
            native.value_to_base::<i64>(a),
            actual.value_to_base::<i64>(b)
        );
    }
}
#[test]
fn native_union_rekeys_before_numeric_merge_and_late_rows_reenable_guards() {
    let Some(mut s) = setup(&extension()) else {
        return;
    };
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 10) (set (Best (Leaf 3)) 4)")
        .unwrap();
    rounds(&mut s);
    s.input("(union (Leaf 2) (Leaf 3)) (Offer (Leaf 2) (Single (Leaf 1)) 3) (Offer (Leaf 3) (Single (Leaf 1)) 2)").unwrap();
    rounds(&mut s);
    check(
        &s,
        "(check (= (Best (Leaf 2)) 1)) (check (= (Best (Leaf 3)) 1))",
    );
    assert_eq!(s.stats.native_sets, 1);
    assert_eq!(s.stats.accepted_bindings, 2);
    assert!(s.stats.union_events > 0);
    s.input("(Offer (Leaf 3) (Single (Leaf 1)) 0)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) -1))");
    assert!(s.stats.audit_uncovered.iter().all(|v| *v == 0));
}
#[test]
fn max_merge_and_updated_guard_use_actual_payloads() {
    let ext = extension()
        .replace("min old new", "max old new")
        .replace("(< cost old)", "(> cost old)")
        .replace("(- cost 1)", "(+ cost 1)");
    let Some(mut s) = setup(&ext) else { return };
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) -100) (Offer (Leaf 2) (Single (Leaf 1)) 3) (Offer (Leaf 2) (Single (Leaf 1)) 7)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 8))");
    assert_eq!(s.stats.native_sets, 1);
    s.input("(set (Best (Leaf 2)) 100)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 100))");
    assert_eq!(s.stats.native_sets, 1);
}
#[test]
fn overflow_commits_no_partial_batch_and_recovery_keeps_valid_completions() {
    let ext = extension()
        .replace("min old new", "max old new")
        .replace("(< cost old)", "(>= cost old)")
        .replace("(- cost 1)", "(+ cost 1)");
    let Some(mut s) = setup(&ext) else { return };
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 0) (Offer (Leaf 2) (Single (Leaf 1)) 4) (Offer (Leaf 2) (Single (Leaf 1)) 9223372036854775807)").unwrap();
    assert!(
        s.round()
            .unwrap_err()
            .to_string()
            .contains("checked i64 RHS failed")
    );
    check(&s, "(check (= (Best (Leaf 2)) 0))");
    let counts = s.stats.device["execution_counts"].clone();
    s.input("(delete (Offer (Leaf 2) (Single (Leaf 1)) 9223372036854775807))")
        .unwrap();
    s.round().unwrap();
    check(&s, "(check (= (Best (Leaf 2)) 5))");
    for (i, c) in counts.as_array().unwrap().iter().enumerate() {
        assert_eq!(*c, s.stats.device["execution_counts"][i]);
    }
}
#[test]
fn non_lattice_merge_and_extra_effects_are_not_miscompiled() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    for ext in [
        extension().replace("min old new", "+ old new"),
        extension().replace(
            "((set (Best dst) (- cost 1)))",
            "((set (Best dst) (- cost 1)) (union dst x))",
        ),
    ] {
        let source = Session::with_extension(
            &dir,
            "",
            "cost-improve",
            vec!["single-get".into()],
            vec![
                Part {
                    atoms: vec![0],
                    cell: true,
                },
                Part {
                    atoms: vec![1],
                    cell: false,
                },
                Part {
                    atoms: vec![2],
                    cell: false,
                },
            ],
            &ext,
        )
        .unwrap();
        let env = format!("{}\n{ext}", environment(&dir).unwrap().0);
        assert!(NumericSession::new(source, &env, Path::new("/not-a-worker")).is_err());
    }
}

#[test]
fn new_function_row_can_be_created_without_an_old_value_guard() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    let ext = "(function Best (Expr) i64 :merge (min old new)) (relation Offer (Expr Expr i64)) (rule ((Offer dst (Single x) cost)) ((set (Best dst) cost)) :name \"cost-improve\")";
    let mut source = Session::with_extension(
        &dir,
        "",
        "cost-improve",
        vec!["single-get".into()],
        vec![
            Part {
                atoms: vec![0],
                cell: true,
            },
            Part {
                atoms: vec![1],
                cell: false,
            },
        ],
        ext,
    )
    .unwrap();
    source.set_audit(true);
    let env = format!("{}\n{ext}", environment(&dir).unwrap().0);
    let mut s = NumericSession::new(source, &env, Path::new(&worker)).unwrap();
    s.input("(Single (Leaf 1)) (Offer (Leaf 2) (Single (Leaf 1)) -9223372036854775808)")
        .unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) -9223372036854775808))");
    assert_eq!(s.stats.native_sets, 1);
}
