use egg_layout::{
    saturated_rule_composition::Comparison,
    whole_cell::{Library, Plan, automatic::AutomaticRuntime},
};
const ENV: &str = include_str!("../experiments/whole_cell_chain/math.egg");
fn plan() -> Plan {
    serde_json::from_str(include_str!("../experiments/whole_cell_chain/plan.json")).unwrap()
}
fn setup(env: &str, p: Plan, input: &str) -> (AutomaticRuntime, egglog::EGraph) {
    let lib = Library::bake(env, &p).unwrap();
    let run = AutomaticRuntime::new(lib, input).unwrap();
    let mut native = egglog::EGraph::default();
    native.parse_and_run_program(None, env).unwrap();
    native.parse_and_run_program(None, input).unwrap();
    (run, native)
}
fn check(run: &mut AutomaticRuntime, native: &mut egglog::EGraph, code: &str, set: &str) {
    run.commit(code).unwrap();
    run.run(&format!("(run-schedule (saturate {set}))"))
        .unwrap();
    native
        .parse_and_run_program(None, &format!("{code}\n(run-schedule (saturate {set}))"))
        .unwrap();
    assert!(
        matches!(
            run.verify_against(native).unwrap(),
            Comparison::Equivalent { .. }
        ),
        "{}",
        run.report()
    );
}
#[test]
fn automatically_routes_multiple_interleaved_whole_cells_without_smooth_queries() {
    let input = "(let x (Var 1)) (let y (Var 2)) (let k (Var 3)) (let z (Var 4)) (let p (Var 11)) (let q (Var 12)) (let r (Var 13)) (let s (Var 14))";
    let (mut run, mut native) = setup(ENV, plan(), input);
    for code in [
        "(let a (Add x y))",
        "(let d (Add p q))",
        "(let b (Mul k a))",
        "(let e (Mul r d))",
        "(let c (Add b (Mul k z)))",
        "(let f (Add e (Mul r s)))",
    ] {
        check(&mut run, &mut native, code, "math");
        assert_eq!(run.counters.native_fallbacks, 0, "{}", run.report());
    }
    let c = run.library().describe()["steps"][2]["target"]
        .as_u64()
        .unwrap() as usize;
    assert!(
        run.instances()
            .iter()
            .filter(|i| i.valid && i.state == c)
            .count()
            >= 2
    );
    assert!(
        run.report()["native_smooth_matches"]
            .as_object()
            .unwrap()
            .values()
            .all(|n| n == 0)
    );
    assert!(
        run.transitions
            .iter()
            .any(|e| e["transition"] == "C" && e["source_state"] == 1)
    );
    let n = run.counters.macro_transitions;
    run.commit("(Add b (Mul k z))").unwrap();
    run.saturate().unwrap();
    assert_eq!(
        run.counters.macro_transitions, n,
        "deduplicated facts must not replay macros"
    );
}
#[test]
fn native_coarse_rule_commits_drive_the_same_dispatcher() {
    let env = format!("{ENV}\n(constructor Start (Math Math Math Math) Math)");
    let (mut run, mut native) = setup(
        &env,
        plan(),
        "(let x (Var 1)) (let y (Var 2)) (let k (Var 3)) (let z (Var 4)) (Start x y k z)",
    );
    let rules = r#"(ruleset graft)
(rule ((Start x y k z)) ((Add x y)) :ruleset graft :name "start")
(rule ((Start x y k z) (= a (Add x y))) ((Mul k a)) :ruleset graft :name "grow")
(rule ((Start x y k z) (= b (Mul k (Add x y)))) ((Add b (Mul k z))) :ruleset graft :name "grow-again")"#;
    run.install_grafts("graft", rules).unwrap();
    native.parse_and_run_program(None, rules).unwrap();
    for _ in 0..3 {
        run.step_grafts("graft").unwrap();
        run.saturate().unwrap();
        native
            .parse_and_run_program(None, "(run graft 1) (run-schedule (saturate math))")
            .unwrap();
        assert!(matches!(
            run.verify_against(&native).unwrap(),
            Comparison::Equivalent { .. }
        ));
    }
    assert_eq!(run.counters.native_fallbacks, 0, "{}", run.report());
    for s in ["A", "B", "C"] {
        assert!(run.transitions.iter().any(|e| e["transition"] == s));
    }
}
#[test]
fn unknown_grafts_and_aliases_fall_back_without_losing_facts() {
    let (mut run, mut native) = setup(
        ENV,
        plan(),
        "(let x (Var 1)) (let y (Var 2)) (let k (Var 3)) (let z (Var 4))",
    );
    check(
        &mut run,
        &mut native,
        "(let a (Add x y)) (let b (Mul k a))",
        "math",
    );
    check(&mut run, &mut native, "(union x y)", "math");
    assert!(run.counters.invalidated_instances > 0);
    assert!(run.counters.native_fallbacks > 0);
    check(&mut run, &mut native, "(Add b b)", "math");
}
#[test]
fn retained_source_can_take_multiple_successors_including_nonroot_grafts() {
    let mut p = plan();
    let mut d = p.stages[2].clone();
    d.name = "D".into();
    d.graft = "(Add (Mul k x) z)".into();
    p.stages.push(d);
    let (mut run, mut native) = setup(
        ENV,
        p,
        "(let x (Var 1)) (let y (Var 2)) (let k (Var 3)) (let z (Var 4))",
    );
    for code in [
        "(let a (Add x y))",
        "(let b (Mul k a))",
        "(Add b (Mul k z))",
        "(Add (Mul k x) z)",
    ] {
        check(&mut run, &mut native, code, "math");
    }
    assert!(run.transitions.iter().any(|e| e["transition"] == "C"));
    assert!(run.transitions.iter().any(|e| e["transition"] == "D"));
    assert!(run.instances().iter().any(|i| i.valid && i.state == 1));
}
#[test]
fn all_grafts_can_arrive_before_the_source_instance_exists() {
    let (mut run, mut native) = setup(
        ENV,
        plan(),
        include_str!("../experiments/whole_cell_chain/heldout.egg"),
    );
    check(
        &mut run,
        &mut native,
        "(Add (Mul k (Add x y)) (Mul k z))",
        "math",
    );
    assert_eq!(run.counters.native_fallbacks, 0, "{}", run.report());
    assert!(run.transitions.iter().any(|e| e["transition"] == "C"));
}
#[test]
fn two_closed_views_do_not_suppress_an_uncovered_cross_cell_match() {
    let env = r#"(datatype T (Leaf i64) (P T) (Q T) (R T)) (ruleset r)
    (rule ((P x) (Q x)) ((R x)) :ruleset r :name "cross")"#;
    let p: Plan = serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[
        {"name":"P","parent":null,"parameters":[{"name":"x","sort":"T"}],"graft":"(P x)"},
        {"name":"Q","parent":null,"parameters":[{"name":"x","sort":"T"}],"graft":"(Q x)"}]}))
    .unwrap();
    let (mut run, mut native) = setup(env, p, "(let x (Leaf 1))");
    check(&mut run, &mut native, "(P x) (Q x)", "r");
    assert!(run.counters.native_fallbacks > 0);
    run.engine()
        .clone()
        .parse_and_run_program(None, "(check (R x))")
        .unwrap();
}
#[test]
fn a_late_union_enables_a_match_across_previously_disjoint_cells() {
    let env = r#"(datatype T (Leaf i64) (P T) (Q T) (R T)) (ruleset r)
    (rule ((P x) (Q x)) ((R x)) :ruleset r :name "cross")"#;
    let p: Plan = serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[
        {"name":"P","parent":null,"parameters":[{"name":"x","sort":"T"}],"graft":"(P x)"},
        {"name":"Q","parent":null,"parameters":[{"name":"x","sort":"T"}],"graft":"(Q x)"}]}))
    .unwrap();
    let (mut run, mut native) = setup(env, p, "(let x (Leaf 1)) (let y (Leaf 2))");
    check(&mut run, &mut native, "(P x) (Q y)", "r");
    check(&mut run, &mut native, "(union x y)", "r");
    run.engine()
        .clone()
        .parse_and_run_program(None, "(check (R x))")
        .unwrap();
}
#[test]
fn disconnected_lhs_uses_native_fallback_even_when_both_components_have_cells() {
    let env = r#"(datatype T (Leaf i64) (P T) (Q T) (R T T)) (ruleset r)
    (rule ((P x) (Q y)) ((R x y)) :ruleset r :name "cross")"#;
    let p: Plan = serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[
        {"name":"P","parent":null,"parameters":[{"name":"x","sort":"T"}],"graft":"(P x)"},
        {"name":"Q","parent":null,"parameters":[{"name":"x","sort":"T"}],"graft":"(Q x)"}]}))
    .unwrap();
    let (mut run, mut native) = setup(env, p, "(let x (Leaf 1)) (let y (Leaf 2))");
    check(&mut run, &mut native, "(P x) (Q y)", "r");
    assert!(run.counters.native_fallbacks > 0);
}
#[test]
fn a_known_union_transition_publishes_its_quotiented_target() {
    let env = r#"(datatype T (Leaf i64) (Pair T T) (Link T T) (Wrap T)) (ruleset r)
      (rewrite (Pair a b) (Pair b a) :ruleset r :name "comm")
      (rule ((Link a b)) ((union a b)) :ruleset r :name "link")
      (rewrite (Wrap a) (Pair a a) :ruleset r :name "wrap")"#;
    let p:Plan=serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[
      {"name":"A","parent":null,"parameters":[{"name":"x","sort":"T"},{"name":"y","sort":"T"}],"graft":"(Pair x y)"},
      {"name":"B","parent":"A","parameters":[],"graft":"(Link x y)"},
      {"name":"C","parent":"B","parameters":[],"graft":"(Wrap parent)"}]})).unwrap();
    let (mut run, mut native) = setup(env, p, "(let x (Leaf 1)) (let y (Leaf 2))");
    for code in ["(Pair x y)", "(let b (Link x y))", "(Wrap b)"] {
        check(&mut run, &mut native, code, "r");
    }
    assert!(run.transitions.iter().any(|e| e["transition"] == "C"));
    assert_eq!(run.counters.native_fallbacks, 0, "{}", run.report());
}
#[test]
fn typed_frames_and_literal_trigger_arguments_are_preserved() {
    let env = "(datatype A (Leaf i64) (F A i64)) (datatype B (Box A)) (ruleset r) (rewrite (F x 7) x :ruleset r :name \"f\")";
    let p: Plan = serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[
      {"name":"A","parent":null,"parameters":[{"name":"x","sort":"A"}],"graft":"(F x 7)"},
      {"name":"B","parent":"A","parameters":[],"graft":"(Box parent)"}]}))
    .unwrap();
    let (mut run, mut native) = setup(env, p, "(let x (Leaf 2))");
    check(&mut run, &mut native, "(let a (F x 7)) (Box a)", "r");
    assert!(run.transitions.iter().any(|e| e["transition"] == "B"));
    assert_eq!(run.counters.native_fallbacks, 0);
}

