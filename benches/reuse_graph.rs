//! Bounded batches: setup/drop outside timing; allocations in each API call count.
#[path = "fixtures/reuse.rs"]
mod fixtures;
use egg_layout::reuse_graph::*;
use fixtures::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, hint::black_box, time::Instant};

fn measure<S>(
    name: &str,
    setup: impl Fn() -> S,
    mut run: impl FnMut(&mut S),
    results: &mut BTreeMap<String, Value>,
) {
    const BATCH: usize = 128;
    let mut samples = Vec::new();
    for sample in 0..35 {
        let mut state = setup();
        let start = Instant::now();
        for _ in 0..BATCH {
            run(black_box(&mut state));
        }
        let ns = start.elapsed().as_nanos() as f64 / BATCH as f64;
        if sample >= 5 {
            samples.push(ns);
        }
        black_box(&state);
    }
    samples.sort_by(f64::total_cmp);
    let median = (samples[14] + samples[15]) / 2.;
    println!("{name:40} {median:12.1} ns/op");
    results.insert(
        name.into(),
        json!({"median_ns":median,"samples_ns":samples,"batch":BATCH}),
    );
}
fn main() {
    let mut results = BTreeMap::new();
    for size in [1, 64, 256] {
        let prepare = || {
            let mut g = ReuseGraph::default();
            for i in 0..size {
                let mut s = state();
                s.scope = json!({"environment":i});
                let r = report(&s);
                g.intern_body(RipSatEgraph::from_native_export(s, &r).unwrap())
                    .unwrap();
            }
            let b = g.body(RipSatId(size - 1)).unwrap().clone();
            (g, b)
        };
        measure(
            &format!("body_hit_last/{size}"),
            prepare,
            |(g, b)| {
                black_box(g.intern_body(b.clone()).unwrap());
            },
            &mut results,
        );
    }
    measure(
        "descriptor_hit",
        setup,
        |(g, e, _)| {
            let d = g.enclosure(*e).unwrap().clone();
            black_box(
                g.register_enclosure(d, Admission::RuntimeSupplement)
                    .unwrap(),
            );
        },
        &mut results,
    );
    measure(
        "instance_hit",
        setup,
        |(g, e, _)| {
            black_box(
                g.instantiate(*e, 10, vec![value(100)], vec![value(101), value(102)])
                    .unwrap(),
            );
        },
        &mut results,
    );
    measure(
        "instance_insert",
        || (setup(), 0u64),
        |((g, e, _), n)| {
            *n += 1;
            black_box(
                g.instantiate(*e, 10, vec![value(1000 + *n)], vec![value(101), value(102)])
                    .unwrap(),
            );
        },
        &mut results,
    );
    measure(
        "graft_insert",
        || (setup(), 0u64),
        |((g, _, i), n)| {
            *n += 1;
            black_box(
                g.observe_graft(
                    Graft {
                        event: RuntimeEvent { run: 10, id: *n },
                        episode: *n,
                        instance: *i,
                        revision: 0,
                        rule: "add-mul".into(),
                        upper: [0].into(),
                        lower: [0, 1].into(),
                        inner_facts: [(0, fact("Add", 1)), (1, fact("Mul", 2))].into(),
                    },
                    true,
                )
                .unwrap(),
            );
        },
        &mut results,
    );
    measure(
        "reuse_insert",
        || (setup(), 0u64),
        |((g, _, i), n)| {
            *n += 1;
            black_box(
                g.record_applied_reuse(
                    *i,
                    0,
                    RuntimeEvent { run: 10, id: *n },
                    ReuseKind::InternalDerivation,
                )
                .unwrap(),
            );
        },
        &mut results,
    );
    let out = json!({"schema":"reuse-bench/v1","arch":std::env::consts::ARCH,"os":std::env::consts::OS,"scope":"synthetic ReuseGraph API microbenchmark; not native pipeline","benchmarks":results});
    if let Ok(path) = std::env::var("REUSE_BENCH_OUTPUT") {
        std::fs::write(path, serde_json::to_string_pretty(&out).unwrap()).unwrap();
    }
}
