use egg_layout::saturated_rule_composition::{Comparison, SaturatedRuleComposition, compare};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn run(source: &Path, out: &Path, reuse: bool, extra: &[&str]) -> Value {
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env(
            "EGG_LAYOUT_RIPEN_PARENT_REUSE",
            if reuse { "1" } else { "0" },
        )
        .env("RUST_MIN_STACK", "16777216")
        .args([
            "analyze",
            "--source",
            source.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .args(extra)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    serde_json::from_slice(&fs::read(out.join("analysis.json")).unwrap()).unwrap()
}
fn state(job: &Value) -> SaturatedRuleComposition {
    serde_json::from_slice(
        &fs::read(
            PathBuf::from(job["folder"].as_str().unwrap()).join("saturated-rule-composition.json"),
        )
        .unwrap(),
    )
    .unwrap()
}
#[test]
fn default_analyze_reuses_completed_parent_and_preserves_final_states() {
    let root =
        std::env::temp_dir().join(format!("parent-graft-integration-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.egg");
    fs::write(
        &source,
        r#"
        (datatype E (V i64) (A E) (B E) (X E) (C E) (D E))
        (ruleset first) (ruleset second) (ruleset third)
        (rule ((A x)) ((B x)) :ruleset first)
        (rule ((B x) (X x)) ((C x)) :ruleset second)
        (rule ((C x)) ((D x)) :ruleset third)
        (A (V 0)) (X (V 0))
        (run-schedule first second third)
    "#,
    )
    .unwrap();
    let cold = run(&source, &root.join("cold"), false, &[]);
    let warm = run(&source, &root.join("warm"), true, &[]);
    let a = &cold["saturated_rule_composition"];
    let b = &warm["saturated_rule_composition"];
    assert!(
        b["parent_snapshot_reuse"]["stats"]["hits"]
            .as_u64()
            .unwrap()
            >= 1,
        "{}",
        b["parent_snapshot_reuse"]
    );
    assert_eq!(b["parent_snapshot_reuse"]["stats"]["graft_errors"], 0);
    let x = a["jobs"].as_array().unwrap();
    let y = b["jobs"].as_array().unwrap();
    assert_eq!(x.len(), y.len());
    for (x, y) in x.iter().zip(y) {
        assert_eq!(x["state"], y["state"]);
        if x["state"] == "Saturated" {
            if y["initialization"]["path"] == "parent_snapshot" {
                let replay = fs::read_to_string(
                    PathBuf::from(y["folder"].as_str().unwrap()).join("ripened.egg"),
                )
                .unwrap();
                egglog::EGraph::default()
                    .parse_and_run_program(None, &replay)
                    .unwrap();
            }
            assert!(matches!(
                compare(&state(x), &state(y)).unwrap(),
                Comparison::Equivalent { .. }
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn mutable_environment_falls_back_without_parent_reordering() {
    let root = std::env::temp_dir().join(format!("parent-graft-mutable-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.egg");
    fs::write(
        &source,
        r#"
        (datatype E (V) (A E) (B E))
        (function score (E) i64 :merge new)
        (ruleset first) (ruleset second)
        (rule ((= x (V))) ((A x)) :ruleset first)
        (rule ((A x)) ((B x)) :ruleset second)
        (V) (run-schedule first second)
    "#,
    )
    .unwrap();
    let result = run(&source, &root.join("run"), true, &[]);
    let stats = &result["saturated_rule_composition"]["parent_snapshot_reuse"]["stats"];
    assert_eq!(stats["hits"], 0);
    assert!(
        stats["fallbacks"]["nonpositive rule environment"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "{stats}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unfinished_parent_releases_waiting_child_to_rebuild() {
    let root = std::env::temp_dir().join(format!("parent-graft-unfinished-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.egg");
    fs::write(
        &source,
        r#"
        (datatype E (V) (G E))
        (rule ((= x (G y))) ((G x)))
        (G (V)) (run 2)
    "#,
    )
    .unwrap();
    let result = run(
        &source,
        &root.join("run"),
        true,
        &["--ripen-observe-facts", "20"],
    );
    let r = &result["saturated_rule_composition"];
    let stats = &r["parent_snapshot_reuse"]["stats"];
    assert_eq!(stats["hits"], 0);
    assert!(
        stats["fallbacks"]["parent unfinished, rejected, or without certified snapshot"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "{stats}"
    );
    assert!(
        r["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["state"] != "WaitingParent" && j["state"] != "Running")
    );
    fs::remove_dir_all(root).unwrap();
}
