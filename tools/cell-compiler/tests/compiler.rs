use serde_json::{Value as J, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
const SOURCE: &str = r#"
; cell-choice/v1 {"function":"pick","tie_break":"structural"}
(datatype E (Leaf i64) (Wrap E) (Left E) (Right E))
(function pick (i64) E :merge old)
(ruleset compute)
(rule ((= t (Wrap (Leaf i)))) ((Left t) (set (pick i) t)) :ruleset compute :name "left")
(rule ((= t (Wrap (Leaf i)))) ((Right t)) :ruleset compute :name "right")
(Wrap (Leaf 7))
(run-schedule (saturate compute))
(check (= (pick 7) (Wrap (Leaf 7))) (= (Left (Wrap (Leaf 7))) (Left (pick 7))) (= x (Right (pick 7))))
"#;
fn command(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"))
        .args(args)
        .output()
        .unwrap()
}
fn pass(args: &[&str]) -> Output {
    let output = command(args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn json_file(p: &Path) -> J {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
#[test]
fn source_to_checked_program_and_bench() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("check.egg");
    fs::write(&src, SOURCE).unwrap();
    let out = tmp.path().join("artifact");
    pass(&[
        "compile",
        "--source",
        src.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let artifact = json_file(&out.join("program.json"));
    let selection = json_file(&out.join("marker-selection.json"));
    assert!(selection["selected"].as_u64().unwrap() > 0);
    assert!(
        artifact["source_core"]["structural_key"]["choices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["function"] == "pick")
    );
    let mut previous = None;
    for lane in ["baseline", "nodes", "filtered"] {
        let b = pass(&[
            "bench",
            "--program",
            out.join("program.json").to_str().unwrap(),
            "--lane",
            lane,
            "--repeat",
            "2",
            "--verify-stages",
        ]);
        let measured: J = serde_json::from_slice(&b.stdout).unwrap();
        assert!(measured["counts"]["all_candidate_visits"].is_number());
        if let Some(ref before) = previous {
            assert_eq!(before, &measured["final_state_sha256"])
        }
        previous = Some(measured["final_state_sha256"].clone());
    }
    let manifest = json_file(&out.join("manifest.json"));
    let before = manifest["program_sha256"].clone();
    assert!(
        !command(&[
            "compile",
            "--source",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(before, json_file(&out.join("program.json"))["sha256"]);
    let native = |s: &str| {
        let mut eg = egglog::EGraph::default();
        egg_layout::structural_key::source::install(&mut eg, s).unwrap();
        eg.parse_and_run_program(None, s).unwrap();
        ["Leaf", "Wrap", "Left", "Right", "pick"].map(|n| eg.get_size(n))
    };
    assert_eq!(
        native(&fs::read_to_string(out.join("source_sk.egg")).unwrap()),
        native(&fs::read_to_string(out.join("source_sk_mk.egg")).unwrap())
    );
    let mut costs = serde_json::Map::new();
    for g in selection["offered_fragments"].as_array().unwrap() {
        let saved = g["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| (n.as_str().unwrap().into(), json!(-1)))
            .collect::<serde_json::Map<_, _>>();
        costs.insert(
            g["key"].as_str().unwrap().into(),
            json!({"maintenance_ns":100,"saved_search_ns_by_rule":saved}),
        );
    }
    let profile = tmp.path().join("profile.json");
    fs::write(&profile,json!({"schema":"marker-cost-profile/v1","source_sha256":manifest["files"]["source_sk.egg"],"markers":costs}).to_string()).unwrap();
    let measured = tmp.path().join("profiled");
    pass(&[
        "compile",
        "--source",
        src.to_str().unwrap(),
        "--output",
        measured.to_str().unwrap(),
        "--profile",
        profile.to_str().unwrap(),
    ]);
    assert_eq!(
        json_file(&measured.join("marker-selection.json"))["selected"],
        0
    );
}
#[test]
fn incompatible_training_does_not_publish_output() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("source");
    let train = tmp.path().join("training");
    let out = tmp.path().join("artifact");
    fs::write(&target, SOURCE).unwrap();
    fs::write(&train, "(datatype E (Different)) (Different) (run 1)").unwrap();
    let result = command(&[
        "compile",
        "--source",
        target.to_str().unwrap(),
        "--bake-source",
        train.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Bake declaration differs"));
    assert!(!out.exists());
}

#[test]
fn witnessed_handoff_is_bound_to_the_compiled_recipe() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("chain.egg");
    fs::write(
        &src,
        r#"(datatype E (Seed i64) (A E) (B E))
(ruleset calc)
(rule ((= x (Seed i))) ((A x)) :ruleset calc :name "start")
(rule ((= a (A x))) ((B a)) :ruleset calc :name "finish")
(Seed 3)
(run-schedule (saturate calc))
(check (= x (B (A (Seed 3)))))"#,
    )
    .unwrap();
    let compile = |out: &str, extra: &[&str]| {
        let dir = tmp.path().join(out);
        let mut args = vec![
            "compile",
            "--source",
            src.to_str().unwrap(),
            "--output",
            dir.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        command(&args)
    };
    let initial = compile("initial", &[]);
    assert!(
        initial.status.success(),
        "{}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let recipe = json_file(&tmp.path().join("initial/recipe-program.json"));
    let rules = recipe["rules"].as_array().unwrap();
    let rule = |n: &str| {
        rules
            .iter()
            .position(|r| r["metadata"]["name"] == n)
            .unwrap()
    };
    let evidence = tmp.path().join("handoffs.json");
    fs::write(
        &evidence,
        json!({"recipe_sha256":recipe["sha256"],"pairs":[[rule("start"),rule("finish")]]})
            .to_string(),
    )
    .unwrap();
    let args = ["--composition-evidence", evidence.to_str().unwrap()];
    let ok = compile("with-evidence", &args);
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let program = json_file(&tmp.path().join("with-evidence/program.json"));
    assert!(
        !program["compositions"]["edges"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::write(&evidence, r#"{"recipe_sha256":"stale","pairs":[]}"#).unwrap();
    assert!(!compile("invalid", &args).status.success());
    assert!(!tmp.path().join("invalid").exists());
}

#[test]
fn a_late_bridge_wakes_a_rule_with_an_empty_join_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("late-bridge.egg");
    fs::write(
        &source,
        r#"
(relation Starts (i64))
(relation Bridge (i64 i64))
(relation Ends (i64))
(relation Hit (i64))
(ruleset search)
(ruleset insert)
(rule ((Starts x) (Bridge x y) (Ends y)) ((Hit x)) :ruleset search)
(rule ((Starts x) (Ends y)) ((Bridge x y)) :ruleset insert)
(Starts 10) (Bridge 20 30) (Ends 40)
(run-schedule (seq (run search) (run insert) (run search)))
(check (Hit 10))
"#,
    )
    .unwrap();
    let output = tmp.path().join("compiled");
    pass(&[
        "compile",
        "--source",
        source.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    let result = pass(&[
        "bench",
        "--program",
        output.join("program.json").to_str().unwrap(),
        "--lane",
        "filtered",
        "--repeat",
        "1",
        "--verify-stages",
    ]);
    let report: J = serde_json::from_slice(&result.stdout).unwrap();
    assert!(report["counts"]["empty_join_skips"].as_u64().unwrap() > 0);
}

#[test]
fn reused_constructor_prefix_preserves_later_cell_identities() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("constructor-prefix.egg");
    fs::write(
        &src,
        r#"
(datatype E (Seed i64) (A E) (B E) (C E))
(ruleset build)
(rule ((= s (Seed i))) ((C (B (A (Seed i)))) (A (Seed i))) :ruleset build)
(A (Seed 1))
(C (B (A (Seed 2))))
(Seed 3)
(run-schedule (run build))
(check (= x (C (B (A (Seed 1))))) (= y (C (B (A (Seed 3))))))
"#,
    )
    .unwrap();
    let out = tmp.path().join("compiled");
    pass(&[
        "compile",
        "--source",
        src.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let program = out.join("program.json");
    let report: J = serde_json::from_slice(
        &pass(&[
            "bench",
            "--program",
            program.to_str().unwrap(),
            "--lane",
            "baseline",
            "--repeat",
            "1",
        ])
        .stdout,
    )
    .unwrap();
    let combined: J = serde_json::from_slice(
        &pass(&[
            "bench",
            "--program",
            program.to_str().unwrap(),
            "--lane",
            "baseline",
            "--modules",
            "static-aliases",
            "--repeat",
            "2",
            "--verify-stages",
        ])
        .stdout,
    )
    .unwrap();
    let base = &report["counts"];
    let counts = &combined["counts"];
    // Seed(2) takes the full-hit path; Seed(1) and Seed(3) miss at different
    // positions. Later constructors read earlier aliases after materialization.
    assert_eq!(counts["static_eliminated"], 9);
    assert_eq!(counts["lazy_prefix_eliminated"], 5);
    assert_eq!(
        base["constructor_requests"].as_u64().unwrap()
            - counts["constructor_requests"].as_u64().unwrap(),
        9
    );
    assert_eq!(base["rhs_frames_avoided"], 1);
    assert_eq!(counts["rhs_frames_avoided"], base["rhs_frames_avoided"]);
    assert_eq!(counts["cells_created"], base["cells_created"]);
    assert_eq!(combined["final_state_sha256"], report["final_state_sha256"]);

    // Observation retains eager frames to record physical constructor witnesses.
    // Compare every boundary including the IDs of facts created after a full hit.
    let stages = |mode| {
        let run = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"))
            .arg("internal-stages")
            .arg(&program)
            .env("CELL_CPU_KNOWN_RESULTS", mode)
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        serde_json::from_slice::<Vec<String>>(&run.stdout).unwrap()
    };
    assert_eq!(stages("off"), stages("observe"));
}

#[test]
fn free_join_preserves_cyclic_matches_and_late_rows_under_collisions() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("triangle.egg");
    // Many unrelated driver keys make the reverse plan profitable with exact
    // fingerprints. With forced collisions the same shape must fall back safely.
    let noise = (10..106)
        .map(|x| format!("(R {x} {x})"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        &source,
        format!(
            r#"
(relation R (i64 i64))
(relation S (i64 i64))
(relation T (i64 i64))
(relation Hit (i64 i64 i64))
(ruleset find)
(ruleset extend)
(rule ((R x y) (S y z) (T z x)) ((Hit x y z)) :ruleset find)
(rule ((R x y) (= x 1)) ((T 8 x)) :ruleset extend)
{noise}
(R 1 2) (R 1 3) (R 4 2)
(S 2 7) (S 2 8) (S 3 7) (S 3 9)
(T 7 1) (T 7 4) (T 9 4)
(run-schedule (seq (run find) (run extend) (run find)))
(check (Hit 1 2 7) (Hit 4 2 7) (Hit 1 3 7) (Hit 1 2 8))
"#
        ),
    )
    .unwrap();
    let out = tmp.path().join("artifact");
    pass(&[
        "compile",
        "--emit-free-join-entry",
        "--source",
        source.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let pg = out.join("program.json");
    let trial = pass(&[
        "bench",
        "--program",
        pg.to_str().unwrap(),
        "--lane",
        "free-join",
        "--repeat",
        "1",
        "--verify-stages",
    ]);
    let counters: J = serde_json::from_slice(&trial.stdout).unwrap();
    assert!(counters["counts"]["free_join"]["probes"].as_u64().unwrap() > 0);
    assert!(
        counters["counts"]["free_join"]["subtrie_uses"]
            .as_u64()
            .unwrap()
            > 0
    );

    let raw = json_file(&pg);
    assert!(
        !raw["free_join_entry"]["projections"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut corrupt = raw.clone();
    corrupt["free_join_entry"]["projections"][0]["table"] = json!(usize::MAX);
    assert!(eggfuse_cell_runtime::program::Program::parse(corrupt).is_err());
    let mut legacy = raw.clone();
    legacy.as_object_mut().unwrap().remove("free_join_entry");
    legacy.as_object_mut().unwrap().remove("sha256");
    legacy["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&legacy));
    let old = tmp.path().join("old.json");
    fs::write(&old, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let run_entry = |path: &Path| {
        let result = pass(&[
            "bench",
            "--program",
            path.to_str().unwrap(),
            "--lane",
            "free-join-entry",
            "--repeat",
            "1",
            "--verify-stages",
        ]);
        serde_json::from_slice::<J>(&result.stdout).unwrap()
    };
    let compiled = run_entry(&pg);
    assert!(
        compiled["counts"]["free_join"]["entry_probes"]
            .as_u64()
            .unwrap()
            > 0
    );
    let derived = run_entry(&old);
    assert_eq!(compiled["counts"], derived["counts"]);
    assert_eq!(
        compiled["final_state_sha256"],
        derived["final_state_sha256"]
    );
    let stages = |mode| {
        let output = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"))
            .args(["internal-stages", pg.to_str().unwrap()])
            .env("CELL_CPU_SHARED_NODE_SCHEDULE", "1")
            .env("CELL_CPU_SIEVE", "1")
            .env("CELL_CPU_JOIN_INTERSECTION", "1")
            .env("CELL_CPU_HASH_BITS", "0")
            .env("CELL_CPU_FREE_JOIN", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    let expected = stages("0");
    assert_eq!(expected, stages("1"));
    assert_eq!(expected, stages("entry"));
    assert_eq!(expected, stages("planned"));
    let planned = pass(&[
        "bench",
        "--program",
        pg.to_str().unwrap(),
        "--lane",
        "free-join-planned",
        "--repeat",
        "3",
        "--verify-stages",
    ]);
    let planned: J = serde_json::from_slice(&planned.stdout).unwrap();
    assert!(
        planned["counts"]["free_join"]["reverse_stages"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        compiled["final_state_sha256"],
        planned["final_state_sha256"]
    );
}

#[test]
fn benchmark_modules_report_effective_congruence_and_compose() {
    let tmp = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("benchmarks/runtime-modules/congruence-chain.egg");
    let out = tmp.path().join("chain");
    pass(&[
        "compile",
        "--source",
        source.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let program = out.join("program.json");
    let run = |modules: &str| {
        command(&[
            "bench",
            "--program",
            program.to_str().unwrap(),
            "--lane",
            "free-join-planned",
            "--modules",
            modules,
            "--repeat",
            "2",
            "--verify-stages",
        ])
    };
    let enabled = run("union-graft,stage-spine,local-congruence");
    assert!(
        enabled.status.success(),
        "{}",
        String::from_utf8_lossy(&enabled.stderr)
    );
    let report: J = serde_json::from_slice(&enabled.stdout).unwrap();
    let counts = &report["counts"];
    assert_eq!(
        report["modules_requested"],
        json!(["local-congruence", "stage-spine", "union-graft"])
    );
    assert_eq!(counts["local_congruence"]["stats"]["proven_unions"], 32);
    assert_eq!(counts["union_graft"]["enabled"], true);
    assert_eq!(counts["stage_spine"]["enabled"], true);
    for invalid in ["typo", "stage-spine,stage-spine"] {
        assert!(!run(invalid).status.success());
    }
}

#[test]
fn spine_routes_live_witnesses_and_plans_the_rest_of_the_same_query() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("routed.egg");
    // A(1) comes from a committed producer. Bootstrap A(2) shares its pivot
    // but has no cursor. A(99) gains a partner only after A(1) is retired.
    fs::write(
        &src,
        r#"(relation Input (i64))
(relation A (i64))
(relation B (i64))
(relation Hit (i64))
(ruleset publish)
(ruleset consume)
(ruleset later)
(rule ((Input x)) ((A x)) :ruleset publish)
(rule ((A x) (B x)) ((Hit x)) :ruleset consume)
(rule ((Input x)) ((B 99) (delete (A x))) :ruleset later)
(Input 1)
(A 2) (A 99)
(B 1) (B 2)
(run-schedule (seq (run publish) (run consume) (run later) (run consume)))
(check (Hit 1) (Hit 2) (Hit 99))
"#,
    )
    .unwrap();
    let out = tmp.path().join("artifact");
    pass(&[
        "compile",
        "--emit-free-join-entry",
        "--source",
        src.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let pg = out.join("program.json");
    let run = pass(&[
        "bench",
        "--program",
        pg.to_str().unwrap(),
        "--lane",
        "free-join-planned",
        "--modules",
        "stage-spine",
        "--repeat",
        "2",
        "--verify-stages",
    ]);
    let report: J = serde_json::from_slice(&run.stdout).unwrap();
    let counts = &report["counts"];
    let spine = &counts["stage_spine"];
    assert!(spine["mixed_routed_queries"].as_u64().unwrap() > 0);
    assert!(spine["resumed_steps"].as_u64().unwrap() > 0);
    assert!(counts["free_join"]["entry_probes"].as_u64().unwrap() > 0);
    assert_eq!(
        counts["consumer_seed_deliveries"].as_u64().unwrap(),
        spine["routed_spine_seeds"].as_u64().unwrap()
            + spine["routed_matcher_seeds"].as_u64().unwrap()
    );
    let stages = |enabled: &str, join: &str, sieve: &str, shared: &str| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("CELL_CPU_") {
                cmd.env_remove(key);
            }
        }
        let run = cmd
            .args(["internal-stages", pg.to_str().unwrap()])
            .env("CELL_CPU_SHARED_NODE_SCHEDULE", shared)
            .env("CELL_CPU_SIEVE", sieve)
            .env("CELL_CPU_JOIN_INTERSECTION", "1")
            .env("CELL_CPU_FREE_JOIN", join)
            .env("CELL_CPU_HASH_BITS", "0")
            .env("CELL_CPU_STAGE_SPINE", enabled)
            .env("CELL_CPU_STAGE_SPINE_RECEIPTS", "1")
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        run.stdout
    };
    for (join, sieve, shared) in [("planned", "1", "1"), ("0", "0", "1"), ("1", "1", "0")] {
        assert_eq!(
            stages("0", join, sieve, shared),
            stages("1", join, sieve, shared)
        );
    }
}

#[test]
fn spine_coalesces_canonical_anchor_keys_after_all_effects() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("anchors.egg");
    // The two A anchors acquire one canonical key during repair. The last
    // producer is the shallower one. A third receipt is deleted before publish.
    fs::write(
        &source,
        r#"(datatype K (I i64))
(relation Start (K))
(relation Mid (K))
(relation A (K))
(relation Need (K))
(relation Hit (K))
(ruleset first)
(ruleset emit)
(ruleset read)
(rule ((Start x)) ((Mid x)) :ruleset first)
(rule ((Mid x)) ((A x)) :ruleset emit)
(rule ((= y (I 2))) ((A y)) :ruleset emit)
(rule ((= a (I 1)) (= b (I 2))) ((union a b)) :ruleset emit)
(rule ((= dead (I 3))) ((delete (A dead))) :ruleset emit)
(rule ((A x) (Need x)) ((Hit x)) :ruleset read)
(Start (I 1)) (Start (I 3)) (Need (I 2)) (Need (I 3))
(run-schedule (seq (run first) (run emit) (run read)))
(check (Hit (I 1)))
"#,
    )
    .unwrap();
    let out = tmp.path().join("artifact");
    pass(&[
        "compile",
        "--source",
        source.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let pg = out.join("program.json");
    let measured = pass(&[
        "bench",
        "--program",
        pg.to_str().unwrap(),
        "--lane",
        "free-join-planned",
        "--modules",
        "stage-spine",
        "--repeat",
        "2",
        "--verify-stages",
    ]);
    let report: J = serde_json::from_slice(&measured.stdout).unwrap();
    let spine = &report["counts"]["stage_spine"];
    assert!(
        spine["captured_anchors"].as_u64().unwrap()
            > spine["distinct_anchor_keys"].as_u64().unwrap()
    );
    assert_eq!(spine["receipt_redirects"], 1);
    assert_eq!(spine["capture_reads"], 1); // the deleted receipt uses the exact-key fallback
    assert_eq!(spine["resumed_steps"], 3);
    assert_eq!(spine["max_chain"], 2);
    assert!(spine["pre_resume_port_lookups"].as_u64().unwrap() > 0);

    let stages = |receipts| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"));
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("CELL_CPU_") {
                cmd.env_remove(k);
            }
        }
        let run = cmd
            .args(["internal-stages", pg.to_str().unwrap()])
            .env("CELL_CPU_SHARED_NODE_SCHEDULE", "1")
            .env("CELL_CPU_SIEVE", "1")
            .env("CELL_CPU_FREE_JOIN", "planned")
            .env("CELL_CPU_JOIN_INTERSECTION", "1")
            .env("CELL_CPU_STAGE_SPINE", "1")
            .env("CELL_CPU_STAGE_SPINE_RECEIPTS", receipts)
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        run.stdout
    };
    assert_eq!(stages("0"), stages("1"));
}

#[test]
fn only_an_applied_continuation_materializes_a_shared_spine_root() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("fanout.egg");
    // Two writers propose both anchors of key 7. Neither anchor of key 8
    // survives repair. Key 9 is probed but fails its guard. Only the two
    // consumers of key 7 resume, sharing a single materialization.
    fs::write(
        &src,
        r#"(relation WriteKey (i64 i64))
(relation DropKey (i64))
(relation L (i64))
(relation R (i64))
(relation SawWrite (i64))
(relation SawLeft (i64))
(relation SawRight (i64))
(function Threshold (i64) i64 :merge (max old new))
(ruleset emit)
(ruleset read)
(rule ((WriteKey serial key)) ((L key) (R key) (SawWrite serial)) :ruleset emit)
(rule ((DropKey key)) ((delete (L key))) :ruleset emit)
(rule ((DropKey key)) ((delete (R key))) :ruleset emit)
(rule ((L key) (= value (Threshold key)) (> value 0)) ((SawLeft key)) :ruleset read)
(rule ((R key) (= value (Threshold key)) (> value 0)) ((SawRight key)) :ruleset read)
(WriteKey 1 7) (WriteKey 2 7) (WriteKey 3 8) (DropKey 8) (WriteKey 4 9)
(set (Threshold 7) 1) (set (Threshold 9) -1)
(run-schedule (seq (run emit) (run read)))
"#,
    )
    .unwrap();
    let result = pass(&[
        "bench",
        "--source",
        src.to_str().unwrap(),
        "--lane",
        "free-join-planned",
        "--modules",
        "stage-spine",
        "--repeat",
        "2",
        "--verify-stages",
    ]);
    let report: J = serde_json::from_slice(&result.stdout).unwrap();
    let spine = &report["counts"]["stage_spine"];
    assert_eq!(spine["root_attempts"], 4);
    assert_eq!(spine["root_handles_created"], 2);
    assert_eq!(spine["pre_resume_port_lookups"], 4);
    assert_eq!(spine["port_cache_fills"], 0);
    assert_eq!(spine["captured_anchors"], 8);
    assert_eq!(spine["distinct_anchor_keys"], 6);
    assert_eq!(spine["anchor_publications"], 4);
    assert_eq!(spine["contexts_created"], 1);
    assert_eq!(spine["resumed_steps"], 2);
    assert_eq!(spine["max_chain"], 2);
}

#[test]
fn elided_anchors_use_the_proved_fact_and_recheck_visibility_after_repair() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("alias-receipts.egg");
    // F(I(1)) and F(I(2)) share a result but have different keys and only
    // the second becomes hidden. G(F(x)) aliases an earlier RHS after normalization.
    fs::write(
        &src,
        r#"(datatype E (I i64) (F E) (G E))
(relation Input (E))
(relation SeenF (E))
(relation SeenG (E))
(ruleset emit)
(ruleset read)
(rule ((Input x) (= f (F x))) ((G f) (G (F x))) :ruleset emit)
(rule ((= dead (I 2))) ((subsume (F dead))) :ruleset emit)
(rule ((= f (F x))) ((SeenF x)) :ruleset read)
(rule ((= g (G x))) ((SeenG x)) :ruleset read)
(Input (I 1)) (Input (I 2))
(union (F (I 1)) (F (I 2)))
(run-schedule (seq (run emit) (run read)))
(check (SeenF (I 1)) (SeenG (F (I 1))))
"#,
    )
    .unwrap();
    let result = pass(&[
        "bench",
        "--source",
        src.to_str().unwrap(),
        "--lane",
        "free-join-planned",
        "--modules",
        "stage-spine,static-aliases",
        "--repeat",
        "2",
        "--verify-stages",
    ]);
    let report: J = serde_json::from_slice(&result.stdout).unwrap();
    let counts = &report["counts"];
    let spine = &counts["stage_spine"];
    assert_eq!(counts["static_eliminated"], 4);
    assert_eq!(spine["alias_query_forwards"], 2);
    assert_eq!(spine["alias_effect_forwards"], 2);
    assert_eq!(spine["receipt_rejections"], 1);
    assert_eq!(spine["capture_reads"], 1);
    assert_eq!(spine["resumed_steps"], 2);
}

#[test]
fn physical_plans_preserve_order_and_refresh_range_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("plans.egg");
    let items = (0..80)
        .map(|i| format!("(set (Item 1 {i}) {i})"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&src,format!(r#"(function Item (i64 i64) i64 :merge new)
(function Limit (i64) i64 :merge new)
(function Best (i64) i64 :merge (min new old))
(function High (i64) i64 :merge (max old new))
(function First (i64) i64 :merge old)
(function Total (i64) i64 :merge (+ old new))
(relation Seen (i64 i64))
(relation AtMost (i64)) (relation Above (i64)) (relation AtLeast (i64))
(relation Tick ())
(ruleset search)
(ruleset change)
(rule ((= k (Limit g)) (= x (Item g id)) (< x k)) ((Seen id x)) :ruleset search)
(rule ((= k (Limit g)) (= x (Item g id)) (<= x k)) ((AtMost id)) :ruleset search)
(rule ((= k (Limit g)) (= x (Item g id)) (< k x)) ((Above id)) :ruleset search)
(rule ((= k (Limit g)) (= x (Item g id)) (<= k x)) ((AtLeast id)) :ruleset search)
(rule ((Tick)) ((set (Item 1 58) -2) (delete (Item 1 2)) (set (Limit 1) 5) (set (Best 0) 1) (set (High 0) 5) (set (First 0) 8) (set (Total 0) 4)) :ruleset change)
{items}
(set (Limit 1) 4)
(set (Best 0) 9) (set (Best 0) 2) (set (Best 0) 7)
(set (High 0) -8) (set (High 0) 13) (set (High 0) 4)
(set (First 0) 2) (set (First 0) 3)
(set (Total 0) 2) (set (Total 0) 3)
(Tick)
(run-schedule (seq (run search) (run change) (run search)))
(check (Seen 0 0) (Seen 58 -2) (Seen 4 4) (= (Best 0) 1) (= (High 0) 13) (= (First 0) 2) (= (Total 0) 9))
"#)).unwrap();
    let out = tmp.path().join("artifact");
    pass(&[
        "compile",
        "--emit-free-join-entry",
        "--source",
        src.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let program = out.join("program.json");
    let value = json_file(&program);
    assert!(value["physical_plan"]["ranges"].is_array());
    let mut corrupt = value.clone();
    corrupt["physical_plan"]["dedup"] = json!([]);
    assert_eq!(
        eggfuse_cell_runtime::program::Program::parse(corrupt).unwrap_err(),
        "physical plan differs from typed recipe"
    );
    let result = pass(&[
        "bench",
        "--program",
        program.to_str().unwrap(),
        "--lane",
        "free-join-planned",
        "--modules",
        "range-guards,fd-keys,scalar-merge",
        "--repeat",
        "2",
        "--verify-stages",
    ]);
    let report: J = serde_json::from_slice(&result.stdout).unwrap();
    let p = &report["counts"]["physical"];
    assert!(p["range"]["builds"].as_u64().unwrap() > 0);
    assert!(p["range"]["changed_entries"].as_u64().unwrap() >= 2);
    assert!(p["range"]["broad_fallbacks"].as_u64().unwrap() > 0);
    let range = &p["range"];
    assert!(range["partial_domains"].as_u64().unwrap() > 0);
    assert!(
        range["mixed_prefixes"].as_u64().unwrap() >= range["partial_domains"].as_u64().unwrap()
    );
    assert!(range["partial_output_rows"].as_u64().unwrap() > 0);
    assert!(
        range["partial_output_rows"].as_u64().unwrap()
            < range["partial_input_rows"].as_u64().unwrap()
    );
    assert!(
        p["range"]["output_rows"].as_u64().unwrap() < p["range"]["input_rows"].as_u64().unwrap()
    );
    assert!(p["dedup_elements"].as_u64().unwrap() < p["dedup_full_elements"].as_u64().unwrap());
    assert!(p["scalar_old_projection"].as_u64().unwrap() > 0);
    assert!(p["scalar_merge_calls"].as_u64().unwrap() > p["scalar_unchanged"].as_u64().unwrap());
    let stages = |flag: &str| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"));
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("CELL_CPU_") {
                cmd.env_remove(k);
            }
        }
        let run = cmd
            .args(["internal-stages", program.to_str().unwrap()])
            .env("CELL_CPU_SHARED_NODE_SCHEDULE", "1")
            .env("CELL_CPU_FREE_JOIN", "planned")
            .env("CELL_CPU_HASH_BITS", "0")
            .env("CELL_CPU_SIEVE", "1")
            .env("CELL_CPU_JOIN_INTERSECTION", "1")
            .env("CELL_CPU_RANGE_GUARDS", flag)
            .env("CELL_CPU_FD_KEYS", flag)
            .env("CELL_CPU_SCALAR_MERGE", flag)
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        run.stdout
    };
    assert_eq!(stages("0"), stages("1"));
}

#[test]
fn producer_slots_follow_run_order_and_reject_changed_wiring() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("slots.egg");
    // F(1) already belongs to another producer. F(2) is removed before
    // the second consume Run. Only 3 retains both effects of make-pair.
    fs::write(
        &src,
        r#"(datatype E (N i64) (F E) (G E) (S E E) (One E))
(ruleset make) (ruleset consume) (ruleset retire)
(rule ((= x (N i))) ((F x) (G x)) :ruleset make :name "make-pair")
(rule ((= g (G x)) (= f (F x))) ((S g f)) :ruleset consume :name "consume-pair")
(rule ((= f (F x))) ((One f)) :ruleset consume :name "consume-one")
(rule ((= x (N 2))) ((delete (F x))) :ruleset retire)
(N 1) (N 2) (N 3)
(F (N 1))
(run-schedule (seq (run consume) (run make) (run retire) (run consume)))
(check (= q (S (G (N 3)) (F (N 3)))) (= q2 (One (F (N 1)))))
"#,
    )
    .unwrap();
    let out = tmp.path().join("artifact");
    pass(&[
        "compile",
        "--source",
        src.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let program = out.join("program.json");
    let bench = |path: &Path, mode: &str| {
        let mut args = vec![
            "bench",
            "--program",
            path.to_str().unwrap(),
            "--lane",
            "free-join-planned",
            "--verify-stages",
            "--repeat",
            "1",
        ];
        if mode == "legacy" {
            args.extend(["--modules", "stage-spine,slot-transfer"]);
        } else {
            args.extend(["--modules", "stage-spine", "--slot-transfer", mode]);
        }
        serde_json::from_slice::<J>(&pass(&args).stdout).unwrap()
    };
    for (modules, mode) in [("stage-spine", "typo"), ("slot-transfer", "parent")] {
        assert!(
            !command(&[
                "bench",
                "--program",
                program.to_str().unwrap(),
                "--modules",
                modules,
                "--slot-transfer",
                mode
            ])
            .status
            .success()
        );
    }
    let compiled = bench(&program, "legacy");
    let counts = &compiled["counts"]["slot_transfer"];
    assert!(counts["spine_atoms"].as_u64().unwrap() > 0);
    assert!(counts["sealed_moves"].as_u64().unwrap() > 0);
    assert!(counts["rhs_parent_bindings"].as_u64().unwrap() > 0);

    for mode in [
        "off",
        "siblings",
        "parent",
        "both",
        "local-only",
        "parent-check",
        "parent-owned",
    ] {
        let trial = bench(&program, mode);
        assert_eq!(trial["final_state_sha256"], compiled["final_state_sha256"]);
        assert_eq!(trial["counts"]["slot_transfer_mode"], mode);
        let actual = &trial["counts"]["slot_transfer"];
        let spine_wires = matches!(mode, "siblings" | "both");
        let eligible_parents = matches!(mode, "parent" | "both" | "parent-check" | "parent-owned");
        assert_eq!(
            actual["spine_atoms"].as_u64().unwrap() > 0,
            spine_wires,
            "{mode}"
        );
        assert_eq!(
            actual["rhs_parent_bindings"].as_u64().unwrap() > 0,
            eligible_parents,
            "{mode}"
        );
        if mode == "both" {
            assert_eq!(trial["counts"], compiled["counts"]);
        }
    }
    let mut raw = json_file(&program);
    let mut tampered = raw.clone();
    let corrupt_first_assignment = |plan: &mut J| {
        for route in plan["multi_stage"]["routes"].as_array_mut().unwrap() {
            for step in route["steps"].as_array_mut().unwrap() {
                if step["assignments"]
                    .as_array()
                    .is_some_and(|xs| !xs.is_empty())
                {
                    step["assignments"][0][0] = json!(usize::MAX);
                    return;
                }
            }
        }
        panic!("compiler omitted producer slot wiring");
    };
    corrupt_first_assignment(&mut tampered);
    match eggfuse_cell_runtime::program::Program::parse(tampered) {
        Err(message) => assert!(
            message.contains("invalid producer slot wiring"),
            "{message}"
        ),
        Ok(_) => panic!("accepted changed producer slot wiring"),
    }
    for route in raw["multi_stage"]["routes"].as_array_mut().unwrap() {
        for step in route["steps"].as_array_mut().unwrap() {
            step.as_object_mut().unwrap().remove("assignments");
        }
    }
    raw.as_object_mut().unwrap().remove("sha256");
    raw["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&raw));
    let legacy = tmp.path().join("legacy.json");
    fs::write(&legacy, serde_json::to_vec(&raw).unwrap()).unwrap();
    let derived = bench(&legacy, "both");
    assert_eq!(compiled["counts"], derived["counts"]);
    assert_eq!(
        compiled["final_state_sha256"],
        derived["final_state_sha256"]
    );
}

const BOUNDED_CHAIN: &str = r#"
(datatype E (N i64) (A E) (B E) (C E) (D E) (F E))
(ruleset one) (ruleset two) (ruleset three) (ruleset four)
(rule ((= a (A x))) ((B a)) :ruleset one :name "ab")
(rule ((= a (B x))) ((C a)) :ruleset two :name "bc")
(rule ((= a (C x))) ((D a)) :ruleset three :name "cd")
(rule ((= a (D x))) ((F a)) :ruleset four :name "df")
(A (N 1)) (A (N 2))
(run-schedule (seq (run one) (run two) (run three) (run four)))
"#;
#[test]
fn native_bake_cuts_before_preparation_and_reopens_the_frontier() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("chain.egg");
    fs::write(&source, BOUNDED_CHAIN).unwrap();
    let bake = |dir: &str, depth: &str, nodes: &str, budget: &str| {
        let out = tmp.path().join(dir);
        let result = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"))
            .args([
                "internal-bake",
                source.to_str().unwrap(),
                out.to_str().unwrap(),
            ])
            .env("EGG_LAYOUT_GRAPHS", "0")
            .env("EGG_LAYOUT_BAKE_MAX_DEPTH", depth)
            .env("EGG_LAYOUT_BAKE_MAX_NODES", nodes)
            .env("EGG_LAYOUT_BAKE_MAX_UNITS", budget)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        json_file(&out.join("analysis.json"))["saturated_rule_composition"].clone()
    };
    let r = bake("depth", "2", "8", "64");
    let cs = &r["cs"];
    assert!(cs["depth_cuts"].as_u64().unwrap() > 0);
    let units = cs["units"].as_array().unwrap();
    assert!(units.iter().all(|u| u["depth"].as_u64().unwrap() <= 2));
    assert!(
        units
            .iter()
            .any(|u| u["coarse"] == json!([4]) && u["depth"] == 1)
    );
    assert!(
        units
            .iter()
            .any(|u| u["coarse"] == json!([6]) && u["depth"] == 2)
    );
    assert!(
        cs["windows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["members"].as_array().unwrap().len() > 1)
            .all(|w| units[w["unit"].as_u64().unwrap() as usize]["depth"] == 2)
    );
    let r = bake("width", "9", "1", "64");
    assert!(r["cs"]["size_cuts"].as_u64().unwrap() > 0);
    assert!(
        r["cs"]["units"]
            .as_array()
            .unwrap()
            .iter()
            .all(|u| u["depth"] == 1)
    );
    let r = bake("total", "4", "8", "2");
    assert_eq!(r["cs"]["units"].as_array().unwrap().len(), 2);
    assert!(r["cs"]["budget_cuts"].as_u64().unwrap() > 0);
    assert_eq!(r["jobs"].as_array().unwrap().len(), 2);
}

#[cfg(feature = "iterative-bake")]
#[test]
fn a_learned_pair_executes_and_becomes_an_operand_in_the_next_round() {
    use eggfuse_cell_runtime::{
        checksum::artifact_digest, compiler::bake_spine_blocks, program::Program,
    };
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("chain.egg");
    let compiled = tmp.path().join("compiled");
    fs::write(&source, BOUNDED_CHAIN).unwrap();
    pass(&[
        "compile",
        "--source",
        source.to_str().unwrap(),
        "--output",
        compiled.to_str().unwrap(),
    ]);
    let p = Program::parse(json_file(&compiled.join("program.json"))).unwrap();
    // Seed single-entry recipes. All binary evidence below comes from real Runs.
    let profile = json!({"schema":"eggfuse-spine-shapes/v1","program_sha256":p.artifact["sha256"],"prefixes":p.multi_stage.stage_entries.iter().map(|(r,a,_)|json!({"edge":[0,0,r,a],"visits":1})).collect::<Vec<_>>()});
    let leaves = bake_spine_blocks(&p, &profile, &p, 32).unwrap();
    let base = leaves["entries"].as_array().unwrap().len();
    let mut raw = p.artifact;
    raw["spine_blocks"] = leaves;
    raw.as_object_mut().unwrap().remove("sha256");
    raw["sha256"] = json!(artifact_digest(&raw));
    let seed = tmp.path().join("seed.json");
    fs::write(&seed, serde_json::to_vec(&raw).unwrap()).unwrap();
    let run = |name: &str, extra: &[&str]| {
        let out = tmp.path().join(name);
        let result = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"))
            .args([
                "iterate",
                "--program",
                seed.to_str().unwrap(),
                "--output",
                out.to_str().unwrap(),
                "--rounds",
                "4",
            ])
            .args(extra)
            .env("CELL_CPU_STAGE_SPINE", "1")
            .env("CELL_CPU_SHARED_NODE_SCHEDULE", "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        out
    };
    let out = run("grown", &["--max-depth", "3", "--new-per-round", "1"]);
    let rounds = json_file(&out.join("rounds.json"));
    let paths = json_file(&out.join("expanded.json"));
    let report = rounds.as_array().unwrap().last().unwrap();
    let path = paths
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["depth"] == 3)
        .expect("a previously learned pair grows to a triple");
    assert!(
        report["work"]["applied_units"][path["id"].to_string()]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(report["work"]["saved_origin_key_slots"].as_u64().unwrap() > 0);
    let pg = json_file(&out.join("program.json"));
    let joins = pg["iterative_bake"]["joins"].as_array().unwrap();
    assert!(
        joins
            .iter()
            .any(|j| j["left"].as_u64().unwrap() >= base as u64
                || j["right"].as_u64().unwrap() >= base as u64)
    );
    let mut bad = pg.clone();
    bad["iterative_bake"]["joins"][0]["left"] = json!(base);
    assert!(Program::parse(bad).is_err());
    let mut bad = pg;
    bad["iterative_bake"]["limits"]["depth"] = json!(2);
    assert!(Program::parse(bad).is_err());
    let stopped = run("no-growth", &["--max-nodes", "1"]);
    let history = json_file(&stopped.join("rounds.json"));
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(history[0]["stop"], "no_profitable_candidate");
    assert_eq!(
        fs::read(out.join("rhs.json")).unwrap(),
        fs::read(stopped.join("rhs.json")).unwrap()
    );
}

#[test]
fn checked_register_plans_join_and_publish_function_writes() {
    use eggfuse_cell_runtime::{checksum::artifact_digest, program::Program};
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("flow.egg");
    let out = tmp.path().join("compiled");
    fs::write(
        &source,
        r#"
(relation Pair (i64 i64))
(relation Activate (i64))
(function Weight (i64) i64 :merge (max old new))
(function Result (i64) i64 :merge (max old new))
(ruleset join) (ruleset update)
(rule ((Pair a b) (= w (Weight b))) ((set (Result a) (+ w 1))) :ruleset join)
(rule ((Activate b)) ((set (Weight b) 12)) :ruleset update)
(Pair 1 3) (Pair 2 3) (Pair 4 5) (set (Weight 3) 7) (Activate 3)
(run-schedule (seq (run join) (run update) (run join)))
(check (= (Result 1) 13) (= (Result 2) 13))
"#,
    )
    .unwrap();
    pass(&[
        "compile",
        "--source",
        source.to_str().unwrap(),
        "--emit-free-join-entry",
        "--output",
        out.to_str().unwrap(),
    ]);
    let file = out.join("program.json");
    let pg = json_file(&file);
    let run = |path: &Path| {
        serde_json::from_slice::<J>(
            &pass(&[
                "bench",
                "--program",
                path.to_str().unwrap(),
                "--lane",
                "free-join-planned",
                "--repeat",
                "1",
                "--verify-stages",
            ])
            .stdout,
        )
        .unwrap()
    };
    let fast = run(&file);
    assert!(fast["counts"]["register_answers"].as_u64().unwrap() > 0);
    assert!(fast["counts"]["single_writes"].as_u64().unwrap() > 0);
    for key in ["single_writes", "row_kernels"] {
        let mut corrupt = pg.clone();
        corrupt[key] = json!([]);
        assert!(Program::parse(corrupt).is_err());
    }
    let mut old = pg;
    for key in ["single_writes", "row_kernels", "sha256"] {
        old.as_object_mut().unwrap().remove(key);
    }
    old["sha256"] = json!(artifact_digest(&old));
    let legacy = tmp.path().join("legacy.json");
    fs::write(&legacy, serde_json::to_vec(&old).unwrap()).unwrap();
    assert_eq!(fast["counts"], run(&legacy)["counts"]);
}

#[cfg(feature = "iterative-bake")]
#[test]
fn switched_rhs_carries_a_two_run_old_fact_and_restarts_on_a_deleted_sibling() {
    use eggfuse_cell_runtime::{
        checksum::artifact_digest,
        compiler::bake_spine_blocks,
        iterative::{Catalog, Limits},
        program::Program,
    };
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("handoff.egg");
    let out = tmp.path().join("compiled");
    fs::write(
        &source,
        r#"
(datatype E (N i64) (Start E) (Left E) (Right E) (Gateway E) (Mid E))
(ruleset init) (ruleset a) (ruleset b) (ruleset c) (ruleset retire)
(rule ((= x (N i))) ((Start x)) :ruleset init :name "start")
(rule ((= s (Start x))) ((Left x) (Right x) (Gateway x)) :ruleset a :name "producer")
(rule ((= g (Gateway x))) ((Mid x)) :ruleset b :name "bridge")
(rule ((= m (Mid x))) ((Left x) (Right x)) :ruleset c :name "consume")
(rule ((= x (N 2))) ((delete (Right x))) :ruleset retire)
(N 1) (N 2)
(run-schedule (seq (run init) (run a) (run b) (run retire) (run c)))
"#,
    )
    .unwrap();
    pass(&[
        "compile",
        "--source",
        source.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let p = Program::parse(json_file(&out.join("program.json"))).unwrap();
    let rules = p.artifact["recipe"]["rules"].as_array().unwrap();
    let id = |name| {
        rules
            .iter()
            .position(|r| r["metadata"]["name"] == name)
            .unwrap()
    };
    let (a, b, c) = (id("producer"), id("bridge"), id("consume"));
    let op = |r: usize, t: &str| {
        rules[r]["effects"]
            .as_array()
            .unwrap()
            .iter()
            .position(|v| v["call"]["table"] == t)
            .unwrap()
    };
    let (gate, mid) = (op(a, "Gateway"), op(b, "Mid"));
    let prefixes = [a, b, c].map(|r| json!({"edge":[0,0,r,0],"visits":2}));
    let profile = json!({"schema":"eggfuse-spine-shapes/v1","program_sha256":p.artifact["sha256"],"prefixes":prefixes});
    let leaves = bake_spine_blocks(&p, &profile, &p, 3).unwrap();
    let ids = |r| {
        leaves["entries"]
            .as_array()
            .unwrap()
            .iter()
            .position(|e| e["consumer"] == r)
            .unwrap()
    };
    let mut raw = p.artifact.clone();
    raw["spine_blocks"] = leaves.clone();
    let program = Program::parse(raw.clone()).unwrap();
    let mut plan = Catalog::seed(&program, Limits::default()).unwrap();
    plan["joins"] = json!([{"left":ids(a),"output":gate,"right":ids(b)},{"left":3,"output":mid,"right":ids(c)}]);
    raw["iterative_bake"] = plan.clone();
    let bench = |mut r: J, label: &str| {
        r.as_object_mut().unwrap().remove("sha256");
        r["sha256"] = artifact_digest(&r).into();
        let file = tmp.path().join(label);
        fs::write(&file, serde_json::to_vec(&r).unwrap()).unwrap();
        serde_json::from_slice::<J>(
            &pass(&[
                "bench",
                "--program",
                file.to_str().unwrap(),
                "--lane",
                "free-join-planned",
                "--modules",
                "stage-spine,static-aliases",
                "--repeat",
                "1",
                "--verify-stages",
            ])
            .stdout,
        )
        .unwrap()
    };
    let base = bench(raw.clone(), "base.json");
    Catalog::emit_handoffs(&program, &mut plan).unwrap();
    raw["iterative_bake"] = plan;
    let result = bench(raw.clone(), "optimized.json");
    assert_eq!(base["final_state_sha256"], result["final_state_sha256"]);
    let counts = &result["counts"];
    assert_eq!(counts["handoff_closed"], 1);
    assert_eq!(counts["handoff_closed_fallback"], 1);
    assert_eq!(counts["handoff_deep"], 3);
    assert_eq!(counts["handoff_lets"], 3);
    let mut training_seed = program.artifact.clone();
    training_seed.as_object_mut().unwrap().remove("sha256");
    training_seed["sha256"] = artifact_digest(&training_seed).into();
    let seed_path = tmp.path().join("training-seed.json");
    fs::write(&seed_path, serde_json::to_vec(&training_seed).unwrap()).unwrap();
    let learned = tmp.path().join("learned");
    let run = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"))
        .args([
            "iterate",
            "--program",
            seed_path.to_str().unwrap(),
            "--output",
            learned.to_str().unwrap(),
            "--rounds",
            "4",
            "--objective",
            "rhs-lookups",
        ])
        .env("CELL_CPU_STAGE_SPINE", "1")
        .env("CELL_CPU_SHARED_NODE_SCHEDULE", "1")
        .env("CELL_CPU_KNOWN_RESULTS", "static")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let rounds = json_file(&learned.join("rounds.json"));
    let rounds = rounds.as_array().unwrap();
    assert!(
        rounds[0]["admitted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e[1]["bridge_support"].as_u64().unwrap() > 0)
    );
    assert_eq!(rounds.last().unwrap()["rhs_completion"]["deep"], 3);
    raw["iterative_bake"]["rhs_handoffs"]["max_back"] = 99.into();
    assert!(Program::parse(raw).is_err());
}

#[test]
fn keyed_context_tracks_correlated_ports_and_late_terminal_deltas() {
    use eggfuse_cell_runtime::{checksum::artifact_digest, program::Program};
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("context.egg");
    let out = tmp.path().join("compiled");
    let program = out.join("program.json");
    fs::write(
        &src,
        r#"
(datatype E (N i64) (Hub E E) (End i64 E))
(relation Edge (E E))
(function Kind (E) i64 :merge (max old new))
(relation Hit (E E i64 E))
(ruleset observe) (ruleset link) (ruleset grow) (ruleset cut) (ruleset fix)
(rule ((Edge b k) (= e (End i k)) (= typ (Kind a)) (= h (Hub a b)) (Edge a left))
      ((Hit h left typ e)) :ruleset observe :name "observe-context")
(rule ((= x (N 2))) ((Edge x (N 7))) :ruleset link)
(rule ((= x (N 7))) ((End 11 x)) :ruleset grow)
(rule ((= x (N 2))) ((delete (Edge x (N 7)))) :ruleset cut)
(rule ((= x (N 2))) ((union (N 7) (N 8)) (Edge x (N 8)) (set (Kind (N 1)) 9)) :ruleset fix)
(Hub (N 1) (N 2)) (Hub (N 3) (N 4))
(Edge (N 1) (N 21)) (Edge (N 1) (N 22)) (Edge (N 3) (N 23)) (Edge (N 4) (N 8))
(set (Kind (N 1)) 4) (set (Kind (N 3)) 5)
(End 1 (N 7)) (End 2 (N 8)) (End 3 (N 7))
(run-schedule (seq (run observe) (run link) (run observe) (run grow) (run observe)
                   (run cut) (run observe) (run fix) (run observe)))
(check (Hit (Hub (N 1) (N 2)) (N 21) 9 (End 11 (N 7)))
       (Hit (Hub (N 1) (N 2)) (N 22) 4 (End 1 (N 7)))
       (Hit (Hub (N 3) (N 4)) (N 23) 5 (End 2 (N 8))))
"#,
    )
    .unwrap();
    pass(&[
        "compile",
        "--source",
        src.to_str().unwrap(),
        "--emit-free-join-entry",
        "--output",
        out.to_str().unwrap(),
    ]);
    let pg = json_file(&program);
    let rule = pg["recipe"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .position(|r| r["metadata"]["name"] == "observe-context")
        .unwrap();
    let plan = &pg["local_contexts"][rule];
    let atoms = &pg["recipe"]["rules"][rule]["query"];
    assert_eq!(
        atoms[plan["root"].as_u64().unwrap() as usize]["call"]["table"],
        "Hub"
    );
    assert_eq!(
        atoms[plan["tail"].as_u64().unwrap() as usize]["call"]["table"],
        "End"
    );
    let run = |artifact: &Path| -> J {
        let o = pass(&[
            "bench",
            "--program",
            artifact.to_str().unwrap(),
            "--lane",
            "free-join-planned",
            "--modules",
            "local-context",
            "--repeat",
            "1",
            "--verify-stages",
        ]);
        serde_json::from_slice(&o.stdout).unwrap()
    };
    let result = run(&program);
    let stats = &result["counts"]["local_context"]["rules"][rule.to_string()];
    for k in [
        "calls",
        "root_reuses",
        "delta_terminal_rows",
        "full_terminal_rows",
        "bindings",
    ] {
        assert!(stats[k].as_u64().unwrap() > 0, "{k} in {stats}");
    }
    assert!(stats["peak_prefix_rows"].as_u64().unwrap() > stats["peak_roots"].as_u64().unwrap());
    let stages = |on: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"))
            .arg("internal-stages")
            .arg(&program)
            .env("CELL_CPU_HASH_BITS", "0")
            .env("CELL_CPU_SHARED_NODE_SCHEDULE", "1")
            .env("CELL_CPU_LOCAL_CONTEXT", on)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        o.stdout
    };
    assert_eq!(stages("0"), stages("1"));
    let mut corrupt = pg.clone();
    corrupt["local_contexts"][rule]["root"] = corrupt["local_contexts"][rule]["tail"].clone();
    assert!(Program::parse(corrupt).is_err());
    let mut legacy = pg;
    legacy.as_object_mut().unwrap().remove("local_contexts");
    legacy.as_object_mut().unwrap().remove("sha256");
    legacy["sha256"] = json!(artifact_digest(&legacy));
    let old = tmp.path().join("legacy.json");
    fs::write(&old, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(result["counts"], run(&old)["counts"]);
}

#[test]
fn wide_context_falls_back_and_recovers_after_a_port_shrinks() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("budget.egg");
    let out = tmp.path().join("compiled");
    let edges = (0..65)
        .map(|i| format!("(Side (N 1) {i}) (Side (N 2) {i})"))
        .collect::<Vec<_>>()
        .join("\n");
    let trim = (0..8)
        .map(|i| format!("(delete (Side a {i}))"))
        .collect::<Vec<_>>()
        .join(" ");
    fs::write(
        &src,
        format!(
            r#"
(datatype E (N i64) (Hub E E) (End i64 i64))
(relation Side (E i64)) (relation Ready (E)) (relation Hit (E i64 E))
(ruleset see) (ruleset trim) (ruleset grow)
(rule ((= h (Hub a b)) (Ready a) (Side a x) (Side b k) (= e (End i k))) ((Hit h x e)) :ruleset see)
(rule ((= a (N 2))) ({trim}) :ruleset trim)
(rule ((= a (N 1))) ((End 99 64)) :ruleset grow)
(Ready (N 1)) (Hub (N 1) (N 2)) (End 1 1) {edges}
(run-schedule (seq (run see) (run trim) (run see) (run grow) (run see)))
(check (Hit (Hub (N 1) (N 2)) 64 (End 1 1)) (Hit (Hub (N 1) (N 2)) 0 (End 99 64)))
"#
        ),
    )
    .unwrap();
    pass(&[
        "compile",
        "--source",
        src.to_str().unwrap(),
        "--emit-free-join-entry",
        "--output",
        out.to_str().unwrap(),
    ]);
    let program = out.join("program.json");
    let o = pass(&[
        "bench",
        "--program",
        program.to_str().unwrap(),
        "--lane",
        "free-join-planned",
        "--modules",
        "local-context",
        "--repeat",
        "1",
        "--verify-stages",
    ]);
    let report: J = serde_json::from_slice(&o.stdout).unwrap();
    let ctx = &report["counts"]["local_context"];
    let stats = ctx["rules"].as_object().unwrap().values().next().unwrap();
    assert_eq!(stats["fallbacks"], 1);
    assert!(stats["calls"].as_u64().unwrap() > 0);
    assert_eq!(stats["peak_prefix_rows"], 65 * 57);
}
