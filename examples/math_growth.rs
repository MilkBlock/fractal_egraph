//! Round-boundary measurements; no logical history capture.
use egglog::{EGraph, CommandOutput, ast::{Command, GenericSchedule}};
use egg_layout::reuse_graph::{library::Library, native_storage};
use std::{collections::BTreeSet, time::Instant};
use serde_json::json;
fn sample(eg: &EGraph, round: usize, matches: usize, elapsed: f64, mode: &str) {
    // RSS before the diagnostic scan; no second engine or exported graph.
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().unwrap();
    let rss = String::from_utf8(out.stdout).unwrap().trim().parse::<u64>().unwrap()*1024;
    let mut classes = BTreeSet::new();
    let mut enodes = 0usize;
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        if f.schema().output.name() != "Math" { continue; }
        eg.function_for_each(&name, |r| {
            enodes += 1; // native rebuilt table keys already unique, virtual rows decoded
            classes.insert(eg.get_canonical_value(*r.vals.last().unwrap(), &f.schema().output));
        }).unwrap();
    }
    let virtual_rows: usize = eg.virtual_fact_stats().iter().map(|(_,s)|s.virtual_rows).sum();
    println!("{}",json!({"mode":mode,"round":round,"cumulative_matches":matches,"rss_bytes":rss,"expanded_enodes":enodes,"expanded_eclasses":classes.len(),"virtual_rows":virtual_rows,"execution_seconds":elapsed}));
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len()!=3 { return Err("math_growth SOURCE.egg native|virtual LIBRARY.json".into()); }
    let mode=&args[1];
    if mode!="native" && mode!="virtual" {return Err("invalid mode".into());}
    let lib=if mode=="virtual" {Some(Library::load(std::path::Path::new(&args[2]))?)} else {None};
    let mut eg=EGraph::default();
    let cmds=eg.parse_program(Some(args[0].clone()), &std::fs::read_to_string(&args[0])?)?;
    let (mut round,mut matches,mut elapsed)=(0,0,0.0);
    for cmd in cmds {
        match cmd {
            Command::RunSchedule(GenericSchedule::Repeat(_, n, inner)) => {
                if !matches!(inner.as_ref(),GenericSchedule::Run(..)) {return Err("simple (run N) required".into());}
                sample(&eg,round,matches,elapsed,mode);
                for _ in 0..n {
                    let t=Instant::now();
                    let outputs=eg.run_program(vec![Command::RunSchedule((*inner).clone())])?;
                    let mut stop=false;
                    for o in outputs { if let CommandOutput::RunSchedule(r)=o {matches+=r.num_matches_per_rule.values().sum::<usize>();stop=r.can_stop;} }
                    round+=1;
                    if round==1 {if let Some(l)=&lib {native_storage::pack_library(&mut eg,l)?;} }
                    elapsed+=t.elapsed().as_secs_f64();
                    sample(&eg,round,matches,elapsed,mode);
                    if stop {break;}
                }
            }
            other => {eg.run_program(vec![other])?;}
        }
    }
    Ok(())
}
