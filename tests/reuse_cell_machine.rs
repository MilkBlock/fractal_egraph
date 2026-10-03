use egg_layout::reuse_graph::adaptive::{self, transitions::Spec};
use serde_json::Value;
use std::path::PathBuf;
fn dir(s: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cell-machine-{s}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
const ENV: &str = "(datatype Math (V i64) (A Math) (B Math) (C Math) (Flag Math) (Pair Math Math) (Done Math Math) (U Math) (Gate Math) (Finish Math Math) (Result Math Math Math)) (rewrite (A x) (B (C x))) (rewrite (Pair (B (C x)) (Flag y)) (U (Done x y))) (rewrite (Finish (U (Done x y)) (Gate z)) (Result x y z))";
fn train(env: &str, seed: &str) -> PathBuf {
    let out = dir("train");
    adaptive::run_with_min_reuse(&format!("{env} {seed} (run 2)"), None, &out, "adaptive", 1)
        .unwrap();
    out.join("bake_lib_round1.json")
}
fn count(r: &Value, sample: usize, cell: usize) -> u64 {
    r["samples"][sample]["cell_states"]["states"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["table"] == format!("adaptive_cell_state_{cell}"))
        .unwrap()["instances"]
        .as_u64()
        .unwrap()
}
#[test]
fn t_to_u_to_v_registers_states_and_chains_without_extra_math_rounds() {
    let lib = train(
        ENV,
        "(A (V 90)) (Pair (B (C (V 90))) (Flag (V 91))) (Finish (U (Done (V 90) (V 91))) (Gate (V 92)))",
    );
    let source = format!(
        "{ENV} (let a (A (V 0))) (run 1) (let mid (Pair a (Flag (V 1)))) (Finish mid (Gate (V 2))) (run 1) (run 1) (check (= (Finish mid (Gate (V 2))) (Result (V 0) (V 1) (V 2))))"
    );
    let plans = [
        Spec {
            source_rule: 0,
            consumer_rule: 1,
            cut_path: vec![0],
            anchor_path: vec![0],
        },
        Spec {
            source_rule: 1,
            consumer_rule: 2,
            cut_path: vec![0],
            anchor_path: vec![0],
        },
    ];
    let normal =
        adaptive::run_with_min_reuse(&source, Some(&lib), &dir("control"), "frozen", 1).unwrap();
    let r = adaptive::run_cell_machine(&source, &lib, &dir("machine"), &plans, 1).unwrap();
    assert_eq!(count(&r, 0, 0), 1);
    assert_eq!(count(&r, 0, 1), 0);
    assert_eq!(count(&r, 0, 2), 0);
    assert_eq!(count(&r, 1, 1), 1);
    assert!(r["dynamic_instances"]["state_revisions"].as_u64().unwrap() >= 2, "{r:#}");
    assert_eq!(r["dynamic_instances"]["unresolved_bindings"], 0);
    let automatic = adaptive::run_cell_machine(&source, &lib, &dir("auto-machine"), &[], 1).unwrap();
    assert!(automatic["dynamic_instances"]["state_revisions"].as_u64().unwrap() >= 2, "{automatic:#}");
    assert_eq!(automatic["final"]["enodes"], r["final"]["enodes"]);
    assert_eq!(automatic["final"]["eclasses"], r["final"]["eclasses"]);
    assert_eq!(count(&r, 1, 2), 0);
    assert_eq!(count(&r, 2, 2), 1);
    for k in ["enodes", "eclasses"] {
        assert_eq!(normal["final"][k], r["final"][k]);
    }
}
#[test]
fn saturated_cell_seeds_other_completed_cells_and_suppresses_replay() {
    let env = "(datatype Math (V i64) (A Math) (B Math) (C Math)) (rewrite (A x) (B x)) (rewrite (B x) (C x))";
    let lib = train(env, "(A (V 99)) (B (V 99))");
    let source = format!("{env} (A (V 0)) (run 3)");
    let r = adaptive::run_cell_machine(&source, &lib, &dir("seeded"), &[], 1).unwrap();
    assert_eq!(count(&r, 0, 0), 1);
    assert_eq!(count(&r, 0, 1), 1);
    assert!(
        r["samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["completed_prefixes"].as_u64().unwrap() > 0),
        "{r:#}"
    );
}
#[test]
fn completed_prefix_filter_does_not_reuse_stale_sibling_bindings() {
    for threads in [1, 4] {
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| {
   let mut eg=egglog::EGraph::default();
   let mut s="(relation P (i64)) (relation Q (i64)) (relation Link (i64 i64)) (relation Out (i64 i64)) (relation Known (i64 i64)) (rule ((P x) (Q y) (Link x y)) ((Out x y)) :name \"work\")".to_string();
   for x in 0..160 {s+=&format!("(P {x}) (Q {x})");for y in 0..160 {s+=&format!("(Link {x} {y})");if x%2==0 && y%2==0 {s+=&format!("(Out {x} {y}) (Known {x} {y})");}}}
   eg.parse_and_run_program(None,&s).unwrap();
   eg.step_rules_with_completed_cells("",vec![("work".into(),"Known".into(),vec!["x".into(),"y".into()])]).unwrap();
   assert_eq!(eg.get_size("Out"),160*160);
   assert!(eg.completed_cell_counts().0>0,"{:?}",eg.completed_cell_counts());
   eg.parse_and_run_program(None,"(P 999) (Q 999) (Link 999 999)").unwrap();eg.step_rules("").unwrap();assert_eq!(eg.get_size("Out"),160*160+1);
  });
    }
}

#[test]
fn staged_completion_is_not_reported_as_committed_and_is_step_scoped() {
    rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap().install(|| {
  let mut eg=egglog::EGraph::default();
  let mut s="(relation P (i64)) (relation Out (i64)) (relation KP (i64)) (relation KC (i64)) (rule ((P x)) ((Out x) (KP x) (KC x)) :name \"producer\") (rule ((P x)) ((Out x) (KC x)) :name \"consumer\")".to_string();
  for i in 0..4096 {s+=&format!("(P {i})");}
  eg.parse_and_run_program(None,&s).unwrap();
  eg.step_rules_with_cell_promises("",vec![("producer".into(),"KP".into(),vec!["x".into()]),("consumer".into(),"KC".into(),vec!["x".into()])]).unwrap();
  assert_eq!(eg.get_size("Out"),4096);
  assert!(eg.promised_cell_prefixes()>0);
  assert_eq!(eg.completed_cell_counts().0,0);
  let before=eg.promised_cell_prefixes();
  eg.parse_and_run_program(None,"(P 99999)").unwrap();eg.step_rules("").unwrap();
  assert_eq!(eg.get_size("Out"),4097);assert_eq!(eg.promised_cell_prefixes(),before);
 });
}

#[test]
fn symmetric_cells_queue_one_positive_recipe_per_binding_orbit() {
    use std::collections::BTreeMap;
    for threads in [1, 4] {
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| {
            let mut eg = egglog::EGraph::default();
            let mut program = "(relation P (i64 i64)) (relation Out (i64 i64)) (relation Known (i64 i64)) (rule ((P x y)) ((Out x y) (Out y x) (Known x y) (Known y x)) :name \"symmetric\")".to_string();
            for x in 0..80 { for y in 0..80 { program += &format!("(P {x} {y})"); } }
            eg.parse_and_run_program(None, &program).unwrap();
            let entries = vec![("symmetric".into(), "Known".into(), vec!["x".into(), "y".into()])];
            let bad = BTreeMap::from([("symmetric".into(), vec![vec![0, 0]])]);
            assert!(eg.step_rules_with_cell_machine("", entries.clone(), bad).is_err());
            eg.step_rules_with_cell_machine("", entries, BTreeMap::from([("symmetric".into(), vec![vec![0, 1], vec![1, 0]])])).unwrap();
            assert_eq!(eg.get_size("Out"), 6400);
            assert_eq!(eg.get_size("Known"), 6400);
            assert!(eg.queued_cell_prefixes() + eg.promised_cell_prefixes() > 0);
            eg.parse_and_run_program(None, "(P 100 101)").unwrap();
            eg.step_rules("").unwrap();
            assert_eq!(eg.get_size("Out"), 6402);
        });
    }
}

