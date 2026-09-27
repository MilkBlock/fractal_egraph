//! Local RipSat reuse microbenchmark. Not a tier0 end-to-end speedup claim.
use egg_layout::reuse_graph::ripsat_runtime::RipSatRuntime;
use serde_json::{Value, json};
use std::{path::Path, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("ripsat_incremental LIBRARY.json OUTPUT.json".into());
    }
    let lib: Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let source = lib["entries"][0]["source"]
        .as_str()
        .ok_or("missing entry source")?;
    // Concrete boundary extension of the first math Add composition.
    let graft = "(Add (RipenInput 0) (Var \"external\"))";
    let check = "(check (= (Add (RipenInput 0) (Var \"external\")) (Add (Var \"external\") (RipenInput 0))))";
    let mut runtime = RipSatRuntime::new(source)?;
    let t = Instant::now();
    let first = runtime.graft(0, graft)?;
    let incremental = t.elapsed().as_secs_f64();
    runtime.check(first["state"].as_u64().unwrap() as usize, check)?;
    let t = Instant::now();
    let hit = runtime.graft(0, graft)?;
    let reuse = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut fresh = RipSatRuntime::new(&format!("{source}\n{graft}"))?;
    let from_scratch = t.elapsed().as_secs_f64();
    fresh.check(0, check)?;
    let path = Path::new(&args[1]);
    runtime.save_recipes(&path.with_extension("recipes.json"))?;
    let report = json!({"scope":"one completed math library entry, one same-binding external graft; not full math benchmark or automatic tier0 placement", "incremental_seconds":incremental,"cached_transition_seconds":reuse,"fresh_compile_and_saturate_seconds":from_scratch,"first":first,"hit":hit,"check":"passed"});
    std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    Ok(())
}
