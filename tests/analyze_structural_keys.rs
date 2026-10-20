use serde_json::Value;
use std::{path::PathBuf, process::Command};
const BASE: &str = include_str!("../experiments/parameterized_cell/terms.egg");
fn temp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("analyze-keys-{name}-{}", std::process::id()));
    assert!(!p.exists());
    std::fs::create_dir(&p).unwrap();
    p
}
#[test]
fn cli_records_normalized_policy_and_replays_key_literals() {
    let root = temp("capture");
    let src = root.join("input.egg");
    let out = root.join("capture");
    std::fs::write(&src,format!(r#"{BASE}
      (function Expr-size (Expr) i64 :merge (min old new))
      (function to-hoist (Expr Expr) Expr :merge new)
      (ruleset initialization)
      (rule () ((TermLeaf 1) (TermLeaf 2) (V 0) (PotentialExtractedExpr (V 0) (TCPair (TermLeaf 2) 1))) :ruleset initialization :name "seed-input")
      (run-schedule (repeat 1 (run initialization)))
      (run-schedule (saturate terms-helpers (saturate terms-helpers-helpers)))
      (PotentialExtractedExpr (V 0) (TCPair (TermLeaf 1) 1))
      (run-schedule (saturate terms-helpers (saturate terms-helpers-helpers)))
      (check (= (ExtractedExpr (V 0)) (TCPair (TermLeaf 1) 1)))
    "#)).unwrap();
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--source"])
        .arg(&src)
        .arg("--output")
        .arg(&out)
        .args(["--structural-keys", "--save-history"])
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env("RAYON_NUM_THREADS", "1")
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let result: Value =
        serde_json::from_slice(&std::fs::read(out.join("analysis.json")).unwrap()).unwrap();
    assert_eq!(result["structural_keys"]["enabled"], true);
    assert_eq!(
        result["structural_keys"]["whole_graph_deterministic"],
        false
    );
    assert_eq!(result["semantic_audit"]["status"], "potentially_observable");
    assert_eq!(result["semantic_audit"]["cell_replacement"], "not_certified");
    assert!(out.join("semantic-audit.json").exists());
    assert!(String::from_utf8_lossy(&r.stderr).contains("[semantic warning]"));
    assert!(result["summary"]["applications"].as_u64().unwrap() > 0);
    let text = std::fs::read_to_string(out.join("normalized.egg")).unwrap();
    assert!(text.contains("stable-equal-cost"));
    let history: Value =
        serde_json::from_slice(&std::fs::read(out.join("history.json")).unwrap()).unwrap();
    assert!(
        history["source_text"]
            .as_str()
            .unwrap()
            .starts_with("; egg-layout structural-keys/eggcc-v1")
    );
    let seed = history["rules"]
        .as_array()
        .unwrap()
        .iter()
        .position(|r| r["code"].as_str().unwrap().contains(":name \"seed-input\""));
    let seed = seed.expect("initializer rule metadata is retained");
    assert!(
        !history["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["rule"].as_u64() == Some(seed as u64))
    );
    assert!(!history["rules"].as_array().unwrap().iter().any(|r| {
        r["code"]
            .as_str()
            .unwrap()
            .contains(":ruleset cell-stable-keys")
    }));
    assert!(history["tokens"].as_array().unwrap().iter().any(|t| {
        t["sort"] == "CellStructuralKey"
            && t["literal"]
                .as_str()
                .is_some_and(|s| s.starts_with("(cell-key-decode "))
    }));
    let cells = out.join("saturated-rule-composition/cells");
    let entry = std::fs::read_dir(&cells)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("entry.egg"))
        .find(|p| p.exists())
        .expect("at least one replayable entry");
    assert!(
        std::fs::read_to_string(&entry)
            .unwrap()
            .starts_with("; egg-layout structural-keys/eggcc-v1")
    );
    let direct = root.join("direct-ripen");
    let check = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .arg("ripen")
        .arg(&entry)
        .arg(&direct)
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env("RAYON_NUM_THREADS", "1")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    std::fs::remove_file(&src).unwrap();
    let replay = root.join("replay");
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--replay-history"])
        .arg(out.join("history.json"))
        .arg("--output")
        .arg(&replay)
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env("RAYON_NUM_THREADS", "1")
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let restored: Value =
        serde_json::from_slice(&std::fs::read(replay.join("analysis.json")).unwrap()).unwrap();
    assert_eq!(restored["structural_keys"]["enabled"], true);
    assert_eq!(restored["semantic_audit"], result["semantic_audit"]);
    assert_eq!(
        restored["summary"]["applications"],
        result["summary"]["applications"]
    );
    std::fs::remove_dir_all(root).unwrap();
}
