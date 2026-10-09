//! Execute cell-kernel setup and actions on the actual egglog runtime.
//!
//! Usage: cell_kernel_oracle SETUP.egg ACTIONS.egg REPORT.json [CHECKS.egg]
//!
//! Setup and actions share one EGraph. Parsing, setup execution, and action
//! execution are timed separately; result inspection and JSON output are not
//! charged to execution. The action timer includes the normal egglog command
//! resolution, checks, schedules, insertions, merges, and rebuilds it performs.
//! This is an execution/commit oracle, not a logical-match or tracing benchmark.

use egglog::{EGraph, ast::Command};
use serde_json::json;
use std::{collections::BTreeMap, error::Error, fs, time::Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Read committed live relation inputs, decoding i64 through the native store.
/// Keep all rows rather than deduplicating them so the report reflects the table.
fn relation_rows(eg: &mut EGraph, name: &str) -> Result<Vec<Vec<i64>>> {
    let function = eg
        .get_function(name)
        .ok_or_else(|| format!("required relation {name} is missing"))?;
    let schema = function.schema().clone();
    // Current egglog desugars a relation into a constructor whose result is a
    // fresh internal non-unionable sort. Older relation schemas return Unit.
    let relation_output = schema.output.name() == "Unit"
        || (schema.output.name().starts_with('@')
            && !eg.type_info().is_sort_unionable(&schema.output));
    if !relation_output || schema.input.iter().any(|sort| sort.name() != "i64") {
        return Err(format!(
            "{name} must be a relation with only i64 input columns; actual inputs={:?}, output={}",
            schema
                .input
                .iter()
                .map(|sort| sort.name())
                .collect::<Vec<_>>(),
            schema.output.name(),
        )
        .into());
    }
    let arity = schema.input.len();
    let mut rows = Vec::new();
    eg.function_for_each(name, |row| {
        if !row.subsumed {
            rows.push(
                row.vals[..arity]
                    .iter()
                    .map(|value| eg.value_to_base::<i64>(*value))
                    .collect(),
            );
        }
    })?;
    rows.sort();
    Ok(rows)
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if !(4..=5).contains(&args.len()) {
        return Err(format!(
            "usage: {} SETUP.egg ACTIONS.egg REPORT.json [CHECKS.egg]",
            args[0]
        )
        .into());
    }
    let setup_path = &args[1];
    let actions_path = &args[2];
    let report_path = &args[3];
    let validation_path = args.get(4);
    let total_start = Instant::now();

    let tick = Instant::now();
    let setup_source = fs::read_to_string(setup_path)?;
    let actions_source = fs::read_to_string(actions_path)?;
    let source_read_seconds = tick.elapsed().as_secs_f64();

    let tick = Instant::now();
    EGraph::set_num_threads(1);
    let mut eg = EGraph::default();
    let engine_init_seconds = tick.elapsed().as_secs_f64();

    let tick = Instant::now();
    let setup = eg.parse_program(Some(setup_path.clone()), &setup_source)?;
    let setup_parse_seconds = tick.elapsed().as_secs_f64();
    let setup_command_count = setup.len();
    let setup_schedule_count = setup
        .iter()
        .filter(|command| matches!(command, Command::RunSchedule(_)))
        .count();

    let tick = Instant::now();
    let setup_outputs = eg.run_program(setup)?;
    let setup_execute_seconds = tick.elapsed().as_secs_f64();

    // Parse after executing setup so any parser declarations in setup are live.
    let tick = Instant::now();
    let actions = eg.parse_program(Some(actions_path.clone()), &actions_source)?;
    let actions_parse_seconds = tick.elapsed().as_secs_f64();
    let actions_command_count = actions.len();
    let actions_schedule_count = actions
        .iter()
        .filter(|command| matches!(command, Command::RunSchedule(_)))
        .count();

    let tick = Instant::now();
    let actions_outputs = eg.run_program(actions)?;
    let actions_execute_seconds = tick.elapsed().as_secs_f64();

    let mut validation_read_seconds = 0.0;
    let mut validation_parse_seconds = 0.0;
    let mut validation_execute_seconds = 0.0;
    let mut positive_check_count = 0;
    let mut negative_check_count = 0;
    let mut validation_outputs = Vec::new();
    if let Some(path) = validation_path {
        let tick = Instant::now();
        let source = fs::read_to_string(path)?;
        validation_read_seconds = tick.elapsed().as_secs_f64();

        let tick = Instant::now();
        let checks = eg.parse_program(Some(path.clone()), &source)?;
        validation_parse_seconds = tick.elapsed().as_secs_f64();
        for command in &checks {
            match command {
                Command::Check(..) => positive_check_count += 1,
                Command::Fail(_, inner) if matches!(**inner, Command::Check(..)) => {
                    negative_check_count += 1;
                }
                _ => {
                    return Err(
                        "CHECKS.egg may only contain (check ...) and (fail (check ...))".into(),
                    );
                }
            }
        }
        let tick = Instant::now();
        validation_outputs = eg.run_program(checks)?;
        validation_execute_seconds = tick.elapsed().as_secs_f64();
    }

    let tick = Instant::now();
    let kernel_result = relation_rows(&mut eg, "KernelResult")?;
    let selected = relation_rows(&mut eg, "Selected")?;
    let table_sizes: BTreeMap<_, _> = eg
        .get_function_names()
        .into_iter()
        .map(|name| {
            let size = eg.get_size(&name);
            (name, size)
        })
        .collect();
    let tuple_count = eg.num_tuples();
    let inspection_seconds = tick.elapsed().as_secs_f64();
    let total_before_report_seconds = total_start.elapsed().as_secs_f64();

    let report = json!({
        "schema_version": 1,
        "runtime": "actual egglog runtime",
        "same_process_setup_and_actions": true,
        "tracing_enabled": false,
        "threads": eg.num_threads(),
        "setup_source": setup_path,
        "actions_source": actions_path,
        "setup_command_count": setup_command_count,
        "setup_schedule_count": setup_schedule_count,
        "actions_command_count": actions_command_count,
        "actions_schedule_count": actions_schedule_count,
        "timing_seconds": {
            "source_read": source_read_seconds,
            "engine_init": engine_init_seconds,
            "setup_parse": setup_parse_seconds,
            "setup_execute": setup_execute_seconds,
            "actions_parse": actions_parse_seconds,
            "actions_execute": actions_execute_seconds,
            "execution_total": setup_execute_seconds + actions_execute_seconds,
            "validation_read": validation_read_seconds,
            "validation_parse": validation_parse_seconds,
            "validation_execute": validation_execute_seconds,
            "inspection": inspection_seconds,
            "total_before_report": total_before_report_seconds,
        },
        "timing_scope": "Execute timings include normal egglog command resolution and execution. Source reads, parsing, inspection, output formatting, report writing, and process startup are excluded.",
        "row_scope": "Committed live relation input tuples, sorted lexicographically; the runtime's relation output is omitted.",
        "validation": {
            "source": validation_path,
            "status": if validation_path.is_some() { "passed" } else { "not_requested" },
            "command_count": positive_check_count + negative_check_count,
            "positive_check_count": positive_check_count,
            "negative_check_count": negative_check_count,
        },
        "rows": {
            "KernelResult": kernel_result,
            "Selected": selected,
        },
        "row_counts": {
            "KernelResult": kernel_result.len(),
            "Selected": selected.len(),
        },
        "table_sizes": table_sizes,
        "tuple_count": tuple_count,
        "setup_outputs": setup_outputs.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "actions_outputs": actions_outputs.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "validation_outputs": validation_outputs.iter().map(ToString::to_string).collect::<Vec<_>>(),
    });
    let mut encoded = serde_json::to_vec_pretty(&report)?;
    encoded.push(b'\n');
    fs::write(report_path, encoded)?;
    println!(
        "KernelResult={} Selected={} setup_seconds={setup_execute_seconds:.9} actions_seconds={actions_execute_seconds:.9} report={report_path}",
        kernel_result.len(),
        selected.len(),
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cell_kernel_oracle: {error}");
        std::process::exit(1);
    }
}
