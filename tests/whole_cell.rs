use egg_layout::whole_cell::{Library, Plan, Runtime};
use std::collections::BTreeMap;
const ENV: &str = include_str!("../experiments/whole_cell_chain/math.egg");
fn plan() -> Plan {
    serde_json::from_str(include_str!("../experiments/whole_cell_chain/plan.json")).unwrap()
}
#[test]
fn complete_cells_advance_without_the_original_smooth_matcher() {
    let plan = plan();
    let lib = Library::bake(ENV, &plan).unwrap();
    let mut run = Runtime::new(
        lib,
        include_str!("../experiments/whole_cell_chain/heldout.egg"),
    )
    .unwrap();
    let mut ids = vec![];
    for s in &plan.stages {
        let e = run
            .advance(
                &s.name,
                &s.parameters
                    .iter()
                    .map(|p| (p.name.clone(), p.name.clone()))
                    .collect::<BTreeMap<_, _>>(),
            )
            .unwrap();
        ids.push(run.current().unwrap().state);
        assert!(
            e["smooth_query_watermarks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|w| w["before"] == w["after"])
        );
        assert!(run.smooth_counts().values().all(|n| *n == 0));
        let audit = run.audit_saturation().unwrap();
        assert!(
            audit["audit_original_matcher"]["smooth_matches"]
                .as_object()
                .unwrap()
                .values()
                .map(|n| n.as_u64().unwrap())
                .sum::<u64>()
                > 0
        );
    }
    assert_eq!(ids, vec![0, 1, 2]);
}

#[test]
fn the_same_bake_works_for_unseen_bindings_and_equals_independent_native_runs() {
    use egg_layout::saturated_rule_composition::Comparison;
    let p = plan();
    let lib = Library::bake(ENV, &p).unwrap();
    for inputs in [
        "(let x (Var 11)) (let y (Var 19)) (let k (Var 23)) (let z (Var 31))",
        "(let z (Var -8)) (let k (Var -6)) (let y (Var -4)) (let x (Var -2))",
    ] {
        let mut run = Runtime::new(lib.clone(), inputs).unwrap();
        let mut native = egglog::EGraph::default();
        native.parse_and_run_program(None, ENV).unwrap();
        native.parse_and_run_program(None, inputs).unwrap();
        let mut parent = String::new();
        for s in &p.stages {
            let expr = s.graft.replace("parent", &parent);
            parent = expr.clone();
            native
                .parse_and_run_program(
                    None,
                    &format!("(let ref_{} {expr}) (run-schedule (saturate math))", s.name),
                )
                .unwrap();
            run.advance(
                &s.name,
                &s.parameters
                    .iter()
                    .map(|p| (p.name.clone(), p.name.clone()))
                    .collect(),
            )
            .unwrap();
            let root = native
                .eval_expr(
                    &egglog::ast::Parser::default()
                        .get_expr_from_string(None, &format!("ref_{}", s.name))
                        .unwrap(),
                )
                .unwrap()
                .1;
            assert!(matches!(
                run.verify_against(&native, root).unwrap(),
                Comparison::Equivalent { .. }
            ));
        }
    }
}

#[test]
fn unbaked_aliases_wrong_source_and_active_input_context_are_not_silently_accepted() {
    let p = plan();
    let lib = Library::bake(ENV, &p).unwrap();
    assert!(Runtime::new(lib.clone(), "(let x (Add (Var 1) (Var 2)))").is_err());
    let mut run = Runtime::new(
        lib,
        "(let x (Var 1)) (let y (Var 2)) (let k x) (let z (Var 4))",
    )
    .unwrap();
    run.advance(
        "A",
        &BTreeMap::from([("x".into(), "x".into()), ("y".into(), "y".into())]),
    )
    .unwrap();
    let before = serde_json::to_value(run.full_snapshot().unwrap()).unwrap();
    assert!(
        run.advance("C", &BTreeMap::from([("z".into(), "z".into())]))
            .is_err()
    );
    assert!(
        run.advance("B", &BTreeMap::from([("k".into(), "k".into())]))
            .unwrap_err()
            .to_string()
            .contains("alias")
    );
    assert_eq!(
        serde_json::to_value(run.full_snapshot().unwrap()).unwrap(),
        before
    );
    assert_eq!(run.current().unwrap().state, 0);
}

#[test]
fn union_can_merge_old_parent_slots_and_rebuild_old_rows() {
    let env = r#"(datatype T (Leaf i64) (Pair T T) (Link T T) (Wrap T)) (ruleset r)
      (rewrite (Pair a b) (Pair b a) :ruleset r :name "comm")
      (rule ((Link a b)) ((union a b)) :ruleset r :name "link")
      (rewrite (Wrap a) (Pair a a) :ruleset r :name "wrap")"#;
    let p:Plan=serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[
      {"name":"A","parent":null,"parameters":[{"name":"x","sort":"T"},{"name":"y","sort":"T"}],"graft":"(Pair x y)"},
      {"name":"B","parent":"A","parameters":[],"graft":"(Link x y)"},
      {"name":"C","parent":"B","parameters":[],"graft":"(Wrap parent)"}]})).unwrap();
    let lib = Library::bake(env, &p).unwrap();
    let desc = lib.describe();
    assert!(desc["steps"][1]["union_actions"].as_u64().unwrap() > 0);
    let input = "(let x (Leaf 100)) (let y (Leaf 200))";
    let mut run = Runtime::new(lib, input).unwrap();
    let mut native = egglog::EGraph::default();
    native.parse_and_run_program(None, env).unwrap();
    native.parse_and_run_program(None, input).unwrap();
    let mut previous = String::new();
    for s in &p.stages {
        let term = s.graft.replace("parent", &previous);
        previous = term.clone();
        native
            .parse_and_run_program(
                None,
                &format!("(let ref_{} {term}) (run-schedule (saturate r))", s.name),
            )
            .unwrap();
        run.advance(
            &s.name,
            &s.parameters
                .iter()
                .map(|p| (p.name.clone(), p.name.clone()))
                .collect(),
        )
        .unwrap();
        let root = native
            .eval_expr(
                &egglog::ast::Parser::default()
                    .get_expr_from_string(None, &format!("ref_{}", s.name))
                    .unwrap(),
            )
            .unwrap()
            .1;
        assert!(matches!(
            run.verify_against(&native, root).unwrap(),
            egg_layout::saturated_rule_composition::Comparison::Equivalent { .. }
        ));
    }
    run.engine()
        .clone()
        .parse_and_run_program(None, "(check (= x y))")
        .unwrap();
}

