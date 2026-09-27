use egg_layout::reuse_graph::adaptive::{self, transitions::Spec};
use serde_json::Value;
use std::path::PathBuf;
fn dir(s: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cell-transitions-{s}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
const ENV: &str = "(datatype Math (V i64) (A Math) (B Math) (C Math) (D Math) (Flag Math) (Pair Math Math) (Done Math Math)) (rewrite (A x) (B (C (D x)))) (rewrite (Pair (B (C (D x))) (Flag y)) (Done x y))";
fn library() -> PathBuf {
    let out = dir("train");
    adaptive::run_with_min_reuse(
        &format!("{ENV} (A (V 99)) (Pair (B (C (D (V 99)))) (Flag (V 98))) (run 2)"),
        None,
        &out,
        "adaptive",
        1,
    )
    .unwrap();
    out.join("bake_lib_round1.json")
}
fn spec() -> Spec {
    Spec {
        source_rule: 0,
        consumer_rule: 1,
        cut_path: vec![0],
        anchor_path: vec![0, 0],
    }
}
fn run(work: &str) -> (Value, Value, PathBuf) {
    let lib = library();
    let source = format!("{ENV} {work}");
    let out = dir("run");
    let control =
        adaptive::run_with_min_reuse(&source, Some(&lib), &dir("control"), "frozen", 1).unwrap();
    let residual = adaptive::run_with_transitions(&source, &lib, &out, &[spec()], 1).unwrap();
    for k in ["enodes", "eclasses"] {
        assert_eq!(control["final"][k], residual["final"][k], "{residual}");
    }
    (control, residual, out)
}
#[test]
fn cell_emits_proven_interface_and_skips_internal_builder() {
    let (_, r, out) = run(
        "(A (V 0)) (run 2) (Pair (B (C (D (V 0)))) (Flag (V 1))) (run 2) (check (= (Pair (B (C (D (V 0)))) (Flag (V 1))) (Done (V 0) (V 1))))",
    );
    assert_eq!(
        r["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["cell_transition"]["fallback_matches"].as_u64().unwrap())
            .sum::<u64>(),
        0
    );
    let proofs: Value =
        serde_json::from_slice(&std::fs::read(out.join("cell-transitions.json")).unwrap()).unwrap();
    assert_eq!(proofs[0]["internal_atoms"], 3);
    assert_eq!(proofs[0]["original_lhs_atoms"], 5);
    assert_eq!(proofs[0]["residual_lhs_atoms"], 3);
}
#[test]
fn uncovered_facts_and_union_enabled_matches_are_preserved() {
    let (_, r, _) = run(
        "(B (C (D (V 0)))) (Pair (B (C (D (V 0)))) (Flag (V 1))) (run 2) (check (= (Pair (B (C (D (V 0)))) (Flag (V 1))) (Done (V 0) (V 1))))",
    );
    assert!(
        r["samples"][0]["cell_transition"]["fallback_matches"]
            .as_u64()
            .unwrap()
            > 0
    );
    // Union introduces a fresh C alternative under a previously covered B root.
    run(
        "(A (V 0)) (run 2) (Pair (B (C (D (V 0)))) (Flag (V 9))) (union (C (D (V 0))) (C (D (V 1)))) (run 2) (check (= (Pair (B (C (D (V 0)))) (Flag (V 9))) (Done (V 1) (V 9))))",
    );
}
#[test]
fn unsafe_nonfunctional_anchor_is_rejected() {
    let lib = library();
    let mut s = spec();
    s.anchor_path = vec![];
    let result = adaptive::run_with_transitions(ENV, &lib, &dir("reject"), &[s], 1);
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("unique flat anchor")
    );
    let mut s = spec();
    s.anchor_path = vec![0];
    assert!(
        adaptive::run_with_transitions(ENV, &lib, &dir("reject2"), &[s], 1)
            .unwrap_err()
            .to_string()
            .contains("unique flat anchor")
    );
}

#[test]
fn exported_program_is_valid_and_input_union_rekeys_metadata() {
    let (_, _, out) = run(
        "(A (V 0)) (run 2) (union (V 0) (V 1)) (Pair (B (C (D (V 1)))) (Flag (V 9))) (run 2) (check (= (Pair (B (C (D (V 1)))) (Flag (V 9))) (Done (V 1) (V 9))))",
    );
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        &std::fs::read_to_string(out.join("cell-transitions.egg")).unwrap(),
    )
    .unwrap();
}
#[test]
fn parallel_coverage_keeps_uncovered_inputs() {
    let lib = library();
    for threads in [1, 4] {
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| {
            let mut source=ENV.to_string();
            for i in 0..2400 {source+=&format!("(A (V {i}))");}
            source+="(run 2)";
            for i in 0..2450 {source+=&format!("(Pair (B (C (D (V {i})))) (Flag (V 9999)))");}
            source+="(run 2) (check (= (Pair (B (C (D (V 2449)))) (Flag (V 9999))) (Done (V 2449) (V 9999))))";
            let baseline=adaptive::run_with_min_reuse(&source,Some(&lib),&dir("parallel-control"),"frozen",1).unwrap();
            let residual=adaptive::run_with_transitions(&source,&lib,&dir("parallel-residual"),&[spec()],1).unwrap();
            for k in ["enodes","eclasses"] {assert_eq!(baseline["final"][k],residual["final"][k]);}
            let fallback=residual["samples"].as_array().unwrap().iter().map(|s|s["cell_transition"]["fallback_matches"].as_u64().unwrap()).sum::<u64>();
            assert!(fallback>=50,"uncovered tuples must run through native matching");
        });
    }
}

