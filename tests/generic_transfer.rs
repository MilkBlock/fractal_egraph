#[allow(dead_code)]
#[path = "../examples/support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::{parameterized_cell::artifact::Bundle, table_live};
use egg_layout::native_analyze::transfer;
use egglog::ast::Command;
use serde_json::Value;

const IF: &str = include_str!("../experiments/observed_transfer_queries/if-training.egg");
const SAME: &str = include_str!("../experiments/observed_transfer_queries/if-same-indices.egg");
const NEW: &str = include_str!("../experiments/observed_transfer_queries/if-new-indices.egg");
const TUPLE: &str = include_str!("../experiments/observed_transfer_queries/tuple-training.egg");

fn bake(source: &str) -> Bundle {
    transfer::bake_text("observed.egg", source).unwrap()
}

fn check(bundle: &Bundle, input: &str) -> Value {
    let mut runtime = Bundle::parse(&bundle.text().unwrap())
        .unwrap()
        .instantiate()
        .unwrap();
    assert_eq!(runtime.stats.program_compilations, 0);
    let base = runtime.engine().clone();
    let commands = base.clone().parse_program(None, input).unwrap();
    let schedules = commands
        .iter()
        .filter(|c| matches!(c, Command::RunSchedule(_)))
        .cloned()
        .collect::<Vec<_>>();
    let mut reference = table_live::Runtime::new_full(base.clone(), &schedules, None, false, None).unwrap();
    let mut pending = String::new();
    for c in commands {
        if matches!(c, Command::Check(..) | Command::Fail(..)) {
            runtime.input(&pending).unwrap();
            reference.input(&pending).unwrap();
            pending.clear();
            runtime
                .engine()
                .clone()
                .run_program(vec![c.clone()])
                .unwrap();
            reference.engine().clone().run_program(vec![c]).unwrap();
            continue;
        }
        if !matches!(c, Command::RunSchedule(_)) {
            pending += &format!("{c}\n");
            continue;
        }
        runtime.input(&pending).unwrap();
        reference.input(&pending).unwrap();
        pending.clear();
        runtime.run(std::slice::from_ref(&c)).unwrap();
        reference.run(std::slice::from_ref(&c)).unwrap();
        runtime.audit_mirror().unwrap();
        compare::equal(&base, reference.engine(), runtime.engine()).unwrap();
    }
    if !pending.is_empty() {
        runtime.input(&pending).unwrap();
        reference.input(&pending).unwrap();
    }
    compare::equal(&base, reference.engine(), runtime.engine()).unwrap();
    let report = runtime.report();
    assert_eq!(report["stats"]["native_fallback_runs"], 0);
    report
}

#[test]
fn if_indices_are_slots_not_training_constants_and_allow_aliases() {
    let a = bake(IF);
    let b = bake(SAME);
    assert_eq!(a.report()["version"], 3);
    assert_eq!(a.report()["programs"], 1);
    assert_eq!(
        a.report()["query_schemes"][0]["numeric_slots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let programs = |b: &Bundle| {
        b.text()
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("; @cell-program "))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        programs(&a),
        programs(&b),
        "changing training indices must not change executable wiring"
    );
    for input in [
        NEW.to_string(),
        NEW.replace(" 8)", " 9)"),
        NEW.replace(" 8)", " -8)"),
    ] {
        let r = check(&b, &input);
        assert!(r["programs"][0]["emitted_bindings"].as_u64().unwrap() > 0);
        assert!(
            r["certificates"]["certificates"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["strength"] == "complete_query")
        );
    }
}

