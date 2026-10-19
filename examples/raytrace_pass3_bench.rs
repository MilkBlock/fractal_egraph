//! SOURCE_FLAT.egg WORKER NEW_OUTPUT [time|check|audit|cpu-pair]. Complete Pass 3.
//! schedule, not a single hotspot. Existing benchmark comparison policies apply.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod bench;
use egg_layout::whole_cell::Library;
use egglog::{EGraph, ast::Command};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path, time::Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 || args.len() > 4 {
        return Err("SOURCE WORKER NEW_OUTPUT [time|check|audit|cpu-pair]".into());
    }
    let mode = args.get(3).map(String::as_str).unwrap_or("time");
    if !["time", "check", "audit", "cpu-pair"].contains(&mode) {
        return Err("unknown measurement mode".into());
    }
    let checked = args.get(3).is_some_and(|s| s == "check");
    let audit_only = args.get(3).is_some_and(|s| s == "audit");
    let cpu_pair = mode == "cpu-pair";
    let out = Path::new(&args[2]);
    std::fs::create_dir(out)?;
    EGraph::set_num_threads(1);
    egglog::set_probe_counts_enabled(false);
    let source = std::fs::read_to_string(&args[0])?;
    let mut eg = EGraph::default();
    let commands = eg.parse_program(Some(args[0].clone()), &source)?;
    let last = commands
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("missing pass schedule")?;
    if last + 1 != commands.len() {
        return Err("unexpected commands after main pass schedule".into());
    }
    let selected = vec![
        "terms".to_string(),
        "terms-helpers".to_string(),
        "terms-helpers-helpers".to_string(),
    ];
    let main = commands[last..].to_vec();
    let mut environment = String::new();
    for c in &commands[..last] {
        let keep = match c {
            Command::Datatype { .. }
            | Command::Constructor { .. }
            | Command::Sort { .. }
            | Command::Function { .. }
            | Command::AddRuleset(..) => true,
            Command::Rule { rule } => selected.contains(&rule.ruleset),
            Command::Rewrite(s, ..) | Command::BiRewrite(s, ..) => selected.contains(s),
            _ => false,
        };
        if keep {
            environment.push_str(&c.to_string());
            environment.push('\n');
        }
    }
    let scope_error = Library::validate_environment(&environment, "terms")
        .err()
        .map(|e| e.to_string());
    let start = Instant::now();
    eg.run_program(commands[..last].to_vec())?;
    let initialization_seconds = start.elapsed().as_secs_f64();
    // Read actual compiled query traits, not guesses from rule names. Diagnostic
    // sinks live only in this discarded clone, outside every timed lane.
    let mut diagnostic = eg.clone();
    let mut queries = vec![];
    for set in &selected {
        for name in diagnostic.cell_rule_names(set)? {
            let sink = diagnostic.prepare_cell_action(set, &name)?;
            let functions = sink
                .atoms
                .iter()
                .filter(|a| {
                    !diagnostic
                        .get_function(&a.function)
                        .unwrap()
                        .is_constructor()
                })
                .map(|a| a.function.clone())
                .collect::<BTreeSet<_>>();
            queries.push(json!({"ruleset":set,"rule":name,"table_atoms":sink.atoms.len(),"i64_guards":sink.guards.len(),"non_constructor_reads":functions}));
        }
    }
    drop(diagnostic);
    std::fs::write(
        out.join("admission.json"),
        serde_json::to_vec_pretty(&json!({
        "status":"whole_cell_skip_not_installed","certified_saturation_boundaries":0,"sat_skip_timing_seconds":null,
        "whole_cell_environment_error":scope_error,"selected_rulesets":selected,"queries":queries,
        "reason":"No verified whole-closure library for the captured environment; the current certificate contract is positive constructor/union and a single direct ruleset. Query compilation coverage is not Cell coverage.",
        "initialization_seconds_excluded":initialization_seconds,"initialized_rows":eg.num_tuples(),
        "scope":"All original Pass-3 rules execute. Terms family uses GPU/cached-CPU matching; other rulesets remain native. No trace, re-Bake, fake rejecting hook, or timed identical third GPU lane."}))?,
    )?;
    eprintln!(
        "Full Pass 3: native baseline, same-commit CPU, then terms-assisted GPU. Sat skip has no admissible library."
    );
    if audit_only {
        let mut runtime = cell_assisted_egglog::table_live::Runtime::new_hybrid(
            eg,
            &main,
            Some(Path::new(&args[1])),
            true,
            &selected,
        )?;
        let result = runtime.run(&main);
        std::fs::write(
            out.join("audit.json"),
            serde_json::to_vec_pretty(
                &json!({"complete":result.is_ok(),"error":result.as_ref().err().map(ToString::to_string),"runtime":runtime.report()}),
            )?,
        )?;
        return result;
    }
    if cpu_pair {
        use cell_assisted_egglog::table_live::Runtime;
        let mut a = Runtime::new_hybrid(eg.clone(), &main, None, false, &selected)?;
        a.run(&main)?;
        let mut b = Runtime::new_hybrid(eg.clone(), &main, None, false, &selected)?;
        b.run(&main)?;
        let equal = bench::equal(&eg, a.engine(), b.engine());
        let quality = bench::extraction_costs(&eg, a.engine(), b.engine());
        std::fs::write(
            out.join("cpu-pair.json"),
            serde_json::to_vec_pretty(&json!({
            "exact_graph":equal.as_ref().ok(),"graph_error":equal.as_ref().err().map(ToString::to_string),
            "input_anchor_quality":quality.as_ref().ok(),"quality_error":quality.as_ref().err().map(ToString::to_string),
            "first":a.report(),"second":b.report(),"scope":"Two identical CPU-only hybrid schedules from the same initial graph; diagnostic, not a performance average."}))?,
        )?;
        return Ok(());
    }
    bench::benchmark_hybrid(
        &eg,
        &main,
        "complete captured raytrace Pass 3 main schedule",
        Path::new(&args[1]),
        checked,
        &out.join("report.json"),
        &selected,
    )?;
    let r: Value = serde_json::from_slice(&std::fs::read(out.join("report.json"))?)?;
    println!(
        "native={} s; same-commit CPU={} s; hybrid GPU={} s; exact same-commit={}",
        r["native"]["run_seconds"],
        r["sorted_commit_cpu"]["runtime"]["run_seconds"],
        r["gpu"]["runtime"]["run_seconds"],
        r["same_commit_verification"].is_object()
    );
    if !r["same_commit_verification"].is_object() {
        return Err("complete Pass-3 same-commit graph verification failed; see report".into());
    }
    Ok(())
}
