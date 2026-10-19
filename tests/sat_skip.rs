use cell_assisted_egglog::table_live::Runtime as TableRuntime;
use egg_layout::{
    saturated_rule_composition::Comparison,
    whole_cell::{Library, Plan, automatic::AutomaticRuntime},
};
use std::path::PathBuf;
#[path = "../examples/support/saturation_hook.rs"]
mod saturation_hook;
const ENV: &str = r#"
(datatype T (Leaf i64) (P T) (Q T) (R T) (Seen T) (Extra T))
(ruleset s)
(rule ((P x)) ((Q x)) :ruleset s :name "pq")
(rule ((Q x)) ((R x)) :ruleset s :name "qr")
"#;
fn library() -> Library {
    let plan: Plan = serde_json::from_value(serde_json::json!({"ruleset":"s","stages":[
        {"name":"PQR","parent":null,"parameters":[{"name":"x","sort":"T"}],"graft":"(P x)"}]}))
    .unwrap();
    Library::bake(ENV, &plan).unwrap()
}
fn native_flags(out: Vec<egglog::CommandOutput>) -> (bool, bool) {
    let (mut updated, mut stop) = (false, true);
    for o in out {
        if let egglog::CommandOutput::RunSchedule(r) = o {
            updated |= r.updated;
            stop &= r.can_stop;
        }
    }
    (updated, stop)
}
#[test]
fn ordinary_runs_cannot_see_future_closure_and_stop_flags_match() {
    let mut r = AutomaticRuntime::new(library(), "(let x (Leaf 7))").unwrap();
    let mut native = egglog::EGraph::default();
    native.parse_and_run_program(None, ENV).unwrap();
    native
        .parse_and_run_program(None, "(let x (Leaf 7))")
        .unwrap();
    let observer = "(ruleset observe) (rule ((R x)) ((Seen x)) :ruleset observe)";
    r.install_grafts("observe", observer).unwrap();
    native.parse_and_run_program(None, observer).unwrap();
    r.commit("(P x)").unwrap();
    native.parse_and_run_program(None, "(P x)").unwrap();
    assert_eq!(r.counters.macro_transitions, 0);
    assert_eq!(r.engine().get_size("Q"), 0);
    for command in [
        "(run s 1)",
        "(run observe 1)",
        "(run-schedule (saturate s))",
        "(run observe 1)",
        "(run-schedule (saturate s))",
        "(run s 1)",
        "(run-schedule (repeat 4 (seq (saturate s) observe)))",
    ] {
        let expected = native_flags(native.parse_and_run_program(None, command).unwrap());
        let got = r.run(command).unwrap();
        assert_eq!(
            (got["updated"] == true, got["can_stop"] == true),
            expected,
            "{command}: {got}"
        );
        assert!(
            matches!(
                r.verify_against(&native).unwrap(),
                Comparison::Equivalent { .. }
            ),
            "{command}"
        );
        if command == "(run observe 1)" && r.counters.saturation_boundaries == 0 {
            assert_eq!(
                r.engine().get_size("Seen"),
                0,
                "future R must not leak through a queued graft"
            );
        }
    }
    assert!(r.counters.skipped_saturations > 0);
    assert_eq!(r.engine().get_size("Seen"), 1);
}

#[test]
fn shared_runtime_only_calls_hook_at_saturation_and_keeps_unknowns() {
    let worker = std::env::var_os("CELL_MATRIX_WORKER").map(PathBuf::from);
    for gpu in [false, true] {
        if gpu && worker.is_none() {
            continue;
        }
        let a = AutomaticRuntime::new(library(), "(let x (Leaf 7)) (let y (Leaf 8))").unwrap();
        let (mut engine, mut observer) = a.detach();
        observer.set_audit(true);
        let commands = engine
            .parse_program(None, "(run s 1) (run-schedule (saturate s))")
            .unwrap();
        let mut r = if gpu {
            TableRuntime::new(engine, &commands, worker.as_ref().unwrap(), true).unwrap()
        } else {
            TableRuntime::new_cpu(engine, &commands).unwrap()
        };
        r.set_saturation_hook(Box::new(saturation_hook::WholeCellHook(observer)));
        r.input("(P x)").unwrap();
        r.run(&commands[..1]).unwrap();
        assert_eq!(r.engine().get_size("R"), 0);
        assert_eq!(r.report()["runtime"]["skipped_saturations"], 0);
        r.run(&commands[1..]).unwrap();
        assert_eq!(r.engine().get_size("R"), 1);
        assert_eq!(
            r.report()["runtime"]["skipped_saturations"],
            1,
            "{}",
            r.report()
        );
        // Q(y) is an unknown entry (no P(y) and no certified PQR instance).
        r.input("(Q y)").unwrap();
        r.run(&commands[1..]).unwrap();
        assert_eq!(r.engine().get_size("R"), 2);
        assert!(
            r.report()["runtime"]["partial_saturations"]
                .as_u64()
                .unwrap()
                > 0
        );
        r.engine()
            .clone()
            .parse_and_run_program(None, "(check (R y))")
            .unwrap();
    }
}