#[test]
fn graph_export_distinguishes_metadata_functions_from_real_constructors() {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None,"(datatype Math (adaptive_cell_view_user)) (adaptive_cell_view_user) (function adaptive_cell_view_0 () Math :merge old) (set (adaptive_cell_view_0) (adaptive_cell_view_user))").unwrap();
    let graph = egg_layout::reuse_graph::enclosure_audit::export_math(&eg);
    assert_eq!(graph.as_array().unwrap().len(), 1);
    assert_eq!(graph[0]["op"], "adaptive_cell_view_user");
}

#[test]
fn literal_anchor_preserves_guards_for_a_zero_argument_view() {
    let env = "(datatype Math (A Math) (B Math) (Const i64) (Flag Math) (Pair Math Math) (Done Math)) (rewrite (A x) (B (Const 0))) (rewrite (Pair (B (Const 0)) (Flag y)) (Done y))";
    let training = dir("literal-train");
    adaptive::run_with_min_reuse(
        &format!("{env} (A (Const 99)) (run 1)"),
        None,
        &training,
        "adaptive",
        1,
    )
    .unwrap();
    let library = training.join("bake_lib_round1.json");
    let work = format!(
        "{env} (A (Const 99)) (run 2) (Pair (B (Const 0)) (Flag (Const 1))) (Pair (B (Const 2)) (Flag (Const 3))) (run 2) (check (= (Pair (B (Const 0)) (Flag (Const 1))) (Done (Const 1))))"
    );
    let baseline =
        adaptive::run_with_min_reuse(&work, Some(&library), &dir("literal-control"), "frozen", 1)
            .unwrap();
    let r = adaptive::run_with_transitions(
        &work,
        &library,
        &dir("literal-residual"),
        &[Spec {
            source_rule: 0,
            consumer_rule: 1,
            cut_path: vec![0],
            anchor_path: vec![0],
        }],
        1,
    )
    .unwrap();
    for k in ["enodes", "eclasses"] {
        assert_eq!(baseline["final"][k], r["final"][k]);
    }
    assert_eq!(baseline["matches"], r["matches"]);
}
