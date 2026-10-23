#[allow(dead_code)]
#[path = "../examples/support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::{parameterized_cell::artifact::Bundle, table_live};
use egg_layout::native_analyze::transfer;
use egglog::ast::Command;
use serde_json::{Value, json};
use std::{path::PathBuf, process::Command as Process};
const SOURCE: &str = include_str!("../experiments/parameterized_cell/terms.egg");
const INPUT: &str = include_str!("../experiments/parameterized_cell/competing_updates.egg");

fn baked() -> String {
    transfer::bake_text("training.egg", &format!("{SOURCE}\n{INPUT}"))
        .unwrap()
        .text()
        .unwrap()
}
fn check_updates(text: &str, input: &str) {
    let bundle = Bundle::parse(text).unwrap();
    let mut runtime = bundle.instantiate().unwrap();
    assert_eq!(runtime.stats.program_compilations, 0);
    assert_eq!(runtime.stats.loaded_programs, 7);
    assert_eq!(runtime.stats.bootstrap_rows, 0);
    let base = runtime.engine().clone();
    let commands = base.clone().parse_program(None, input).unwrap();
    let schedules = commands
        .iter()
        .filter(|c| matches!(c, Command::RunSchedule(_)))
        .cloned()
        .collect::<Vec<_>>();
    let mut reference = table_live::Runtime::new_cpu(base.clone(), &schedules).unwrap();
    let mut pending = String::new();
    for command in commands {
        if !matches!(command, Command::RunSchedule(_)) {
            pending.push_str(&format!("{command}\n"));
            continue;
        }
        runtime.input(&pending).unwrap();
        reference.input(&pending).unwrap();
        pending.clear();
        runtime.run(std::slice::from_ref(&command)).unwrap();
        reference.run(std::slice::from_ref(&command)).unwrap();
        runtime.audit_mirror().unwrap();
        compare::equal(&base, reference.engine(), runtime.engine()).unwrap();
    }
    if !pending.is_empty() {
        runtime.input(&pending).unwrap();
        reference.input(&pending).unwrap();
    }
    compare::equal(&base, reference.engine(), runtime.engine()).unwrap();
    assert!(
        runtime.report()["port_reuse"]["reused_reads"]
            .as_u64()
            .unwrap()
            > 0
    );
}
#[test]
fn analyze_export_load_matches_every_boundary_and_an_unseen_update() {
    let text = baked();
    let b = Bundle::parse(&text).unwrap();
    assert!(
        b.evidence["records_with_committed_changes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(!b.evidence["connections"].as_array().unwrap().is_empty());
    assert!(
        b.evidence["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["set_witnesses"].as_u64().unwrap() > 0)
    );
    assert!(
        !text.contains("(TermLeaf 20)"),
        "training facts leaked into program"
    );
    let input = format!(
        "{INPUT}\n(PotentialExtractedExpr (V 0) (TCPair (TermLeaf 200) 0))\n{}\n(check (= (ExtractedExpr (Bop (Add) (V 0) (V 1))) (TCPair (TermBop (Add) (TermLeaf 200) (TermLeaf 200)) 1)))",
        cell_assisted_egglog::parameterized_cell::joint::SCHEDULE
    );
    check_updates(&b.text().unwrap(), &input);
    // Loading must not call the hard-coded `terms` compiler again.
    check_updates(
        &text.replace("terms", "renamed"),
        &input.replace("terms", "renamed"),
    );
}
fn edit_program(text: &str, name: &str, edit: impl Fn(&mut Value)) -> String {
    text.lines()
        .map(|line| {
            let Some(s) = line.strip_prefix("; @cell-program ") else {
                return line.to_string();
            };
            let mut value: Value = serde_json::from_str(s).unwrap();
            if value["rule"] == name {
                edit(&mut value);
            }
            format!("; @cell-program {}", serde_json::to_string(&value).unwrap())
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn loader_rejects_incomplete_wiring_guards_versions_and_executable_training_data() {
    let text = baked();
    for damaged in [
        edit_program(&text, "bop", |p| p["seed"] = json!(999)),
        edit_program(&text, "bop", |p| {
            p["steps"].as_array_mut().unwrap().pop();
        }),
        edit_program(&text, "bop", |p| {
            let step = p["steps"][0].clone();
            p["steps"].as_array_mut().unwrap().push(step);
        }),
        edit_program(&text, "improve", |p| p["shape"]["guards"] = json!([])),
        edit_program(&text, "bop", |p| p["ruleset"] = json!("missing")),
        text.lines()
            .filter(|l| !l.starts_with("; @cell-program ") || !l.contains("\"rule\":\"bop\""))
            .collect::<Vec<_>>()
            .join("\n"),
        format!("{text}\n(V 999)"),
        text.replace("\"version\":2", "\"version\":99"),
    ] {
        assert!(
            Bundle::parse(&damaged)
                .and_then(|b| b.instantiate())
                .is_err()
        );
    }
}

#[test]
fn an_unobserved_negative_cost_branch_is_still_executed_after_loading() {
    let mut runtime = Bundle::parse(&baked()).unwrap().instantiate().unwrap();
    let mut native = runtime.engine().clone();
    let input = "(PotentialExtractedExpr (V 99) (TCPair (TermLeaf 99) -1))";
    runtime.input(input).unwrap();
    native.parse_and_run_program(None, input).unwrap();
    let run = native.parse_program(None, "(run terms-helpers 1)").unwrap();
    let expected = native.run_program(run.clone()).unwrap_err().to_string();
    let actual = runtime.run(&run).unwrap_err().to_string();
    assert_eq!(actual, expected);
    assert!(
        runtime.run(&run).is_err(),
        "a failed loaded transfer must not resume"
    );
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "cell-transfer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn a_fresh_process_runs_after_the_original_training_file_is_removed() {
    let t = Temp::new();
    let source = t.0.join("train.egg");
    let bundle = t.0.join("transfers.egg");
    let live = t.0.join("live.egg");
    let out = t.0.join("cold");
    std::fs::write(&source, format!("{SOURCE}\n{INPUT}")).unwrap();
    let output = Process::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "analyze-transfer",
            source.to_str().unwrap(),
            bundle.to_str().unwrap(),
        ])
        .env("RAYON_NUM_THREADS", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_file(&source).unwrap();
    std::fs::write(&live, INPUT).unwrap();
    let output = Process::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "run-transfer",
            bundle.to_str().unwrap(),
            live.to_str().unwrap(),
            out.to_str().unwrap(),
        ])
        .env("RAYON_NUM_THREADS", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let r: Value =
        serde_json::from_slice(&std::fs::read(out.join("results.json")).unwrap()).unwrap();
    assert_eq!(r["passed"], true);
    assert_eq!(r["runtime"]["stats"]["program_compilations"], 0);
    assert_eq!(r["runtime"]["stats"]["loaded_programs"], 7);
    assert_eq!(r["runtime"]["stats"]["bootstrap_rows"], 0);
    assert_eq!(r["boundaries"].as_array().unwrap().len(), 7);
}

#[test]
fn the_normal_analyze_pipeline_can_export_from_its_existing_capture() {
    let t = Temp::new();
    let source = t.0.join("train.egg");
    let out = t.0.join("analysis");
    let input = format!(
        "{SOURCE}\n(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2))\n(set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3))\n(Bop (Add) (V 0) (V 1))\n{}",
        cell_assisted_egglog::parameterized_cell::joint::SCHEDULE
    );
    std::fs::write(&source, input).unwrap();
    let output = Process::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "analyze",
            "--source",
            source.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--bake-transfers",
        ])
        .env("RAYON_NUM_THREADS", "1")
        .env("EGG_LAYOUT_GRAPHS", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value =
        serde_json::from_slice(&std::fs::read(out.join("analysis.json")).unwrap()).unwrap();
    assert_eq!(report["parameterized_transfers"]["programs"], 7);
    assert_eq!(
        Bundle::load(&out.join("transfers.egg"))
            .unwrap()
            .instantiate()
            .unwrap()
            .stats
            .loaded_programs,
        7
    );
    assert!(!out.join("history.json").exists());
}

#[test]
fn analyze_exports_a_scalar_closure_without_any_terms_symbols() {
    let source=r#"
      (datatype Obj (Leaf i64) (Pair Obj Obj))
      (function extent (Obj) i64 :merge (min old new))
      (ruleset measure)
      (rule ((= x (Leaf n))) ((set (extent x) 1)) :ruleset measure :name "leaf")
      (rule ((= p (Pair a b)) (= x (extent a)) (= y (extent b)))
            ((set (extent p) (+ x y))) :ruleset measure :name "pair")
      (Pair (Leaf 1) (Leaf 2))
      (run-schedule (saturate measure))
    "#;
    let bundle=transfer::bake_text("scalar-capture.egg",source).unwrap();
    assert_eq!(bundle.report()["version"],2);
    assert_eq!(bundle.report()["scalar_schemes"].as_array().unwrap().len(),2);
    let mut r=Bundle::parse(&bundle.text().unwrap()).unwrap().instantiate().unwrap();
    assert_eq!(r.stats.program_compilations,0);
    r.input("(Pair (Leaf 7) (Pair (Leaf 8) (Leaf 9)))").unwrap();
    let cs=r.engine().clone().parse_program(None,"(run-schedule (saturate measure))").unwrap();
    r.run(&cs).unwrap();
    r.input("(check (= (extent (Pair (Leaf 7) (Pair (Leaf 8) (Leaf 9)))) 3))").unwrap();
    assert!(r.report()["certificates"]["certificates"].as_array().unwrap().iter().any(|c|c["method"]=="monotone-min-plus-closure/v1"));
}

#[test]
fn loaded_certificate_attaches_to_live_native_rows_and_rechecks_its_authority() {
    let text=baked();let bundle=Bundle::parse(&text).unwrap();
    let mut eg=egglog::EGraph::default();
    let mut commands=eg.parse_program(None,SOURCE).unwrap();
    commands.extend(eg.parse_program(None,"(V 909) (Bop (Add) (V 909) (V 910))").unwrap());
    eg.run_program(commands.clone()).unwrap();
    let r=bundle.attach(eg.clone(),&commands).unwrap();
    assert_eq!(r.stats.loaded_programs,7);assert_eq!(r.stats.program_compilations,0);
    assert!(r.stats.bootstrap_rows>0);
    assert!(r.report()["certificates"]["certificates"].as_array().unwrap().iter().any(|x|x["strength"]=="guarded_closure"));
    // A file can neither drop coverage nor invent a new closure authority.
    let missing=text.lines().map(|line|if let Some(s)=line.strip_prefix("; @cell-transfer "){
        let mut v:Value=serde_json::from_str(s).unwrap();v["rulesets"]=json!([]);format!("; @cell-transfer {v}")
    }else{line.into()}).collect::<Vec<_>>().join("\n");
    assert!(Bundle::parse(&missing).unwrap().attach(eg.clone(),&commands).is_err());
    let extra=eg.parse_program(None,"(rule ((= x (Smaller a b))) ((union x b)) :ruleset terms-helpers-helpers :name \"foreign\")").unwrap();
    eg.run_program(extra.clone()).unwrap();commands.extend(extra);
    assert!(bundle.attach(eg,&commands).is_err());
}

#[test]
fn analyze_bakes_finite_certificates_when_no_numeric_scheme_applies() {
    let out=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("out/certificate-positive-test-{}",std::process::id()));
    if out.exists(){std::fs::remove_dir_all(&out).unwrap();}
    let result=Process::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze","--source","experiments/cell_spine/fixtures/assoc.egg","--output"])
        .arg(&out).arg("--bake-certificates").env("EGG_LAYOUT_GRAPHS","0").env("EGG_LAYOUT_CELL_WINDOWS","0").output().unwrap();
    assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
    let report:Value=serde_json::from_slice(&std::fs::read(out.join("analysis.json")).unwrap()).unwrap();
    assert_eq!(report["certificates"]["kind"],"finite","{report}");
    let lib=egg_layout::whole_cell::Library::load_certificate(&out.join("certificates.egg")).unwrap();
    let mut run=egg_layout::whole_cell::automatic::AutomaticRuntime::new(lib,"").unwrap();
    let input="(Plus (Plus (Leaf \"new-a\") (Leaf \"new-b\")) (Leaf \"new-c\"))";
    run.commit(input).unwrap();run.run("(run-schedule (saturate algebra))").unwrap();
    assert!(run.counters.macro_transitions>0);
    assert_eq!(run.counters.native_fallbacks,0,"{}",run.report());
    let mut native=egglog::EGraph::default();
    let source=std::fs::read_to_string("experiments/cell_spine/fixtures/assoc.egg").unwrap();
    native.parse_and_run_program(None,&source[..source.find("(let root").unwrap()]).unwrap();
    native.parse_and_run_program(None,&format!("{input} (run-schedule (saturate algebra))")).unwrap();
    assert!(matches!(run.verify_against(&native).unwrap(),egg_layout::saturated_rule_composition::Comparison::Equivalent{..}));
}

#[test]
fn loaded_certificates_drive_the_shared_cpu_gpu_event_loop() {
    let text=baked();let worker=std::env::var_os("CELL_MATRIX_WORKER");
    for gpu in [false,true] {
        if gpu && worker.is_none(){continue}
        let (base,observer)=Bundle::parse(&text).unwrap().instantiate().unwrap().detach_joint().unwrap();
        let commands=base.clone().parse_program(None,INPUT).unwrap();
        let schedules=commands.iter().filter(|c|matches!(c,Command::RunSchedule(_))).cloned().collect::<Vec<_>>();
        let selected=["terms","terms-helpers","terms-helpers-helpers"].map(String::from);
        let mut cell=table_live::Runtime::new_hybrid(base.clone(),&schedules,gpu.then(||std::path::Path::new(worker.as_ref().unwrap())),gpu,&selected).unwrap();
        cell.set_saturation_hook(Box::new(observer));
        let mut reference=table_live::Runtime::new_cpu(base.clone(),&schedules).unwrap();
        let mut pending=String::new();
        for command in commands {
            if !matches!(command,Command::RunSchedule(_)){pending+=&format!("{command}\n");continue}
            reference.input(&pending).unwrap();cell.input(&pending).unwrap();pending.clear();
            reference.run(std::slice::from_ref(&command)).unwrap();cell.run(std::slice::from_ref(&command)).unwrap();
            compare::equal(&base,reference.engine(),cell.engine()).unwrap();
        }
        if !pending.is_empty(){reference.input(&pending).unwrap();cell.input(&pending).unwrap();}
        compare::equal(&base,reference.engine(),cell.engine()).unwrap();
        let report=cell.report();
        assert_eq!(report["saturation_diagnostics"]["stats"]["program_compilations"],0);
        assert_eq!(report["saturation_diagnostics"]["stats"]["loaded_programs"],7);
        assert!(report["saturation_diagnostics"]["certificates"]["markers"].as_array().unwrap().iter()
            .any(|m|m["validity"].as_array().unwrap().iter().any(|v|v.as_str().unwrap().contains("saturation"))));
    }
}
