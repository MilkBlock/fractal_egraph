//! Native engine clone+drop timings. Setup excluded; indexes/metadata still clone.
use egglog::EGraph;
use std::{collections::BTreeMap, hint::black_box, time::Instant};
fn sample(base: &EGraph) -> f64 {
    let mut times = vec![];
    for i in 0..35 {
        let start = Instant::now();
        for _ in 0..16 {
            let copy = base.clone();
            black_box(copy.get_size("R"));
        }
        if i >= 5 {
            times.push(start.elapsed().as_secs_f64() / 16.);
        }
    }
    times.sort_by(f64::total_cmp);
    (times[14] + times[15]) / 2.
}
fn main() {
    let mut base = EGraph::default();
    let mut code = "(datatype E (V i64)) (relation R (E E E E E E E E))".to_string();
    for i in 0..2048 {
        code.push_str(&format!(
            "(R {})",
            std::iter::repeat_n(format!("(V {i})"), 8)
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    base.parse_and_run_program(None, &code).unwrap();
    let deep = sample(&base);
    let mut frozen = base.clone();
    let _ = frozen.fork_shared();
    let shared = sample(&frozen);
    let copies: Vec<_> = (0..4).map(|_| frozen.clone()).collect();
    let mut unique = BTreeMap::new();
    let mut logical = 0;
    for eg in std::iter::once(&frozen).chain(&copies) {
        for (_, id, owners, bytes) in eg.shared_row_storage() {
            assert!(owners >= 5);
            logical += bytes;
            unique.insert(id, bytes);
        }
    }
    println!(
        "{}",
        serde_json::json!({"deep_clone_drop_median_seconds":deep,"shared_clone_drop_median_seconds":shared,"ratio":shared/deep,"avoided_logical_row_bytes":logical-unique.values().sum::<usize>(),"copies":4,"scope":"actual native same-binding snapshots; row payload only; indexes, metadata and processes excluded"})
    );
}