#[test]
fn recursive_tuple_preserves_finite_waves_late_length_updates_and_union() {
    let b = bake(TUPLE);
    assert_eq!(
        b.report()["programs"],
        5,
        "all five observed rules should be portable"
    );
    let input = r#"
      (set (tuple-length (V 30)) 2)
      (Not-Just-Concat (V 30))
      (Add-All-Gets (V 90) (V 30) 4 0)
      (run always-run 1)
      (check (Add-All-Gets (V 90) (V 30) 4 1))
      (fail (check (Add-All-Gets (V 90) (V 30) 4 2)))
      (run always-run 1)
      (check (= (Get (V 90) 5) (Get (V 30) 1)))
      (delete (tuple-length (V 30)))
      (set (tuple-length (V 30)) 4)
      (run always-run 1)
      (fail (check (Add-All-Gets (V 90) (V 30) 4 4)))
      (run always-run 2)
      (check (= (Get (V 90) 7) (Get (V 30) 3)))
      (Add-All-Gets (V 91) (V 31) 8 0)
      (run always-run 1)
      (fail (check (Add-All-Gets (V 91) (V 31) 8 1)))
      (set (tuple-length (V 31)) 4)
      (run always-run 1)
      (check (Add-All-Gets (V 91) (V 31) 8 1))
      (union (V 30) (V 31))
      (run always-run 2)
      (union (V 90) (V 30))
      (run always-run 2)
      (run always-run 1)
    "#;
    check(&b, input);
}

#[test]
fn generic_and_scalar_programs_share_one_original_commit_with_unobserved_residuals() {
    let env = r#"
      (datatype E (N i64) (Seen i64) (Late i64))
      (function cost (E) i64 :merge (min old new))
      (ruleset s)
      (rule ((= e (N n))) ((set (cost e) 1)) :ruleset s :name "cost")
      (rule ((N n) (> n 0)) ((Seen (+ n 1))) :ruleset s :name "seen")
      (rule ((N n) (< n 0)) ((Late n)) :ruleset s :name "late")
    "#;
    let b = bake(&format!("{env} (N 10) (run s 2)"));
    assert_eq!(b.report()["scalar_schemes"].as_array().unwrap().len(), 1);
    assert_eq!(b.report()["query_schemes"].as_array().unwrap().len(), 1);
    check(
        &b,
        "(N 20) (N -3) (run s 1) (check (Seen 21)) (check (Late -3)) (check (= (cost (N 20)) 1)) (run s 1)",
    );
}

