//! Execute native commands in order; export diagnostics without changing schedules.
use egg_layout::{
    block_visualization::export_snapshot,
    dependency_blocks::{DependencyBlockStore, RuleSpec},
};
use egglog::{EGraph, TraceSession};
use serde_json::json;
use std::{collections::BTreeSet, error::Error, path::PathBuf};
fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: egg_trace FILE.egg [OUTPUT.json]")?,
    );
    let output = std::env::args().nth(2).map(PathBuf::from);
    let mut eg = EGraph::default();
    let source = std::fs::read_to_string(&path)?;
    let commands = eg.parse_program(Some(path.to_string_lossy().into()), &source)?;
    let trace = TraceSession::with_dependencies();
    let mut store = DependencyBlockStore::default();
    let mut names = BTreeSet::new();
    let mut phases = Vec::new();
    let mut error = None;
    for (index, command) in commands.into_iter().enumerate() {
        let label = command.to_string();
        let before = trace.matches().len();
        let resets = trace.scope_resets().len();
        let result = eg.run_program_with_trace(vec![command], &trace);
        // Use exact runtime names, including desugared and included rules.
        // Generic observations remain opaque until typed composition is certified.
        for m in trace.matches() {
            if names.insert(m.rule.to_string()) {
                store.register_rule(RuleSpec::opaque(&m.rule, &m.rule, ""))?;
            }
        }
        store.ingest_committed(&trace)?;
        if trace.matches().len() != before
            || trace.scope_resets().len() != resets
            || result.is_err()
        {
            let mut snapshot =
                export_snapshot(&store, &trace, &eg, &format!("command {index}: {label}"));
            snapshot["matches"] = json!(trace.matches().len());
            snapshot["complete_witnesses"] = json!(
                trace
                    .matches()
                    .iter()
                    .filter(|m| m.physical_witness_complete)
                    .count()
            );
            phases.push(snapshot);
        }
        if let Err(e) = result {
            error = Some(e.to_string());
            break;
        }
    }
    if phases.is_empty() {
        phases.push(export_snapshot(&store, &trace, &eg, "no rule execution"));
    }
    let data = json!({"scope":"actual egglog runtime; native schedules and checks; diagnostic blocks, no memory compression", "source":path, "error":error,
      "cases":[{"name":path.display().to_string(),"phases":phases}]});
    let text = serde_json::to_string_pretty(&data)?;
    if let Some(output) = output {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output, text)?;
    } else {
        println!("{text}");
    }
    if let Some(error) = error {
        return Err(error.into());
    }
    Ok(())
}
