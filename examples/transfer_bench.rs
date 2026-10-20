//! LIBRARY.egg NEW_OUTPUT INSTANCES. One run per lane, no averaging.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::parameterized_cell::{Runtime, artifact::Bundle, joint::SETS};
use egglog::{
    EGraph,
    ast::{Command, Expr, Literal},
};
use serde_json::json;
use std::{path::Path, time::Instant};

struct Phase {
    input: String,
    schedule: Command,
    checks: String,
}
fn phases(n: usize) -> Result<Vec<Phase>, Box<dyn std::error::Error>> {
    let mut eg = EGraph::default();
    let commands = eg.parse_program(
        None,
        include_str!("../experiments/parameterized_cell/competing_updates.egg"),
    )?;
    let mut phases = Vec::<Phase>::new();
    let mut pending = String::new();
    for c in commands {
        if matches!(c, Command::RunSchedule(_)) {
            phases.push(Phase {
                input: std::mem::take(&mut pending),
                schedule: c,
                checks: String::new(),
            });
            continue;
        }
        let mut expanded = String::new();
        for i in 0..n {
            let offset = i64::try_from(i)?
                .checked_mul(1000)
                .ok_or("instance identity overflow")?;
            let copy = c.clone().visit_exprs(&mut |mut e| {
                if let Expr::Call(_, op, xs) = &mut e {
                    if matches!(op.as_str(), "V" | "TermLeaf") {
                        if let [Expr::Lit(_, Literal::Int(id))] = xs.as_mut_slice() {
                            *id = id.checked_add(offset).expect("fixture identity overflow");
                        }
                    }
                }
                e
            });
            expanded.push_str(&format!("{copy}\n"));
        }
        if matches!(c, Command::Check(..)) {
            phases
                .last_mut()
                .ok_or("fixture check before phase")?
                .checks
                .push_str(&expanded);
        } else {
            pending.push_str(&expanded);
        }
    }
    if !pending.is_empty() {
        return Err("fixture has unexecuted trailing input".into());
    }
    Ok(phases)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("LIBRARY.egg NEW_OUTPUT INSTANCES".into());
    }
    let n: usize = args[2].parse()?;
    if n == 0 {
        return Err("instances must be positive".into());
    }
    EGraph::set_num_threads(1);
    egglog::set_probe_counts_enabled(false);
    let phases = phases(n)?; // Input generation outside all measured intervals.
    let out = Path::new(&args[1]);
    std::fs::create_dir(out)?;
    let source = include_str!("../experiments/parameterized_cell/terms.egg");
    let mut lanes = vec![];
    for name in ["source_dataflow", "loaded_dataflow", "source_joint"] {
        let start = Instant::now();
        let runtime = match name {
            "source_dataflow" => {
                let mut eg = EGraph::default();
                eg.parse_and_run_program(None, source)?;
                Runtime::new(eg, SETS)?
            }
            "loaded_dataflow" => Bundle::load(Path::new(&args[0]))?.instantiate()?,
            _ => Runtime::new_joint(source)?,
        };
        lanes.push((
            name,
            runtime,
            start.elapsed().as_secs_f64(),
            0.0,
            0.0,
            vec![],
        ));
    }
    let base = lanes[0].1.engine().clone();
    let schedules = phases
        .iter()
        .map(|p| p.schedule.clone())
        .collect::<Vec<_>>();
    let mut reference =
        cell_assisted_egglog::table_live::Runtime::new_cpu(base.clone(), &schedules)?;
    let mut verification = vec![];
    for (phase, p) in phases.iter().enumerate() {
        for (_, r, _, input_seconds, run_seconds, times) in &mut lanes {
            let start = Instant::now();
            r.input(&p.input)?;
            let input = start.elapsed().as_secs_f64();
            *input_seconds += input;
            let start = Instant::now();
            r.run(std::slice::from_ref(&p.schedule))?;
            let run = start.elapsed().as_secs_f64();
            *run_seconds += run;
            times.push(json!({"phase":phase,"input_seconds":input,"run_seconds":run}));
        }
        // Native read-only checks and complete graph comparisons are untimed.
        for (_, r, _, _, _, _) in &mut lanes {
            r.input(&p.checks)?;
        }
        reference.input(&p.input)?;
        reference.run(std::slice::from_ref(&p.schedule))?;
        reference.input(&p.checks)?;
        let native = compare::equal(&base, reference.engine(), lanes[0].1.engine())?;
        let loaded = compare::equal(&base, lanes[0].1.engine(), lanes[1].1.engine())?;
        let joint = compare::equal(&base, lanes[0].1.engine(), lanes[2].1.engine())?;
        verification.push(json!({"phase":phase,"native":native,"loaded":loaded,"joint":joint}));
    }
    let rows = lanes
        .into_iter()
        .map(|(name, r, setup, input, run, times)| {
            json!({
                "lane":name,"setup_seconds":setup,"input_seconds":input,"run_seconds":run,
                "total_seconds":setup+input+run,"phases":times,"runtime":r.report()
            })
        })
        .collect::<Vec<_>>();
    let report = json!({"transfer_bench_verified":true,"instances":n,"lanes":rows,
        "verification":verification,"scope":"Release actual native runtime, seven update phases, one execution per lane. Setup and input separate; checks, generation, comparison, serialization and drop excluded. Same workload, complete public graphs checked. CPU microbenchmark, not raytrace Pass 3 or GPU speedup."});
    std::fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    for r in report["lanes"].as_array().unwrap() {
        println!(
            "{} setup={} input={} run={} total={}",
            r["lane"], r["setup_seconds"], r["input_seconds"], r["run_seconds"], r["total_seconds"]
        );
    }
    Ok(())
}
