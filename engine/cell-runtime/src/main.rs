use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let opt = |k: &str| args.windows(2).find(|w| w[0] == k).map(|w| w[1].clone());
    if args.iter().any(|x| x == "--help") || args.is_empty() {
        println!(
            "eggfuse-cell-runtime --program FILE [--inspect-program | --output DIR] [--audit-stages] [--checkpoint-after RUNS] [--save-checkpoint] [--resume FILE] [--extract-unit-cost OPERATOR]"
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
    if let Err(error) = runtime.execute_until(stop) {
        std::fs::write(
            out.join("failure.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"error":error,"status":"error","completed_steps":runtime.steps}),
            )?,
        )?;
        return Err(error.into());
    }
    let seconds = start.elapsed().as_secs_f64();
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
