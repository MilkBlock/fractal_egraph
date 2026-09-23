use std::{fs, process::Command};
#[test]
fn compact_stream_keeps_analysis_but_omits_per_application_rendering() {
    let root = std::env::temp_dir().join(format!("compact-trace-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("input.egg");
    fs::write(
        &source,
        "(datatype T (X i64) (Y i64)) (rewrite (X n) (Y n)) (X 1) (run 1)",
    )
    .unwrap();
    let mut summaries = vec![];
    for full in [false, true] {
        let out = root.join(if full { "full" } else { "compact" });
        let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .args(["debug-stream", source.to_str().unwrap()])
            .env("EGG_LAYOUT_FULL_TRACE", if full { "1" } else { "0" })
            .env("EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT", &out)
            .env("EGG_LAYOUT_EAGER_SVG", "0")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let rows: Vec<serde_json::Value> = String::from_utf8(result.stdout)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(rows.iter().any(|r| r["kind"] == "application"), full);
        assert!(rows.iter().any(|r| r["kind"] == "complete"));
        let q = rows
            .iter()
            .filter_map(|r| r.get("saturated_rule_composition"))
            .last()
            .unwrap();
        assert_eq!(q["observation_limits"]["milliseconds"], 50);
        assert_eq!(q["observation_limits"]["facts"], 200000);
        summaries.push(q["counts"].clone());
    }
    assert_eq!(summaries[0], summaries[1]);
    fs::remove_dir_all(root).unwrap();
}