#[test]
fn a_cyclic_body_is_grounded_and_equivalent_successors_share_the_whole_state() {
    let env =
        "(datatype E (Leaf i64) (F E)) (ruleset r) (rewrite (F x) x :ruleset r :name \"erase\")";
    let p: Plan = serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[
       {"name":"A","parent":null,"parameters":[{"name":"x","sort":"E"}],"graft":"(F x)"},
       {"name":"B","parent":"A","parameters":[],"graft":"(F parent)"}]}))
    .unwrap();
    let lib = Library::bake(env, &p).unwrap();
    let d = lib.describe();
    assert_eq!(d["states"].as_array().unwrap().len(), 1);
    assert_eq!(d["steps"][1]["emitted_rows"], 0);
    let mut run = Runtime::new(lib, "(let x (Leaf 9))").unwrap();
    run.advance("A", &BTreeMap::from([("x".into(), "x".into())]))
        .unwrap();
    run.advance("B", &BTreeMap::new()).unwrap();
    assert_eq!(run.current().unwrap().state, 0);
    run.audit_saturation().unwrap();
}

#[test]
fn effectful_or_finite_run_environments_are_rejected_before_execution() {
    let p = plan();
    for source in [
        format!("{ENV} (run math 1)"),
        format!("{ENV} (function Cost (Math) i64 :merge (+ old new))"),
        format!("{ENV} (rule ((Var n) (> n 0)) ((Var (- n 1))) :ruleset math)"),
    ] {
        assert!(Library::bake(&source, &p).is_err());
    }
}

#[test]
fn a_nonroot_parent_value_can_be_used_by_the_next_graft() {
    let mut p = plan();
    p.stages[2].graft = "(Add (Mul k x) z)".into();
    let lib = Library::bake(ENV, &p).unwrap();
    let mut run = Runtime::new(
        lib,
        include_str!("../experiments/whole_cell_chain/heldout.egg"),
    )
    .unwrap();
    let mut native = egglog::EGraph::default();
    native.parse_and_run_program(None, ENV).unwrap();
    native
        .parse_and_run_program(
            None,
            include_str!("../experiments/whole_cell_chain/heldout.egg"),
        )
        .unwrap();
    let mut prior = String::new();
    for s in &p.stages {
        let term = s.graft.replace("parent", &prior);
        prior = term.clone();
        native
            .parse_and_run_program(
                None,
                &format!("(let ref_{} {term}) (run-schedule (saturate math))", s.name),
            )
            .unwrap();
        run.advance(
            &s.name,
            &s.parameters
                .iter()
                .map(|p| (p.name.clone(), p.name.clone()))
                .collect(),
        )
        .unwrap();
        let root = native
            .eval_expr(
                &egglog::ast::Parser::default()
                    .get_expr_from_string(None, &format!("ref_{}", s.name))
                    .unwrap(),
            )
            .unwrap()
            .1;
        assert!(matches!(
            run.verify_against(&native, root).unwrap(),
            egg_layout::saturated_rule_composition::Comparison::Equivalent { .. }
        ));
    }
}

