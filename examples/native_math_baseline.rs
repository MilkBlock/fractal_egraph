//! Isolate native execution and dependency tracing, without the CS/ripen pipeline.
use egglog::{
    EGraph, TraceSession,
    ast::{Command, GenericSchedule},
};
#[path = "support/phased_bench.rs"]
mod phased_bench;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args
        .get(1)
        .is_some_and(|s| s == "bench" || s == "bench-probes")
    {
        if args[1] == "bench-probes" {
            phased_bench::run_with_probes(
                &args[2],
                args.get(3).map(String::as_str),
                egglog::probe_counts,
            );
        } else {
            phased_bench::run(&args[2], args.get(3).map(String::as_str));
        }
        return;
    }
    let source = &args[1];
    let text = std::fs::read_to_string(source).unwrap();
    let traced = args.get(2).is_some_and(|s| s == "trace");
    let start = std::time::Instant::now();
    let mut eg = EGraph::default();
    let mut outputs = vec![];
    if traced {
        let trace = TraceSession::with_dependencies();
        let mut round = 0;
        for cmd in eg.parse_program(Some(source.clone()), &text).unwrap() {
            let commands = if let Command::RunSchedule(GenericSchedule::Repeat(_, n, inner)) = &cmd
            {
                if matches!(**inner, GenericSchedule::Run(..)) {
                    vec![Command::RunSchedule((**inner).clone()); *n]
                } else {
                    vec![cmd]
                }
            } else {
                vec![cmd]
            };
            for c in commands {
                let scheduled = matches!(c, Command::RunSchedule(_));
                let tick = std::time::Instant::now();
                outputs.extend(eg.run_program_with_trace(vec![c], &trace).unwrap());
                let batch = trace.drain_completed();
                if scheduled {
                    round += 1;
                    eprintln!(
                        "trace_round={} seconds={:.9} matches={} reads={} writes={} unions={} events={}",
                        round,
                        tick.elapsed().as_secs_f64(),
                        batch.matches.len(),
                        batch.reads.len(),
                        batch.writes.len(),
                        batch.unions.len(),
                        batch.len()
                    );
                }
            }
        }
    } else {
        outputs = eg
            .parse_and_run_program(Some(source.clone()), &text)
            .unwrap();
    }
    let seconds = start.elapsed().as_secs_f64();
    if args.get(2).is_some_and(|s| s == "shape") {
        let mut classes = std::collections::BTreeSet::new();
        let mut nodes = 0usize;
        for name in eg.get_function_names() {
            let f = eg.get_function(&name).unwrap();
            if f.is_let_binding() || f.schema().output.name() != "Math" {
                continue;
            }
            let sort = f.schema().output.clone();
            eg.function_for_each(&name, |r| {
                nodes += 1;
                classes.insert(
                    eg.value_to_class_id(&sort, *r.vals.last().unwrap())
                        .to_string(),
                );
            })
            .unwrap();
        }
        eprintln!(
            "shape_enodes={nodes} shape_eclasses={} shape_average={:.9}",
            classes.len(),
            nodes as f64 / classes.len() as f64
        );
    }
    for o in outputs {
        println!("{o}");
    }
    eprintln!("baseline_native_seconds={seconds:.9} traced={traced}");
}
