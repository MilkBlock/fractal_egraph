//! Read-only storage accounting for a concrete compressed program. No trace or export.
use egglog::EGraph;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[path = "cell_names.rs"]
mod cell_names;

fn snapshot(eg: &EGraph) -> Value {
    let mut tables = BTreeMap::new();
    let (mut rows, mut slots, mut active, mut retired, mut retired_slots) = (0, 0, 0, 0, 0);
    for name in eg.get_function_names() {
        let function = eg.get_function(&name).unwrap();
        let mut count = 0;
        let mut hidden = 0;
        eg.function_for_each(&name, |r| {
            count += 1;
            hidden += usize::from(r.subsumed);
        })
        .unwrap();
        // SchemaMath::table_columns: inputs, output, timestamp, optional subsume flag.
        let columns = function.schema().input.len() + 2 + usize::from(function.can_subsume());
        rows += count;
        slots += count * columns;
        if cell_names::summary(&name) {
            active += count - hidden;
            retired += hidden;
            retired_slots += hidden * columns;
        }
        if count != 0 {
            tables.insert(
                name,
                json!({"rows":count,"subsumed":hidden,"columns":columns,
                                      "value_slots":count*columns}),
            );
        }
    }
    json!({"rows":rows,"value_slots":slots,"active_markers":active,
           "retired_markers":retired,"retired_marker_value_slots":retired_slots,"tables":tables,
           "materialized_indexes":eg.materialized_indexes()})
}

pub fn run(path: &str) {
    let request: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, request["environment"].as_str().unwrap())
        .unwrap();
    eg.parse_and_run_program(None, request["input"].as_str().unwrap())
        .unwrap();
    let initial = snapshot(&eg);
    eg.parse_and_run_program(None, request["run"].as_str().unwrap())
        .unwrap();
    let retained = snapshot(&eg);
    let peak = initial["value_slots"]
        .as_u64()
        .unwrap()
        .max(retained["value_slots"].as_u64().unwrap());
    println!(
        "{}",
        json!({"schema":"cell-instance-storage/v1","initial":initial,"retained":retained,
        "checkpoint_peak_value_slots":peak,
        "scope":"retained function-table Value slots at input and post-saturation checkpoints, including timestamp/subsume columns and retired rows; not allocation capacity, bytes, RSS or the within-sweep peak",
        "unpriced":["hashcons capacity","join/rebuild index bytes","union-find","stale backing rows","allocator capacity","temporary buffers"]})
    );
}
