//! Synthetic filter cost/quality microbenchmark; not a native execution speedup.
#[path = "fixtures/reuse.rs"]
#[allow(dead_code)]
mod fixtures;
use egg_layout::reuse_graph::{
    RipSatEgraph,
    features::{Features, Probe, Region, TypedAtom},
};
use serde_json::json;
use std::{hint::black_box, time::Instant};
fn time(mut f: impl FnMut()) -> f64 {
    let mut samples = vec![];
    for i in 0..35 {
        let start = Instant::now();
        for _ in 0..128 {
            f();
        }
        if i >= 5 {
            samples.push(start.elapsed().as_nanos() as f64 / 128.);
        }
    }
    samples.sort_by(f64::total_cmp);
    (samples[14] + samples[15]) / 2.
}
fn main() {
    let mut results = vec![];
    for rows in [2, 64, 256] {
        let mut state = fixtures::state();
        state.rows = (0..rows)
            .map(|i| {
                let mut row = state.rows[0].clone();
                row.op = format!("Op{i}");
                row
            })
            .collect();
        let report = fixtures::report(&state);
        let body = RipSatEgraph::from_native_export(state, &report).unwrap();
        let filter = Features::bake(&body);
        let queries: Vec<_> = (0..256)
            .map(|i| TypedAtom {
                op: if i % 2 == 0 {
                    format!("Op{}", i % rows)
                } else {
                    format!("Absent{i}")
                },
                arity: 2,
                columns: vec![(if i % 4 == 0 { "E" } else { "MissingSort" }.into(), None); 3],
                equal_columns: vec![],
            })
            .collect();
        let rejects = queries
            .iter()
            .filter(|q| filter.probe_typed(std::slice::from_ref(q), Region::BodyOnly) == Probe::No)
            .count();
        let bake_ns = time(|| {
            black_box(Features::bake(black_box(&body)));
        });
        let mut n = 0;
        let probe_ns = time(|| {
            black_box(filter.probe_typed(
                std::slice::from_ref(&queries[n % queries.len()]),
                Region::BodyOnly,
            ));
            n += 1;
        });
        let boundary_ns = time(|| {
            black_box(filter.probe_typed(black_box(&queries), Region::ThroughBoundary));
        });
        results.push(json!({"rows":rows,"bytes":Features::STORAGE_BYTES,"bake_median_ns":bake_ns,"probe_median_ns":probe_ns,"boundary_median_ns":boundary_ns,"queries":queries.len(),"rejected":rejects,"rejection_fraction":rejects as f64/queries.len() as f64}));
    }
    println!(
        "{}",
        json!({"scope":"synthetic immutable filter maintenance and probes; independent marginal Bloom features; not native runtime latency","samples":30,"warmup_batches":5,"batch":128,"results":results})
    );
}
