fn run(
    source: &str,
    input: Option<&std::path::Path>,
    out: &std::path::Path,
    mode: &str,
) -> egg_layout::pipeline::Result<serde_json::Value> {
    adaptive::run_with_min_reuse(source, input, out, mode, 1)
}
use egg_layout::reuse_graph::adaptive;
fn dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "adaptive-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
const FINITE: &str = "(datatype Math (V i64) (A Math) (B Math) (C Math)) (rewrite (A x) (B x)) (rewrite (B x) (C x)) (A (V 1)) (run 3) (check (= (A (V 1)) (C (V 1))))";
#[test]
fn observed_closure_is_executable_after_save_and_binding_changes() {
    let d = dir("learn");
    let r = run(FINITE, None, &d, "adaptive").unwrap();
    assert!(r["samples"][0]["bodies"].as_u64().unwrap() > 0);
    let second = FINITE.replace("(V 1)", "(V 9)");
    let out = dir("frozen");
    let reused = run(
        &second,
        Some(&d.join("bake_lib_round1.json")),
        &out,
        "frozen",
    )
    .unwrap();
    assert_eq!(reused["learning_seconds"].as_f64().unwrap() < 0.01, true);
    assert!(
        reused["samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["packing"]["virtual_rows"].as_u64().unwrap() > 0)
    );
    let native = run(&second, None, &dir("native"), "native").unwrap();
    assert_eq!(reused["final"]["enodes"], native["final"]["enodes"]);
    assert_eq!(reused["final"]["eclasses"], native["final"]["eclasses"]);
}
#[test]
fn union_and_external_rows_remain_enabled_after_learning() {
    let s = "(datatype Math (V i64) (A Math) (B Math) (Pair Math Math) (Done Math)) (rewrite (A x) (B x)) (rewrite (Pair (B x) (B x)) (Done x)) (A (V 1)) (A (V 2)) (run 2) (Pair (B (V 1)) (B (V 2))) (union (V 1) (V 2)) (run 3) (check (= (Pair (B (V 1)) (B (V 2))) (Done (V 1))))";
    let out = dir("union");
    run(s, None, &out, "adaptive").unwrap();
    run(
        s,
        Some(&out.join("bake_lib.json")),
        &dir("union-frozen"),
        "frozen",
    )
    .unwrap();
}
#[test]
fn incompatible_environment_is_rejected() {
    let d = dir("env");
    run(FINITE, None, &d, "adaptive").unwrap();
    assert!(
        run(
            &FINITE.replace("(rewrite (B x) (C x))", ""),
            Some(&d.join("bake_lib.json")),
            &dir("bad"),
            "frozen"
        )
        .is_err()
    );
}

#[test]
fn readable_library_is_valid_egglog_and_source_rules_are_replaced() {
    let out = dir("readable");
    let report = run(FINITE, None, &out, "adaptive").unwrap();
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        &std::fs::read_to_string(out.join("bake_lib.egg")).unwrap(),
    )
    .unwrap();
    assert!(
        report["samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["bodies"].as_u64().unwrap() > 0)
    );
}

#[test]
fn identical_actual_bindings_preserve_parametric_closure() {
    let train = "(datatype Math (V i64) (Add Math Math)) (rewrite (Add a b) (Add b a)) (Add (V 1) (V 2)) (run 2)";
    let d = dir("alias");
    run(train, None, &d, "adaptive").unwrap();
    let test = "(datatype Math (V i64) (Add Math Math)) (rewrite (Add a b) (Add b a)) (Add (V 9) (V 9)) (run 2) (check (= (Add (V 9) (V 9)) (Add (V 9) (V 9))))";
    let r = run(
        test,
        Some(&d.join("bake_lib.json")),
        &dir("alias-frozen"),
        "frozen",
    )
    .unwrap();
    assert_eq!(r["final"]["enodes"], 2);
    assert_eq!(r["final"]["eclasses"], 2);
}

#[test]
fn unfinished_symbolic_closure_keeps_native_rule() {
    let s = "(datatype Math (V i64) (Sub Math Math) (F Math) (Add Math Math)) (rewrite (F x) (Add (F (Sub x (V 1))) (V 1))) (F (V 9)) (run 2)";
    let d = dir("unfinished");
    let r = run(s, None, &d, "adaptive").unwrap();
    let b: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.join("bake_lib.json")).unwrap()).unwrap();
    assert!(b["macros"].as_object().unwrap().is_empty());
    assert!(
        b["unfinished"]["0"]
            .as_str()
            .unwrap()
            .contains("unfinished")
    );
    assert!(r["samples"][1]["source_matches"].as_u64().unwrap() > 0);
}

#[test]
fn notification_drain_does_not_remove_constructor_facts() {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype Math (V i64)) (relation Packets (Math)) (Packets (V 1)) (Packets (V 2))",
    )
    .unwrap();
    assert_eq!(eg.drain_notification_relation("Packets").unwrap().len(), 2);
    assert_eq!(eg.get_size("Packets"), 0);
    assert_eq!(eg.get_size("V"), 2);
    assert!(eg.drain_notification_relation("V").is_err());
    eg.parse_and_run_program(None, "(Packets (V 1))").unwrap();
    assert_eq!(eg.drain_notification_relation("Packets").unwrap().len(), 1);
}

#[test]
fn singleton_storage_is_skipped_without_discarding_proved_closures() {
    let single =
        adaptive::run_with_min_reuse(FINITE, None, &dir("single-filter"), "adaptive", 2).unwrap();
    assert_eq!(
        single["samples"].as_array().unwrap().last().unwrap()["packing"]["virtual_rows"],
        0
    );
    let repeated = FINITE.replace("(A (V 1)) (run 3)", "(A (V 1)) (A (V 2)) (run 3)");
    let r = adaptive::run_with_min_reuse(&repeated, None, &dir("repeat-filter"), "adaptive", 2)
        .unwrap();
    assert!(
        r["samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["packing"]["virtual_rows"].as_u64().unwrap() > 0)
    );
}
