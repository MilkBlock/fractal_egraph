#[allow(dead_code)]
#[path = "../examples/support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::parameterized_cell::Runtime;
use egglog::CommandOutput;
const BASE: &str = include_str!("../experiments/parameterized_cell/terms.egg");
fn source(observer: bool) -> String {
    format!(
        r#"{BASE}
(constructor Copy (Expr) Expr) (constructor Inc (Expr) Expr)
(function Size (Expr) i64 :merge (min old new)) (ruleset work)
(rule ((= e (Bop op x y)) (= a (Size x)) (= b (Size y)) (= sum (+ a b))) ((set (Size e) (+ sum 1))) :ruleset work :name "plus")
(rule ((= e (Copy x)) (= a (Size x))) ((set (Size e) a)) :ruleset work :name "copy")
(rule ((= e (Inc x)) (= a (Size x))) ((set (Size e) (+ a 1))) :ruleset work :name "inc")
{}"#,
        if observer {
            r#"(relation Seen (Expr i64)) (rule ((= n (Size e))) ((Seen e n)) :ruleset work :name "observe")"#
        } else {
            ""
        }
    )
}
fn chain(n: usize, high: i64) -> String {
    let mut s = format!("(set (Size (V 1000)) 0) (set (Size (V 0)) {high})\n");
    for i in 1..=n {
        s += &format!(
            "(set (Size (V {i})) {high}) (union (V {i}) (Copy (V {})))\n",
            if i == 1 { 1000 } else { i - 1 }
        );
    }
    s += &format!(
        "(union (V 0) (Copy (V {n}))) (set (Size (V 2000)) {high}) (union (V 2000) (Bop (Add) (V 0) (V 0)))"
    );
    s
}
#[test]
fn a_closed_numeric_cell_collapses_a_long_recurrence() {
    let mut r = Runtime::new_joint(&source(false)).unwrap();
    r.input(&chain(24, 1000)).unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native
        .parse_program(None, "(run-schedule (saturate work))")
        .unwrap();
    let reports = native.run_program(commands.clone()).unwrap();
    r.run(&commands).unwrap();
    compare::equal(&base, &native, r.engine()).unwrap();
    r.audit_mirror().unwrap();
    let iterations: usize = reports
        .iter()
        .map(|r| {
            if let CommandOutput::RunSchedule(r) = r {
                r.iterations.len()
            } else {
                0
            }
        })
        .sum();
    let report = r.report();
    assert!(
        report["scalar_cells"][0]["closure"]["updates"]
            .as_u64()
            .unwrap()
            > 10,
        "{report}"
    );
    assert!(
        report["scalar_cells"][0]["stats"]["runs"].as_u64().unwrap() < iterations as u64,
        "{report}"
    );
    r.run(&commands).unwrap();
    compare::equal(&base, &native, r.engine()).unwrap();
}
#[test]
fn an_observer_of_intermediate_values_keeps_the_original_iterations() {
    let mut r = Runtime::new_joint(&source(true)).unwrap();
    r.input(&chain(8, 1000)).unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native
        .parse_program(None, "(run-schedule (saturate work))")
        .unwrap();
    native.run_program(commands.clone()).unwrap();
    r.run(&commands).unwrap();
    compare::equal(&base, &native, r.engine()).unwrap();
    assert_eq!(r.report()["scalar_cells"][0]["scope_allowed"], false);
    assert_eq!(r.report()["scalar_cells"][0]["closure"]["updates"], 0);
}
#[test]
fn input_aliases_preserve_partial_lhs_overflow() {
    let mut r = Runtime::new_joint(&source(false)).unwrap();
    r.input(&chain(8, 5_000_000_000_000_000_000)).unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native
        .parse_program(None, "(run-schedule (saturate work))")
        .unwrap();
    native.run_program(commands.clone()).unwrap();
    r.run(&commands).unwrap();
    compare::equal(&base, &native, r.engine()).unwrap();
    assert!(
        r.report()["scalar_cells"][0]["stats"]["lhs_filtered"]
            .as_u64()
            .unwrap()
            > 0,
        "{}",
        r.report()
    );
}
#[test]
fn a_future_rhs_overflow_is_not_hidden_by_the_fixed_point() {
    let mut r = Runtime::new_joint(&source(false)).unwrap();
    r.input(
        "(set (Size (V 0)) 9223372036854775806) (set (Size (V 1)) 0) (Inc (Bop (Add) (V 0) (V 1)))",
    )
    .unwrap();
    let mut native = r.engine().clone();
    let commands = native
        .parse_program(None, "(run-schedule (saturate work))")
        .unwrap();
    let a = native
        .run_program(commands.clone())
        .unwrap_err()
        .to_string();
    let b = r.run(&commands).unwrap_err().to_string();
    assert_eq!(a, b);
}

