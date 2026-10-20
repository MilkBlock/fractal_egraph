use egg_layout::semantic_audit::{compare_continuation, risks};

fn states() -> (String, String) {
    (
        include_str!("../experiments/structural_keys/observability/left.egg").into(),
        include_str!("../experiments/structural_keys/observability/right.egg").into(),
    )
}

#[test]
fn same_winner_can_be_distinguished_by_a_real_later_consumer() {
    let (a, b) = states();
    let r = compare_continuation(
        &a,
        &b,
        include_str!("../experiments/structural_keys/observability/continuation.egg"),
    )
    .unwrap();
    assert_eq!(r["observations"][0]["left"], true);
    assert_eq!(r["observations"][0]["right"], true);
    assert_eq!(r["status"], "counterexample");
    assert_eq!(r["counterexample"]["left"], true);
    assert_eq!(r["counterexample"]["right"], false);
    assert_eq!(r["observations"][2]["right"], true); // continues after false probes
    assert_eq!(r["cell_replacement"], "rejected_for_this_continuation");
}

#[test]
fn agreement_is_scoped_to_the_probed_continuation_not_a_reuse_certificate() {
    let (a, b) = states();
    let r = compare_continuation(&a, &b, "(check (= result a))").unwrap();
    assert_eq!(r["status"], "no_counterexample_in_checked_continuation");
    assert_eq!(r["cell_replacement"], "not_certified");
    assert_eq!(r["all_continuations_proved"], false);
}

#[test]
fn missing_or_invalid_queries_are_not_successes() {
    let (a, b) = states();
    for future in ["", "(check (Undefined 1))"] {
        let r = compare_continuation(&a, &b, future).unwrap();
        assert_eq!(r["status"], "unknown");
    }
    assert!(compare_continuation(&a, &b, "(print-size)").is_err());
}

#[test]
fn checks_on_equalities_and_function_values_distinguish_states() {
    let env = "(datatype E (V i64)) (function cost (E) i64 :merge (min old new)) (V 1) (V 2)";
    let a = format!("{env} (union (V 1) (V 2)) (set (cost (V 1)) 3)");
    let b = format!("{env} (set (cost (V 1)) 4)");
    let r =
        compare_continuation(&a, &b, "(check (= (V 1) (V 2))) (check (= (cost (V 1)) 3))").unwrap();
    assert!(
        r["observations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|q| q["distinguishes"] == true)
    );
}

#[test]
fn visibility_is_tested_by_matching_not_only_by_checking_hidden_facts() {
    let env = "(datatype E (V i64)) (relation Seen (i64)) (ruleset see) (rule ((V x)) ((Seen x)) :ruleset see) (V 1)";
    let r = compare_continuation(
        env,
        &format!("{env} (subsume (V 1))"),
        "(run see 1) (check (Seen 1))",
    )
    .unwrap();
    assert_eq!(r["status"], "counterexample");
}

#[test]
fn static_audit_keeps_rhs_reads_merge_callbacks_and_union_propagation() {
    let source = r#"
      (datatype E (V i64) (Pair E E) (Smaller E E))
      (relation Seed (E))
      (relation Output (E))
      (function F (E) E :merge (Smaller old new))
      (function G (E) E :merge new)
      (ruleset r)
      (rule ((Seed x)) ((set (G x) (F x))) :ruleset r :name "rhs-read")
      (rule ((= s (Smaller a b))) ((union s a)) :ruleset r :name "resolve")
      (rule ((Pair x x)) ((Output x)) :ruleset r :name "alias-reader")
    "#;
    let r = risks(source, &["F"]).unwrap();
    let consumers = r["consumers"].as_array().unwrap();
    for label in ["rhs-read", "merge:F", "resolve", "alias-reader"] {
        assert!(
            consumers
                .iter()
                .any(|x| x["consumer"].as_str().unwrap().contains(label)),
            "{label}: {r}"
        );
    }
    assert_eq!(r["status"], "potentially_observable");
    assert_eq!(r["cell_replacement"], "not_certified");
}

#[test]
fn real_eggcc_snapshot_reports_smaller_readers_without_running_inference() {
    let r = risks(
        include_str!("../experiments/structural_keys/add-pass3.egg"),
        &["Smaller", "IVTMin", "to-hoist", "ExtractedExpr"],
    )
    .unwrap();
    assert!(r["consumers"].as_array().unwrap().iter().any(|x| {
        x["ruleset"] == "terms-helpers-helpers"
            && x["direct_reads"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x == "Smaller")
            && x["effects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x == "union_rebuild")
    }));
    assert_eq!(r["final_output_equivalence"], "not_checked_by_this_run");
}
