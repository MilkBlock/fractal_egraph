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
        text.replace("\"version\":1", "\"version\":99"),
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
