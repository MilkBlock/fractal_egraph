use std::{path::PathBuf, process::Command};
fn run(args: &[&str], out: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("PATH", "/nonexistent")
        .env("PYTHON", "/nonexistent")
        .args(["analyze"])
        .args(args)
        .arg("--output")
        .arg(out)
        .output()
        .unwrap()
}
fn report(out: &std::path::Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(out.join("analysis.json")).unwrap()).unwrap()
}
fn check(source: &str, rounds: &str, tag: &str) {
    let base = std::env::temp_dir().join(format!("native-history-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let copied_source = base.join("input.egg");
    std::fs::copy(source, &copied_source).unwrap();
    let source = copied_source.to_str().unwrap();
    let online = base.join("online");
    let offline = base.join("offline");
    let replay = base.join("replay");
    for (mode, out) in [("online", &online), ("offline", &offline)] {
        let result = run(
            &[
                "--recapture-tier0",
                "--build",
                mode,
                "--save-history",
                "--source",
                source,
                "--rounds",
                rounds,
            ],
            out,
        );
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        if mode == "online" {
            assert!(
                String::from_utf8_lossy(&result.stderr).contains("applications before next round")
            );
        }
    }
    let read_history = |dir: &std::path::Path| -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(dir.join("history.json")).unwrap()).unwrap()
    };
    let oh = read_history(&online);
    let fh = read_history(&offline);
    assert_eq!(oh, fh, "online/offline resolved bindings and effects");
    std::fs::remove_file(copied_source).unwrap();
    let h = online.join("history.json");
    let result = run(&["--replay-history", h.to_str().unwrap()], &replay);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let a = report(&online);
    let b = report(&offline);
    let c = report(&replay);
    for other in [&b, &c] {
        let normalize = |mut view: serde_json::Value| {
            for node in view["nodes"].as_object_mut().unwrap().values_mut() {
                node.as_object_mut().unwrap().remove("comb");
            }
            for lane in view["lanes"].as_array_mut().unwrap() {
                lane.as_object_mut().unwrap().remove("higher");
            }
            view
        };
        assert_eq!(
            normalize(a["view"].clone()),
            normalize(other["view"].clone())
        );
        // Egraph allocation IDs are deliberately not compared across builds.
        for key in [
            "events",
            "imported_events",
            "excluded_events",
            "extensions",
            "higher_rules",
            "executed_rounds",
        ] {
            assert_eq!(a["summary"][key], other["summary"][key], "{key}");
        }
    }
    assert_eq!(c["summary"]["history_replayed"], true);
    assert!(!replay.join("history.json").exists());
    let mut history: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&h).unwrap()).unwrap();
    history["version"] = serde_json::json!(999);
    let bad = base.join("bad.json");
    std::fs::write(&bad, serde_json::to_vec(&history).unwrap()).unwrap();
    assert!(
        !run(
            &["--replay-history", bad.to_str().unwrap()],
            &base.join("bad-version")
        )
        .status
        .success()
    );
    std::fs::write(&bad, b"{\"version\":1,").unwrap();
    assert!(
        !run(
            &["--replay-history", bad.to_str().unwrap()],
            &base.join("truncated")
        )
        .status
        .success()
    );
    std::fs::remove_dir_all(base).unwrap();
}
#[test]
fn history_rebuilds_without_tier0() {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli_math_run11.egg");
    check(source.to_str().unwrap(), "3", "small");
}
#[test]
#[ignore = "release Math online/offline/history equivalence"]
fn math_history_has_identical_bindings_effects_and_views() {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("egglog/tests/math-microbenchmark.egg");
    check(source.to_str().unwrap(), "6", "math");
}
