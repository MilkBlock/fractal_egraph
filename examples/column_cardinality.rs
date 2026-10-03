//! Read-only: run a whole egglog program and report, per table, the row count
//! and the number of distinct values in each column.
//!
//! This is the "width side" the cell records cannot provide: it measures the
//! actual global database, so a projected driver key can be compared against
//! the table it would replace. It never inserts, never builds an index, and
//! does not change the program.
//!
//! Usage: column_cardinality PROGRAM.egg [OUT.json]
use egglog::EGraph;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let program: PathBuf = args
        .next()
        .expect("usage: column_cardinality PROGRAM.egg [OUT.json]")
        .into();
    let out = args.next().map(PathBuf::from);
    let text = std::fs::read_to_string(&program).expect("read program");
    let mut eg = EGraph::default();
    let started = std::time::Instant::now();
    eg.parse_and_run_program(None, &text)
        .expect("run program");
    let seconds = started.elapsed().as_secs_f64();

    let mut tables = eg.column_cardinality();
    tables.sort();
    let mut total_rows = 0usize;
    let json_tables: Vec<_> = tables
        .iter()
        .map(|(name, rows, cols)| {
            total_rows += rows;
            let columns: Vec<_> = cols
                .iter()
                .map(|(col, distinct)| {
                    serde_json::json!({
                        "column": col,
                        "distinct": distinct,
                        "rows_per_key": if *distinct > 0 { *rows as f64 / *distinct as f64 } else { 0.0 },
                    })
                })
                .collect();
            serde_json::json!({"table": name, "rows": rows, "columns": columns})
        })
        .collect();

    let report = serde_json::json!({
        "program": program.display().to_string(),
        "seconds": seconds,
        "total_rows": total_rows,
        "tables": json_tables,
        "scope": "whole-program run of the unmodified source; read-only observer, no index built, no facts changed",
    });
    let text = serde_json::to_string_pretty(&report).unwrap();
    match out {
        Some(path) => {
            std::fs::write(&path, text + "\n").expect("write report");
            println!("wrote {}", path.display());
        }
        None => println!("{text}"),
    }
}
