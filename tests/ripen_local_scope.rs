use egg_layout::{
    closure_contract::{Change, LocalDependencies, Registry},
    saturated_rule_composition::{Comparison, SaturatedRuleComposition, compare},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

fn run(name: &str, source: &str) -> (PathBuf, Value, Value) {
    let dir = std::env::temp_dir().join(format!("ripen-scope-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("source.egg");
    fs::write(&input, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["ripen"])
        .arg(&input)
        .arg(dir.join("run"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value =
        serde_json::from_slice(&fs::read(dir.join("run/ripen.json")).unwrap()).unwrap();
    assert_eq!(report["ripen"]["state"], "Saturated");
    let state = if dir.join("run/saturated-rule-composition.json").exists() {
        "saturated-rule-composition.json"
    } else {
        "observed-saturated-state.json"
    };
    let graph = serde_json::from_slice(&fs::read(dir.join("run").join(state)).unwrap()).unwrap();
    (dir, report, graph)
}

#[test]
fn unused_containers_and_ordered_merges_do_not_poison_a_pure_cell() {
    let base = "(datatype E (A) (B)) (rewrite (A) (B)) (A)";
    let extra = "(datatype E (A) (B)) (sort ES (Set E)) (constructor Box (ES) E) (function Unused (E) i64 :merge new) (rewrite (A) (B)) (A)";
    let (a, _, x) = run("base", base);
    let (b, r, y) = run("unrelated", extra);
    assert_eq!(r["saturated_rule_composition"]["status"], "exported");
    assert_eq!(
        y["scope"]["dependency_contract"]["dependencies"]["tables"],
        json!(["A", "B"])
    );
    let x: SaturatedRuleComposition = serde_json::from_value(x).unwrap();
    let y: SaturatedRuleComposition = serde_json::from_value(y).unwrap();
    assert!(matches!(
        compare(&x, &y).unwrap(),
        Comparison::Equivalent { .. }
    ));
    fs::remove_dir_all(a).unwrap();
    fs::remove_dir_all(b).unwrap();
}

#[test]
fn absent_premises_are_watched_and_false_numeric_guards_are_not_proofs() {
    let prefix =
        "(datatype E (A) (B)) (relation Gate (E)) (rule ((= a (A)) (Gate a) (< 1 0)) ((B))) (A)";
    let (a, r, x) = run("absent", prefix);
    assert_eq!(r["saturated_rule_composition"]["status"], "exported");
    let contract = &x["scope"]["dependency_contract"];
    assert_eq!(contract["dependencies"]["empty_frontier"], json!(["Gate"]));
    let deps: LocalDependencies = serde_json::from_value(contract["dependencies"].clone()).unwrap();
    let mut leases = Registry::default();
    leases.watch_local(0, &deps, [10, 11]);
    leases.apply(&Change {
        tables: vec!["Unrelated".into()],
        ..Default::default()
    });
    assert!(leases.valid(0));
    leases.apply(&Change {
        tables: vec!["Gate".into()],
        ..Default::default()
    });
    assert!(!leases.valid(0));
    let (b, r, _) = run("gate-present", &format!("{prefix} (Gate (A))"));
    assert_eq!(r["saturated_rule_composition"]["status"], "unavailable");
    assert!(
        r["saturated_rule_composition"]["reason"]
            .as_str()
            .unwrap()
            .contains("primitive guards")
    );
    fs::remove_dir_all(a).unwrap();
    fs::remove_dir_all(b).unwrap();
}

#[test]
fn future_rules_and_merge_callback_reads_are_in_the_dependency_closure() {
    let (a, r, x) = run(
        "future",
        "(datatype E (A) (B)) (sort ES (Set E)) (constructor Box (ES) E) (rule ((A)) ((B))) (rule ((B)) ((Box (set-of (A))))) (A)",
    );
    assert_eq!(r["saturated_rule_composition"]["status"], "unavailable");
    let tables = x["scope"]["dependency_contract"]["dependencies"]["tables"]
        .as_array()
        .unwrap();
    assert!(tables.contains(&json!("Box")));
    let initial: Value =
        serde_json::from_slice(&fs::read(a.join("run/trigger-state.json")).unwrap()).unwrap();
    assert!(
        initial["scope"]["dependency_contract"]["dependencies"]["tables"]
            .as_array()
            .unwrap()
            .contains(&json!("Box"))
    );
    assert!(
        !initial["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["op"] == "Box")
    );
    let (b, r, x) = run(
        "merge",
        "(datatype E (A)) (function Aux (i64) i64 :merge (min old new)) (function Score (E) i64 :merge (+ (Aux old) new)) (rule ((= a (A))) ((set (Score a) 0))) (A)",
    );
    assert_eq!(r["saturated_rule_composition"]["status"], "unavailable");
    assert!(
        r["saturated_rule_composition"]["reason"]
            .as_str()
            .unwrap()
            .contains("Score")
    );
    assert!(
        x["scope"]["dependency_contract"]["dependencies"]["tables"]
            .as_array()
            .unwrap()
            .contains(&json!("Aux"))
    );
    fs::remove_dir_all(a).unwrap();
    fs::remove_dir_all(b).unwrap();
}

#[test]
fn interior_alias_updates_visibility_and_schedule_invalidate_instance_leases() {
    let deps = LocalDependencies {
        tables: ["Used".into()].into(),
        empty_frontier: ["Gate".into()].into(),
        rulesets: ["s".into()].into(),
    };
    let mut leases = Registry::default();
    // 10 is an input; 11 is an internal, non-root identity.
    leases.watch_local(0, &deps, [10, 11]);
    leases.apply(&Change {
        unions: vec![(11, 20)],
        ..Default::default()
    });
    assert!(!leases.valid(0));
    leases.watch_local(1, &deps, [10, 20]);
    leases.apply(&Change {
        unions: vec![(11, 30)],
        ..Default::default()
    });
    assert!(!leases.valid(1));
    leases.watch_local(2, &deps, [10, 30]);
    // The same table notification is required for set, delete and subsume.
    leases.apply(&Change {
        tables: vec!["Used".into()],
        ..Default::default()
    });
    assert!(!leases.valid(2));
    leases.watch_local(3, &deps, [10, 30]);
    leases.apply(&Change {
        reset: true,
        ..Default::default()
    });
    assert!(!leases.valid(3));
}
