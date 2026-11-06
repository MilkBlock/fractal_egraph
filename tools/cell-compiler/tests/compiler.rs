use serde_json::{Value as J, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
#[cfg(feature="function-blocks")]
#[test]
fn function_blocks_preserve_min_max_custom_merge_and_cross_run_consumers() {
    use eggfuse_cell_runtime::{executor::Executor,graph::Graph,program::Program};
    let tmp=tempfile::tempdir().unwrap();
    let input=tmp.path().join("functions.egg");let out=tmp.path().join("compiled");
    fs::write(&input,r#"
(datatype E (Leaf i64) (Wrap E) (Seen E i64))
(function smallest (E) i64 :merge (min old new))
(function largest (E) i64 :merge (max old new))
(function first (E) i64 :merge old)
(function total (E) i64 :merge (+ old new))
(ruleset seed) (ruleset improve) (ruleset propagate) (ruleset observe) (ruleset merge)
(rule ((= x (Leaf n))) ((set (smallest x) (+ n 10))) :ruleset seed)
(rule ((= x (Leaf n))) ((set (largest x) n)) :ruleset seed)
(rule ((= x (Leaf n))) ((set (first x) n)) :ruleset seed)
(rule ((= x (Leaf n))) ((set (total x) n)) :ruleset seed)
(rule ((= x (Leaf n))) ((set (smallest x) n)) :ruleset improve)
(rule ((= x (Leaf n))) ((set (largest x) (+ n 10))) :ruleset improve)
(rule ((= x (Leaf n))) ((set (first x) (+ n 10))) :ruleset improve)
(rule ((= x (Leaf n))) ((set (total x) (+ n 1))) :ruleset improve)
(rule ((= w (Wrap x)) (= n (smallest x))) ((set (smallest w) n)) :ruleset propagate)
(rule ((= n (smallest x))) ((Seen x n)) :ruleset observe)
(rule ((= n (total x))) ((Seen x n)) :ruleset observe)
(rule ((= a (Leaf 1)) (= b (Leaf 2))) ((union a b)) :ruleset merge)
(Wrap (Leaf 1)) (Wrap (Leaf 2))
(run-schedule (run seed) (run propagate) (run observe) (run improve) (run propagate) (run observe) (run merge) (run propagate) (run observe))
"#).unwrap();
    pass(&["compile","--source",input.to_str().unwrap(),"--output",out.to_str().unwrap()]);
    let artifact=json_file(&out.join("program.json"));
    let run=|on|{
        let mut e=Executor::new(Graph::new(Program::parse(artifact.clone()).unwrap()).unwrap(),true);
        e.graph.configure_function_blocks(on);e.execute().unwrap();
        (e.snapshots,e.binding_audit,e.graph.function_blocks_report(),e.graph.stats)
    };
    let off=run(false);let on=run(true);
    assert_eq!(off.0,on.0);assert_eq!(off.1,on.1);
    assert_eq!(off.3.function_updates,on.3.function_updates);
    assert_eq!(off.3.merge_calls,on.3.merge_calls);
    assert!(on.2["installed"].as_u64().unwrap()>0);
    assert_eq!(on.2["fallback"],0,"{}",on.2);
    assert!(on.2["parent_links_avoided"].as_u64().unwrap()>0,"{}",on.2);
    let mut bad=artifact;
    let at=bad["function_blocks"].as_array().unwrap().iter().position(|x|x.is_object()).unwrap();
    bad["function_blocks"][at]["slots"][0][0]=json!(9999);
    bad["sha256"]=json!(eggfuse_cell_runtime::checksum::artifact_digest(&bad));
    assert!(Program::parse(bad).unwrap_err().contains("function block certificate"));
    pass(&["bench","--program",out.join("program.json").to_str().unwrap(),"--reference-program",out.join("program.json").to_str().unwrap(),"--function-blocks","on","--repeat","1"]);
}
#[cfg(feature = "deferred-incidence")]
#[test]
fn deferred_intermediates_preserve_reads_forwarding_and_scheduled_consumers() {
    use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
    let tmp = tempfile::tempdir().unwrap();
    for (queued, hidden) in [(false,false), (true,false), (true,true)] {
        let source = format!(r#"
(datatype E (A) (B) (F E) (G E) (H E) (Seen E))
(ruleset producer)
(ruleset consumer)
(rule ((= a (A)) (= b (B))) ({union} (G (F b)) (G (H b))) :ruleset producer)
(rule ((= b (B))) ((Seen (F b))) :ruleset producer)
(rule ((= x (G y))) ((Seen x)) :ruleset consumer)
{hide_rule}
(A) (B) (F (A))
(run-schedule (run producer) (run consumer) {hide_run})
"#, union=if queued {"(union a b)"} else {""},
            hide_rule=if hidden {"(ruleset hidden) (rule ((= f (F x))) ((subsume (F x))) :ruleset hidden)"} else {""},
            hide_run=if hidden {"(run hidden)"} else {""});
        let input = tmp.path().join(format!("{queued}-{hidden}.egg"));
        let out = tmp.path().join(format!("{queued}-{hidden}"));
        fs::write(&input, source).unwrap();
        pass(&["compile", "--source", input.to_str().unwrap(), "--output", out.to_str().unwrap()]);
        let artifact = json_file(&out.join("program.json"));
        let run = |enabled| {
            let mut e = Executor::new(Graph::new(Program::parse(artifact.clone()).unwrap()).unwrap(), true);
            e.graph.configure_deferred_incidence(enabled);
            #[cfg(feature="port-frames")]
            e.graph.configure_port_frames(enabled);
            e.execute().unwrap();
            (e.report(), e.snapshots, e.binding_audit)
        };
        let a = run(false);
        let b = run(true);
        assert_eq!(a.1,b.1);
        assert_eq!(a.2,b.2);
        for key in ["logical_facts_created", "function_updates", "unions", "repaired_dependencies"] {
            assert_eq!(a.0["stats"][key],b.0["stats"][key],"{key}");
        }
        let stats = &b.0["deferred_incidence"]["stats"];
        if queued {
            assert!(stats["deferred_births"].as_u64().unwrap()>0,"{stats}");
            assert_eq!(stats["retired_before_subscription"].as_u64().unwrap()>0,!hidden,"{stats}");
            assert!(stats["promoted"].as_u64().unwrap()>0,"{stats}");
            if !hidden { assert!(b.0["deferred_incidence"]["reads_before_repair"].as_u64().unwrap()>0); }
        } else { assert_eq!(stats["deferred_births"],0); }
        assert_eq!(b.0["deferred_incidence"]["pending"],0);
        if queued {
            assert!(stats["saved_birth_row_payloads"].as_u64().unwrap()>0);
            pass(&["bench", "--program", out.join("program.json").to_str().unwrap(),
                "--reference-program", out.join("program.json").to_str().unwrap(),
                "--deferred-incidence", "on", "--repeat", "1"]);
        }
        let mut bad = artifact.clone();
        let row = bad["deferred_incidence"].as_array_mut().unwrap().iter_mut()
            .find(|x| !x.as_array().unwrap().is_empty()).unwrap();
        row[0] = json!(!row[0].as_bool().unwrap());
        bad["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&bad));
        assert!(Program::parse(bad).unwrap_err().contains("deferred incidence"));
        let mut admitted = artifact.clone();
        admitted["deferred_incidence_admission"] = json!({
            "schema":"eggfuse-deferred-incidence-admission/v1", "tables":[]});
        admitted["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&admitted));
        let mut e = Executor::new(Graph::new(Program::parse(admitted.clone()).unwrap()).unwrap(),true);
        e.graph.configure_deferred_incidence(true);
        e.execute().unwrap();
        assert_eq!(e.snapshots,a.1);
        assert_eq!(e.report()["deferred_incidence"]["stats"]["deferred_births"],0);
        admitted["deferred_incidence_admission"]["tables"] = json!([{"name":"F","sorts":[]}]);
        admitted["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&admitted));
        assert!(Program::parse(admitted).unwrap_err().contains("type mismatch"));
    }
}

#[cfg(feature = "port-frames")]
#[test]
fn port_producers_survive_consumers_unions_and_subsumption() {
    use eggfuse_cell_runtime::{executor::Executor,graph::Graph,program::Program};
    let tmp=tempfile::tempdir().unwrap();
    let input=tmp.path().join("ports.egg");let out=tmp.path().join("compiled");
    fs::write(&input,r#"
(datatype E (Leaf i64) (Wrap E) (Pair E E) (Seen E))
(ruleset build) (ruleset consume) (ruleset merge) (ruleset hide)
(rule ((= x (Leaf n))) ((Wrap x)) :ruleset build)
(rule ((= w (Wrap x))) ((Pair w x) (Seen w)) :ruleset consume)
(rule ((= a (Leaf 1)) (= b (Leaf 2))) ((union a b)) :ruleset merge)
(rule ((= w (Wrap x))) ((subsume (Wrap x))) :ruleset hide)
(Leaf 1) (Leaf 2)
(run-schedule (run build) (run consume) (run merge) (run build) (run consume) (run hide))
"#).unwrap();
    pass(&["compile","--source",input.to_str().unwrap(),"--output",out.to_str().unwrap()]);
    let artifact=json_file(&out.join("program.json"));
    let run=|enabled| {
        let mut e=Executor::new(Graph::new(Program::parse(artifact.clone()).unwrap()).unwrap(),true);
        e.graph.configure_port_frames(enabled);e.execute().unwrap();
        let ports=e.graph.port_frames_report();
        (e.snapshots,e.binding_audit,e.graph.stats.cells_created,ports)
    };
    let a=run(false);let b=run(true);
    assert_eq!(a.0,b.0);assert_eq!(a.1,b.1);
    assert!(b.2<a.2);assert!(b.3["stats"]["installed"].as_u64().unwrap()>0);
    pass(&["bench","--program",out.join("program.json").to_str().unwrap(),
        "--reference-program",out.join("program.json").to_str().unwrap(),"--port-frames","on","--repeat","1"]);
    let mut bad=artifact;
    let at=bad["port_frames"].as_array().unwrap().iter().position(|v|v.is_array()).unwrap();
    bad["port_frames"][at]=json!([99999]);
    bad["sha256"]=json!(eggfuse_cell_runtime::checksum::artifact_digest(&bad));
    assert!(Program::parse(bad).unwrap_err().contains("port frame certificate"));
}

#[cfg(feature="two-run-survey")]
#[test]
fn two_run_blocks_preserve_orphaned_children_and_child_first_retirement(){
    use eggfuse_cell_runtime::{executor::Executor,graph::Graph,program::Program};
    let tmp=tempfile::tempdir().unwrap();
    for (name,src) in [
        ("parent-first",r#"
(datatype E (Leaf i64) (Link E) (Out E E) (Seen E))
(ruleset a) (ruleset b) (ruleset merge) (ruleset observe)
(rule ((= x (Leaf n)) (= y (Leaf m)) (= m (+ n 1))) ((union (Link x) y)) :ruleset a)
(rule ((= y (Link x)) (= x (Leaf 1))) ((Out x y)) :ruleset b)
(rule ((= left (Leaf 0)) (= right (Leaf 1))) ((union left right)) :ruleset merge)
(rule ((= out (Out x y))) ((Seen out)) :ruleset observe)
(Leaf 0) (Leaf 1) (Leaf 2)
(run-schedule (run a) (run b) (run merge) (run observe))
"#),
        ("child-first",r#"
(datatype E (Leaf i64) (Link E) (Out E))
(ruleset a) (ruleset b) (ruleset merge)
(rule ((= x (Leaf 0)) (= y (Leaf 2))) ((union (Link x) y)) :ruleset a)
(rule ((= y (Link x))) ((Out y)) :ruleset b)
(rule ((= left (Leaf 2)) (= right (Leaf 3))) ((union left right)) :ruleset merge)
(Leaf 3) (Out (Leaf 3)) (Leaf 0) (Leaf 2)
(run-schedule (run a) (run b) (run merge))
"#)
    ]{
        let input=tmp.path().join(format!("{name}.egg"));let out=tmp.path().join(name);
        fs::write(&input,src).unwrap();pass(&["compile","--source",input.to_str().unwrap(),"--output",out.to_str().unwrap()]);
        let original=json_file(&out.join("program.json"));let p=Program::parse(original.clone()).unwrap();
        let mut survey=Executor::new(Graph::new(p.clone()).unwrap(),false);
        survey.graph.configure_port_frames(true);survey.graph.configure_two_run_survey(true);survey.execute().unwrap();
        let mut report=survey.report();report["two_run_blocks"]=survey.graph.two_run_report();
        let catalog=p.bake_two_run(&report,&p).unwrap();
        assert_eq!(catalog["entries"].as_array().unwrap().len(),1,"{name}: {report}");
        let mut artifact=original;artifact["two_run_blocks"]=catalog;
        artifact["sha256"]=json!(eggfuse_cell_runtime::checksum::artifact_digest(&artifact));
        let run=|on|{
            let mut e=Executor::new(Graph::new(Program::parse(artifact.clone()).unwrap()).unwrap(),true);
            e.graph.configure_port_frames(true);e.configure_two_run(on);e.execute().unwrap();
            (e.snapshots,e.binding_audit,e.graph.port_frames_report())
        };
        let control=run(false);let fused=run(true);
        assert_eq!(control.0,fused.0,"{name}");assert_eq!(control.1,fused.1,"{name}");
        assert!(fused.2["stats"]["fused_children"].as_u64().unwrap()>0,"{name}: {}",fused.2);
        assert_eq!(control.2["all_producer_records"],fused.2["all_producer_records"]);
        if name=="parent-first"{assert!(fused.2["stats"]["detached_children"].as_u64().unwrap()>0);}
        let mut invalid=artifact.clone();invalid["two_run_blocks"]["entries"][0]["inputs"][0][1]=json!({"Literal":{"Int":999}});
        invalid["sha256"]=json!(eggfuse_cell_runtime::checksum::artifact_digest(&invalid));
        assert!(Program::parse(invalid).unwrap_err().contains("two-run certificate"));
        let planned=out.join("two-run.json");
        artifact.as_object_mut().unwrap().remove("sha256");
        artifact["sha256"]=json!(eggfuse_cell_runtime::checksum::artifact_digest(&artifact));
        fs::write(&planned,serde_json::to_vec(&artifact).unwrap()).unwrap();
        pass(&["bench","--program",planned.to_str().unwrap(),"--reference-program",out.join("program.json").to_str().unwrap(),"--two-run-blocks","on","--repeat","1"]);
    }
}

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
fn stage_output(path: &Path, hash_bits: &str) -> Vec<u8> {
    let o = Command::new(env!("CARGO_BIN_EXE_eggfuse-cell-compiler"))
        .arg("internal-stages")
        .arg(path)
        .env("CELL_CPU_HASH_BITS", hash_bits)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    o.stdout
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
    let measured = pass(&[
        "bench",
        "--program",
        out.join("program.json").to_str().unwrap(),
        "--repeat",
        "2",
    ]);
    assert!(
        serde_json::from_slice::<J>(&measured.stdout).unwrap()["counts"]["all_candidate_visits"]
            .is_number()
    );
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
        eggfuse_bake::structural_key::source::install(&mut eg, s).unwrap();
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
        "--repeat",
        "1",
    ]);
    let report: J = serde_json::from_slice(&result.stdout).unwrap();
    assert!(report["counts"]["empty_join_skips"].as_u64().unwrap() > 0);
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
        "--source",
        source.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let pg = out.join("program.json");
    let trial = pass(&["bench", "--program", pg.to_str().unwrap(), "--repeat", "1"]);
    let counters: J = serde_json::from_slice(&trial.stdout).unwrap();
    assert!(
        counters["counts"]["free_join"]["entry_probes"]
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
    let guards = raw["free_join_entry"]["guards"].as_array().unwrap();
    assert!(!guards.is_empty());
    let mut invalid = raw.clone();
    invalid["free_join_entry"]["guards"][0]["driver"] = json!(usize::MAX);
    assert!(eggfuse_cell_runtime::program::Program::parse(invalid).is_err());
    let mut old_entry = raw.clone();
    old_entry["free_join_entry"]
        .as_object_mut()
        .unwrap()
        .remove("guards");
    assert!(eggfuse_cell_runtime::program::Program::parse(old_entry).is_ok());
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
            "--repeat",
            "1",
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
    assert_eq!(stage_output(&pg, "64"), stage_output(&old, "64"));
    assert_eq!(
        compiled["final_state_sha256"],
        derived["final_state_sha256"]
    );
    assert_eq!(stage_output(&pg, "64"), stage_output(&pg, "0"));
    let planned = pass(&["bench", "--program", pg.to_str().unwrap(), "--repeat", "3"]);
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
        "--source",
        src.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let pg = out.join("program.json");
    let run = pass(&["bench", "--program", pg.to_str().unwrap(), "--repeat", "2"]);
    let report: J = serde_json::from_slice(&run.stdout).unwrap();
    let counts = &report["counts"];
    let spine = &counts["stage_spine"];
    assert!(spine["mixed_routed_queries"].as_u64().unwrap() > 0);
    assert!(spine["resumed_steps"].as_u64().unwrap() > 0);
    #[cfg(not(feature = "sparse-cells"))]
    assert!(counts["free_join"]["entry_probes"].as_u64().unwrap() > 0);
    #[cfg(feature = "sparse-cells")]
    {
        // Both residual atoms have a complete functional key. The register
        // matcher verifies B(2) and the late B(99) on the normal schedule.
        assert_eq!(counts["free_join"]["entry_probes"].as_u64(),Some(0));
        assert!(counts["register_answers"].as_u64().unwrap()>0);
        assert!(spine["routed_matcher_seeds"].as_u64().unwrap()>0);
    }
    assert_eq!(
        counts["consumer_seed_deliveries"].as_u64().unwrap(),
        spine["routed_spine_seeds"].as_u64().unwrap()
            + spine["routed_matcher_seeds"].as_u64().unwrap()
    );
    assert_eq!(stage_output(&pg, "64"), stage_output(&pg, "0"));
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
                "--repeat",
                "1",
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
    assert_eq!(stage_output(&file, "64"), stage_output(&legacy, "64"));
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
                "--repeat",
                "1",
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
            "--repeat",
            "1",
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
    assert_eq!(stage_output(&program, "64"), stage_output(&program, "0"));
    let mut corrupt = pg.clone();
    corrupt["local_contexts"][rule]["root"] = corrupt["local_contexts"][rule]["tail"].clone();
    assert!(Program::parse(corrupt).is_err());
    let mut legacy = pg;
    legacy.as_object_mut().unwrap().remove("local_contexts");
    legacy.as_object_mut().unwrap().remove("sha256");
    legacy["sha256"] = json!(artifact_digest(&legacy));
    let old = tmp.path().join("legacy.json");
    fs::write(&old, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(stage_output(&program, "64"), stage_output(&old, "64"));
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
        "--output",
        out.to_str().unwrap(),
    ]);
    let program = out.join("program.json");
    let o = pass(&[
        "bench",
        "--program",
        program.to_str().unwrap(),
        "--repeat",
        "1",
    ]);
    let report: J = serde_json::from_slice(&o.stdout).unwrap();
    let ctx = &report["counts"]["local_context"];
    let stats = ctx["rules"].as_object().unwrap().values().next().unwrap();
    assert_eq!(stats["fallbacks"], 1);
    assert!(stats["calls"].as_u64().unwrap() > 0);
    assert_eq!(stats["peak_prefix_rows"], 65 * 57);
}

#[test]
fn connected_cuts_span_chains_and_transfer_evidence_to_renamed_relations() {
    use eggfuse_cell_runtime::{
        checksum::artifact_digest,
        context_admission::{bake, install},
        program::Program,
    };
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"
(relation Start (i64)) (relation Edge (i64 i64)) (relation Tail (i64 i64))
(relation Hit (i64 i64)) (ruleset apply) (ruleset grow) (ruleset cut)
(rule ((Start x) (Edge x y) (Tail y z) (= w (+ x z)) (< w 10))
      ((Hit x w)) :ruleset apply :name "chain")
(rule ((Start x)) ((Tail 3 4)) :ruleset grow)
(rule ((Start x)) ((delete (Edge x 3))) :ruleset cut)
(Start 1) (Edge 1 3) (Edge 1 2) (Tail 2 7) (Tail 3 90)
(run-schedule (seq (run apply) (run grow) (run apply) (run cut) (run apply)))
(check (Hit 1 5) (Hit 1 8))
"#;
    let compile = |label: &str, text: &str| {
        let file = tmp.path().join(format!("{label}.egg"));
        let dir = tmp.path().join(label);
        fs::write(&file, text).unwrap();
        pass(&[
            "compile",
            "--source",
            file.to_str().unwrap(),
            "--output",
            dir.to_str().unwrap(),
        ]);
        json_file(&dir.join("program.json"))
    };
    let raw = compile("source", src);
    let rid = raw["recipe"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .position(|r| r["metadata"]["name"] == "chain")
        .unwrap();
    assert!(raw["local_contexts"][rid].is_null());
    let cuts = raw["context_cuts"][rid].as_array().unwrap();
    assert_eq!(cuts.len(), 3);
    assert!(cuts.iter().any(|c| !c[3].as_array().unwrap().is_empty()));
    let mut corrupt = raw.clone();
    corrupt["context_cuts"][rid] = json!([]);
    assert!(Program::parse(corrupt).is_err());
    let p = Program::parse(raw).unwrap();
    let profile = |on: bool, visits: u64| {
        json!({"status":"complete","program_sha256":p.artifact["sha256"],
        "steps":[],"local_context":{"general":on,"enabled":on,"rules":{rid.to_string():{"calls":2,"port_probes":1}}},
        "query_profile":[{"rule":rid,"candidates":visits,"bindings":3,"calls":2}]})
    };
    let base = profile(false, 50);
    let wide = profile(true, 12);
    let policy = bake(&p, &base, &wide).unwrap();
    assert_eq!(policy["families"].as_array().unwrap().len(), 1);
    let mut bad = wide.clone();
    bad["query_profile"][0]["bindings"] = 0.into();
    assert!(bake(&p, &base, &bad).is_err());
    let renamed = src
        .replace("Start", "Seed")
        .replace("Edge", "Link")
        .replace("Tail", "Leaf")
        .replace(
            "(Seed x) (Link x y) (Leaf y z)",
            "(Leaf y z) (Seed x) (Link x y)",
        );
    let mut target = compile("renamed", &renamed);
    let target_ir = Program::parse(target.clone()).unwrap();
    let expected = install(&target_ir, policy.clone()).unwrap();
    let empty = bake(&p, &base, &profile(true, 80)).unwrap();
    target["context_admission"] = install(&target_ir, empty).unwrap();
    let filtered = Program::parse(target.clone()).unwrap();
    assert_eq!(filtered.context_cut_counts(), (0, 0));
    target["context_admission"] = install(&filtered, policy).unwrap();
    assert_eq!(target["context_admission"], expected);
    let mut stale = target.clone();
    stale["context_admission"]["recipe"] = json!("stale");
    assert!(Program::parse(stale).is_err());
    target.as_object_mut().unwrap().remove("sha256");
    target["sha256"] = artifact_digest(&target).into();
    let pg = tmp.path().join("learned.json");
    fs::write(&pg, serde_json::to_vec(&target).unwrap()).unwrap();
    let r = pass(&["bench", "--program", pg.to_str().unwrap(), "--repeat", "1"]);
    let report: J = serde_json::from_slice(&r.stdout).unwrap();
    let ctx = &report["counts"]["local_context"];
    assert_eq!(ctx["cut_rules"], 1);
    assert!(
        ctx["rules"]
            .as_object()
            .unwrap()
            .values()
            .any(|s| s["bindings"].as_u64().unwrap() > 0)
    );
    assert_eq!(stage_output(&pg, "64"), stage_output(&pg, "0"));
}

#[test]
fn changed_child_ports_follow_value_replacement_and_key_repair() {
    use eggfuse_cell_runtime::{checksum::artifact_digest, program::Program};
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("propagate.egg");
    let dest = dir.path().join("compiled");
    fs::write(
        &src,
        r#"
(datatype Tree (Item i64) (Fork Tree Tree) (One Tree))
(function score (Tree) i64 :merge (max old new))
(ruleset calc) (ruleset touch) (ruleset remove) (ruleset restore) (ruleset coalesce)
(rule ((= t (Fork a b)) (= y (score b)) (= x (score a)) (= total (+ x y)))
      ((set (score t) total)) :ruleset calc :name "combine")
(rule ((= t (One a)) (= v (score a))) ((set (score t) v)) :ruleset calc :name "copy-score")
(rule ((= t (Item 1))) ((set (score t) 7)) :ruleset touch)
(rule ((= t (Item 2))) ((delete (score t))) :ruleset remove)
(rule ((= t (Item 2))) ((set (score t) 13)) :ruleset restore)
(rule ((= t (Item 3))) ((union t (Item 2))) :ruleset coalesce)
(Fork (Item 1) (Item 2)) (Fork (Item 1) (Item 3)) (Fork (Item 1) (Item 4))
(Fork (Item 2) (Item 1)) (Fork (Item 2) (Item 2)) (One (Item 1))
(set (score (Item 1)) 1) (set (score (Item 2)) 2) (set (score (Item 3)) 3)
(run-schedule (seq (run calc) (run remove) (run touch) (run calc) (run restore)
                   (run calc) (run coalesce) (run calc)))
(check (= (score (Fork (Item 1) (Item 2))) 20) (= (score (Fork (Item 2) (Item 2))) 26))
"#,
    )
    .unwrap();
    pass(&[
        "compile",
        "--source",
        src.to_str().unwrap(),
        "--output",
        dest.to_str().unwrap(),
    ]);
    let pg = dest.join("program.json");
    let raw = json_file(&pg);
    let rid = raw["recipe"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .position(|r| r["metadata"]["name"] == "combine")
        .unwrap();
    assert_eq!(
        raw["neighbor_entries"][rid]["children"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let bench = |path: &Path| -> J {
        let res = pass(&[
            "bench",
            "--program",
            path.to_str().unwrap(),
            "--repeat",
            "1",
        ]);
        serde_json::from_slice(&res.stdout).unwrap()
    };
    let fast = bench(&pg);
    let result = &fast["counts"]["neighbor_context"];
    let stats = &result["rules"][rid.to_string()]["stats"];
    assert!(stats["seeds"].as_u64().unwrap() > 0);
    assert!(stats["point_hits"].as_u64().unwrap() > 0);
    assert!(result["publication"]["refreshed"].as_u64().unwrap() > 0);

    assert_eq!(stage_output(&pg, "64"), stage_output(&pg, "0"));
    let mut corrupt = raw.clone();
    corrupt["neighbor_entries"][rid]["pivots"] = json!([]);
    assert!(Program::parse(corrupt).is_err());
    let mut legacy = raw;
    legacy.as_object_mut().unwrap().remove("neighbor_entries");
    legacy.as_object_mut().unwrap().remove("sha256");
    legacy["sha256"] = artifact_digest(&legacy).into();
    let old = dir.path().join("old.json");
    fs::write(&old, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(stage_output(&pg, "64"), stage_output(&old, "64"));
}

#[test]
fn mismatched_catalog_is_rejected_before_publication() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.egg");
    fs::write(&source, SOURCE).unwrap();
    let catalog = tmp.path().join("catalog.json");
    fs::write(
        &catalog,
        br#"{"schema":"eggfuse-cpu-catalog/v1","recipe_sha256":"wrong"}"#,
    )
    .unwrap();
    let dest = tmp.path().join("out");
    let res = command(&[
        "compile",
        "--source",
        source.to_str().unwrap(),
        "--catalog",
        catalog.to_str().unwrap(),
        "--output",
        dest.to_str().unwrap(),
    ]);
    assert!(!res.status.success());
    assert!(String::from_utf8_lossy(&res.stderr).contains("catalog does not match"));
    assert!(!dest.exists());
}

#[test]
fn producer_and_consumers_share_a_checked_port_on_the_original_schedule() {
    use eggfuse_cell_runtime::program::Program;
    let tmp = tempfile::tempdir().unwrap();
    let text = include_str!("fixtures/view-consumers.egg");
    let compile = |name: &str, body: &str| {
        let input = tmp.path().join(format!("{name}.egg"));
        let output = tmp.path().join(name);
        fs::write(&input, body).unwrap();
        pass(&[
            "compile",
            "--source",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ]);
        output.join("program.json")
    };
    let pg = compile("renamed", text);
    let raw = json_file(&pg);
    let blocks = raw["view_consumer_blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    let table = block["view"].as_u64().unwrap() as usize;
    assert_eq!(raw["recipe"]["tables"][table]["name"], "Ticket");
    assert_eq!(block["consumers"]["members"].as_array().unwrap().len(), 3);
    assert!(block["consumers"]["dispatch"].is_object());
    // Frozen ordinary execution includes both fresh pivots, a late peer, union,
    // removal and reinsertion. Each digest includes the ordered RHS and stage graph.
    let expected = serde_json::json!([
        "ce93a735fd0c26467d1a34fe387af8ae9758ad7991a0815b4560e3e4900ad803",
        "9f99a78d53ad8270e7948c482c80e148ab142972b952b10fe6aba9c96ffa7f27",
        "209ae7a3624b07f9a77356a21363e0f2fc9cc719dddf2792aac4b05b190e1271",
        "48ed5708219762f8a98cec4c8d0f00e2b38db044a77a1001495e5ccef06b9b54",
        "c62459792ac8a0daf41c36b36c110e75e9a694ece14ee14ab214e85d65380348",
        "d385b2d1690d278321d8a9b871bc5c6058e216ac2af3eb6d1df3107b91f98a42",
        "afc110eeef13beb11fcd4eff54790780908d8b69cfaf2ece2711b193817a9846",
        "d291af02fa82965e3ff35b500ba0e6472f25e4bcc238665f10443ffa7d194548",
        "ee28a12bbfbd7a64cee5bc4d5f2e1eb2da50c93bacdf7315d04fcdd4975fe508",
        "fc5bc52d6c90b2fda284ccbe9067c2a26f1a43b6d49328b883cbf8357e7defc1"
    ]);
    for bits in ["64", "0"] {
        let stages: J = serde_json::from_slice(&stage_output(&pg, bits)).unwrap();
        assert_eq!(stages, expected);
    }
    let mut invalid = raw.clone();
    invalid["view_consumer_blocks"][0]["consumers"]["dispatch"]["equal"] = json!(375);
    assert!(
        Program::parse(invalid)
            .unwrap_err()
            .contains("view consumer block")
    );
    let mut legacy = raw;
    legacy
        .as_object_mut()
        .unwrap()
        .remove("view_consumer_blocks");
    assert!(Program::parse(legacy).is_ok());

    let bench = pass(&["bench", "--program", pg.to_str().unwrap(), "--repeat", "1"]);
    let result: J = serde_json::from_slice(&bench.stdout).unwrap();
    if cfg!(feature = "view-ports") {
        let counts = &result["counts"];
        assert_eq!(
            counts["view_consumers"]["tables"][0]["generic_columns"],
            false
        );
        assert!(counts["view_storage"]["direct_sources"].as_u64().unwrap() > 0);
        assert!(counts["view_storage"]["deleted"].as_u64().unwrap() > 0);
        assert!(
            counts["view_consumers"]["tables"][0]["port_rows"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            counts["view_consumers"]["groups"][1]["counts"]["comparison_dispatches"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
    let overlapping = compile(
        "overlap",
        &text.replace("(< cost bound)", "(<= cost bound)"),
    );
    let general = json_file(&overlapping);
    assert_eq!(general["view_consumer_blocks"].as_array().unwrap().len(), 1);
    assert!(general["view_consumer_blocks"][0]["consumers"]["dispatch"].is_null());
    assert_eq!(
        stage_output(&overlapping, "64"),
        stage_output(&overlapping, "0")
    );

    let marker = "(Choice (Cost 1 3) (Cost 2 6))";
    let extra=text.replace(marker,&format!("(ruleset inspect)\n(rule ((Ticket v a b n)) ((set (Ledger 7) n)) :ruleset inspect)\n{marker}"));
    let other = compile("extra-reader", &format!("{extra}\n(run inspect 1)"));
    assert!(
        json_file(&other)["view_consumer_blocks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[cfg(feature = "structural-depth")]
#[test]
fn three_structural_edges_complete_on_the_scheduled_run() {
    use eggfuse_cell_runtime::program::Program;
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("input.egg");
    let output = tmp.path().join("compiled");
    fs::write(&input, include_str!("fixtures/structural-depth.egg")).unwrap();
    pass(&[
        "compile",
        "--source",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    let pg = output.join("program.json");
    let mut bad = json_file(&pg);
    bad["structural_ports"]["edges"][0][1] = json!(999);
    bad["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&bad));
    assert!(
        Program::parse(bad)
            .unwrap_err()
            .contains("structural port plan")
    );
    for d in 0..=3 {
        let o = pass(&[
            "bench",
            "--program",
            pg.to_str().unwrap(),
            "--reference-program",
            pg.to_str().unwrap(),
            "--repeat",
            "1",
            "--structural-depth",
            &d.to_string(),
        ]);
        let result: J = serde_json::from_slice(&o.stdout).unwrap();
        let stats = &result["counts"]["structural_depth"];
        assert_eq!(
            stats["closed_rhs"][3].as_u64(),
            Some(if d == 3 { 2 } else { 0 })
        );
        assert_eq!(
            stats["spine_reads"][3].as_u64(),
            Some(if d == 3 { 2 } else { 0 })
        );
    }
    assert!(
        !command(&[
            "bench",
            "--program",
            pg.to_str().unwrap(),
            "--structural-depth",
            "4"
        ])
        .status
        .success()
    );
}

#[cfg(feature = "structural-depth")]
#[test]
fn learned_port_program_checks_deleted_and_missing_witnesses() {
    use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
    let tmp = tempfile::tempdir().unwrap();
    let source = include_str!("fixtures/structural-depth.egg");
    let compile = |name: &str, body: &str| {
        let input = tmp.path().join(format!("{name}.egg"));
        let out = tmp.path().join(name);
        fs::write(&input, body).unwrap();
        pass(&[
            "compile",
            "--source",
            input.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ]);
        out.join("program.json")
    };
    let train = compile("training", source);
    let held=compile("held",&source
        .replace("(Seed 1) (Seed 2)", "(ruleset retire)\n(rule ((= x (Seed 1))) ((delete (Inner x))) :ruleset retire)\n(Seed 1) (Seed 2)")
        .replace("(run third) (run consume)","(run third) (run retire) (run consume)")
        .replace("Seen (Inner (Seed 1))","Seen (Inner (Seed 2))"));
    let mut e = Executor::new(
        Graph::new(Program::parse(json_file(&train)).unwrap()).unwrap(),
        false,
    );
    e.configure_depth(3, true).unwrap();
    e.execute().unwrap();
    let mut catalog = e.depth_report()["catalog"].clone();
    let path = tmp.path().join("catalog.json");
    let count = |source: &Path, catalog: &J| {
        fs::write(&path, serde_json::to_vec(catalog).unwrap()).unwrap();
        let o = pass(&[
            "bench",
            "--program",
            source.to_str().unwrap(),
            "--reference-program",
            source.to_str().unwrap(),
            "--repeat",
            "1",
            "--structural-depth",
            "3",
            "--structural-catalog",
            path.to_str().unwrap(),
        ]);
        serde_json::from_slice::<J>(&o.stdout).unwrap()["counts"]["structural_depth"]["spine_reads"]
            [3]
        .as_u64()
        .unwrap()
    };
    assert_eq!(count(&train, &catalog), 2);
    assert_eq!(count(&held, &catalog), 1);
    // The two Tail publications collide at repair. The later writer either
    // clears or supplies the slot, depending on the input allocation order.
    let forward = include_str!("fixtures/structural-slot-forward.egg");
    let empty_last = compile("empty-last", forward);
    let full_last = compile(
        "full-last",
        &forward.replace("(Seed 1) (Seed 2)", "(Seed 2) (Seed 1)"),
    );
    assert_eq!(count(&empty_last, &catalog), 0);
    assert_eq!(count(&full_last, &catalog), 1);
    catalog["paths"][0][0][0] = json!("absent-operator");
    assert_eq!(count(&held, &catalog), 0);
}

#[cfg(feature = "rhs-locals")]
#[test]
fn local_rhs_keeps_existing_intermediates_and_promotes_at_the_first_new_fact() {
    use eggfuse_cell_runtime::program::Program;
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("sample.egg");
    fs::write(&input, include_str!("fixtures/local-rhs.egg")).unwrap();
    let dir = tmp.path().join("compiled");
    pass(&[
        "compile",
        "--source",
        input.to_str().unwrap(),
        "--output",
        dir.to_str().unwrap(),
    ]);
    let pg = dir.join("program.json");
    let file = pg.to_str().unwrap();
    let mut bad = json_file(&pg);
    let block = bad["local_rhs_blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|b| b["steps"].as_array().is_some_and(|v| v.len() == 2))
        .unwrap();
    block["intermediate_ops"] = json!([999]);
    bad["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&bad));
    assert!(
        Program::parse(bad)
            .unwrap_err()
            .contains("local RHS blocks differ")
    );
    let run = |width| {
        let o = pass(&[
            "bench",
            "--program",
            file,
            "--reference-program",
            file,
            "--repeat",
            "1",
            "--rhs-local-width",
            width,
        ]);
        serde_json::from_slice::<J>(&o.stdout).unwrap()["counts"]["rhs_locals"]["stats"].clone()
    };
    let catalog = tmp.path().join("profile.json");
    let program = Program::parse(json_file(&pg)).unwrap();
    let shape = (0..program.rules.len())
        .filter_map(|r| program.rhs_operator_shape(r))
        .find(|s| s.len() == 2)
        .unwrap();
    let selected = |shapes: J| {
        fs::write(
            &catalog,
            serde_json::to_vec(&json!({"schema":"eggfuse/rhs-work-admission/v1","shapes":shapes}))
                .unwrap(),
        )
        .unwrap();
        let o = pass(&[
            "bench",
            "--program",
            file,
            "--reference-program",
            file,
            "--repeat",
            "1",
            "--rhs-local-width",
            "8",
            "--rhs-local-profile",
            catalog.to_str().unwrap(),
        ]);
        serde_json::from_slice::<J>(&o.stdout).unwrap()["counts"]["rhs_locals"].clone()
    };
    assert_eq!(selected(json!([[["Let", "absent"]]]))["stats"]["enter"], 0);
    assert!(
        selected(json!([shape]))["stats"]["block_local_intermediates"]
            .as_u64()
            .unwrap()
            > 0
    );
    let single = run("1");
    let full = run("8");
    assert_eq!(single["block_local_intermediates"], 0);
    for field in [
        "block_local_intermediates",
        "promoted",
        "prefix_lets_on_promotion",
        "ordered_unions",
    ] {
        assert!(full[field].as_u64().unwrap() > 0, "{field}: {full}");
    }
}

#[test]
fn released_validation_ir_preserves_scheduled_execution_and_remains_explicit() {
    use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("source.egg");
    let out = tmp.path().join("compiled");
    fs::write(&input, SOURCE).unwrap();
    pass(&[
        "compile",
        "--source",
        input.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    let artifact = json_file(&out.join("program.json"));
    let run = |release| {
        let mut e = Executor::new(
            Graph::new(Program::parse(artifact.clone()).unwrap()).unwrap(),
            true,
        );
        if release {
            e.release_validation_artifact().unwrap();
            e.release_validation_artifact().unwrap();
            assert!(Program::parse(e.graph.program.artifact.clone()).is_err());
        }
        e.execute().unwrap();
        assert_eq!(e.report()["program_sha256"], artifact["sha256"]);
        assert!(e.release_validation_artifact().is_err());
        (e.snapshots, e.binding_audit)
    };
    assert_eq!(run(false), run(true));
}

#[cfg(feature = "origin-demand")]
#[test]
fn write_only_unit_receipts_release_producers_but_declared_readers_keep_them() {
    use eggfuse_cell_runtime::{
        executor::Executor,
        graph::{Dependency, Graph},
        program::Program,
    };
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"(datatype E (Leaf i64))
(function TokenLedger (E) Unit :merge old)
(ruleset publish)
(rule ((= x (Leaf n))) ((set (TokenLedger x) ())) :ruleset publish)
(Leaf 1)
(run-schedule (run publish))
"#;
    for reader in [false, true] {
        let name = if reader { "with-reader" } else { "write-only" };
        let input = tmp.path().join(format!("{name}.egg"));
        let out = tmp.path().join(name);
        fs::write(
            &input,
            format!(
                "{src}{}",
                if reader {
                    "(check (TokenLedger (Leaf 1)))"
                } else {
                    ""
                }
            ),
        )
        .unwrap();
        pass(&[
            "compile",
            "--source",
            input.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ]);
        let mut tampered = json_file(&out.join("program.json"));
        let at = tampered["recipe"]["tables"]
            .as_array()
            .unwrap()
            .iter()
            .position(|t| t["name"] == "TokenLedger")
            .unwrap();
        tampered["origin_demand"][at] = serde_json::json!(!reader);
        tampered["sha256"] =
            serde_json::json!(eggfuse_cell_runtime::checksum::artifact_digest(&tampered));
        assert!(
            Program::parse(tampered)
                .unwrap_err()
                .contains("origin demand")
        );
        let mut e = Executor::new(
            Graph::new(Program::parse(json_file(&out.join("program.json"))).unwrap()).unwrap(),
            false,
        );
        e.execute().unwrap();
        let t = e
            .graph
            .program
            .tables
            .iter()
            .position(|t| t.name == "TokenLedger")
            .unwrap();
        let i = e
            .graph
            .functions
            .iter()
            .position(|f| f.live && f.table == t)
            .unwrap();
        assert_eq!(e.graph.origin(Dependency::Function(i)).is_some(), reader);
        assert_eq!(e.graph.witness_count(t), 1);
    }
}
