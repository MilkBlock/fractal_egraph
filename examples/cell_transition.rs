//! NEW_OUTPUT. Existing Cell + candidate updates, with a port-reuse ablation.
#[path = "support/cell_transition_case.rs"]
mod case;
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod compare;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 1 {
        return Err("NEW_OUTPUT".into());
    }
    egglog::EGraph::set_num_threads(1);
    let out = std::path::Path::new(&args[0]);
    std::fs::create_dir(out)?;
    std::fs::write(
        out.join("program.egg"),
        format!("{}\n{}", case::SOURCE, case::UPDATES),
    )?;
    let report = case::run()?;
    std::fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report["phases"])?);
    Ok(())
}
