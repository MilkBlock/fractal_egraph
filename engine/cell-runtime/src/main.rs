#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;
use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let opt = |k: &str| args.windows(2).find(|w| w[0] == k).map(|w| w[1].clone());
    if args.iter().any(|x| x == "--help") || args.is_empty() {
        println!(
            "eggfuse-cell-runtime --program FILE [--inspect-program | --verify-reference-views | --output DIR] [--audit-stages] [--checkpoint-after RUNS] [--save-checkpoint] [--resume FILE] [--extract-unit-cost OPERATOR]"
        );
        return Ok(());
    }
    let bytes = std::fs::read(opt("--program").ok_or("--program required")?)?;
    let program = Program::parse(serde_json::from_slice(&bytes)?)?;
    if args.iter().any(|x| x == "--inspect-program") {
        println!(
            "{}",
            serde_json::json!({"backend":"cell-cpu", "validated":true,
            "congruence_reorder_safe":program.congruence.reorder_safe,
            "congruence_templates":program.congruence.templates.len()})
        );
        return Ok(());
    }
    let verify_compositions = args.iter().any(|x| x == "--verify-compositions");
    if args.iter().any(|x| x == "--verify-reference-views") || verify_compositions {
        let mut control = Executor::new(
            if verify_compositions {
                Graph::new(program.clone())?
            } else {
                Graph::new_with_reference_views(program.clone(), false)?
            },
            true,
        );
        let mut candidate = Executor::new(
            if verify_compositions {
                Graph::new(program)?
            } else {
                Graph::new_with_reference_views(program, true)?
            },
            true,
        );
        if verify_compositions {
            control.compositions_enabled = false;
            candidate.compositions_enabled = true;
        }
        fn state(j: serde_json::Value) -> serde_json::Value {
            let mut j = j;
            let rows = j["rows"].as_array_mut().unwrap();
            rows.sort_by_cached_key(|r| serde_json::to_string(r).unwrap());
            j
        }
        let mut n = 0;
        if state(control.graph.export()?) != state(candidate.graph.export()?) {
            return Err("initial reference-view state differs".into());
        }
        while !control.tasks.is_empty() || !candidate.tasks.is_empty() {
            control.execute_until(Some(n + 1))?;
            candidate.execute_until(Some(n + 1))?;
            if control.steps.len() != candidate.steps.len() {
                return Err(format!("Run count differs at {n}").into());
            }
            if control.steps.len() == n {
                break;
            }
            n += 1;
            for key in [
                "ordinal",
                "ruleset",
                "path",
                "iterations",
                "epoch",
                "logical_matches",
                "changed",
            ] {
                if control.steps[n - 1][key] != candidate.steps[n - 1][key] {
                    return Err(format!("Run {n} {key} differs").into());
                }
            }
            if state(control.snapshots.pop().unwrap()) != state(candidate.snapshots.pop().unwrap())
            {
                return Err(format!("Run {n} full state differs").into());
            }
            if control.binding_audit.pop() != candidate.binding_audit.pop() {
                return Err(format!("Run {n} bindings differ").into());
            }
        }
        println!(
            "{}",
            serde_json::json!({"all_stage_graphs_and_bindings_equal":true,"runs":n,"reference_views":candidate.graph.reference_report(),"compositions":candidate.report()["compositions"],"control_live_cells":control.graph.cells.len(),"candidate_live_cells":candidate.graph.cells.len(),"scope":if verify_compositions{"same-binary multi-rule ablation; standalone Cell CPU, not native egglog"}else{"streaming same-binary storage ablation; standalone Cell CPU, not native egglog"}})
        );
        return Ok(());
    }
    let out = PathBuf::from(opt("--output").ok_or("--output required")?);
    std::fs::create_dir(&out)?;
    let start = Instant::now();
    let audit = args.iter().any(|x| x == "--audit-stages");
    let mut runtime = if let Some(path) = opt("--resume") {
        Executor::resume(
            program,
            serde_json::from_slice(&std::fs::read(path)?)?,
            audit,
        )?
    } else {
        Executor::new(Graph::new(program)?, audit)
    };
    let setup = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let stop = opt("--checkpoint-after")
        .map(|s| s.parse::<usize>())
        .transpose()?;
    #[cfg(feature = "slot-transfer-audit")]
    eggfuse_cell_runtime::transfer_audit::set_active(true);
    if let Err(error) = runtime.execute_until(stop) {
        std::fs::write(
            out.join("failure.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"error":error,"status":"error","completed_steps":runtime.steps}),
            )?,
        )?;
        return Err(error.into());
    }
    #[cfg(feature = "slot-transfer-audit")]
    {
        eggfuse_cell_runtime::transfer_audit::set_active(false);
        std::fs::write(
            out.join("cost-samples.json"),
            serde_json::to_vec(&eggfuse_cell_runtime::transfer_audit::costs())?,
        )?;
        std::fs::write(
            out.join("copies.json"),
            serde_json::to_vec(&eggfuse_cell_runtime::transfer_audit::counts())?,
        )?;
        std::fs::write(
            out.join("rhs.json"),
            serde_json::to_vec(&eggfuse_cell_runtime::transfer_audit::take_digests())?,
        )?;
    }
    let seconds = start.elapsed().as_secs_f64();
    #[cfg(feature = "spine-shapes")]
    std::fs::write(out.join("spine-shapes.json"), serde_json::to_vec(&runtime.spine_shape_report())?)?;
    let mut report = runtime.report();
    report["inference_seconds"] = seconds.into();
    report["setup_seconds"] = setup.into();
    let export_start = Instant::now();
    let mut writer = std::io::BufWriter::new(std::fs::File::create(out.join("state.json"))?);
    runtime.graph.write_export(&mut writer)?;
    std::io::Write::flush(&mut writer)?;
    report["export_seconds"] = export_start.elapsed().as_secs_f64().into();
    let checkpoint = stop.is_some() || args.iter().any(|x| x == "--save-checkpoint");
    report["checkpoint_written"] = checkpoint.into();
    if checkpoint {
        std::fs::write(
            out.join("checkpoint.json"),
            serde_json::to_vec(&runtime.checkpoint())?,
        )?;
    }
    if let Some(op) = opt("--extract-unit-cost") {
        std::fs::write(
            out.join("extraction.json"),
            serde_json::to_vec_pretty(&runtime.graph.extract_unit_cost(&op)?)?,
        )?;
    }
    if runtime.audit {
        std::fs::write(
            out.join("stages.json"),
            serde_json::to_vec(&runtime.snapshots)?,
        )?;
        std::fs::write(
            out.join("binding-audit.json"),
            serde_json::to_vec(
                &serde_json::json!({"initial":runtime.initial_audit,"runs":runtime.binding_audit}),
            )?,
        )?;
    }
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}",
        serde_json::json!({"status":report["status"],"inference_seconds":seconds,"live_cells":runtime.graph.cells.len(),"stats":runtime.graph.stats})
    );
    Ok(())
}
