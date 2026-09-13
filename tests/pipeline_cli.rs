use std::process::Command;
#[test]
fn help_and_bad_arguments_do_not_start_an_experiment() {
    let exe = env!("CARGO_BIN_EXE_egg_layout");
    let help = Command::new(exe).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("saved tier-1"));
    for args in [
        vec!["relations"],
        vec!["schema", "only-input"],
        vec!["unknown"],
        vec!["view", "unexpected"],
    ] {
        let out = Command::new(exe).args(args).output().unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("invalid command or arguments"));
    }
}
#[test]
fn unified_native_operation_matches_fixed_reference() {
    let output =
        std::env::temp_dir().join(format!("egg-layout-pipeline-{}.json", std::process::id()));
    let status = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["relations", "experiments/tier2/math.egg"])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let actual: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../experiments/tier2/math_native.json")).unwrap();
    std::fs::remove_file(output).unwrap();
    assert_eq!(actual, expected);
}
