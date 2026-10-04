use std::{fs, process::Command};

#[test]
fn finite_container_observation_is_exported_without_a_sharing_certificate() {
    let root = std::env::temp_dir().join(format!("ripen-observed-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("source.egg");
    fs::write(
        &input,
        r#"
        (datatype E (V i64))
        (sort ES (Set E)) (constructor Box (ES) E)
        (sort Unused (Vec E)) (constructor UnusedBox (Unused) E)
        (function score (E) i64 :merge (+ old new))
        (rule ((= x (V 99))) ((panic "should never run")))
        (Box (set-of (V 1) (V 2)))
        (set (score (V 1)) 7)
    "#,
    )
    .unwrap();
    let out = root.join("out");
    let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .args(["ripen", input.to_str().unwrap(), out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("observed-saturated-state.json")).unwrap())
            .unwrap();
    assert_eq!(state["scope"]["observation_only"], true);
    assert_eq!(
        state["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["op"] == "@member:ES")
            .count(),
        2
    );
    assert!(!out.join("saturated-rule-composition.json").exists());
    let bad = root.join("bad.egg");
    fs::write(
        &bad,
        "(datatype E (A)) (rule ((A)) ((panic \"invalid boundary\"))) (A)",
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .args([
            "ripen",
            bad.to_str().unwrap(),
            root.join("bad-out").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !result.status.success(),
        "must preserve active error checks"
    );
    let growing = root.join("growing.egg");
    fs::write(&growing, "(datatype E (Z) (S E)) (rule ((= x (S y))) ((S x))) (S (Z))").unwrap();
    let growing_out = root.join("growing-out");
    let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .args(["ripen", growing.to_str().unwrap(), growing_out.to_str().unwrap()])
        .output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let report: serde_json::Value = serde_json::from_slice(&fs::read(growing_out.join("ripen.json")).unwrap()).unwrap();
    assert_eq!(report["ripen"]["state"], "ObservedUnfinished");
    assert!(!growing_out.join("saturated-rule-composition.json").exists());
    fs::remove_dir_all(root).unwrap();
}
