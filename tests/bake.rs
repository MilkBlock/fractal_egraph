use serde_json::Value;
use std::{path::Path, process::Command};
fn cmd(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("EGG_LAYOUT_BINDING_ALGEBRA")
        .env("EGG_LAYOUT_DISCOVER_RECURSION", "1") // frozen mode must ignore this
        .args(args)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) {
    let r = cmd(args);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
}
fn json(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn multisample_bake_frozen_holdouts_and_certified_queries() {
    let dir = std::env::temp_dir().join(format!("bake-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let baked = dir.join("baked");
    ok(&["bake", "experiments/bake/manifest.json", path(&baked)]);
    let mut baked_dsl = egglog::EGraph::default();
    baked_dsl
        .parse_and_run_program(
            None,
            &std::fs::read_to_string(baked.join("baked.egg")).unwrap(),
        )
        .unwrap();
    assert_eq!(baked_dsl.get_size("BakedSequence"), 2);
    assert_eq!(baked_dsl.get_size("ArrayAt"), 0);
    let libpath = baked.join("library.egg");
    let original = std::fs::read(&libpath).unwrap();
    let lib = egg_layout::native_analyze::bake::inspect_library(&libpath).unwrap();
    assert!(!baked.join("library.json").exists());
    let text = std::fs::read_to_string(&libpath).unwrap();
    assert!(text.contains("(bake-library 1"));
    assert!(text.contains("(fractal-rule"));
    assert!(text.contains("; @egg-viz-json"));
    let legacy = dir.join("legacy.json");
    std::fs::write(&legacy, serde_json::to_vec(&lib).unwrap()).unwrap();
    let converted = dir.join("converted.egg");
    ok(&["bake-format", path(&legacy), path(&converted)]);
    assert_eq!(
        egg_layout::native_analyze::bake::inspect_library(&converted).unwrap(),
        lib
    );
    let corrupt_bindings = dir.join("bad-bindings.egg");
    std::fs::write(
        &corrupt_bindings,
        text.replacen("(parent 0", "(parent 1", 1),
    )
    .unwrap();
    assert!(egg_layout::native_analyze::bake::inspect_library(&corrupt_bindings).is_err());
    let harmless = dir.join("commented.egg");
    std::fs::write(
        &harmless,
        format!("; ordinary user note\n{text}\n; (panic \"must never execute\")\n"),
    )
    .unwrap();
    assert_eq!(
        egg_layout::native_analyze::bake::inspect_library(&harmless).unwrap(),
        lib
    );
    assert_eq!(lib["templates"].as_array().unwrap().len(), 2);
    for t in lib["templates"].as_array().unwrap() {
        assert_eq!(t["support"].as_array().unwrap().len(), 2);
        assert!(!t["contract"].is_null());
    }
    let recursive = lib["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["kind"] == "recursive_dag")
        .unwrap();
    let linear = lib["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["kind"] == "linear_extension")
        .unwrap();
    assert!(!linear["array_summary"].is_null());
    assert_eq!(recursive["array_summary"]["kind"], "radix_frontier");

    let used = dir.join("used");
    ok(&[
        "bake-use",
        path(&libpath),
        "experiments/bake/heldout-binary.egg",
        path(&used),
        "--save-history",
    ]);
    let r = json(used.join("use.json"));
    for field in [
        "discovery_calls",
        "templates_added",
        "tier1_nodes_built",
        "tier2_search_runs",
    ] {
        assert_eq!(r[field], 0);
    }
    assert_eq!(r["contract_instances"], 30);
    assert_eq!(r["covered_events"].as_array().unwrap().len(), 90);
    assert_eq!(r["eligible_events"], 93);
    let batches = r["capture_batches"].as_array().unwrap();
    assert!(
        batches
            .iter()
            .any(|b| b["pending_units"].as_u64().unwrap() > 0)
    );
    assert!(
        batches
            .iter()
            .any(|b| b["locally_complete_units"].as_u64().unwrap() > 0)
    );
    assert_eq!(batches.last().unwrap()["pending_units"], 0);
    // Compare against independent full discovery of the exact same capture.
    let full = dir.join("full");
    ok(&[
        "analyze",
        "--replay-history",
        path(&used.join("history.json")),
        "--output",
        path(&full),
    ]);
    let discovered = json(full.join("recursive_patterns.json"));
    let expected: std::collections::BTreeSet<_> = discovered["event_witnesses"]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.parse::<u64>().unwrap())
        .collect();
    let actual: std::collections::BTreeSet<_> = r["covered_events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(actual, expected);

    let counter = dir.join("counter");
    ok(&[
        "bake-use",
        path(&libpath),
        "experiments/bake/heldout-increment.egg",
        path(&counter),
    ]);
    let c = json(counter.join("use.json"));
    assert!(
        c["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["state"] == "linear_run" && m["depth"].as_u64().unwrap() > 100)
    );
    let missing = dir.join("missing");
    ok(&[
        "bake-use",
        path(&libpath),
        "experiments/bake/uncovered.egg",
        path(&missing),
    ]);
    let miss = json(missing.join("use.json"));
    assert_eq!(miss["matches"].as_array().unwrap().len(), 0);
    assert_eq!(miss["templates_added"], 0);
    assert!(!miss["uncovered_events"].as_array().unwrap().is_empty());
    assert_eq!(original, std::fs::read(&libpath).unwrap());

    let id = linear["id"].as_str().unwrap();
    for (i, (start, limit)) in [
        (31_i64, 200_i64),
        (-2, 2),
        (7, 7),
        (8, 3),
        (i64::MAX, i64::MAX),
    ]
    .into_iter()
    .enumerate()
    {
        let out = dir.join(format!("query{i}.json"));
        ok(&[
            "bake-eval",
            path(&libpath),
            id,
            &start.to_string(),
            &limit.to_string(),
            path(&out),
        ]);
        let q = json(out);
        let expected: i64 = if start < limit {
            (start + 1..=limit).sum()
        } else {
            0
        };
        assert_eq!(q["result"]["sum"], format!("(EInt {expected})"));
        assert_eq!(q["tier0_rule_applications"], 0);
        assert_eq!(q["result"]["element_queries"], 0);
        assert_eq!(q["result"]["expanded_prefixes"], 0);
    }
    // Independent actual tier0 enumeration on a held-out program agrees.
    let source = std::fs::read_to_string("experiments/bake/heldout-increment.egg").unwrap();
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None, &source).unwrap();
    let mut sum = 0_i64;
    eg.function_for_each("A", |row| {
        let n = eg.value_to_base::<i64>(row.vals[0]);
        if n > 31 {
            sum += n;
        }
    })
    .unwrap();
    assert_eq!(sum, 19604);
    let frontier = dir.join("frontier.json");
    ok(&[
        "bake-eval",
        path(&libpath),
        recursive["id"].as_str().unwrap(),
        "11",
        "--depth",
        "5",
        path(&frontier),
    ]);
    let f = json(&frontier);
    assert_eq!(f["elements"], 32);
    assert_eq!(
        f["theoretical_basic_applies_for_full_unfolding"],
        r["raw_match_events"]
    );
    assert_eq!(f["result"]["sum"], "(EInt 11760)");
    let mut reference = egglog::EGraph::default();
    reference
        .parse_and_run_program(
            None,
            &std::fs::read_to_string("experiments/bake/heldout-binary.egg").unwrap(),
        )
        .unwrap();
    let mut sum = 0;
    let mut count = 0;
    reference
        .function_for_each("A", |row| {
            let n = reference.value_to_base::<i64>(row.vals[0]);
            if (352..=383).contains(&n) {
                sum += n;
                count += 1;
            }
        })
        .unwrap();
    assert_eq!((sum, count), (11760, 32));
    let large = dir.join("large.json");
    ok(&[
        "bake-eval",
        path(&libpath),
        recursive["id"].as_str().unwrap(),
        "11",
        "--depth",
        "20",
        path(&large),
    ]);
    let f = json(large);
    assert_eq!(f["elements"], 1048576);
    assert_eq!(f["tier0_rule_applications"], 0);
    assert_eq!(f["result"]["element_queries"], 0);
    let overflow = dir.join("overflow.json");
    assert!(
        !cmd(&[
            "bake-eval",
            path(&libpath),
            recursive["id"].as_str().unwrap(),
            "11",
            "--depth",
            "63",
            path(&overflow)
        ])
        .status
        .success()
    );
    assert!(!overflow.exists());
    let noquery = dir.join("noquery.json");
    assert!(
        !cmd(&[
            "bake-eval",
            path(&libpath),
            recursive["id"].as_str().unwrap(),
            "1",
            "20",
            path(&noquery)
        ])
        .status
        .success()
    );
    assert!(!noquery.exists());
    let mut corrupt = lib.clone();
    corrupt["version"] = serde_json::json!(999);
    let bad = dir.join("bad.json");
    std::fs::write(&bad, serde_json::to_vec(&corrupt).unwrap()).unwrap();
    assert!(
        !cmd(&[
            "bake-use",
            path(&bad),
            "experiments/bake/heldout-binary.egg",
            path(&dir.join("bad-use"))
        ])
        .status
        .success()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
