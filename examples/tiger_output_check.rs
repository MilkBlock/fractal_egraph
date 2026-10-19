//! REPORT.json BASELINE-TIGER.egg CANDIDATE-TIGER.egg ...
//! Reconstruct actual Tiger output and compare every selected constructor fact.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod bench;
use egglog::EGraph;
use std::{collections::BTreeSet, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 {
        return Err("REPORT.json BASELINE.egg CANDIDATE.egg ...".into());
    }
    EGraph::set_num_threads(1);
    let mut engines = vec![];
    let mut rows = vec![];
    for file in &args[1..] {
        let start = Instant::now();
        let mut eg = EGraph::default();
        eg.parse_and_run_program(Some(file.clone()), &std::fs::read_to_string(file)?)?;
        let mut functions = BTreeSet::new();
        eg.get_function("Function")
            .ok_or("Tiger output has no Function table")?;
        eg.function_for_each("Function", |row| {
            functions.insert(eg.value_to_base::<egglog::sort::S>(row.vals[0]).to_string());
        })?;
        rows.push(serde_json::json!({"file":file,"reconstruction_seconds":start.elapsed().as_secs_f64(),"selected_rows":eg.num_tuples(),"functions":functions,"function_count":functions.len()}));
        engines.push(eg);
    }
    let base = EGraph::default();
    let mut checks = vec![];
    for (i, eg) in engines.iter().enumerate().skip(1) {
        let equal = bench::equal(&base, &engines[0], eg);
        checks.push(serde_json::json!({"candidate":args[i+1],"exact_selected_dag":equal.as_ref().ok(),"error":equal.as_ref().err().map(ToString::to_string)}));
    }
    let passed = checks.iter().all(|v| v["exact_selected_dag"].is_object());
    std::fs::write(
        &args[0],
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":passed,"outputs":rows,"checks":checks,"scope":"Actual production Tiger outputs reconstructed as constructor DAGs; all selected operators, constants, parameters and function roots compared modulo allocated IDs. Identical selected DAG implies identical objective under the same cost model. No surrogate tree-size quality score or new executable runtime/interpreter test."}),
        )?,
    )?;
    if !passed {
        return Err("Tiger selected DAGs differ".into());
    }
    Ok(())
}
