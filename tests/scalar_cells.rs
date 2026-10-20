#[allow(dead_code)]
#[path = "../examples/support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::{parameterized_cell::Runtime, table_live};
use egglog::{CommandOutput, EGraph};
const BASE: &str = include_str!("../experiments/parameterized_cell/terms.egg");
const SCALAR: &str = r#"
(function Size (Expr) i64 :merge (min old new))
(relation Seen (Expr i64)) (ruleset work)
(rule ((= e (Bop op x y)) (= a (Size x)) (= b (Size y)) (= c (+ a b)))
      ((set (Size e) (+ c 1))) :ruleset work :name "size")
(rule ((= e (Bop op x y)) (= c (Size e))) ((Seen e c)) :ruleset work :name "observe")
"#;
fn source() -> String {
    format!("{BASE}\n{SCALAR}")
}
fn flags(outputs: Vec<CommandOutput>) -> bool {
    outputs
        .into_iter()
        .any(|o| matches!(o,CommandOutput::RunSchedule(r) if r.updated))
}

#[test]
fn scalar_and_remaining_rules_read_one_frozen_state_and_commit_together() {
    let mut r = Runtime::new_joint(&source()).unwrap();
    let base = r.engine().clone();
    let mut native = base.clone();
    let commands = native.parse_program(None, "(run work 1)").unwrap();
    for input in [
        "(Bop (Add) (V 0) (V 1)) (set (Size (V 0)) 10) (set (Size (V 1)) 20)",
        "",
        "(set (Size (V 0)) 3)",
        "",
        "(union (V 0) (V 1))",
        "",
    ] {
        native.parse_and_run_program(None, input).unwrap();
        r.input(input).unwrap();
        let updated = flags(native.run_program(commands.clone()).unwrap());
        r.run(&commands).unwrap();
        assert_eq!(r.last_updated, updated);
        r.audit_mirror().unwrap();
        compare::equal(&base, &native, r.engine()).unwrap();
    }
    assert!(
        r.report()["scalar_cells"][0]["stats"]["structural_candidates"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn min_noops_are_certified_but_rhs_overflow_is_not_discarded() {
    let mut r = Runtime::new_joint(&source()).unwrap();
    let input = "(Bop (Add) (V 0) (V 1)) (set (Size (V 0)) 10) (set (Size (V 1)) 20) (set (Size (Bop (Add) (V 0) (V 1))) 1)";
    r.input(input).unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native.parse_program(None, "(run work 1)").unwrap();
    native.run_program(commands.clone()).unwrap();
    r.run(&commands).unwrap();
    compare::equal(&base, &native, r.engine()).unwrap();
    assert_eq!(r.report()["scalar_cells"][0]["stats"]["min_noops"], 1);
    for lhs in [9223372036854775807i64, 9223372036854775806i64] {
        let mut r = Runtime::new_joint(&source()).unwrap();
        r.input(&format!("(Bop (Add) (V 0) (V 1)) (set (Size (V 0)) {lhs}) (set (Size (V 1)) 1) (set (Size (Bop (Add) (V 0) (V 1))) 0)")).unwrap();
        let mut native = r.engine().clone();
        let commands = native.parse_program(None, "(run work 1)").unwrap();
        let expected = native.run_program(commands.clone());
        let actual = r.run(&commands);
        match (expected, actual) {
            (Ok(_), Ok(_)) => (),
            (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
            (a, b) => panic!("native={a:?} cell={b:?}"),
        }
    }
}

#[test]
fn scalar_proposals_and_native_unions_use_the_same_merge_batch() {
    let source = format!(
        "{}\n(rule ((= e (Bop op x y))) ((union e x)) :ruleset work :name \"merge-root\")",
        source()
    );
    let mut r = Runtime::new_joint(&source).unwrap();
    r.input("(Bop (Add) (V 0) (V 1)) (set (Size (V 0)) 2) (set (Size (V 1)) 3)")
        .unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native.parse_program(None, "(run work 1)").unwrap();
    for _ in 0..3 {
        native.run_program(commands.clone()).unwrap();
        r.run(&commands).unwrap();
        compare::equal(&base, &native, r.engine()).unwrap();
    }
}

#[test]
fn scalar_run_hook_operates_in_the_shared_event_loop() {
    let (mut base, hook) = Runtime::new_joint(&source())
        .unwrap()
        .detach_joint()
        .unwrap();
    let commands=base.parse_program(None,"(run work 2) (run-schedule (saturate (seq terms (saturate (seq terms-helpers (saturate terms-helpers-helpers))))))").unwrap();
    let mut native = base.clone();
    let mut r = table_live::Runtime::new_hybrid(
        base.clone(),
        &commands,
        None,
        false,
        &[
            "terms".into(),
            "terms-helpers".into(),
            "terms-helpers-helpers".into(),
        ],
    )
    .unwrap();
    r.set_saturation_hook(Box::new(hook));
    for input in [
        "(Bop (Add) (V 0) (V 1)) (set (Size (V 0)) 2) (set (Size (V 1)) 3)",
        "(set (Size (V 1)) 1)",
    ] {
        native.parse_and_run_program(None, input).unwrap();
        r.input(input).unwrap();
        native.run_program(commands.clone()).unwrap();
        r.run(&commands).unwrap();
        compare::equal(&base, &native, r.engine()).unwrap();
    }
    assert!(
        r.report()["saturation_diagnostics"]["scalar_cells"][0]["stats"]["runs"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn environment_generation_is_exact_and_private_sinks_do_not_change_it() {
    let mut e = EGraph::default();
    e.parse_and_run_program(None, BASE).unwrap();
    let stamp = e.cell_rule_environment_stamp();
    let mut clone = e.clone();
    assert_eq!(clone.cell_rule_environment_stamp(), stamp);
    e.prepare_cell_action("terms", "bop").unwrap();
    assert_eq!(e.cell_rule_environment_stamp(), stamp);
    clone
        .parse_and_run_program(None, "(ruleset other)")
        .unwrap();
    assert_ne!(clone.cell_rule_environment_stamp(), stamp);
    assert_eq!(e.cell_rule_environment_stamp(), stamp);
    let mut e2 = EGraph::default();
    e2.parse_and_run_program(None, BASE).unwrap();
    assert_ne!(e2.cell_rule_environment_stamp(), stamp);
    let old = clone.cell_rule_environment_stamp();
    clone
        .parse_and_run_program(
            None,
            "(rule ((V n)) ((V (+ n 1))) :ruleset terms :name \"new-producer\")",
        )
        .unwrap();
    assert_ne!(old, clone.cell_rule_environment_stamp());
}

#[test]
fn dominated_sets_do_not_change_a_later_stateful_consumers_activation() {
    let source = format!(
        "{}\n(function Count (Expr) i64 :merge (+ old new)) (rule ((= e (Bop op x y)) (= n (Size e))) ((set (Count e) 1)) :ruleset work :name \"count\")",
        source()
    );
    let mut r = Runtime::new_joint(&source).unwrap();
    r.input("(Bop (Add) (V 0) (V 1)) (set (Size (V 0)) 10) (set (Size (V 1)) 20) (set (Size (Bop (Add) (V 0) (V 1))) 1)").unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native.parse_program(None, "(run work 1)").unwrap();
    for _ in 0..4 {
        native.run_program(commands.clone()).unwrap();
        r.run(&commands).unwrap();
        compare::equal(&base, &native, r.engine()).unwrap();
    }
}

#[test]
fn all_scalar_rules_can_be_quiet_and_destination_is_mirrored_without_a_reader() {
    let source = format!(
        "{BASE}\n(function Size (Expr) i64 :merge (min old new)) (ruleset work) (rule ((V n)) ((set (Size (V n)) 1)) :ruleset work :name \"size-base\")"
    );
    let mut r = Runtime::new_joint(&source).unwrap();
    r.input("(V 7)").unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native.parse_program(None, "(run work 1)").unwrap();
    for _ in 0..3 {
        let expected = flags(native.run_program(commands.clone()).unwrap());
        r.run(&commands).unwrap();
        assert_eq!(r.last_updated, expected);
        r.audit_mirror().unwrap();
        compare::equal(&base, &native, r.engine()).unwrap();
    }
}

#[test]
fn same_batch_min_candidates_share_one_destination_without_losing_other_rules() {
    let mut r = Runtime::new_joint(&source()).unwrap();
    r.input("(set (Size (V 0)) 10) (set (Size (V 1)) 20) (set (Size (V 2)) 0) (union (Bop (Add) (V 0) (V 1)) (Bop (Mul) (V 0) (V 2)))").unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native.parse_program(None, "(run work 1)").unwrap();
    for _ in 0..2 {
        native.run_program(commands.clone()).unwrap();
        r.run(&commands).unwrap();
        compare::equal(&base, &native, r.engine()).unwrap();
    }
    assert!(
        r.report()["scalar_cells"][0]["stats"]["batch_dominated"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn wide_keys_and_bindings_spill_without_an_arity_limit() {
    let args = (0..20)
        .map(|i| format!("x{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let types = vec!["Expr"; 20].join(" ");
    let src = format!(
        "{BASE} (constructor Wide ({types}) Expr) (function Size (Expr) i64 :merge (min old new)) (ruleset work) (rule ((= e (Wide {args})) (= n (Size x0))) ((set (Size e) (+ n 1))) :ruleset work :name \"wide\")"
    );
    let mut r = Runtime::new_joint(&src).unwrap();
    let base = r.engine().clone();
    let mut native = base.clone();
    let actual = (0..20)
        .map(|i| format!("(V {i})"))
        .collect::<Vec<_>>()
        .join(" ");
    let commands = native
        .parse_program(None, "(run-schedule (saturate work))")
        .unwrap();
    for input in [
        format!("(Wide {actual}) (set (Size (V 0)) 10)"),
        "(set (Size (V 0)) 2) (union (V 0) (V 1))".into(),
    ] {
        native.parse_and_run_program(None, &input).unwrap();
        r.input(&input).unwrap();
        native.run_program(commands.clone()).unwrap();
        r.run(&commands).unwrap();
        compare::equal(&base, &native, r.engine()).unwrap();
        r.audit_mirror().unwrap();
    }
}

#[test]
fn new_public_functions_invalidate_the_closed_cost_universe() {
    let mut r=Runtime::new_joint(BASE).unwrap();
    let before=r.engine().cell_rule_environment_stamp();
    // Public Runtime input deliberately forbids schema changes. The engine's
    // declaration epoch also protects compiled observers after native handoff.
    assert!(r.input("(constructor Foreign (TermAndCost) TermAndCost)").is_err());
    let mut engine=r.engine().clone();
    engine.parse_and_run_program(None,"(constructor Foreign (TermAndCost) TermAndCost)").unwrap();
    assert_ne!(before,engine.cell_rule_environment_stamp());
}

#[test]
fn full_gpu_partition_preserves_certified_single_run_and_publishes_its_delta() {
    let Some(worker)=std::env::var_os("CELL_MATRIX_WORKER")else{return};
    let src=format!("{} (relation Export (Expr i64)) (ruleset outside) (rule ((Seen e n) (> n 0)) ((Export e n)) :ruleset outside)",source());
    let (mut base,hook)=Runtime::new_joint(&src).unwrap().detach_joint().unwrap();
    let commands=base.parse_program(None,"(run work 1) (run outside 1)").unwrap();
    let mut reference=base.clone();
    let mut cell=table_live::Runtime::new_full(base.clone(),&commands,Some(std::path::Path::new(&worker)),true,None).unwrap();
    cell.set_saturation_hook(Box::new(hook));
    cell.enable_experimental_cell_runs();
    for input in [
        "(Bop (Add) (V 0) (V 1)) (set (Size (V 0)) 2) (set (Size (V 1)) 3)",
        "", "(set (Size (V 1)) 1)", "", "(union (V 0) (V 1))", "",
    ] {
        reference.parse_and_run_program(None,input).unwrap();cell.input(input).unwrap();
        reference.run_program(commands.clone()).unwrap();cell.run(&commands).unwrap();
        compare::equal(&base,&reference,cell.engine()).unwrap();
    }
    let r=cell.report();
    assert_eq!(r["runtime"]["cell_run_calls"],6);
    assert_eq!(r["rules"].as_array().unwrap().iter().filter(|q|q["ruleset"]=="work").map(|q|q["stats"]["emitted_bindings"].as_u64().unwrap()).sum::<u64>(),0);
    assert!(r["rules"].as_array().unwrap().iter().any(|q|q["ruleset"]=="outside"&&q["stats"]["emitted_bindings"].as_u64().unwrap()>0));
}
