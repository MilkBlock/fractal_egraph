use serde_json::Value;
use std::{fs, path::Path, process::Command};
fn json(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
#[test]
fn mainline_analysis_emits_upstream_slotted_layers_and_graft_dsl() {
    let root = std::env::temp_dir().join(format!("mainline-slotted-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let out = root.join("out");
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "analyze",
            "--source",
            "experiments/math_mini3/source.egg",
            "--rounds",
            "1",
            "--output",
            out.to_str().unwrap(),
            "--ripen-observe-ms",
            "1",
            "--ripen-observe-facts",
            "1",
        ])
        .env("EGG_LAYOUT_ANALYSIS_THREADS", "1")
        .env("EGG_LAYOUT_EAGER_SVG", "0")
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let report = json(out.join("slotted-canon.json"));
    assert_eq!(report["schema"], "slotted-layer/v2");
    assert!(report["applications"].as_u64().unwrap() > 0);
    assert!(report["slotted_eclasses"].as_u64().unwrap() > 0);
    assert!(out.join("slotted-canon.dsl").is_file());
    assert!(out.join("slotted-canon.dot").is_file());
    assert!(out.join("slotted-egraph.dot").is_file());
    let analysis = json(out.join("analysis.json"));
    assert_eq!(analysis["slotted_canon"]["schema"], "slotted-layer/v2");
    fs::remove_dir_all(root).unwrap();
}
