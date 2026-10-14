//! Contextual ancestry is not a certificate that the other graft was necessary.
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{fs, path::PathBuf, process::Command};
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn analyze(source: &str) -> (PathBuf, Value) {
    let dir = std::env::temp_dir().join(format!(
        "graft-diamond-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("source.egg"), source).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--source"])
        .arg(dir.join("source.egg"))
        .arg("--output")
        .arg(dir.join("run"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env("EGG_LAYOUT_CELL_WINDOWS", "1")
        .env("EGG_LAYOUT_ANALYSIS_THREADS", "1")
        .env_remove("EGG_LAYOUT_COMPOSITION_DISCOVERY")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(dir.join("run/analysis.json")).unwrap_or_else(|e| {
        panic!(
            "{dir:?}: {e}; stdout={} stderr={}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
    });
    let report = serde_json::from_slice(&bytes).unwrap();
    (dir, report)
}

const HEADER: &str = r#"
(datatype T (V String) (Seed T T) (Add T T) (GateL T) (GateR T) (Left T) (Right T))
(ruleset all)
(rule ((Seed a b)) ((Add a b)) :ruleset all :name "base")
(rule ((Add a b) (GateL a)) ((Left a)) :ruleset all :name "left")
(rule ((Add a b) (GateR b)) ((Right b)) :ruleset all :name "right")
(let vx (V "x")) (let vy (V "y")) (Seed vx vy) (run all 1)
"#;

#[test]
fn independent_window_does_not_inherit_the_other_sides_effect() {
    for (first, second, unwanted) in [
        ("(GateL vx)", "(GateR vy)", "Left"),
        ("(GateR vy)", "(GateL vx)", "Right"),
    ] {
        let source = format!(
            "{HEADER}{first} (run all 1) {second} (run all 1) (check (Left vx) (Right vy))"
        );
        let (dir, report) = analyze(&source);
        let q = &report["saturated_rule_composition"];
        let window = q["cs"]["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["members"] == serde_json::json!([0, 2]))
            .unwrap();
        let job = q["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["candidate_id"] == window["unit"])
            .unwrap();
        assert_eq!(job["state"], "Saturated");
        let folder = PathBuf::from(job["folder"].as_str().unwrap());
        let state: Value = serde_json::from_slice(
            &fs::read(folder.join("saturated-rule-composition.json")).unwrap(),
        )
        .unwrap();
        assert!(
            !state["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["op"] == unwanted),
            "{state:#}"
        );
        // The causal producer is the base event even though a later boundary
        // can also record the first side as historical context.
        let comp = q["cs"]["compositions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["consumer"] == 2)
            .unwrap();
        assert_eq!(comp["producer_parts"], serde_json::json!([0]));
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn same_batch_siblings_do_not_become_each_others_predecessors() {
    let (dir, report) = analyze(&format!(
        "{HEADER}(GateL vx) (GateR vy) (run all 1) (check (Left vx) (Right vy))"
    ));
    let cs = report["saturated_rule_composition"]["cs"]["compositions"]
        .as_array()
        .unwrap();
    for comp in &cs[1..] {
        assert_eq!(comp["parts"], serde_json::json!([0]));
    }
    fs::remove_dir_all(dir).unwrap();
}