#[test]
fn full_graph_check_rejects_extra_facts_even_if_the_extra_graph_is_saturated() {
    let lib = Library::bake(ENV, &plan()).unwrap();
    let mut run = Runtime::new(
        lib,
        include_str!("../experiments/whole_cell_chain/heldout.egg"),
    )
    .unwrap();
    run.advance(
        "A",
        &BTreeMap::from([("x".into(), "x".into()), ("y".into(), "y".into())]),
    )
    .unwrap();
    let mut wrong = run.engine().clone();
    wrong
        .parse_and_run_program(None, "(Mul x y) (run-schedule (saturate math))")
        .unwrap();
    assert!(matches!(
        run.verify_against(&wrong, run.root().unwrap()).unwrap(),
        egg_layout::saturated_rule_composition::Comparison::Different { .. }
    ));
}

#[test]
fn unexpected_alias_created_by_hashcons_does_not_publish_a_target_state() {
    let env = "(datatype E (Leaf i64) (Wrap E) (Pair E E)) (ruleset r) (rewrite (Wrap x) (Pair x (Leaf 0)) :ruleset r)";
    let p:Plan=serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[{"name":"A","parent":null,"parameters":[{"name":"x","sort":"E"}],"graft":"(Wrap x)"}]})).unwrap();
    let lib = Library::bake(env, &p).unwrap();
    let mut run = Runtime::new(lib, "(let x (Leaf 0))").unwrap();
    let binding = BTreeMap::from([("x".into(), "x".into())]);
    assert!(
        run.advance("A", &binding)
            .unwrap_err()
            .to_string()
            .contains("target alias")
    );
    assert!(run.current().is_none());
    assert!(
        run.advance("A", &binding)
            .unwrap_err()
            .to_string()
            .contains("cannot resume")
    );
}

#[test]
fn whole_frames_and_union_recipes_are_typed_across_multiple_datatypes() {
    let env = "(datatype K (Key i64)) (datatype E (Leaf i64) (Box K E) (Wrap E)) (ruleset r) (rewrite (Wrap (Box k x)) (Box k (Wrap x)) :ruleset r)";
    let p:Plan=serde_json::from_value(serde_json::json!({"ruleset":"r","stages":[
      {"name":"A","parent":null,"parameters":[{"name":"k","sort":"K"},{"name":"x","sort":"E"}],"graft":"(Box k x)"},
      {"name":"B","parent":"A","parameters":[],"graft":"(Wrap parent)"},
      {"name":"C","parent":"B","parameters":[],"graft":"(Wrap parent)"}]})).unwrap();
    let lib = Library::bake(env, &p).unwrap();
    let input = "(let k (Key 12)) (let x (Leaf 34))";
    let mut run = Runtime::new(lib, input).unwrap();
    let mut native = egglog::EGraph::default();
    native.parse_and_run_program(None, env).unwrap();
    native.parse_and_run_program(None, input).unwrap();
    let mut previous = String::new();
    for s in &p.stages {
        let term = s.graft.replace("parent", &previous);
        previous = term.clone();
        native
            .parse_and_run_program(
                None,
                &format!("(let root_{} {term}) (run-schedule (saturate r))", s.name),
            )
            .unwrap();
        run.advance(
            &s.name,
            &s.parameters
                .iter()
                .map(|p| (p.name.clone(), p.name.clone()))
                .collect(),
        )
        .unwrap();
        let root = native
            .eval_expr(
                &egglog::ast::Parser::default()
                    .get_expr_from_string(None, &format!("root_{}", s.name))
                    .unwrap(),
            )
            .unwrap()
            .1;
        assert!(matches!(
            run.verify_against(&native, root).unwrap(),
            egg_layout::saturated_rule_composition::Comparison::Equivalent { .. }
        ));
    }
}

#[test]
fn projection_catalog_uses_the_verified_bodies_and_deduplicates_repeated_training() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut p = plan();
    p.stages.truncate(1);
    let mut repeat = p.stages[0].clone();
    repeat.name = "again".into();
    p.stages.push(repeat);
    let lib = Library::bake(ENV, &p).unwrap();
    assert_eq!(lib.describe()["states"].as_array().unwrap().len(), 1);
    let dir = std::env::temp_dir().join(format!(
        "verified-projection-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    lib.export_projection_catalog(&dir).unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("catalog/states/state-0000.json")).unwrap())
            .unwrap();
    assert_eq!(saved, serde_json::to_value(lib.body(0)).unwrap());
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("catalog/catalog.json")).unwrap()).unwrap();
    assert_eq!(catalog["triggers"].as_array().unwrap().len(), 2);
    assert!(
        catalog["triggers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["binding_origin"]["bake"]["delta_replay_verified"] == true)
    );
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None, ENV).unwrap();
    let loaded = cell_assisted_egglog::cell_prefix::Library::load(&mut eg, &dir).unwrap();
    assert_eq!(loaded.evidence["loaded_states"], serde_json::json!([0]));
    assert!(lib.export_projection_catalog(&dir).is_err());
    assert_eq!(
        std::fs::read_dir(dir.join("catalog/states"))
            .unwrap()
            .count(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}
