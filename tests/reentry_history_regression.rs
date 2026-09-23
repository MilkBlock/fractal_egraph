//! Compare the same bounded native work, not wall-clock-dependent prefixes.
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn scrub_timing(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.remove("active_seconds");
            for v in m.values_mut() {
                scrub_timing(v)
            }
        }
        Value::Array(a) => {
            for v in a {
                scrub_timing(v)
            }
        }
        _ => {}
    }
}
fn run_pair(name: &str) {
    let root = std::env::temp_dir().join(format!("reentry-history-{name}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("experiments/reentry_forms/{name}.egg"));
    let mut results = vec![];
    for history in [false, true] {
        let out = root.join(if history { "full" } else { "compact" });
        let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .args([
                "analyze",
                "--source",
                source.to_str().unwrap(),
                "--output",
                out.to_str().unwrap(),
                "--ripen-observe-ms",
                "60000",
                "--ripen-observe-facts",
                "80",
                "--ripen-sweep-steps",
                "4",
            ])
            .env("EGG_LAYOUT_GRAPHS", "0")
            .env("EGG_LAYOUT_RIPEN_HISTORY", if history { "1" } else { "0" })
            .output()
            .unwrap();
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let a = read(out.join("analysis.json"));
        let q = &a["saturated_rule_composition"];
        assert_eq!(q["counts"], json!({"ObservedUnfinished":1}));
        assert!(
            q["catalog"].is_null(),
            "unfinished growth must not become a saturated equivalence"
        );
        let cell = out.join("saturated-rule-composition/cells/CSUnit-000000");
        let r = read(cell.join("ripen.json"));
        assert_eq!(r["internal_history"], history);
        assert_eq!(r["tier1"].is_null(), !history);
        assert_eq!(r["native_stop_reason"], "pending_fact_reservations");
        assert_eq!(
            r["tables"]["D"], 1,
            "startup D must not be mistaken for the recurring body"
        );
        for op in ["A", "B", "C"] {
            assert!(
                r["tables"][op].as_u64().unwrap() >= 3,
                "{name}: insufficient repeated body: {}",
                r["tables"]
            );
        }
        if name == "branching" {
            for op in ["Left", "Right"] {
                assert!(r["tables"][op].as_u64().unwrap() >= 3);
            }
        } else {
            assert!(r["tables"]["Next"].as_u64().unwrap() >= 3);
        }
        let expected = if name == "branching" {
            json!(["A", "B", "C", "Left", "Right"])
        } else {
            json!(["A", "B", "C", "Next"])
        };
        assert_eq!(
            q["reentry_buckets"]["groups"][0]["new_node_operators"],
            expected
        );
        let observation = &q["reentry_buckets"]["conditions"][0]["observations"][0];
        assert_eq!(observation["admission"], "growth_observation");
        assert_eq!(
            observation["starting_condition_resolved"], false,
            "growth alone does not identify the loop trigger"
        );
        assert!(
            q["structural_reentry"]["0"]["new_bindings"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let mut snapshot = read(cell.join("unfinished-state.json"));
        assert_eq!(snapshot["scope"]["observation_only"], true);
        snapshot.as_object_mut().unwrap().remove("local_ids");
        let mut evidence = json!({"tables":r["tables"],"snapshot":snapshot,"buckets":q["reentry_buckets"],"weak_dominate":q["weak_dominate"],"structural_reentry":q["structural_reentry"],"bucket_trigger_matches":q["bucket_trigger_matches"]});
        scrub_timing(&mut evidence);
        results.push(evidence);
    }
    assert_eq!(
        results[0], results[1],
        "{name}: local diagnostic history changed inference"
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn dabcabc_growth_and_reentry_evidence_do_not_depend_on_local_history() {
    run_pair("dabc");
}
#[test]
fn branching_growth_and_reentry_evidence_do_not_depend_on_local_history() {
    run_pair("branching");
}
