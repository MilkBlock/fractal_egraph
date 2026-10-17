//! Discovery must be a function of captured analyze evidence, including replay.
use serde_json::Value;

#[test]
fn observed_families_come_from_history_and_replay_without_the_source_file() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("analyze-families-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.egg");
    std::fs::copy(
        root.join("experiments/cell_egraph/observed-families.egg"),
        &source,
    )
    .unwrap();
    let run = dir.join("run");
    let replay = dir.join("replay");
    let invoke = |args: Vec<std::ffi::OsString>| {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .args(args)
            .env("EGG_LAYOUT_GRAPHS", "0")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    invoke(vec![
        "analyze".into(),
        "--source".into(),
        source.clone().into(),
        "--output".into(),
        run.clone().into(),
        "--save-history".into(),
    ]);
    let read = |p: &std::path::Path| -> Value {
        serde_json::from_slice(&std::fs::read(p.join("cell-egraph.json")).unwrap()).unwrap()
    };
    let a = read(&run);
    assert_eq!(
        a["certified_unions"], 0,
        "observed families must not create equality certificates"
    );
    assert_eq!(
        a["eclasses"], a["registered_compositions"],
        "family membership must not merge these distinct Cells"
    );
    let f = &a["repetition_families"];
    assert_eq!(f["source"], "analyze captured committed applications");
    let families = f["families"].as_array().unwrap();
    assert!(!families.is_empty());
    assert!(
        f["membership_rows"].as_u64().unwrap() > 0,
        "families must be present in the native Cell egraph relation"
    );
    assert!(families.iter().any(|x| {
        x["rules"]
            .as_array()
            .unwrap()
            .contains(&Value::from("fib-expand"))
    }));
    assert!(families.iter().any(|x| {
        x["rules"]
            .as_array()
            .unwrap()
            .contains(&Value::from("a-to-b"))
    }));
    assert!(
        !f.to_string().contains("never-executed"),
        "uncalled source recursion must not enter discovery"
    );
    let history: Value =
        serde_json::from_slice(&std::fs::read(run.join("history.json")).unwrap()).unwrap();
    for family in families {
        assert_eq!(family["status"], "observed-reentry");
        assert!(!family["units"].as_array().unwrap().is_empty());
        for occurrence in family["occurrences"].as_array().unwrap() {
            for step in occurrence["steps"].as_array().unwrap() {
                let p = step["source_record"].as_u64().unwrap() as usize;
                let q = step["target_record"].as_u64().unwrap() as usize;
                assert!(p < q);
                assert!(
                    history["records"][q]["parents"]
                        .as_array()
                        .unwrap()
                        .contains(&Value::from(p))
                );
            }
        }
    }
    std::fs::remove_file(&source).unwrap();
    invoke(vec![
        "analyze".into(),
        "--replay-history".into(),
        run.join("history.json").into(),
        "--output".into(),
        replay.clone().into(),
    ]);
    let b = read(&replay);
    assert_eq!(f["contracts"], b["repetition_families"]["contracts"]);
    assert_eq!(
        f["binding_contracts"],
        b["repetition_families"]["binding_contracts"]
    );
    assert_eq!(f["families"], b["repetition_families"]["families"]);
    std::fs::remove_dir_all(dir).unwrap();
}
