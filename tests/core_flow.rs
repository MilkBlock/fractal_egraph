use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
fn fresh(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cs-core-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
fn read(p: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
#[test]
fn all_observed_entries_are_registered_and_identical_entries_share_work() {
    let base = fresh("uncapped");
    fs::create_dir_all(&base).unwrap();
    let source = base.join("source.egg");
    let mut s =
        "(datatype E (V i64) (A E) (B E) (C E))\n(rewrite (A x) (B x))\n(rewrite (B x) (C x))\n"
            .to_string();
    for i in 0..270 {
        s += &format!("(A (V {i}))\n");
    }
    s += "(run 3)\n";
    fs::write(&source, s).unwrap();
    let out = base.join("run");
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_RIPEN_JOBS", "1")
        .env("EGG_LAYOUT_RIPEN_ROUNDS", "1")
        .args(["analyze", "--source"])
        .arg(source)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let q = read(out.join("saturated-rule-composition/queue.json"));
    assert!(q["jobs"].as_array().unwrap().len() >= 270);
    assert_eq!(q["not_queued"], 0);
    assert!(q["limits"].is_null());
    assert!(
        q["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["state"] == "Saturated")
    );
    assert!(
        q["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|j| j.get("fully_equivalent_to").is_some())
            .count()
            >= 269
    );
    assert_eq!(q["catalog"]["catalog"]["saturated_rule_compositions"], 1);
    fs::remove_dir_all(base).unwrap();
}
#[test]
fn finite_reentry_is_observed_but_not_called_infinite() {
    let out = fresh("reentry");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/core_flow/reentry.egg");
    let r = egg_layout::native_analyze::run(&source, &out, None, None, false).unwrap();
    let q = &r["saturated_rule_composition"];
    assert_eq!(q["counts"]["Saturated"], 1);
    assert!(
        q["weak_dominate"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["reentry_observation"] == true)
    );
    assert!(q["weak_dominate"].as_array().unwrap().iter().all(|e| {
        e["classification"]
            .as_str()
            .unwrap()
            .starts_with("unclassified")
    }));
    for edge in q["weak_dominate"].as_array().unwrap() {
        let cache = &edge["evidence"]["query_cache"];
        assert_eq!(cache["compilations"], 2);
        assert_eq!(cache["initial_scans"], 1);
        assert_eq!(cache["per_check_engine_clones"], 0);
        assert!(cache["current_scans"].as_u64().unwrap() > 1);
    }
    fs::remove_dir_all(out).unwrap();
}
#[test]
fn infinite_cell_keeps_running_while_finite_cell_completes_and_is_cancellable() {
    let out = fresh("fair");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/core_flow/fair.egg");
    let mut child = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT", &out)
        .arg("debug-stream")
        .arg(source)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // The test controls cancellation; the inference engine has no round cutoff.
    let mut got = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v["kind"] == "ripen_progress" {
            let q = &v["saturated_rule_composition"];
            if q["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|j| j["state"] == "Running" && j["ripen"]["round"].as_u64().unwrap_or(0) >= 6)
            {
                got = Some(q.clone());
                break;
            }
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let q = got.expect("no live progress");
    assert!(q["counts"]["Saturated"].as_u64().unwrap_or(0) > 0);
    assert!(q["counts"]["Running"].as_u64().unwrap_or(0) > 0);
    assert!(
        q["weak_dominate"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["reentry_observation"] == true)
    );
    fs::remove_dir_all(out).unwrap();
}
