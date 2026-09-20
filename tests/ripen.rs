use std::{fs, process::Command};
fn run(source: &str, out: &std::path::Path, budget: usize) -> serde_json::Value {
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "ripen",
            source,
            out.to_str().unwrap(),
            "--max-rounds",
            &budget.to_string(),
        ])
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    serde_json::from_slice(&fs::read(out.join("ripen.json")).unwrap()).unwrap()
}
#[test]
fn native_ripen_closes_after_union_and_feeds_replayable_tier1() {
    let base = std::env::temp_dir().join(format!("ripen-tests-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let dir = base.join("closed");
    let r = run("experiments/ripen/union-entry.egg", &dir, 8);
    assert_eq!(r["ripen"]["state"], "Closed");
    assert_eq!(r["checks"], "passed");
    assert_eq!(r["tier1"]["ripen"], r["ripen"]);
    assert!(r["ripen"]["round"].as_u64().unwrap() >= 3);
    assert!(!r["ripen"]["updated"].as_bool().unwrap());
    assert!(r["imported_applies"].as_u64().unwrap() > 0);
    let mut eg = egglog::EGraph::default();
    let source = fs::read_to_string(dir.join("ripened.egg")).unwrap();
    let commands = eg.parse_program(None, &source).unwrap();
    eg.run_program(commands).unwrap();
    let replay = base.join("replay");
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "analyze",
            "--replay-history",
            dir.join("history.json").to_str().unwrap(),
            "--output",
            replay.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    for item in fs::read_dir(dir.join("rounds")).unwrap() {
        let path = item.unwrap().path();
        assert_eq!(
            fs::read(&path).unwrap(),
            fs::read(replay.join("rounds").join(path.file_name().unwrap())).unwrap()
        );
    }
    let limited = run(
        "experiments/ripen/union-entry.egg",
        &base.join("limited"),
        1,
    );
    assert_eq!(limited["ripen"]["state"], "Suspended");
    assert_eq!(limited["checks"], "deferred");
    let growing = run(
        "experiments/ripen/growing-entry.egg",
        &base.join("growing"),
        4,
    );
    assert_eq!(growing["ripen"]["state"], "Suspended");
    let invalid = base.join("invalid.egg");
    fs::write(&invalid, "(datatype E (A))\n(rewrite (A) (A) :subsume)").unwrap();
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "ripen",
            invalid.to_str().unwrap(),
            base.join("invalid").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!p.status.success());
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn extracts_a_real_use_without_seeding_its_outputs() {
    let base = std::env::temp_dir().join(format!("ripen-use-tests-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let source = base.join("source.egg");
    let mut text = String::from(
        "(datatype Expr (V String) (A Expr) (B Expr) (C Expr) (D Expr))\n(rewrite (A x) (B x))\n(rewrite (B x) (C x))\n(rewrite (C x) (D x))\n",
    );
    for i in 0..8 {
        text += &format!("(A (V \"v{i}\"))\n");
    }
    text += "(run 5)\n";
    fs::write(&source, text).unwrap();
    let captured = base.join("capture");
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "analyze",
            "--recapture-tier0",
            "--source",
            source.to_str().unwrap(),
            "--save-history",
            "--output",
            captured.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let analysis: serde_json::Value =
        serde_json::from_slice(&fs::read(captured.join("analysis.json")).unwrap()).unwrap();
    let reuse = &analysis["layers"]["reuse"];
    let uid = reuse["uses"]
        .as_array()
        .unwrap()
        .iter()
        .position(|u| {
            let steps =
                reuse["templates"][u["template"].as_u64().unwrap() as usize]["pattern"]["steps"]
                    .as_array()
                    .unwrap();
            steps.len() == 2
                && reuse["schemas"][steps[0]["schema"].as_u64().unwrap() as usize]["rule"]
                    .as_str()
                    .unwrap()
                    .contains("(A x)")
        })
        .expect("a real two-apply Use");
    let out = base.join("from-use");
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "ripen-use",
            captured.join("history.json").to_str().unwrap(),
            &uid.to_string(),
            out.to_str().unwrap(),
            "--max-rounds",
            "8",
        ])
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("result.json")).unwrap()).unwrap();
    assert_eq!(result["checks"], "passed");
    assert_eq!(result["ripen"]["state"], "Closed");
    assert_eq!(result["ripen"]["origin"]["use_id"], uid);
    assert_eq!(result["origin"]["parameters"][0]["sort"], "Expr");
    assert_eq!(result["origin"]["initial_tables"]["B"], 0);
    assert_eq!(result["origin"]["initial_tables"]["C"], 0);
    assert_eq!(result["origin"]["original_use_tables"]["D"], 0);
    assert_eq!(result["tables"]["D"], 1);
    let mut eg = egglog::EGraph::default();
    let commands = eg
        .parse_program(
            None,
            &fs::read_to_string(out.join("validate-use.egg")).unwrap(),
        )
        .unwrap();
    eg.run_program(commands).unwrap();
    let mut bad: serde_json::Value =
        serde_json::from_slice(&fs::read(captured.join("history.json")).unwrap()).unwrap();
    for token in bad["tokens"].as_array_mut().unwrap() {
        if token["sort"] == "Expr" {
            token["sort"] = serde_json::json!("i64");
        }
    }
    let path = base.join("bad.json");
    fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "ripen-use",
            path.to_str().unwrap(),
            &uid.to_string(),
            base.join("bad").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!p.status.success());
    assert!(!base.join("bad").exists());
    fs::remove_dir_all(base).unwrap();
}