#[test]
fn partial_keys_and_guards_are_validated_when_loading() {
    let text = bake(IF).text().unwrap();
    assert!(text.contains("Columns"));
    for mode in ["mask", "omitted", "alias", "query"] {
        let bad = text
            .lines()
            .map(|line| {
                if let Some(raw) = line.strip_prefix("; @cell-program ") {
                    let mut p: Value = serde_json::from_str(raw).unwrap();
                    match mode {
                        "mask" => {
                            let step = p["steps"]
                                .as_array_mut()
                                .unwrap()
                                .iter_mut()
                                .find(|s| s[1].get("Columns").is_some())
                                .unwrap();
                            step[1]["Columns"] = Value::from(u64::MAX);
                        }
                        "omitted" => {
                            p["steps"].as_array_mut().unwrap().pop();
                        }
                        "alias" => {
                            p["shape"]["atoms"][0]["columns"][0] = serde_json::json!({"Slot":999});
                        }
                        _ => (),
                    }
                    format!("; @cell-program {p}")
                } else if mode == "query" && line.starts_with("; @cell-transfer ") {
                    let mut h: Value =
                        serde_json::from_str(line.trim_start_matches("; @cell-transfer ")).unwrap();
                    h["queries"] = serde_json::json!([]);
                    format!("; @cell-transfer {h}")
                } else {
                    line.into()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            Bundle::parse(&bad).and_then(|b| b.instantiate()).is_err(),
            "{mode}"
        );
    }
}

#[test]
fn source_literals_and_checked_overflow_are_not_generalized_away() {
    let b = bake(
        r#"
      (datatype E (At i64) (Out i64)) (ruleset s)
      (rule ((At i) (> i 7)) ((Out (+ i 1))) :ruleset s :name "step")
      (At 8) (run s 1)
    "#,
    );
    assert_eq!(b.report()["programs"], 1);
    check(
        &b,
        "(At 7) (At 15) (run s 1) (check (Out 16)) (fail (check (Out 8))) (run s 1)",
    );
    let mut runtime = b.instantiate().unwrap();
    let commands = runtime
        .engine()
        .clone()
        .parse_program(None, "(run s 1)")
        .unwrap();
    let mut reference = table_live::Runtime::new_cpu(runtime.engine().clone(), &commands).unwrap();
    runtime.input("(At 9223372036854775807)").unwrap();
    reference.input("(At 9223372036854775807)").unwrap();
    let actual = runtime.run(&commands).unwrap_err().to_string();
    let expected = reference.run(&commands).unwrap_err().to_string();
    assert_eq!(actual, expected);
    assert!(actual.contains("call of primitive + failed"));
    assert!(
        runtime.run(&commands).is_err(),
        "failed transfer must not resume"
    );
}

#[test]
fn ordinary_analyze_exports_numeric_templates_for_a_source_free_cold_process() {
    use std::{fs, process::Command as Process};
    let dir = std::env::temp_dir().join(format!("generic-cold-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let train = dir.join("train.egg");
    let live = dir.join("live.egg");
    let analysis = dir.join("analysis");
    fs::write(&train, IF).unwrap();
    fs::write(&live, NEW).unwrap();
    let output = Process::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "analyze",
            "--source",
            train.to_str().unwrap(),
            "--output",
            analysis.to_str().unwrap(),
            "--bake-certificates",
        ])
        .env("EGG_LAYOUT_GRAPHS", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let a: Value =
        serde_json::from_slice(&fs::read(analysis.join("analysis.json")).unwrap()).unwrap();
    assert_eq!(a["parameterized_transfers"]["programs"], 1);
    assert_eq!(
        a["parameterized_transfers"]["query_schemes"][0]["numeric_slots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    fs::remove_file(train).unwrap();
    fs::remove_file(analysis.join("source.egg")).unwrap();
    let result = dir.join("cold");
    let output = Process::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "run-transfer",
            analysis.join("transfers.egg").to_str().unwrap(),
            live.to_str().unwrap(),
            result.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let r: Value = serde_json::from_slice(&fs::read(result.join("results.json")).unwrap()).unwrap();
    assert_eq!(r["runtime"]["stats"]["loaded_programs"], 1);
    assert_eq!(r["runtime"]["stats"]["program_compilations"], 0);
    assert_eq!(r["runtime"]["stats"]["native_fallback_runs"], 0);
    assert!(
        r["runtime"]["programs"][0]["emitted_bindings"]
            .as_u64()
            .unwrap()
            > 0
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn exported_query_programs_supply_the_shared_cpu_and_gpu_loop() {
    let b = marked(&bake(IF), true);
    let worker = std::env::var_os("CELL_MATRIX_WORKER");
    for gpu in [false, true] {
        if gpu && worker.is_none() {
            continue;
        }
        let (base, observer) = b.instantiate().unwrap().detach_joint().unwrap();
        let commands = base.clone().parse_program(None, NEW).unwrap();
        let schedules = commands
            .iter()
            .filter(|c| matches!(c, Command::RunSchedule(_)))
            .cloned()
            .collect::<Vec<_>>();
        let selected = vec!["non-weakly-linear".to_string()];
        let mut assisted = table_live::Runtime::new_full_assisted(
            base.clone(),
            &schedules,
            gpu.then(|| std::path::Path::new(worker.as_ref().unwrap())),
            gpu,
            Some(&selected),
            Some(Box::new(observer)),
        )
        .unwrap();
        let mut reference = table_live::Runtime::new_full(base.clone(), &schedules, None, false, None).unwrap();
        let mut pending = String::new();
        for c in commands {
            if !matches!(c, Command::RunSchedule(_)) {
                pending += &format!("{c}\n");
                continue;
            }
            assisted.input(&pending).unwrap();
            reference.input(&pending).unwrap();
            pending.clear();
            assisted.run(std::slice::from_ref(&c)).unwrap();
            reference.run(std::slice::from_ref(&c)).unwrap();
            compare::equal(&base, reference.engine(), assisted.engine()).unwrap();
        }
        assisted.input(&pending).unwrap();
        reference.input(&pending).unwrap();
        compare::equal(&base, reference.engine(), assisted.engine()).unwrap();
        let report = assisted.report();
        assert_eq!(
            report["saturation_diagnostics"]["stats"]["program_compilations"],
            0
        );
        assert_eq!(
            report["saturation_diagnostics"]["stats"]["loaded_programs"],
            1
        );
        assert!(report["runtime"]["cell_query_bindings"].as_u64().unwrap() > 0);
        assert_eq!(
            report["runtime"]["skipped_saturations"], 0,
            "query authority cannot skip saturation"
        );
    }
}

#[test]
fn unsupported_container_query_does_not_block_other_observed_programs() {
    let b = bake(r#"
      (sort Ints (Vec i64))
      (datatype E (N i64) (Box Ints) (Mark i64) (Flag))
      (ruleset s)
      (rule ((N i)) ((Mark i)) :ruleset s :name "number")
      (rule ((Box xs)) ((Flag)) :ruleset s :name "container")
      (N 10) (Box (vec-of 1 2)) (run s 1)
    "#);
    assert_eq!(b.report()["programs"], 1);
    assert_eq!(b.report()["query_schemes"][0]["rule"], "number");
    // The graph comparator has no container encoding. Check this rejection
    // boundary through native observations, not by ignoring container tables.
    assert!(b.report()["evidence"]["query_export_rejections"].as_array().unwrap().iter().any(|r|r["rule"]=="container"));
    let mut r=b.instantiate().unwrap();
    assert_eq!(r.stats.bootstrap_rows,0);
    let mut native=r.engine().clone();
    let facts="(N 99) (Box (vec-of 3 4))";
    let checks="(check (Mark 99)) (check (Flag)) (check (Box (vec-of 3 4)))";
    let schedule=native.parse_program(None,"(run s 1)").unwrap();
    native.parse_and_run_program(None,&format!("{facts} (run s 1) {checks}")).unwrap();
    r.input(facts).unwrap();r.run(&schedule).unwrap();r.input(checks).unwrap();
}

// Exercise storage layouts independently of cost selection. The Python tests
// drive the real solver; here each native frontier checks even omitted payloads.
fn marked(bundle: &Bundle, numeric_only: bool) -> Bundle {
    use cell_assisted_egglog::parameterized_cell::boundary;
    let mut engine=bundle.instantiate().unwrap().engine().clone();
    let sets=bundle.layout_queries().into_iter().map(|(s,_)|s).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>();
    let raw=boundary::inspect(&mut engine,&sets,&Default::default()).unwrap();
    let specs=raw["queries"].as_array().unwrap().iter().filter(|q|
        bundle.layout_queries().contains(&(q["ruleset"].as_str().unwrap().into(),q["rule"].as_str().unwrap().into())))
        .map(|q|{
            let mut p=q["plans"][0].clone();
            p["prefix_len"]=serde_json::json!(p["steps"].as_array().unwrap().len());
            p["carried_slots"]=serde_json::json!(p["shape"]["slots"].as_array().unwrap().iter().enumerate()
                .filter(|(_,s)|!numeric_only || s[1]=="i64").map(|(i,_)|i).collect::<Vec<_>>());
            p["shared_marker"]=true.into();
            serde_json::from_value::<boundary::Spec>(serde_json::json!({"ruleset":q["ruleset"],"rule":q["rule"],
                "root":p["shape"]["atoms"][p["seed"].as_u64().unwrap() as usize]["table"],"plan":p})).unwrap()
        }).collect::<Vec<_>>();
    let environment=bundle.text().unwrap();
    let commands=engine.parse_program(None,&environment).unwrap();
    let mut out=bundle.clone();out.select_layouts(&mut engine,&commands,&specs).unwrap();out
}

#[test]
fn parameter_markers_keep_correlations_guards_and_each_update_frontier() {
    for (source,input) in [(IF,NEW),(TUPLE,include_str!("../experiments/observed_transfer_queries/tuple-updates.egg"))] {
        let b=bake(source);
        for numeric_only in [false,true] {
            let selected=marked(&b,numeric_only);
            assert_eq!(selected.report()["version"],4);
            let r=check(&selected,input);
            assert!(!r["shared_markers"].as_array().unwrap().is_empty());
            assert!(r["programs"].as_array().unwrap().iter().all(|p|p["payload_layout"].is_array()));
            assert!(r["certificates"]["certificates"].as_array().unwrap().iter().all(|c|c["strength"]=="complete_query"));
            assert_eq!(r["stats"]["program_compilations"],0);
        }
    }
}

#[test]
fn loaded_marker_rejects_forged_payload_and_guard() {
    let text=marked(&bake(TUPLE),false).text().unwrap();
    for mode in ["slot","prefix","guard","version"] {
        let bad=text.lines().map(|l|{
            if let Some(raw)=l.strip_prefix("; @cell-program ") {
                let mut p:Value=serde_json::from_str(raw).unwrap();
                match mode {
                    "slot"=>p["marker"]["carried_slots"]=serde_json::json!([9999]),
                    "prefix"=>p["marker"]["prefix_len"]=serde_json::json!(9999),
                    "guard"=>p["shape"]["guards"]=serde_json::json!([]),
                    _=>{}
                }
                format!("; @cell-program {p}")
            } else if mode=="version" && l.starts_with("; @cell-transfer ") {
                let mut h:Value=serde_json::from_str(l.trim_start_matches("; @cell-transfer ")).unwrap();
                h["version"]=3.into();format!("; @cell-transfer {h}")
            } else {l.into()}
        }).collect::<Vec<_>>().join("\n");
        let result=Bundle::parse(&bad).and_then(|b|b.instantiate());
        assert!(result.is_err(),"{mode} must not authorize stale bindings");
    }
}

#[test]
fn analyze_rule_names_rebind_only_by_complete_source_and_layouts_keep_authority() {
    let b=marked(&bake(TUPLE),true);
    let mut eg=egglog::EGraph::default();
    let commands=eg.parse_program(None,TUPLE).unwrap();
    let end=commands.iter().position(|c|matches!(c,Command::RunSchedule(_))).unwrap();
    let setup=&commands[..end];eg.run_program(setup.to_vec()).unwrap();
    assert_eq!(b.layout_queries_for(setup).unwrap().len(),5);
    let mut r=b.attach(eg,setup).unwrap();
    r.run(&commands[end..end+1]).unwrap();
    assert_eq!(r.report()["programs"].as_array().unwrap().len(),5);
    assert_eq!(r.stats.program_compilations,0);
    assert!(r.report()["programs"].as_array().unwrap().iter().all(|p|p["payload_layout"].is_array()));
    let changed=TUPLE.replace("(+ offset pos)","(+ offset 99)");
    let mut other=egglog::EGraph::default();
    let commands=other.parse_program(None,&changed).unwrap();
    other.run_program(commands[..end].to_vec()).unwrap();
    assert!(b.attach(other,&commands[..end]).is_err(),"matching LHS alone cannot identify a different source rule");
}

#[test]
fn solver_selected_files_match_native_at_all_input_frontiers() {
    let Some(path)=std::env::var_os("CELL_SELECTED_MARKER_FIXTURES") else {return};
    for (directory,input) in [("if-scip",NEW),("tuple-gurobi",include_str!("../experiments/observed_transfer_queries/tuple-updates.egg"))] {
        let bundle=Bundle::load(&std::path::Path::new(&path).join(directory).join("selected-certificates.egg")).unwrap();
        check(&bundle,input);
    }
}

#[test]
fn selected_parameter_markers_preserve_referenced_global_updates() {
    let source=include_str!("../experiments/observed_transfer_queries/if-two-consumers.egg");
    let bundle=bake(source);
    assert_eq!(bundle.report()["query_schemes"].as_array().unwrap().len(),2);
    assert_eq!(bundle.report()["evidence"]["environment_globals"],serde_json::json!(["DUMMYCTX"]));
    let selected=marked(&bundle,true);
    let r=check(&selected,include_str!("../experiments/observed_transfer_queries/if-global-updates.egg"));
    assert_eq!(r["stats"]["program_compilations"],0);
}
