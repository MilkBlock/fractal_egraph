use std::{path::PathBuf, process::Command};
#[test]
fn actual_egg_growth_and_history_replay_agree() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base = std::env::temp_dir().join(format!("dependency-growth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let source = base.join("input.egg");
    std::fs::write(
        &source,
        include_str!("../experiments/dependency_growth/growing.egg"),
    )
    .unwrap();
    let execute = |args: Vec<String>| {
        let output = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .current_dir(&root)
            .env("EGG_LAYOUT_DISCOVER_GROWTH", "1")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    execute(vec![
        "analyze".into(),
        "--recapture-tier0".into(),
        "--source".into(),
        source.display().to_string(),
        "--save-history".into(),
        "--output".into(),
        base.join("capture").display().to_string(),
    ]);
    std::fs::remove_file(source).unwrap();
    execute(vec![
        "analyze".into(),
        "--replay-history".into(),
        base.join("capture/history.json").display().to_string(),
        "--output".into(),
        base.join("replay").display().to_string(),
    ]);
    let read = |dir: &str| -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(base.join(dir).join("analysis.json")).unwrap())
            .unwrap()
    };
    let a = read("capture");
    let b = read("replay");
    assert_eq!(a["dependency_growth"], b["dependency_growth"]);
    let g = &a["dependency_growth"];
    assert_eq!(g["held_out_fits"], 1);
    let r = &g["reports"][0];
    assert_eq!(r["mapped_events"], 28);
    assert_eq!(r["templates"].as_array().unwrap().len(), 3);
    assert_eq!(r["exceptions"].as_array().unwrap().len(), 7);
    assert_eq!(r["status"], "held_out_fit_with_boundary");
    for (d, layer) in r["layers"].as_array().unwrap().iter().enumerate() {
        assert_eq!(layer["mapped"], d + 1);
    }
    // Reverse declaration order of the two row rules: event scheduling can
    // change, but coordinate/recipe identification must not depend on that order.
    let mut lines: Vec<_> = include_str!("../experiments/dependency_growth/growing.egg")
        .lines()
        .collect();
    let birth = lines
        .iter()
        .position(|s| s.contains(":name \"birth-b\""))
        .unwrap();
    let carry = lines
        .iter()
        .position(|s| s.contains(":name \"carry-b\""))
        .unwrap();
    lines.swap(birth, carry);
    let permuted = base.join("permuted.egg");
    std::fs::write(&permuted, lines.join("\n")).unwrap();
    execute(vec![
        "analyze".into(),
        "--recapture-tier0".into(),
        "--source".into(),
        permuted.display().to_string(),
        "--output".into(),
        base.join("permuted").display().to_string(),
    ]);
    let p = read("permuted");
    let pr = &p["dependency_growth"]["reports"][0];
    assert_eq!(r["templates"], pr["templates"]);
    assert_eq!(r["layers"], pr["layers"]);
    assert_eq!(r["status"], pr["status"]);
    assert!(base.join("capture/dependency_growth.html").exists());
    std::fs::remove_dir_all(base).unwrap();
}
