use serde_json::Value;
use std::{fs, process::Command};
#[test]
fn newly_constructed_set_result_is_replayable() {
    let root = std::env::temp_dir().join(format!("ripen-new-set-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let src = root.join("source.egg");
    fs::write(
        &src,
        r#"
(datatype E (V) (Box E))
(function chosen (E) E :merge old)
(ruleset build)
(rule ((= x (V))) ((set (chosen x) (Box x))) :ruleset build)
(V)
(run build 1)
(check (= (chosen (V)) (Box (V))))
"#,
    )
    .unwrap();
    let out = root.join("run");
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env("RUST_MIN_STACK", "16777216")
        .args([
            "analyze",
            "--source",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let d: Value = serde_json::from_slice(&fs::read(out.join("analysis.json")).unwrap()).unwrap();
    let jobs = d["saturated_rule_composition"]["jobs"].as_array().unwrap();
    assert!(!jobs.is_empty());
    for j in jobs {
        assert_ne!(j["state"], "Rejected", "{}", j["reason"]);
    }
    fs::remove_dir_all(root).unwrap();
}
