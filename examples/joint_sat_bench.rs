//! WORKER NEW_OUTPUT [SNAPSHOT]. Correctness experiment, not a speedup claim.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::{
    parameterized_cell::{
        Runtime,
        joint::{SCHEDULE, SETS},
    },
    table_live,
};
use egglog::{EGraph, ast::Command};
use serde_json::json;
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !(2..=3).contains(&args.len()) {
        return Err("WORKER NEW_OUTPUT [SNAPSHOT]".into());
    }
    EGraph::set_num_threads(1);
    egglog::set_probe_counts_enabled(false);
    let out = Path::new(&args[1]);
    std::fs::create_dir(out)?;
    let snapshot = args.get(2);
    let source = if let Some(path) = snapshot {
        std::fs::read_to_string(path)?
    } else {
        include_str!("../experiments/parameterized_cell/terms.egg").to_string()
    };
    let mut parser = EGraph::default();
    let mut setup = parser.parse_program(None, &source)?;
    if snapshot.is_some() {
        let first = setup
            .iter()
            .rposition(|c| matches!(c, Command::RunSchedule(_)))
            .ok_or("snapshot has no schedule")?;
        if first + 1 != setup.len() {
            return Err("snapshot must end with its main schedule".into());
        }
        setup.truncate(first); // Retain initialization; omit the main Pass-3 schedule.
    }
    let initial = Runtime::new_joint_commands(setup.clone())?;
    let base = initial.engine().clone();
    let mut parser = base.clone();
    let phases = if snapshot.is_some() {
        vec![
            (
                "captured-input terms only",
                String::new(),
                SCHEDULE.to_string(),
            ),
            ("quiescent", String::new(), SCHEDULE.to_string()),
        ]
    } else {
        let seeds = "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3))";
        let wide=(0..12).map(|i|format!("(set (ExtractedExpr (V {})) (TCPair (TermLeaf {}) {})) (Bop (Add) (V 0) (V {}))",i+2,i+2,i+4,i+2)).collect::<Vec<_>>().join("\n");
        vec![
            (
                "fresh parallel roots and three-deep chain",
                format!(
                    "{seeds}\n{wide}\n(Bop (Mul) (Bop (Add) (V 0) (V 2)) (V 1)) (Bop (Add) (Bop (Mul) (Bop (Add) (V 0) (V 2)) (V 1)) (V 0))"
                ),
                SCHEDULE.into(),
            ),
            (
                "new extension of certified result",
                "(Bop (Mul) (Bop (Add) (V 0) (V 3)) (V 1))".into(),
                SCHEDULE.into(),
            ),
            (
                "finite Run has no macro",
                "(Bop (Mul) (V 0) (V 1))".into(),
                "(run terms 1)".into(),
            ),
            ("finish finite-run work", String::new(), SCHEDULE.into()),
            (
                "external cost update",
                "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 40) 1))".into(),
                SCHEDULE.into(),
            ),
            ("input union", "(union (V 0) (V 1))".into(), SCHEDULE.into()),
            ("quiescent", String::new(), SCHEDULE.into()),
        ]
    };
    let commands = phases
        .iter()
        .map(|(_, _, s)| parser.parse_program(None, s))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let all = commands.iter().flatten().cloned().collect::<Vec<_>>();
    let selected = SETS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut native = base.clone();
    let mut references = vec![];
    let mut native_counts = vec![];
    for ((_, input, _), commands) in phases.iter().zip(&commands) {
        native.parse_and_run_program(None, input)?;
        native.run_program(commands.clone())?;
        references.push(native.clone());
        native_counts.push(
            native
                .get_overall_run_report()
                .num_matches_per_rule
                .iter()
                .filter(|(n, _)| {
                    SETS.iter()
                        .any(|s| native.cell_rule_names(s).unwrap().contains(&n.to_string()))
                })
                .map(|(_, n)| *n)
                .sum::<usize>(),
        );
    }
    let mut lanes = vec![];
    let mut prepared = Some(initial);
    for gpu in [false, true] {
        let initial = match prepared.take() {
            Some(r) => r,
            None => Runtime::new_joint_commands(setup.clone())?,
        };
        let (engine, observer) = initial.detach_joint()?;
        let mut r = table_live::Runtime::new_hybrid(
            engine,
            &all,
            gpu.then_some(Path::new(&args[0])),
            gpu,
            &selected,
        )?;
        r.set_saturation_hook(Box::new(observer));
        let mut evidence = vec![];
        for (i, ((label, input, _), commands)) in phases.iter().zip(&commands).enumerate() {
            r.input(input)?;
            r.run(commands)?;
            let exact = compare::equal(&base, &references[i], r.engine());
            if snapshot.is_none() && i < 2 {
                assert_eq!(
                    r.report()["rules"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x["stats"]["emitted_bindings"].as_u64().unwrap())
                        .sum::<u64>(),
                    0
                );
                assert!(
                    r.report()["saturation_diagnostics"]["joint"]["stats"]["macro_instances"]
                        .as_u64()
                        .unwrap()
                        > 0
                );
            }
            eprintln!(
                "{} {label}: {}",
                if gpu { "GPU" } else { "CPU" },
                exact.is_ok()
            );
            evidence.push(json!({"phase":i,"label":label,"verification":exact.as_ref().ok(),"error":exact.as_ref().err().map(ToString::to_string),"native_smooth_bindings":native_counts[i],"runtime":r.report()}));
            if let Err(error) = exact {
                std::fs::write(
                    out.join("failure.json"),
                    serde_json::to_vec_pretty(&evidence)?,
                )?;
                return Err(error);
            }
        }
        lanes.push(json!({"lane":if gpu{"joint-gpu"}else{"joint-cpu"},"phases":evidence}));
    }
    let report = json!({"joint_sat_verified":true,"snapshot":snapshot,"scope":if snapshot.is_some(){"Actual saved raytrace input after its native initialization; only the three terms rulesets, not full Pass 3 or extraction benchmark."}else{"Actual native commits and shared CPU/GPU scheduler; joint closure macros on fresh producer DAGs. Original queries retained for uncovered states. Correctness, not a speedup benchmark."},"lanes":lanes});
    std::fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if snapshot.is_none() {
        std::fs::write(
            out.join("program.egg"),
            format!(
                "{source}\n{}",
                phases
                    .iter()
                    .map(|(_, i, s)| format!("{i}\n{s}\n"))
                    .collect::<String>()
            ),
        )?;
    }
    Ok(())
}
