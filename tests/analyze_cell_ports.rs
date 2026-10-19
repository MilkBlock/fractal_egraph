use serde_json::Value;

fn audit(report: &Value, history: &Value) {
    let d = &report["discovery"];
    let outcomes = d["outcomes"].as_array().unwrap();
    let records = history["records"].as_array().unwrap();
    assert_eq!(
        outcomes.len(),
        records.len(),
        "no silently omitted capture records"
    );
    assert_eq!(d["total_records"], records.len());
    assert_eq!(d["accounted_records"], records.len());
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut reasons = std::collections::BTreeMap::<String, usize>::new();
    for (i, (o, r)) in outcomes.iter().zip(records).enumerate() {
        assert_eq!(o["record"], i);
        assert_eq!(o["event"], r["id"]);
        let reason = o["reason"].as_str().unwrap();
        let result = &d["contract"]["outcomes"][reason];
        let category = result["category"].as_str().expect("known outcome");
        assert!(!result["next_experiment"].as_str().unwrap().is_empty());
        assert!(
            result["gate"].as_u64().unwrap()
                <= d["contract"]["gates"].as_array().unwrap().len() as u64
        );
        *counts.entry(category.into()).or_default() += 1;
        *reasons.entry(reason.into()).or_default() += 1;
        if category == "recognized" {
            assert_eq!(reason, "indexed");
            assert!(report["families"].as_array().unwrap().iter().any(|f| {
                f["id"] == o["family"]
                    && f["witnesses"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|w| w["record"] == o["record"])
            }));
        }
    }
    assert_eq!(d["counts"], serde_json::to_value(counts).unwrap());
    assert_eq!(d["by_reason"], serde_json::to_value(reasons).unwrap());
}
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
        let report: Value =
            serde_json::from_slice(&std::fs::read(out.join("cell-egraph.json")).unwrap()).unwrap();
        let history: Value =
            serde_json::from_slice(&std::fs::read(out.join("history.json")).unwrap()).unwrap();
        audit(
            &report["repetition_families"]["port_progressions"],
            &history,
        );
        report
    };
    let full = run("full", &source);
    let report = &full["repetition_families"]["port_progressions"];
    assert_eq!(report["discovery"]["counts"]["recognized"], 2);
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
    let rejected: Vec<_> = bad["discovery"]["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["reason"] == "source closures not certified equivalent")
        .collect();
    assert_eq!(rejected.len(), 2);
    assert!(rejected.iter().all(|o| {
        o["target_unit"].is_number()
            && o["reads"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["status"] == "verified")
    }));
    let disconnected = run(
        "disconnected",
        &source.replace("(R (Atom 17) (Atom 2))", "(R (Atom 51) (Atom 64))"),
    );
    let disconnected = &disconnected["repetition_families"]["port_progressions"];
    assert!(disconnected["families"].as_array().unwrap().is_empty());
    assert!(
        disconnected["discovery"]["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["read_count"] != 2),
        "independent repeated bodies do not invent adjacency"
    );
    let branch = run(
        "branch",
        &source.replace("(run 4)", "(R (Atom 17) (Atom 99))\n(run 4)"),
    );
    let branch = &branch["repetition_families"]["port_progressions"];
    assert!(
        branch["discovery"]["by_reason"]["branching"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
    for o in branch["discovery"]["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["reason"] == "branching")
    {
        assert!(
            o["family"].is_number(),
            "nonlinear overlap evidence must remain inspectable"
        );
    }
    let early = run("early", &source.replace("(run 4)", "(run 1)"));
    let early = &early["repetition_families"]["port_progressions"]["discovery"];
    assert!(
        early["total_records"].as_u64().unwrap()
            < report["discovery"]["total_records"].as_u64().unwrap()
    );
    assert_eq!(early["counts"]["recognized"].as_u64().unwrap_or(0), 0);
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
