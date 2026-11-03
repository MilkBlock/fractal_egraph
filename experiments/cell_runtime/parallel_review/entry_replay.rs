// Diagnostic example for an isolated cell-runtime build. Only the pure part of
// entry refresh runs here. Both ports and expected answers came from a real Run.
use eggfuse_cell_runtime::{program::Arg, value::Value};
use rayon::prelude::*;
use serde::Deserialize;
use std::{collections::BTreeMap, time::Instant};

#[derive(Deserialize)]
struct Job {
    args: Vec<Arg>,
    values: Vec<Value>,
    ports: usize,
    expected: Option<Vec<Option<Value>>>,
}

fn project(job: &Job) -> Option<Vec<Option<Value>>> {
    let mut slots = vec![None; job.ports];
    for (arg, value) in job.args.iter().zip(&job.values) {
        match arg {
            Arg::Literal(l) if l != value => return None,
            Arg::Slot(i) => match &slots[*i] {
                Some(old) if old != value => return None,
                None => slots[*i] = Some(value.clone()),
                _ => {}
            },
            _ => {}
        }
    }
    Some(slots)
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).unwrap();
    let stages: BTreeMap<u64, Vec<Job>> = serde_json::from_slice(&bytes).unwrap();
    let workers: usize = args[2].parse().unwrap();
    let pool = (workers > 0).then(|| rayon::ThreadPoolBuilder::new().num_threads(workers).build().unwrap());
    let cutoff: usize = args.get(3).map_or(1, |s| s.parse().unwrap());
    let resident = args.get(4).is_some_and(|s| s == "resident");
    let trial = || {
        let mut elapsed = std::time::Duration::ZERO;
        for jobs in stages.values() {
            let start = Instant::now();
            let output: Vec<_> = if pool.is_none() || jobs.len() < cutoff {
                jobs.iter().map(project).collect()
            } else if resident {
                jobs.par_iter().map(project).collect()
            } else {
                pool.as_ref().unwrap().install(|| jobs.par_iter().map(project).collect())
            };
            elapsed += start.elapsed();
            assert!(output.iter().zip(jobs).all(|(a,j)| a == &j.expected));
        }
        elapsed.as_secs_f64()*1000.0
    };
    let mut samples = Vec::new();
    for i in 0..12 {
        let ms = if resident {pool.as_ref().unwrap().install(trial)} else {trial()};
        if i >= 2 { samples.push(ms); }
    }
    println!("{}", serde_json::json!({"workers":workers, "cutoff":cutoff, "resident":resident, "sample_ms":samples,
        "jobs":stages.values().map(Vec::len).sum::<usize>(), "active_runs":stages.len()}));
}
