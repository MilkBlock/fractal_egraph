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
(rule ((= s (Seed i))) ((C (B (A s)))) :ruleset build)
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
    assert!(report["counts"]["rhs_frames_avoided"].as_u64().unwrap() > 0);

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
    assert!(spine["port_cache_fills"].as_u64().unwrap() > 0);

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
fn only_visible_winners_materialize_a_shared_spine_root() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("fanout.egg");
    // Two writers propose both anchors of key 7. Neither anchor of key 8
    // survives repair. Both consumers of key 7 must share one materialization.
    fs::write(
        &src,
        r#"(relation WriteKey (i64 i64))
(relation DropKey (i64))
(relation L (i64))
(relation R (i64))
(relation SawWrite (i64))
(relation SawLeft (i64))
(relation SawRight (i64))
(ruleset emit)
(ruleset read)
(rule ((WriteKey serial key)) ((L key) (R key) (SawWrite serial)) :ruleset emit)
(rule ((DropKey key)) ((delete (L key))) :ruleset emit)
(rule ((DropKey key)) ((delete (R key))) :ruleset emit)
(rule ((L key)) ((SawLeft key)) :ruleset read)
(rule ((R key)) ((SawRight key)) :ruleset read)
(WriteKey 1 7) (WriteKey 2 7) (WriteKey 3 8) (DropKey 8)
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
    assert_eq!(spine["root_attempts"], 3);
    assert_eq!(spine["captured_anchors"], 6);
    assert_eq!(spine["distinct_anchor_keys"], 4);
    assert_eq!(spine["anchor_publications"], 2);
    assert_eq!(spine["contexts_created"], 1);
    assert_eq!(spine["resumed_steps"], 2);
    assert_eq!(spine["max_chain"], 2);
}
