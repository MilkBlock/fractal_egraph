use egg_layout::{native_analyze::run_with_observation, ripen_observation::Limits};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
fn run(name: &str, facts: usize) -> (PathBuf, Value) {
    let out = std::env::temp_dir().join(format!("growth-{name}-{}", std::process::id()));
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("experiments/reentry_forms/{name}.egg"));
    let r = run_with_observation(
        &source,
        &out,
        None,
        None,
        false,
        Limits {
            sweep_steps: 4,
            facts: Some(facts),
            milliseconds: None,
        },
    )
    .unwrap();
    (out, r)
}
#[test]
fn dabc_prefix_enters_bucket_without_self_reentry() {
    let (out, r) = run("dabc", 11);
    let q = &r["saturated_rule_composition"];
    assert_eq!(r["status"], "observed_unfinished");
    assert_eq!(q["counts"]["ObservedUnfinished"], 1);
    assert!(
        q["structural_reentry"]["0"]["new_bindings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        q["reentry_buckets"]["groups"][0]["new_node_operators"],
        serde_json::json!(["A", "B", "C", "Next"])
    );
    let evidence = &q["reentry_buckets"]["conditions"][0]["observations"][0];
    assert_eq!(evidence["admission"], "growth_observation");
    assert_eq!(evidence["starting_condition_resolved"], false);
    assert!(q["catalog"].is_null());
    let snapshot: Value = serde_json::from_slice(
        &fs::read(out.join("saturated-rule-composition/cells/CSUnit-000000/unfinished-state.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot["scope"]["observation_only"], true);
    let report: Value = serde_json::from_slice(
        &fs::read(out.join("saturated-rule-composition/cells/CSUnit-000000/ripen.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["ripen"]["state"], "ObservedUnfinished");
    assert_eq!(report["tables"]["D"], 1);
    assert!(report["tables"]["Next"].as_u64().unwrap() <= 2);
    assert_eq!(report["native_stop_reason"], "pending_fact_reservations");
    assert_eq!(report["sweep"]["max_steps"], 4);
    assert!(
        report["sweep"]["steps"].as_u64().unwrap() <= 2,
        "observation limit must stop within a four-step sweep"
    );
    fs::remove_dir_all(out).unwrap();
}
#[test]
fn branching_growth_is_kept_without_inventing_a_cycle() {
    let (out, r) = run("branching", 18);
    let q = &r["saturated_rule_composition"];
    assert_eq!(q["counts"]["ObservedUnfinished"], 1);
    assert!(
        q["structural_reentry"]["0"]["new_bindings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let report: Value = serde_json::from_slice(
        &fs::read(out.join("saturated-rule-composition/cells/CSUnit-000000/ripen.json")).unwrap(),
    )
    .unwrap();
    assert!(report["tables"]["Left"].as_u64().unwrap() <= 3);
    assert!(report["tables"]["Right"].as_u64().unwrap() <= 3);
    assert!(report["tables"]["B"].as_u64().unwrap() <= 6);
    assert_eq!(report["native_stop_reason"], "pending_fact_reservations");
    assert_eq!(
        q["reentry_buckets"]["groups"][0]["new_node_operators"],
        serde_json::json!(["A", "B", "C", "Left", "Right"])
    );
    fs::remove_dir_all(out).unwrap();
}
#[test]
fn paused_owner_releases_aliases_and_searches_other_bucket_conditions() {
    let root = std::env::temp_dir().join(format!("growth-alias-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments/reentry_forms/dabc.egg"),
    )
    .unwrap()
    .replace("(D (Z))", "(D (Z)) (D (Next (Z))) (A (Next (Next (Z))))");
    let source = root.join("source.egg");
    fs::write(&source, text).unwrap();
    let r = run_with_observation(
        &source,
        &root.join("run"),
        None,
        None,
        false,
        Limits {
            sweep_steps: 4,
            facts: Some(11),
            milliseconds: None,
        },
    )
    .unwrap();
    let q = &r["saturated_rule_composition"];
    assert!(
        q["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j.get("fully_equivalent_to").is_some())
    );
    assert!(
        q["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["state"] != "WaitingEquivalent")
    );
    assert!(
        q["bucket_trigger_matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["source"] != e["target"] && e["contains_known_C"] == true)
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn observation_limit_selection_uses_shared_defaults() {
    assert_eq!(Limits::default().milliseconds, Some(50));
    assert_eq!(Limits::default().facts, Some(200_000));
    assert_eq!(
        Limits {
            sweep_steps: 4,
            milliseconds: Some(5),
            facts: None
        }
        .reached(0.006, 1),
        vec!["time"]
    );
    assert_eq!(
        Limits {
            sweep_steps: 4,
            milliseconds: None,
            facts: Some(5)
        }
        .reached(0., 5),
        vec!["facts"]
    );
    assert!(
        Limits {
            sweep_steps: 4,
            milliseconds: Some(0),
            facts: None
        }
        .validate()
        .is_err()
    );
}

#[test]
fn explicit_time_observation_pauses_between_sweeps() {
    let out = std::env::temp_dir().join(format!("growth-time-{}", std::process::id()));
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments/reentry_forms/dabc.egg");
    let r = run_with_observation(
        &source,
        &out,
        None,
        None,
        false,
        Limits {
            sweep_steps: 4,
            facts: None,
            milliseconds: Some(1),
        },
    )
    .unwrap();
    let q = &r["saturated_rule_composition"];
    assert_eq!(q["counts"]["ObservedUnfinished"], 1);
    assert_eq!(
        q["jobs"][0]["observation"]["growth"]["reasons"],
        serde_json::json!(["time"])
    );
    fs::remove_dir_all(out).unwrap();
}
