//! SOURCE.egg WORKER NEW_OUTPUT [--sort-hoist|--sequential]. Diagnostic only.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod bench;
#[path = "support/pass3_diff.rs"]
mod pass3_diff;
use egglog::{EGraph, ast::Command};
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3
        || args.len() > 4
        || (args.len() == 4 && !["--sort-hoist", "--sequential"].contains(&args[3].as_str()))
    {
        return Err("SOURCE.egg WORKER NEW_OUTPUT [--sort-hoist|--sequential]".into());
    }
    EGraph::set_num_threads(1);
    egglog::set_probe_counts_enabled(false);
    let mut eg = EGraph::default();
    let commands = eg.parse_program(Some(args[0].clone()), &std::fs::read_to_string(&args[0])?)?;
    let last = commands
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("missing schedule")?;
    if last + 1 != commands.len() {
        return Err("commands after main schedule".into());
    }
    eg.run_program(commands[..last].to_vec())?;
    let mut selected = vec![
        "terms".into(),
        "terms-helpers".into(),
        "terms-helpers-helpers".into(),
    ];
    if args.get(3).is_some_and(|s| s == "--sort-hoist") {
        selected.push("boundary-analysis".into());
    }
    let out = Path::new(&args[2]);
    std::fs::create_dir(out)?;
    pass3_diff::run(
        eg,
        &commands[last..],
        &selected,
        Path::new(&args[1]),
        out,
        args.get(3).is_some_and(|s| s == "--sequential"),
    )
}
