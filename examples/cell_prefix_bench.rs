//! SOURCE.egg ANALYSIS_DIR MATRIX_WORKER REPORT.json [check|profile]
//! One final schedule; interleaved post-run actions are rejected, not moved.
#[path = "support/live_table_bench.rs"]
mod bench;
use egglog::{EGraph, ast::Command};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if !(4..=5).contains(&a.len()) {
        return Err("SOURCE ANALYSIS WORKER REPORT [check|profile]".into());
    }
    let mut eg = EGraph::default();
    let parsed = eg.parse_program(Some(a[0].clone()), &std::fs::read_to_string(&a[0])?)?;
    let mut init = vec![];
    let mut schedules = vec![];
    let mut checks = vec![];
    for c in parsed {
        match &c {
            Command::RunSchedule(..) => schedules.push(c),
            Command::Check(..) if !schedules.is_empty() => checks.push(c),
            _ if schedules.is_empty() => init.push(c),
            _ => return Err("only schedules and checks are accepted after the first run".into()),
        }
    }
    if schedules.is_empty() {
        return Err("missing run schedule".into());
    }
    eg.run_program(init)?;
    let checked = a.get(4).is_some_and(|x| x == "check");
    let profile = a.get(4).is_some_and(|x| x == "profile");
    let mut control = eg.clone();
    control.run_program(schedules.clone())?;
    control.run_program(checks)?;
    bench::benchmark_cells(
        &eg,
        &schedules,
        &a[0],
        a[2].as_ref(),
        a[1].as_ref(),
        checked,
        profile,
        a[3].as_ref(),
    )
}
