use egg_layout::reuse_graph::adaptive;
use std::path::PathBuf;
fn dir(s: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rhs-{s}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
#[test]
fn clean_internal_rhs_is_skipped_and_result_is_preserved() {
    let s = "(datatype Math (V i64) (A Math) (B Math) (C Math)) (rewrite (A x) (B x)) (rewrite (B x) (C x)) (A (V 1)) (run 3) (check (= (A (V 1)) (C (V 1))))";
    let bake = dir("bake");
    adaptive::run_with_min_reuse(s, None, &bake, "adaptive", 1).unwrap();
    let lib = bake.join("bake_lib_round1.json");
    let control = adaptive::run_audit(s, Some(&lib), &dir("control"), "frozen", 1).unwrap();
    let skip = adaptive::run_audit_with_rhs_skip(s, Some(&lib), &dir("skip"), "frozen", 1).unwrap();
    assert!(
        skip["samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["enclosure_audit"]["rhs_skipped"].as_u64().unwrap() > 0)
    );
    for key in ["enodes", "eclasses"] {
        assert_eq!(skip["final"][key], control["final"][key]);
    }
}
#[test]
fn new_aliasing_is_not_suppressed() {
    let s = "(datatype Math (V i64) (A Math) (B Math) (Done Math)) (rewrite (A x) (B x)) (rewrite (A (A x)) (Done x)) (A (V 0)) (run 2) (union (A (V 0)) (V 0)) (run 2) (check (= (Done (V 0)) (V 0)))";
    let result = adaptive::run_audit_with_rhs_skip(s, None, &dir("alias"), "adaptive", 1).unwrap();
    assert!(
        result["samples"][2]["enclosure_audit"]["internally_effectful_bodies"]
            .as_u64()
            .unwrap()
            > 0
    );
}
#[test]
fn kernel_filter_keeps_work_and_skips_noops_across_batches() {
    for threads in [1, 4] {
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(||{
    let mut eg = egglog::EGraph::default();
    let mut s="(relation P (i64)) (relation Q (i64)) (rule ((P x)) ((P x)) :name \"noop\") (rule ((P x)) ((Q x)) :name \"work\")".to_string();
    for i in 0..2048 {
        s += &format!("(P {i})");
    }
    eg.parse_and_run_program(None, &s).unwrap();
    let trace = egglog::TraceSession::with_dependencies();
    trace.set_proven_redundant_rhs_filter(Some(std::sync::Arc::new(|rule, _| rule == "noop")));
    eg.step_rules_with_trace("", &trace).unwrap();
    let b = trace.drain_completed();
    assert_eq!(b.skipped_rhs.len(), 2048);
    assert_eq!(eg.get_size("Q"), 2048);
    assert_eq!(
        b.actions
            .iter()
            .filter(|a| a.outcome == egglog::RuleActionOutcome::SkippedRedundant)
            .count(),
        2048
    );
    assert!(trace.drain_completed().skipped_rhs.is_empty());
    trace.record_scope_reset();
    eg.parse_and_run_program(None,"(P 9999)").unwrap();
    eg.step_rules_with_trace("",&trace).unwrap();
    assert!(trace.drain_completed().skipped_rhs.is_empty());assert_eq!(eg.get_size("Q"),2049);
    });
    }
}

#[test]
fn native_union_counter_counts_merges_not_redundant_requests() {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None, "(datatype E (A) (B)) (A) (B)")
        .unwrap();
    let n = eg.native_union_count();
    eg.parse_and_run_program(None, "(union (A) (B))").unwrap();
    assert_eq!(eg.native_union_count(), n + 1);
    eg.parse_and_run_program(None, "(union (B) (A))").unwrap();
    assert_eq!(eg.native_union_count(), n + 1);
}
