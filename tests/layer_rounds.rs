use std::{path::PathBuf, process::Command};
fn invoke(args: &[&str], out: &std::path::Path) {
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_RIPEN_MILLISECONDS", "100000")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["analyze"])
        .args(args)
        .arg("--output")
        .arg(out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
}
fn read(p: impl AsRef<std::path::Path>) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
#[test]
fn every_round_has_dot_and_replay_preserves_boundaries() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base = std::env::temp_dir().join(format!("layer-rounds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let online = base.join("online");
    let offline = base.join("offline");
    let replay = base.join("replay");
    let source = root.join("experiments/bake/increment-3.egg");
    for (mode, out) in [("online", &online), ("offline", &offline)] {
        invoke(
            &[
                "--recapture-tier0",
                "--source",
                source.to_str().unwrap(),
                "--rounds",
                "8",
                "--build",
                mode,
                "--save-history",
            ],
            out,
        );
        let manifest = read(out.join("rounds/manifest.json"));
        assert_eq!(manifest.as_array().unwrap().len(), 8);
        let mut prev = 0;
        for (i, row) in manifest.as_array().unwrap().iter().enumerate() {
            assert_eq!(row["round"], i + 1);
            let end = row["end"].as_u64().unwrap();
            assert!(end >= prev);
            prev = end;
            for kind in ["layers", "fractals", "coverage", "reuse", "use_fractals"] {
                let dot = std::fs::read_to_string(
                    out.join(format!("rounds/round-{:04}.{kind}.dot", i + 1)),
                )
                .unwrap();
                assert!(dot.starts_with("digraph G"));
                if kind == "reuse" {
                    let data = read(out.join(format!("rounds/round-{:04}.json", i + 1)));
                    let g = &data["graphs"]["reuse"];
                    let nodes = g["nodes"].as_array().unwrap();
                    assert_eq!(
                        nodes.len(),
                        data["reuse"]["roots"].as_array().unwrap().len()
                    );
                    let ids: std::collections::BTreeSet<_> =
                        nodes.iter().map(|n| n["id"].as_str().unwrap()).collect();
                    for edge in g["edges"].as_array().unwrap() {
                        assert!(ids.contains(edge["from"].as_str().unwrap()));
                        assert!(ids.contains(edge["to"].as_str().unwrap()));
                    }
                }
                if kind == "layers" {
                    let data = read(out.join(format!("rounds/round-{:04}.json", i + 1)));
                    for t in data["analysis"]["templates"].as_array().unwrap() {
                        for w in t["witnesses"].as_array().unwrap() {
                            assert!(
                                w.as_array()
                                    .unwrap()
                                    .iter()
                                    .all(|v| v.as_u64().unwrap() < end)
                            );
                        }
                    }
                }
            }
        }
        assert!(
            !out.join("layer_fractals.html").exists(),
            "rendering belongs to tools debugger"
        );
    }
    let history = online.join("history.json");
    invoke(&["--replay-history", history.to_str().unwrap()], &replay);
    for i in 1..=8 {
        let file = format!("rounds/round-{i:04}.json");
        // Saturated-rule-composition provenance contains the owning run directory; logical snapshots
        // must still match after normalizing this one run-local path namespace.
        let normalized = |dir: &std::path::Path| -> serde_json::Value {
            serde_json::from_str(
                &serde_json::to_string(&read(dir.join(&file)))
                    .unwrap()
                    .replace(dir.to_str().unwrap(), "$RUN"),
            )
            .unwrap()
        };
        assert_eq!(normalized(&online), normalized(&offline));
        assert_eq!(normalized(&online), normalized(&replay));
    }
    // Old history cannot recover per-round boundaries. Do not invent five rounds.
    let mut h = read(&history);
    h.as_object_mut().unwrap().remove("boundaries");
    let legacy = base.join("legacy.json");
    std::fs::write(&legacy, serde_json::to_vec(&h).unwrap()).unwrap();
    let old = base.join("old");
    invoke(&["--replay-history", legacy.to_str().unwrap()], &old);
    let manifest = read(old.join("rounds/manifest.json"));
    assert_eq!(manifest.as_array().unwrap().len(), 1);
    assert_eq!(manifest[0]["kind"], "final-history-snapshot");
    assert!(manifest[0]["round"].is_null());
    std::fs::remove_dir_all(base).unwrap();
}
