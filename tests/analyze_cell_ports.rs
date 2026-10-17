use serde_json::Value;
#[test]
fn certified_closure_identity_enables_observed_port_coordinates() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let temp = std::env::temp_dir().join(format!("cell-ports-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let source =
        std::fs::read_to_string(root.join("experiments/cell_egraph/port-progression.egg")).unwrap();
    let run = |name: &str, text: &str| -> Value {
        let path = temp.join(format!("{name}.egg"));
        std::fs::write(&path, text).unwrap();
        let out = temp.join(name);
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .arg("analyze")
            .arg("--source")
            .arg(&path)
            .arg("--output")
            .arg(&out)
            .arg("--save-history")
            .env("EGG_LAYOUT_GRAPHS", "0")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&std::fs::read(out.join("cell-egraph.json")).unwrap()).unwrap()
    };
    let full = run("full", &source);
    let report = &full["repetition_families"]["port_progressions"];
    assert_eq!(report["families"].as_array().unwrap().len(), 1);
    let f = &report["families"][0];
    assert_eq!(
        f["historical_producer_spellings"], 2,
        "different L/R histories must share via closure certificates"
    );
    assert_eq!(f["witnesses"].as_array().unwrap().len(), 2);
    let chain = &f["components"][0];
    assert_eq!(chain["status"], "observed-indexed-chain");
    assert_eq!(chain["window_count"], 3);
    assert_eq!(chain["element_count"], 4);
    for step in chain["interactions"].as_array().unwrap() {
        let i = step["i"].as_u64().unwrap() as usize;
        let witness = f["witnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["event"] == step["event"])
            .unwrap();
        for access in chain["affine_accesses"].as_array().unwrap() {
            let side = access["source"].as_u64().unwrap() as usize;
            let instance = witness[if side == 0 { "from" } else { "to" }]
                .as_u64()
                .unwrap() as usize;
            let port = access["port"].as_u64().unwrap() as usize;
            let index = i * access["coefficient"].as_u64().unwrap() as usize
                + access["offset"].as_u64().unwrap() as usize;
            assert_eq!(
                chain["elements"][index]["token"],
                report["instances"][instance]["ports"][port]["token"]
            );
        }
        for access in chain["target_affine_accesses"].as_array().unwrap() {
            let target = witness["target"].as_u64().unwrap() as usize;
            let port = access["target_port"].as_u64().unwrap() as usize;
            for offset in access["offsets"].as_array().unwrap() {
                let index = i * access["coefficient"].as_u64().unwrap() as usize
                    + offset.as_u64().unwrap() as usize;
                assert_eq!(
                    chain["elements"][index]["token"],
                    report["instances"][target]["ports"][port]["token"]
                );
            }
        }
    }
    assert!(
        chain["interactions"][0]["event"].as_u64().unwrap()
            < chain["interactions"][1]["event"].as_u64().unwrap()
    );
    assert_eq!(
        chain["interactions"][0]["i"], 1,
        "first observed interaction is not the first coordinate"
    );
    let ablated = source
        .lines()
        .filter(|s| !s.contains(":name \"pair-to-left\""))
        .collect::<Vec<_>>()
        .join("\n");
    let without = run("different-closures", &ablated);
    let bad = &without["repetition_families"]["port_progressions"];
    assert_eq!(bad["families"].as_array().unwrap().len(), 0);
    assert_eq!(
        bad["unsupported"]["source closures not certified equivalent"], 2,
        "both actual overlaps remain, but neither has the required source closure equality"
    );
    std::fs::remove_file(temp.join("full.egg")).unwrap();
    let replay = temp.join("replay");
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .arg("analyze")
        .arg("--replay-history")
        .arg(temp.join("full/history.json"))
        .arg("--output")
        .arg(&replay)
        .env("EGG_LAYOUT_GRAPHS", "0")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let replayed: Value =
        serde_json::from_slice(&std::fs::read(replay.join("cell-egraph.json")).unwrap()).unwrap();
    assert_eq!(
        report,
        &replayed["repetition_families"]["port_progressions"]
    );
    std::fs::remove_dir_all(temp).unwrap();
}
