//! Execute native commands in order; export diagnostics without changing schedules.
use egg_layout::{
    block_visualization::{export_coverage, export_snapshot},
    coverage::CoverageLedger,
    dependency_blocks::{DependencyBlockStore, RuleSpec},
};
use egglog::{EGraph, SerializeConfig, TraceSession};
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
            // Logical coverage over the live constructor-row universe.
            if let Some(coverage) = sync_coverage(&mut eg, &store, &trace) {
                snapshot["coverage"] = coverage;
            }
            // Native egraph as a graphviz dot string, for the egraph view.
            snapshot["egraph_dot"] = json!(egraph_dot(&eg));
            phases.push(snapshot);
        }
        if let Err(e) = result {
            error = Some(e.to_string());
            break;
        }
    }
    if phases.is_empty() {
        let mut snapshot = export_snapshot(&store, &trace, &eg, "no rule execution");
        if let Some(coverage) = sync_coverage(&mut eg, &store, &trace) {
            snapshot["coverage"] = coverage;
        }
        snapshot["egraph_dot"] = json!(egraph_dot(&eg));
        phases.push(snapshot);
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

/// Build a fresh coverage ledger over the current user-visible functions and
/// sync it against the block store. Failure (e.g. a container table) degrades to
/// None rather than aborting the run.
fn sync_coverage(
    eg: &mut EGraph,
    store: &DependencyBlockStore,
    trace: &TraceSession,
) -> Option<serde_json::Value> {
    let domain: Vec<String> = eg
        .get_function_names()
        .into_iter()
        .filter(|n| {
            eg.get_function(n)
                .map(|f| !f.is_let_binding())
                .unwrap_or(false)
        })
        .collect();
    if domain.is_empty() {
        return None;
    }
    let mut ledger = CoverageLedger::new(domain);
    ledger.sync_blocks(eg, store, trace).ok()?;
    Some(export_coverage(&ledger))
}

/// Serialize the native egraph to a graphviz dot string. Primitive-heavy graphs
/// are bounded so the dot stays a human-readable overview, not a full dump.
fn egraph_dot(eg: &EGraph) -> String {
    eg.serialize(SerializeConfig {
        max_functions: Some(32),
        max_calls_per_function: Some(48),
        ..SerializeConfig::default()
    })
    .egraph
    .to_dot()
}
