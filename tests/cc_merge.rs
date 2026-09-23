use serde_json::Value;
use std::{fs, path::PathBuf};
#[test]
fn new_families_participate_in_next_generation_without_repeating_pairs() {
    let root = std::env::temp_dir().join(format!("cc-feature-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("rules.egg"), "(datatype E (A E))").unwrap();
    fs::write(
        root.join("seeds.json"),
        r#"[{"parameters":1,"facts":["(A p0)"],"result":"(A p0)"}]"#,
    )
    .unwrap();
    egg_layout::cc_merge::run(
        &root.join("rules.egg"),
        &root.join("seeds.json"),
        &root.join("out"),
        2,
    )
    .unwrap();
    let r: Value =
        serde_json::from_slice(&fs::read(root.join("out/summary.json")).unwrap()).unwrap();
    assert_eq!(r["status"], "complete");
    assert_eq!(r["generations"][0]["pairs"], 1);
    assert_eq!(r["generations"][0]["bindings"], 3);
    let initial_plus_new = 1 + r["generations"][0]["new_families"].as_u64().unwrap();
    assert_eq!(
        r["generations"][1]["pairs"],
        initial_plus_new * initial_plus_new - 1
    );
    assert!(r["generations"][1]["bindings"].as_u64().unwrap() > 3);
    let log = fs::read_to_string(root.join("out/merges.jsonl")).unwrap();
    let rows: Vec<Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(rows.iter().any(|r| r["generation"] == 2
        && (r["left"].as_u64().unwrap() > 0 || r["right"].as_u64().unwrap() > 0)));
    let unique: std::collections::BTreeSet<_> = rows
        .iter()
        .map(|r| format!("{}:{}:{}", r["left"], r["right"], r["binding"]))
        .collect();
    assert_eq!(unique.len(), rows.len());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn demo_configuration_uses_general_cli() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::temp_dir().join(format!("cc-cli-{}", std::process::id()));
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["cc-merge", "--rules"])
        .arg(root.join("experiments/cc_merge_probe/demo.egg"))
        .arg("--seeds")
        .arg(root.join("experiments/cc_merge_probe/seeds.json"))
        .arg("--output")
        .arg(&out)
        .args(["--generations", "1"])
        .status()
        .unwrap();
    assert!(status.success());
    let r: Value = serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
    assert_eq!(r["generations"][0]["bindings"], 68);
    assert_eq!(r["generations"][0]["changed"], 7);
    fs::remove_dir_all(out).unwrap();
}

#[test]
fn scalar_constructor_fields_and_cycles_survive_import() {
    let root = std::env::temp_dir().join(format!("cc-scalars-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("rules.egg"),
        "(datatype E (V i64 String bool) (Loop E)) (rewrite (Loop x) x)",
    )
    .unwrap();
    fs::write(root.join("seeds.json"),r#"[{"parameters":0,"facts":["(V 7 \"tag\" true)"],"result":"(V 7 \"tag\" true)"},{"parameters":1,"facts":["(Loop p0)"],"result":"p0"}]"#).unwrap();
    egg_layout::cc_merge::run(
        &root.join("rules.egg"),
        &root.join("seeds.json"),
        &root.join("out"),
        1,
    )
    .unwrap();
    let states: Vec<Value> = fs::read_dir(root.join("out/families"))
        .unwrap()
        .map(|p| serde_json::from_slice(&fs::read(p.unwrap().path()).unwrap()).unwrap())
        .collect();
    assert!(states.iter().any(|s| {
        s["values"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["literal"] == "\"tag\"")
    }));
    assert!(states.iter().any(|s| {
        s["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["op"] == "Loop" && r["args"][0] == r["result"])
    }));
    fs::remove_dir_all(root).unwrap();
}
