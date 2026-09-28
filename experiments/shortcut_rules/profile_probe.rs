//! Per-iteration cost breakdown for pristine egglog runs.
//!
//! Runs a program whose schedule is a sequence of `(run 1)` commands and prints
//! one JSON object per iteration with the engine's own phase timings:
//! search+apply, merge and rebuild, plus per-rule time and match counts.
use egglog::{CommandOutput, EGraph};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let text = std::fs::read_to_string(&args[0])?;
    let mut eg = EGraph::default();
    let outputs = eg.parse_and_run_program(Some(args[0].clone()), &text)?;
    let mut iteration = 0usize;
    let mut first = true;
    print!("[");
    for out in &outputs {
        let CommandOutput::RunSchedule(report) = out else {
            continue;
        };
        iteration += 1;
        let mut entries = report.search_and_apply_time_per_rule.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let rules = entries
            .iter()
            .map(|(name, time)| {
                let matches = report.num_matches_per_rule.get(&**name).copied().unwrap_or(0);
                format!(
                    "{{\"rule\":{:?},\"search_apply_s\":{:.9},\"matches\":{}}}",
                    name.to_string(),
                    time.as_secs_f64(),
                    matches
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let search = report
            .search_and_apply_time_per_ruleset
            .values()
            .map(|d| d.as_secs_f64())
            .sum::<f64>();
        let merge = report
            .merge_time_per_ruleset
            .values()
            .map(|d| d.as_secs_f64())
            .sum::<f64>();
        let rebuild = report
            .rebuild_time_per_ruleset
            .values()
            .map(|d| d.as_secs_f64())
            .sum::<f64>();
        if !first {
            print!(",");
        }
        first = false;
        print!(
            "{{\"iteration\":{iteration},\"updated\":{},\"can_stop\":{},\
             \"search_apply_s\":{search:.9},\"merge_s\":{merge:.9},\"rebuild_s\":{rebuild:.9},\"rules\":[{rules}]}}",
            report.updated, report.can_stop,
        );
    }
    println!("]");
    Ok(())
}
