//! SOURCE.egg MATRIX_WORKER NEW_OUTPUT [--original] [--joint-sat].
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod bench;
use cell_assisted_egglog::stable_choice;
use egglog::{EGraph, ast::Command};
use std::{path::Path, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() < 3
        || a[3..].iter().any(|s| {
            !["--original", "--joint-sat", "--all-rules", "--audit-only", "--cell-audit-only"].contains(&s.as_str())
        })
    {
        return Err("SOURCE WORKER NEW_OUTPUT [--original] [--joint-sat]".into());
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
    let stable = !a[3..].iter().any(|s| s == "--original");
    let joint = a[3..].iter().any(|s| s == "--joint-sat");
    let all = a[3..].iter().any(|s| s == "--all-rules");
    let cell_audit=a[3..].iter().any(|s|s=="--cell-audit-only");
    let audit = cell_audit || a[3..].iter().any(|s| s == "--audit-only");
    let start = Instant::now();
    let mut hook = None;
    let commands = if joint {
        let r = if stable {
            cell_assisted_egglog::parameterized_cell::Runtime::new_joint_stable_commands(
                c[..last].to_vec(),
            )?
        } else {
            cell_assisted_egglog::parameterized_cell::Runtime::new_joint_commands(
                c[..last].to_vec(),
            )?
        };
        let (engine, observer) = r.detach_joint()?;
        eg = engine;
        hook =
            Some(Box::new(observer) as Box<dyn cell_assisted_egglog::table_live::SaturationHook>);
        if stable {
            stable_choice::schedule(&mut eg, &c[last..])?
        } else {
            c[last..].to_vec()
        }
    } else if stable {
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
            &serde_json::json!({"stable_choice":stable,"setup_seconds_excluded":start.elapsed().as_secs_f64(),"schedule":commands.iter().map(ToString::to_string).collect::<Vec<_>>(),"sat_skip_installed":joint,"policy":policy}),
        )?,
    )?;
    let mut selected = ["terms", "terms-helpers", "terms-helpers-helpers"]
        .map(String::from)
        .to_vec();
    if all {
        fn sets(s: &egglog::ast::Schedule, names: &mut std::collections::BTreeSet<String>) {
            use egglog::ast::GenericSchedule::*;
            match s {
                Run(_, r) => {
                    names.insert(r.ruleset.clone());
                }
                Sequence(_, xs) => {
                    for x in xs {
                        sets(x, names)
                    }
                }
                Repeat(_, _, x) | Saturate(_, x) => sets(x, names),
            }
        }
        let mut names = std::collections::BTreeSet::new();
        for c in &commands {
            if let Command::RunSchedule(s) = c {
                sets(s, &mut names)
            }
        }
        // This is our stable-choice adapter, not a rule from the original .egg.
        names.remove("cell-stable-keys");
        selected = names.into_iter().collect();
    }
    if audit {
        if !all {
            return Err("--audit-only currently requires --all-rules".into());
        }
        let mut r = bench::full_candidate(
            eg,
            &commands,
            Some(Path::new(&a[1])),
            !cell_audit,
            Some(&selected),
            hook,
        )?;
        if cell_audit {r.enable_cell_query_audit()?;}
        let result = r.run(&commands);
        std::fs::write(
            out.join("audit.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"complete":result.is_ok(),"error":result.as_ref().err().map(ToString::to_string),"runtime":r.report()}),
            )?,
        )?;
        result?;
        return Ok(());
    }
    if all {
        bench::benchmark_all_export(
            &eg,
            &commands,
            Path::new(&a[1]),
            &out.join("report.json"),
            &selected,
            &out.join("graphs"),
            false,
            hook,
        )?;
    } else if let Some(hook) = hook {
        bench::benchmark_hybrid_export_joint(
            &eg,
            &commands,
            "complete raytrace Pass 3, joint sat-skip enabled",
            Path::new(&a[1]),
            &out.join("report.json"),
            &selected,
            &out.join("graphs"),
            hook,
        )?;
    } else {
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
    }
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
    if joint && !r["joint_verification"].is_object() {
        return Err("joint hook differs from GPU control; exports retained".into());
    }
    Ok(())
}
