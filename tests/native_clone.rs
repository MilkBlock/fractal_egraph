use egglog::{EGraph, TraceSession};
#[test]
fn cloned_engines_commit_independently_on_multiple_workers() {
    let mut base = EGraph::default();
    base.parse_and_run_program(
        None,
        "(relation Tick (i64)) (rule ((Tick n)) ((Tick (+ n 1))))",
    )
    .unwrap();
    let engines: Vec<_> = (0..4)
        .map(|i| {
            let mut eg = base.clone();
            eg.parse_and_run_program(None, &format!("(Tick {})", i * 100))
                .unwrap();
            eg
        })
        .collect();
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap()
        .install(|| {
            use rayon::prelude::*;
            engines.into_par_iter().enumerate().for_each(|(i, mut eg)| {
                let trace = TraceSession::with_effects();
                for _ in 0..32 {
                    eg.step_rules_with_trace("", &trace).unwrap();
                    trace.drain_completed();
                }
                assert_eq!(eg.get_size("Tick"), 33);
                eg.parse_and_run_program(None, &format!("(check (Tick {}))", i * 100 + 32))
                    .unwrap();
            });
        });
    assert_eq!(base.get_size("Tick"), 0);
}