#[test]
fn a_deleted_pending_graft_cannot_resurrect_a_future_closure() {
    let a = AutomaticRuntime::new(library(), "(let x (Leaf 7))").unwrap();
    let (mut engine, observer) = a.detach();
    let commands = engine
        .parse_program(None, "(run s 1) (run-schedule (saturate s))")
        .unwrap();
    let mut r = TableRuntime::new_cpu(engine, &commands).unwrap();
    r.set_saturation_hook(Box::new(saturation_hook::WholeCellHook(observer)));
    r.input("(P x)").unwrap();
    r.run(&commands[..1]).unwrap();
    // The observer sees the commit without publishing a closure. No macro may
    // infer Q/R from that queued but subsequently deleted P at the boundary.
    r.input("(delete (P x)) (delete (Q x))").unwrap();
    r.run(&commands[1..]).unwrap();
    assert_eq!(r.engine().get_size("P"), 0);
    assert_eq!(r.engine().get_size("Q"), 0);
    assert_eq!(r.engine().get_size("R"), 0);
    r.input("(P x)").unwrap();
    r.run(&commands[1..]).unwrap();
    assert_eq!(r.engine().get_size("R"), 1);
    // Deleting a row from an already certified body invalidates its ownership.
    r.input("(delete (R x))").unwrap();
    r.run(&commands[1..]).unwrap();
    // Native seminaive may not revisit unchanged Q after destructive deletion;
    // compare to that native behavior, never use a stale 'complete' certificate.
    assert_eq!(
        r.engine().get_size("R"),
        0,
        "completed cursors must prevent replay of old Q after deletion"
    );
}

#[test]
fn until_and_changed_ruleset_do_not_use_the_whole_closure_certificate() {
    let mut r = AutomaticRuntime::new(library(), "(let x (Leaf 7))").unwrap();
    r.commit("(P x)").unwrap();
    r.run("(run-schedule (saturate (run s :until (Q x))))")
        .unwrap();
    assert_eq!(r.engine().get_size("Q"), 1);
    assert_eq!(r.engine().get_size("R"), 0);
    assert_eq!(r.counters.macro_transitions, 0);
    let (mut eg, observer) = r.detach();
    eg.parse_and_run_program(
        None,
        "(rule ((R added_value)) ((Extra added_value)) :ruleset s :name \"new-rule\")",
    )
    .unwrap();
    let cmds = eg
        .parse_program(None, "(run-schedule (saturate s))")
        .unwrap();
    let mut r = TableRuntime::new_cpu(eg, &cmds).unwrap();
    r.set_saturation_hook(Box::new(saturation_hook::WholeCellHook(observer)));
    r.run(&cmds).unwrap();
    assert_eq!(r.engine().get_size("Extra"), 1);
    assert_eq!(r.report()["runtime"]["skipped_saturations"], 0);
}

#[test]
fn foreign_merge_semantics_are_not_certified_by_a_positive_only_library() {
    let a = AutomaticRuntime::new(library(), "(let x (Leaf 7))").unwrap();
    let (mut eg, observer) = a.detach();
    eg.parse_and_run_program(
        None,
        "(function Foreign (T) i64 :merge (+ old new)) (set (Foreign x) 1)",
    )
    .unwrap();
    let cmds = eg
        .parse_program(None, "(run-schedule (saturate s))")
        .unwrap();
    let mut r = TableRuntime::new_cpu(eg, &cmds).unwrap();
    r.set_saturation_hook(Box::new(saturation_hook::WholeCellHook(observer)));
    r.input("(P x)").unwrap();
    r.run(&cmds).unwrap();
    assert_eq!(r.engine().get_size("R"), 1);
    assert_eq!(r.report()["runtime"]["skipped_saturations"], 0);
}

#[test]
fn registering_an_already_complete_cell_does_not_report_a_public_update() {
    let mut r = AutomaticRuntime::new(library(), "(let x (Leaf 7))").unwrap();
    r.commit("(P x)").unwrap();
    r.run("(run s 2)").unwrap();
    assert_eq!(r.engine().get_size("R"), 1);
    let done = r.run("(run-schedule (saturate s))").unwrap();
    assert_eq!(done["updated"], false);
    assert_eq!(done["can_stop"], true);
    assert!(
        r.counters.macro_transitions > 0,
        "registration should actually run a no-op macro"
    );
}
