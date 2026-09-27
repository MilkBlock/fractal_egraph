//! Actual native runtime, same frozen Bake and source; no offline training time.
use egg_layout::reuse_graph::adaptive;
use serde_json::json;
use std::{path::Path, time::SystemTime};
fn main() {
    let source = include_str!("../experiments/math_mini3/source.egg").replace("(run 3)", "(run 8)");
    let library = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("experiments/math_mini3/adaptive_runtime/bake_lib_round2.json");
    let root = std::env::temp_dir().join(format!(
        "cell-bench-{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut samples = [vec![], vec![]];
    let mut graphs = vec![];
    for round in 0..5 {
        for i in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
            let out = root.join(format!("{i}-{round}"));
            let r = if i == 0 {
                adaptive::run_with_min_reuse(&source, Some(&library), &out, "frozen", 2)
            } else {
                adaptive::run_cell_machine(&source, &library, &out, &[], 2)
            }
            .unwrap();
            if round > 0 {
                samples[i].push(r["seconds"].as_f64().unwrap());
            }
            graphs.push((r["final"]["enodes"].clone(), r["final"]["eclasses"].clone()));
        }
    }
    assert!(graphs.windows(2).all(|g| g[0] == g[1]));
    let medians = samples
        .iter_mut()
        .map(|s| {
            s.sort_by(f64::total_cmp);
            (s[1] + s[2]) / 2.0
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        json!({"scope":"actual native math8; same frozen library; existing batch packing in both cases; full graph equality verified separately by experiments/cell_machine/bench.py", "reporting_summary": std::env::var("EGG_LAYOUT_RUNTIME_SUMMARY").ok(), "samples_seconds":samples, "batch_seconds":medians[0], "machine_seconds":medians[1], "speedup":medians[0]/medians[1], "enodes":graphs[0].0,"eclasses":graphs[0].1})
    );
    std::fs::remove_dir_all(root).unwrap();
}