#[test]
fn helper_merge_callbacks_are_part_of_the_scope_interface() {
    let src = format!(
        "{}\n(function Pick (Expr) i64 :merge (+ old (Size (V 0)))) (relation Tick (Expr)) (ruleset helper) (rule ((Tick e)) ((set (Pick e) 1)) :ruleset helper :name \"pick\")",
        source(false)
    );
    let mut r = Runtime::new_joint(&src).unwrap();
    r.input(&format!("{} (Tick (V 0))", chain(6, 1000)))
        .unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native
        .parse_program(None, "(run-schedule (saturate (saturate helper) work))")
        .unwrap();
    native.run_program(commands.clone()).unwrap();
    r.run(&commands).unwrap();
    compare::equal(&base, &native, r.engine()).unwrap();
    assert_eq!(r.report()["scalar_cells"][0]["closure"]["updates"], 0);
}

#[test]
fn consumed_constant_producer_does_not_resurrect_a_deleted_output() {
    let src = format!(
        "{} (constructor Seed (i64) Expr) (rule ((= e (Seed i))) ((set (Size e) 0)) :ruleset work :name \"seed\")",
        source(false)
    );
    let mut r = Runtime::new_joint(&src).unwrap();
    r.input("(Seed 0)").unwrap();
    let once = r
        .engine()
        .clone()
        .parse_program(None, "(run work 1)")
        .unwrap();
    r.run(&once).unwrap();
    r.input(&format!("(delete (Size (Seed 0))) {}", chain(8, 1000)))
        .unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native
        .parse_program(None, "(run-schedule (saturate work))")
        .unwrap();
    native.run_program(commands.clone()).unwrap();
    r.run(&commands).unwrap();
    compare::equal(&base, &native, r.engine()).unwrap();
    r.engine()
        .clone()
        .parse_and_run_program(None, "(fail (check (Size (Seed 0))))")
        .unwrap();
}

#[test]
fn independent_helper_scope_uses_closure_in_the_shared_runtime() {
    use cell_assisted_egglog::table_live;
    let src = format!(
        "{} (relation In (Expr)) (relation Out (Expr)) (ruleset helper) (rule ((In e)) ((Out e)) :ruleset helper :name \"helper\")",
        source(false)
    );
    let (mut base, hook) = Runtime::new_joint(&src).unwrap().detach_joint().unwrap();
    let commands=base.parse_program(None,"(run-schedule (saturate (saturate helper) work)) (run terms 1) (run terms-helpers 1) (run terms-helpers-helpers 1)").unwrap();
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
    let input = format!("{} (In (V 0))", chain(24, 1000));
    native.parse_and_run_program(None, &input).unwrap();
    r.input(&input).unwrap();
    native.run_program(commands.clone()).unwrap();
    r.run(&commands).unwrap();
    compare::equal(&base, &native, r.engine()).unwrap();
    assert!(
        r.report()["saturation_diagnostics"]["scalar_cells"][0]["closure"]["updates"]
            .as_u64()
            .unwrap()
            > 10,
        "{}",
        r.report()
    );
}

#[test]
fn missing_ports_are_enabled_by_local_waves_without_requiring_a_full_component() {
    let mut r = Runtime::new_joint(&source(false)).unwrap();
    let mut input = String::from("(set (Size (V 0)) 0) (Copy (V 1000))");
    for i in 1..20 {
        input += &format!(" (union (V {i}) (Copy (V {})))", i - 1)
    }
    r.input(&input).unwrap();
    let mut native = r.engine().clone();
    let base = native.clone();
    let commands = native
        .parse_program(None, "(run-schedule (saturate work))")
        .unwrap();
    native.run_program(commands.clone()).unwrap();
    r.run(&commands).unwrap();
    compare::equal(&base, &native, r.engine()).unwrap();
    r.audit_mirror().unwrap();
    assert!(
        r.report()["scalar_cells"][0]["closure"]["updates"]
            .as_u64()
            .unwrap()
            > 10
    );
    r.engine()
        .clone()
        .parse_and_run_program(None, "(fail (check (Size (Copy (V 1000)))))")
        .unwrap();
}
