//! Identical public-API harness compiled against pristine and current egglog.
//! No assistant, tracing, counters, solver or GPU dependency in this executable.
use egglog::EGraph;
use serde_json::json;
use std::time::Instant;
mod public_snapshot;

fn main()->Result<(),Box<dyn std::error::Error>> {
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    let start=Instant::now();
    let env=std::fs::read_to_string(&args[0])?;
    let input=std::fs::read_to_string(&args[1])?;
    let rounds:usize=args[2].parse()?;
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None,&env)?;
    let setup=start.elapsed().as_secs_f64();
    let t=Instant::now();eg.parse_and_run_program(None,&input)?;
    let input_seconds=t.elapsed().as_secs_f64();
    let t=Instant::now();let mut matches=0;let mut progress=vec![];
    for _ in 0..rounds {
        let r=eg.step_rules("")?;
        matches+=r.num_matches_per_rule.values().sum::<usize>();progress.push(r.updated);
    }
    let run_seconds=t.elapsed().as_secs_f64();
    if let Some(path)=args.get(3){
        let snapshot=public_snapshot::snapshot(&eg,true)?;
        std::fs::write(path,serde_json::to_vec(&snapshot)?)?;
    }
    drop(eg);
    println!("{}",json!({"setup_seconds":setup,"input_seconds":input_seconds,"run_seconds":run_seconds,
        "core_seconds":setup+input_seconds+run_seconds,"through_drop_seconds":start.elapsed().as_secs_f64(),
        "native_matches":matches,"progress":progress,"graph_snapshot_requested":args.len()>3,
        "scope":"shared public-API native harness, System allocator, no tracing or assistance"}));
    Ok(())
}
