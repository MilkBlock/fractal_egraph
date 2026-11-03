//! Diagnostic streaming projection; never used for inference timing.
use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{BufWriter, Write},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let program = Program::parse(serde_json::from_slice(&std::fs::read(&args[1])?)?)?;
    let tables: BTreeSet<String> = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let bindings = args.iter().any(|a| a == "--bindings");
    let all_stages = args.iter().any(|a| a == "--all-stages");
    let mut executor = Executor::new(Graph::new(program)?, bindings);
    let mut writer = BufWriter::new(std::io::stdout().lock());
    fn emit(
        e: &mut Executor,
        tables: &BTreeSet<String>,
        step: Value,
        w: &mut impl Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = e.graph.export()?;
        state["rows"]
            .as_array_mut()
            .unwrap()
            .retain(|r| tables.contains(r["table"].as_str().unwrap()));
        let bindings = e.binding_audit.pop();
        e.binding_audit.clear();
        e.snapshots.clear();
        serde_json::to_writer(
            &mut *w,
            &json!({"step":step,"state":state,"bindings":bindings}),
        )?;
        w.write_all(b"\n")?;
        w.flush()?;
        Ok(())
    }
    emit(&mut executor, &tables, Value::Null, &mut writer)?;
    while !executor.tasks.is_empty() {
        let n = executor.steps.len();
        executor.execute_until(Some(n + 1))?;
        if executor.steps.len() == n {
            break;
        }
        let step = executor.steps[n].clone();
        if !all_stages && step["ruleset"] == "cell-spine-complete" {
            continue;
        }
        emit(&mut executor, &tables, step, &mut writer)?;
    }
    Ok(())
}
