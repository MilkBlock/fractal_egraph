use cell_assisted_egglog::{Session, resident::ResidentSession};
use egglog::EGraph;
use std::path::Path;

fn gpu(eg: EGraph, name: &str) -> Option<ResidentSession> {
    let worker = std::env::var_os("CELL_RESIDENT_WORKER")?;
    Some(
        ResidentSession::new(
            Session::for_query_validation(eg, "", name).unwrap(),
            Path::new(&worker),
        )
        .unwrap(),
    )
}
fn engine(source: &str) -> EGraph {
    let mut e = EGraph::default();
    e.parse_and_run_program(None, source).unwrap();
    e
}
fn check(e: &EGraph, code: &str) {
    e.clone().parse_and_run_program(None, code).unwrap();
}

#[test]
fn gpu_exact_i64_predicates_are_not_intern_id_or_float_comparisons() {
    let vals = [
        i64::MIN,
        -10000000000000001,
        -1,
        0,
        1,
        10000000000000000,
        10000000000000001,
        i64::MAX,
    ];
    for op in ["<", ">", "<=", ">="] {
        let mut e = engine(&format!(
            "(relation P (i64)) (relation Q (i64)) (relation Hit (i64 i64)) (rule ((P a) (Q b) ({op} a b)) ((Hit a b)) :name \"compare\")"
        ));
        for (i, v) in vals.iter().enumerate() {
            e.parse_and_run_program(None, &format!("(P {v}) (Q {})", vals[vals.len() - 1 - i]))
                .unwrap();
        }
        let Some(mut s) = gpu(e, "compare") else {
            return;
        };
        let rows = s.drain_matches().unwrap();
        assert_eq!(rows.len(), if op.len() == 1 { 28 } else { 36 });
        assert_eq!(s.stats.audit_uncovered_per_round, vec![0]);
        assert!(s.stats.gpu["guard_rejected"].as_u64().unwrap() > 0);
    }
}
#[test]
fn custom_function_update_and_late_change_reenable_gpu_match() {
    let e = engine(
        "(datatype E (V i64)) (function Best (E) i64 :merge new) (relation Offer (E i64)) (rule ((Offer e x) (= old (Best e)) (< x old)) ((set (Best e) (+ x 1))) :name \"improve\") (set (Best (V 0)) 0) (Offer (V 0) 4)",
    );
    let Some(mut s) = gpu(e, "improve") else {
        return;
    };
    s.round().unwrap();
    assert_eq!(s.stats.dispatched_bindings, 0);
    s.input("(set (Best (V 0)) 10)").unwrap();
    s.round().unwrap();
    check(s.engine(), "(check (= (Best (V 0)) 5))");
    s.input("(set (Best (V 0)) 3)").unwrap();
    let before = s.stats.dispatched_bindings;
    s.round().unwrap();
    assert_eq!(s.stats.dispatched_bindings, before);
    assert!(s.stats.audit_uncovered_per_round.iter().all(|n| *n == 0));
}
#[test]
fn native_custom_merge_and_union_effects_are_preserved() {
    let source = "(datatype E (V i64)) (datatype T (Term i64)) (datatype TC (TCPair T i64) (Smaller TC TC)) (function Best (E) TC :merge (Smaller old new)) (relation Offer (E TC)) (rule ((Offer e (TCPair t c)) (= (Best e) (TCPair old oc)) (< c oc)) ((set (Best e) (TCPair t c))) :name \"improve\") (set (Best (V 0)) (TCPair (Term 1) 5)) (Offer (V 0) (TCPair (Term 2) 3))";
    let Some(mut s) = gpu(engine(source), "improve") else {
        return;
    };
    s.round().unwrap();
    check(
        s.engine(),
        "(check (= (Best (V 0)) (Smaller (TCPair (Term 1) 5) (TCPair (Term 2) 3))))",
    );
    let mut native = engine(source);
    native.step_rules("").unwrap();
    assert_eq!(native.get_size("Smaller"), s.engine().get_size("Smaller"));
}
#[test]
fn checked_arithmetic_failure_does_not_stick_in_rhs_worklist() {
    let mut e = engine(
        "(relation P (i64)) (relation Q (i64)) (rule ((P n)) ((Q (+ n 1))) :name \"add\") (P 9223372036854775807)",
    );
    let sink = e.prepare_cell_action("", "add").unwrap();
    let rows = e.audit_cell_matches(&sink).unwrap();
    let before = e.num_tuples();
    assert!(
        e.apply_cell_bindings(&sink, &rows)
            .unwrap_err()
            .to_string()
            .contains("primitive + failed")
    );
    e.apply_cell_bindings(&sink, &[]).unwrap();
    assert_eq!(e.get_size("Q"), 0);
    assert_eq!(e.num_tuples(), before);
    e.parse_and_run_program(None, "(delete (P 9223372036854775807)) (P 1)")
        .unwrap();
    let rows = e.audit_cell_matches(&sink).unwrap();
    e.apply_cell_bindings(&sink, &rows).unwrap();
    check(&e, "(check (Q 2))");
}
#[test]
fn negative_cost_panic_is_not_erased() {
    let source = "(relation Cost (i64)) (rule ((Cost c) (< c 0)) ((panic \"Negative cost\")) :name \"negative\") (Cost -1)";
    let Some(mut s) = gpu(engine(source), "negative") else {
        return;
    };
    assert!(s.round().unwrap_err().to_string().contains("Negative cost"));
    assert!(
        engine(source)
            .step_rules("")
            .unwrap_err()
            .to_string()
            .contains("Negative cost")
    );
}
#[test]
fn invalid_guard_cannot_reach_the_native_rhs() {
    let mut e = engine(
        "(relation P (i64)) (relation Q (i64)) (rule ((P x) (> x 0)) ((Q x)) :name \"guard\") (P -1)",
    );
    let sink = e.prepare_cell_action("", "guard").unwrap();
    let mut row = vec![e.base_to_value(()); sink.slots().len()];
    for (i, (_, sort)) in sink.slots().iter().enumerate() {
        if *sort == "i64" {
            row[i] = e.base_to_value(-1i64);
        }
    }
    assert!(e.apply_cell_bindings(&sink, &[row]).is_err());
    assert_eq!(e.get_size("Q"), 0);
}

#[test]
fn analyzed_cell_with_custom_function_inputs_and_updates_runs_on_device() {
    use cell_assisted_egglog::Part;
    let Some(worker) = std::env::var_os("CELL_RESIDENT_WORKER") else {
        return;
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/function-update");
    let mut source = Session::new(
        &root.join("analyze"),
        "",
        "improve",
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
    )
    .unwrap();
    source.set_audit(true);
    let mut session = ResidentSession::new(source, Path::new(&worker)).unwrap();
    session
        .input(&std::fs::read_to_string(root.join("heldout.egg")).unwrap())
        .unwrap();
    for _ in 0..4 {
        session.round().unwrap();
    }
    check(
        session.engine(),
        "(check (= (Best (Leaf 200)) 10000000000000000)) (check (= (Best (Leaf 201)) 3)) (check (= (Best (Leaf 202)) 1)) (check (= (Get (Leaf 200) 0) (Leaf 100)))",
    );
    assert!(session.stats.gpu["active_instances"].as_u64().unwrap() > 0);
    assert_eq!(session.stats.audit_uncovered_per_round, vec![0, 0, 0, 0]);
}
