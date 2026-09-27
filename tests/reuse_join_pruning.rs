use egg_layout::reuse_graph::adaptive;
use std::path::PathBuf;
fn dir(s: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "join-prune-{s}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
#[test]
fn closed_root_prevents_native_matches_not_just_rhs() {
    let s = "(datatype Math (V i64) (A Math) (B Math) (C Math)) (rewrite (A x) (B x)) (rewrite (B x) (C x)) (A (V 1)) (run 4) (check (= (A (V 1)) (C (V 1))))";
    let bake = dir("bake");
    adaptive::run_with_min_reuse(s, None, &bake, "adaptive", 1).unwrap();
    let lib = bake.join("bake_lib_round1.json");
    let control =
        adaptive::run_with_min_reuse(s, Some(&lib), &dir("control"), "frozen", 1).unwrap();
    let skip = adaptive::run_with_join_pruning(s, Some(&lib), &dir("skip"), "frozen", 1).unwrap();
    assert!(skip["samples"].as_array().unwrap().iter().any(|r| {
        r["enclosure_audit"]["certified_rule_roots"]
            .as_u64()
            .unwrap()
            > 0
    }));
    assert!(
        skip["matches"].as_u64().unwrap() < control["matches"].as_u64().unwrap(),
        "{skip:#}"
    );
    for key in ["enodes", "eclasses"] {
        assert_eq!(skip["final"][key], control["final"][key]);
    }
}
#[test]
fn outside_alternative_and_union_still_match() {
    for s in [
        "(datatype Math (V i64) (A Math) (B Math) (C Math) (Out Math)) (rewrite (A x) (B x)) (rewrite (B (C x)) (Out x)) (A (V 0)) (run 2) (union (V 0) (C (V 1))) (run 3) (check (= (A (V 0)) (Out (V 1))))",
        "(datatype Math (V i64) (A Math) (B Math) (Done Math)) (rewrite (A x) (B x)) (rewrite (A (A x)) (Done x)) (A (V 0)) (run 2) (union (A (V 0)) (V 0)) (run 2) (check (= (Done (V 0)) (V 0)))",
    ] {
        let control =
            adaptive::run_with_min_reuse(s, None, &dir("boundary-control"), "adaptive", 1).unwrap();
        let skip =
            adaptive::run_with_join_pruning(s, None, &dir("boundary-skip"), "adaptive", 1).unwrap();
        for key in ["enodes", "eclasses"] {
            assert_eq!(skip["final"][key], control["final"][key]);
        }
    }
}

#[test]
fn native_root_pruning_handles_nested_same_operator_parallel_and_expiration() {
    for threads in [1, 4] {
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| {
            let mut eg=egglog::EGraph::default();
            let mut s="(datatype E (V i64) (A E) (C E)) (rule ((= root (A (A x)))) ((union root (C x))) :name \"nested\")".to_string();
            for i in 0..4000 {s+=&format!("(union (A (A (V {i}))) (C (V {i})))");}
            eg.parse_and_run_program(None,&s).unwrap();
            let mut roots=vec![];
            eg.function_for_each("A",|r| roots.push(*r.vals.last().unwrap())).unwrap();
            let mut control=eg.clone();
            let normal=control.step_rules("").unwrap();
            assert!(normal.num_matches_per_rule.values().sum::<usize>()>=4000);
            let pruned=eg.step_rules_excluding_proven_roots("",vec![("nested".into(),"A".into(),roots)]).unwrap();
            assert_eq!(pruned.num_matches_per_rule.values().sum::<usize>(),0);
            eg.parse_and_run_program(None,"(A (A (V 9999)))").unwrap();
            eg.step_rules("").unwrap();
            eg.parse_and_run_program(None,"(check (= (A (A (V 9999))) (C (V 9999))))").unwrap();
        });
    }
}
