use serde_json::Value;
use std::{fs, process::Command};
fn capture(text: &str, name: &str) -> (std::path::PathBuf, Value) {
    let root = std::env::temp_dir().join(format!("cs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let src = root.join("source.egg");
    fs::write(&src, text).unwrap();
    let out = root.join("run");
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_RIPEN_JOBS", "128")
        .env("EGG_LAYOUT_RIPEN_PER_BOUNDARY", "32")
        .env("EGG_LAYOUT_RIPEN_MILLISECONDS", "100000")
        .args(["analyze", "--recapture-tier0", "--source"])
        .arg(src)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let q = serde_json::from_slice(&fs::read(out.join("closed/queue.json")).unwrap()).unwrap();
    (root, q)
}
#[test]
fn ccss_anchors_independent_cs_pairs() {
    let (root, q) = capture(
        "(datatype E (V) (A E) (A1 E) (A2 E) (B E) (B1 E) (B2 E))\n(rewrite (A x) (A1 x))\n(rewrite (A1 x) (A2 x))\n(rewrite (B x) (B1 x))\n(rewrite (B1 x) (B2 x))\n(A (V))\n(B (V))\n(run 5)",
        "parallel",
    );
    let pairs = q["cs"]["compositions"].as_array().unwrap();
    assert!(pairs.iter().any(|p| p["kind"] == "CCSS"), "{q}");
    for p in pairs.iter().filter(|p| p["kind"] == "CCSS") {
        assert!(!p["anchors"].as_array().unwrap().is_empty());
        assert!(p["links"].as_array().unwrap().is_empty());
    }
    assert!(
        q["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["candidate_kind"] == "CCSS" && j["state"] == "Closed"),
        "{q}"
    );
    assert!(
        q["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|j| j["candidate_kind"] == "CCSS")
            .all(|j| j.get("members").is_none())
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn cscs_keeps_cross_pair_dependency_direction() {
    let (root, q) = capture(
        "(datatype E (V) (A E) (A1 E) (A2 E) (Extra E) (B E) (B1 E))\n(rewrite (A x) (A1 x))\n(rewrite (A1 x) (A2 x))\n(rule ((= a (A2 x)) (= e (Extra x))) ((B x)))\n(rewrite (B x) (B1 x))\n(A (V))\n(Extra (V))\n(run 6)",
        "sequence",
    );
    let pairs = q["cs"]["compositions"].as_array().unwrap();
    assert!(pairs.iter().any(|p| p["kind"] == "CSCS"), "{q}");
    for p in pairs.iter().filter(|p| p["kind"] == "CSCS") {
        assert!(!p["links"].as_array().unwrap().is_empty());
    }
    assert!(
        q["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["candidate_kind"] == "CSCS" && j["state"] == "Closed"),
        "{q}"
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn ccss_rejects_missing_anchor_and_function_updates() {
    let (root, q) = capture(
        "(datatype E (V i64) (A E) (A1 E) (A2 E) (B E) (B1 E) (B2 E))\n(rewrite (A x) (A1 x))\n(rewrite (A1 x) (A2 x))\n(rewrite (B x) (B1 x))\n(rewrite (B1 x) (B2 x))\n(A (V 1))\n(B (V 2))\n(run 4)",
        "no-anchor",
    );
    assert!(
        !q["cs"]["compositions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["kind"] == "CCSS")
    );
    fs::remove_dir_all(root).unwrap();
    let (root, q) = capture(
        "(datatype E (V) (A E) (A1 E) (B E) (B1 E) (B2 E))\n(function flag (E) i64 :merge (max old new))\n(rewrite (A x) (A1 x))\n(rule ((= a (A1 x))) ((set (flag x) 1)))\n(rewrite (B x) (B1 x))\n(rewrite (B1 x) (B2 x))\n(A (V))\n(B (V))\n(run 4)",
        "updates",
    );
    assert!(
        !q["cs"]["compositions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["kind"] == "CCSS")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn closed_compositions_participate_recursively() {
    let (root, q) = capture(
        "(datatype E (V) (A E) (A1 E) (A2 E) (B E) (B1 E) (B2 E) (C E) (C1 E) (C2 E))\n(rewrite (A x) (A1 x))\n(rewrite (A1 x) (A2 x))\n(rewrite (B x) (B1 x))\n(rewrite (B1 x) (B2 x))\n(rewrite (C x) (C1 x))\n(rewrite (C1 x) (C2 x))\n(A (V))\n(B (V))\n(C (V))\n(run 5)",
        "recursive",
    );
    assert!(
        q["cs"]["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["depth"].as_u64().unwrap() >= 2),
        "{q}"
    );
    let units = q["cs"]["units"].as_array().unwrap();
    for p in q["cs"]["compositions"].as_array().unwrap() {
        assert!(p["certificate"]["obligations"].is_array());
        assert_eq!(p["certificate"]["live_substitution"], false);
        for part in p["parts"].as_array().unwrap() {
            assert!((part.as_u64().unwrap() as usize) < units.len());
        }
    }
    fs::remove_dir_all(root).unwrap();
}