#[test]
fn native_driver_acceptance_artifacts_have_exact_graphs_at_every_boundary() {
    let dir = std::env::temp_dir().join(format!("whole-cell-auto-check-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    let r = egg_layout::whole_cell::automatic::run_checked(
        include_str!("../experiments/whole_cell_automatic/math.egg"),
        &plan(),
        include_str!("../experiments/whole_cell_automatic/heldout.egg"),
        include_str!("../experiments/whole_cell_automatic/grafts.egg"),
        &dir,
    )
    .unwrap();
    assert_eq!(r["boundaries"].as_array().unwrap().len(), 3);
    assert_eq!(r["final_runtime"]["counters"]["native_fallbacks"], 0);
    assert_eq!(
        r["boundaries"][2]["enodes"], 46,
        "both concrete instances must reach C"
    );
    assert!(dir.join("2-native.json").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn union_invalidates_only_affected_instances_and_does_not_repeat_old_work() {
    let (mut run, mut native) = setup(
        ENV,
        plan(),
        "(let x (Var 1)) (let y (Var 2)) (let p (Var 3)) (let q (Var 4))",
    );
    check(&mut run, &mut native, "(Add x y)", "math");
    let first = run.instances().len();
    check(&mut run, &mut native, "(Add p q)", "math");
    let before = run.instances()[first..]
        .iter()
        .map(|i| (i.id, i.frame().to_vec(), i.revision))
        .collect::<Vec<_>>();
    check(&mut run, &mut native, "(union x y)", "math");
    for (id, frame, revision) in before {
        let i = &run.instances()[id];
        assert!(i.valid);
        assert_eq!(i.frame(), frame);
        assert_eq!(i.revision, revision);
    }
    let count = run.counters.macro_transitions;
    check(&mut run, &mut native, "(Add p q)", "math");
    assert_eq!(run.counters.macro_transitions, count);
}

#[test]
fn union_can_enable_a_known_graft_that_previously_had_no_source_cell() {
    let (mut run, mut native) = setup(
        ENV,
        plan(),
        "(let x (Var 1)) (let y (Var 2)) (let k (Var 3)) (let u (Var 4))",
    );
    check(&mut run, &mut native, "(let a (Add x y))", "math");
    check(&mut run, &mut native, "(Mul k u)", "math");
    assert!(!run.transitions.iter().any(|e| e["transition"] == "B"));
    let fallbacks = run.counters.native_fallbacks;
    check(&mut run, &mut native, "(union u a)", "math");
    assert!(run.transitions.iter().any(|e| e["transition"] == "B"));
    assert_eq!(
        run.counters.native_fallbacks, fallbacks,
        "the new B certificate should cover the changed component"
    );
}

#[test]
fn repeated_binding_constraints_cannot_be_satisfied_by_different_values() {
    let env = "(datatype T (Leaf i64) (F T T) (G T)) (ruleset r) (rewrite (F x x) (G x) :ruleset r :name \"diagonal\")";
    let p: Plan = serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[
        {"name":"A","parent":null,"parameters":[{"name":"x","sort":"T"}],"graft":"(F x x)"}]}))
    .unwrap();
    let (mut run, mut native) = setup(env, p, "(let x (Leaf 1)) (let y (Leaf 2))");
    check(&mut run, &mut native, "(F x y)", "r");
    assert!(run.instances().is_empty());
    check(&mut run, &mut native, "(F x x)", "r");
    assert!(!run.instances().is_empty());
}
