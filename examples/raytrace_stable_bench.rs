//! SOURCE.egg MATRIX_WORKER NEW_OUTPUT [--original]. Full Pass-3 plus export.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod bench;
use cell_assisted_egglog::stable_choice;
use egglog::{EGraph, ast::Command};
use std::{path::Path, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() < 3 || a.len() > 4 || (a.len() == 4 && a[3] != "--original") {
        return Err("SOURCE WORKER NEW_OUTPUT [--original]".into());
    }
    EGraph::set_num_threads(1);
    egglog::set_probe_counts_enabled(false);
    let out = Path::new(&a[2]);
    std::fs::create_dir(out)?;
    let mut eg = EGraph::default();
    let c = eg.parse_program(Some(a[0].clone()), &std::fs::read_to_string(&a[0])?)?;
    let last = c
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("no schedule")?;
    if last + 1 != c.len() {
        return Err("commands after main schedule".into());
    }
    let stable = a.len() == 3;
    let start = Instant::now();
    let commands = if stable {
        stable_choice::setup(&mut eg, &c[..last])?;
        stable_choice::schedule(&mut eg, &c[last..])?
    } else {
        eg.run_program(c[..last].to_vec())?;
        c[last..].to_vec()
    };
    let policy = if stable {
        "Maximum current Expr-size; ties use frozen input structural anchors then minimum derived constructor height and typed structure. Keys are shared immutable DAGs. No e-class ID or hash-only tie-break."
    } else {
        "Original to-hoist :merge new; stable choice is disabled."
    };
    std::fs::write(
        out.join("setup.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"stable_choice":stable,"setup_seconds_excluded":start.elapsed().as_secs_f64(),"schedule":commands.iter().map(ToString::to_string).collect::<Vec<_>>(),"sat_skip_installed":false,"policy":policy}),
        )?,
    )?;
    let selected = ["terms", "terms-helpers", "terms-helpers-helpers"].map(String::from);
    bench::benchmark_hybrid_export(
        &eg,
        &commands,
        if stable {
            "complete raytrace Pass 3, shared structural choice"
        } else {
            "complete raytrace Pass 3, original merge-new policy"
        },
        Path::new(&a[1]),
        &out.join("report.json"),
        &selected,
        &out.join("graphs"),
    )?;
    let r: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("report.json"))?)?;
    println!(
        "native={} cpu={} gpu={} exact_native={} exact_cpu={}",
        r["native"]["run_seconds"],
        r["sorted_commit_cpu"]["runtime"]["run_seconds"],
        r["gpu"]["runtime"]["run_seconds"],
        r["verification"].is_object(),
        r["same_commit_verification"].is_object()
    );
    if !r["same_commit_verification"].is_object() {
        return Err("strict same-commit graph gate failed; exports retained".into());
    }
    Ok(())
}
