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
