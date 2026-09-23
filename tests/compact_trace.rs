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

#[test]
fn graphless_run_keeps_catalog_without_dot_or_svg_files() {
    let root = std::env::temp_dir().join(format!("graphless-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let src = root.join("input.egg");
    fs::write(
        &src,
        "(datatype T (X i64) (Y i64)) (rewrite (X n) (Y n)) (X 1) (run 1)",
    )
    .unwrap();
    let out = root.join("run");
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "analyze",
            "--source",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env("EGG_LAYOUT_EAGER_SVG", "1")
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    fn check(p: &std::path::Path) {
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                check(&p)
            } else {
                assert!(!matches!(
                    p.extension().and_then(|s| s.to_str()),
                    Some("dot" | "svg")
                ));
            }
        }
    }
    check(&out);
    assert!(out.join("catalog/catalog.json").exists());
    assert!(out.join("analysis.json").exists());
    fs::remove_dir_all(root).unwrap();
}