#[test]
fn union_rekeys_completion_and_enables_a_new_cell_transition() {
    let env = "(datatype Math (V i64) (A Math) (B Math) (C Math) (Pair Math Math) (Done Math)) (rewrite (A x) (B (C x))) (rewrite (Pair (B (C x)) (B (C x))) (Done x))";
    let lib = train(env, "(A (V 99)) (Pair (B (C (V 99))) (B (C (V 99))))");
    let source = format!(
        "{env} (let x (V 0)) (let y (V 1)) (let a (A x)) (let b (A y)) (run 1) (let p (Pair a b)) (run 1) (union x y) (run 2) (check (= p (Done x)))"
    );
    let ordinary =
        adaptive::run_with_min_reuse(&source, Some(&lib), &dir("union-control"), "frozen", 1)
            .unwrap();
    let result = adaptive::run_cell_machine(&source, &lib, &dir("union-machine"), &[], 1).unwrap();
    assert_eq!(count(&result, 0, 0), 2);
    assert_eq!(count(&result, 1, 1), 0);
    assert_eq!(count(&result, 3, 0), 1);
    assert_eq!(count(&result, 3, 1), 1);
    for key in ["enodes", "eclasses"] {
        assert_eq!(ordinary["final"][key], result["final"][key]);
    }
}

