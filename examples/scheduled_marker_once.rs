use egglog::{EGraph, ast::Command};
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut states = vec![];
    let mut reports = vec![];
    for file in std::env::args().skip(1) {
        let mut eg = EGraph::default();
        let start = Instant::now();
        let commands = eg.parse_program(Some(file.clone()), &std::fs::read_to_string(&file)?)?;
        let mut phases = vec![];
        for cmd in commands {
            let label = if matches!(cmd, Command::RunSchedule(_)) {
                Some(cmd.to_string())
            } else {
                None
            };
            let tick = Instant::now();
            eg.run_program(vec![cmd])?;
            if let Some(label) = label {
                phases.push(
                    serde_json::json!({"schedule":label,"ms":tick.elapsed().as_secs_f64()*1000.}),
                );
            }
        }
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        let mut state = std::collections::BTreeMap::new();
        for name in ["P", "Q", "Cell", "External", "Result"] {
            let mut rows = vec![];
            eg.function_for_each(name, |row| {
                if !row.subsumed {
                    rows.push(format!("{:?}", &row.vals[..row.vals.len() - 1]));
                }
            })?;
            rows.sort();
            state.insert(name.to_string(), rows);
        }
        reports.push(serde_json::json!({"file":file,"total_ms":elapsed,"phases":phases,"result_rows":eg.get_size("Result")}));
        states.push(state);
    }
    assert_eq!(states.len(), 2);
    assert!(
        states[0] == states[1],
        "original relation input tuples differ"
    );
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"runs_per_variant":1,"same_original_relations":true,"reports":reports})
        )?
    );
    Ok(())
}