#[test]
fn a_failed_partial_match_cannot_claim_a_parameter_orbit() {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
        (relation P (i64 i64)) (relation Allow (i64 i64))
        (relation Out (i64 i64)) (relation Known (i64 i64))
        (rule ((P x y) (Allow x y))
              ((Out x y) (Out y x) (Known x y) (Known y x)) :name "guarded")
        (P 0 1) (P 1 0) (Allow 1 0)
    "#,
    )
    .unwrap();
    eg.step_rules_with_cell_machine(
        "",
        vec![(
            "guarded".into(),
            "Known".into(),
            vec!["x".into(), "y".into()],
        )],
        std::collections::BTreeMap::from([("guarded".into(), vec![vec![0, 1], vec![1, 0]])]),
    )
    .unwrap();
    eg.parse_and_run_program(None, "(check (Out 0 1) (Out 1 0))")
        .unwrap();
}

#[test]
fn an_unproved_union_cannot_become_a_completion_certificate() {
    let env = "(datatype Math (V i64) (A Math) (B Math)) (rewrite (A x) (B x))";
    let lib = train(env, "(A (V 99))");
    let mut data: Value = serde_json::from_slice(&std::fs::read(&lib).unwrap()).unwrap();
    let recipe = data["macros"]["0"]["actions"].as_str().unwrap().to_owned();
    data["macros"]["0"]["actions"] = format!("{recipe}\n(union adaptive_root x)").into();
    std::fs::write(&lib, serde_json::to_vec(&data).unwrap()).unwrap();
    let result = adaptive::run_cell_machine(
        &format!("{env} (A (V 0)) (run 1)"),
        &lib,
        &dir("invalid-union"),
        &[],
        1,
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("self-completion validation")
    );
}

#[test]
fn compact_permuted_state_survives_native_union_rebuild() {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
        (datatype E (V i64)) (relation P (E E))
        (relation Out (E E)) (relation Known (E E))
        (rule ((P x y)) ((Out x y) (Out y x) (Known x y)) :name "symmetric")
        (let a (V 1)) (let b (V 2)) (let c (V 3))
        (P a b) (Out a b) (Out b a) (Known b c) (union a c)
    "#,
    )
    .unwrap();
    eg.step_rules_with_cell_machine(
        "",
        vec![(
            "symmetric".into(),
            "Known".into(),
            vec!["x".into(), "y".into()],
        )],
        std::collections::BTreeMap::from([("symmetric".into(), vec![vec![0, 1], vec![1, 0]])]),
    )
    .unwrap();
    assert!(eg.completed_cell_counts().0 > 0);
    eg.parse_and_run_program(None, "(check (Out a b) (Out b a))")
        .unwrap();
}
