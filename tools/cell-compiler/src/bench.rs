use super::*;
use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
use std::time::Instant;

pub(super) fn run(args: Vec<String>) -> Result<()> {
    let opts = options(
        args.into_iter(),
        &[
            "--source",
            "--bake-source",
            "--case",
            "--catalog",
            "--program",
            "--repeat",
            #[cfg(feature = "structural-depth")]
            "--structural-depth",
            "--reference-program",
        ],
    )?;
    #[cfg(feature = "structural-depth")]
    let depth = opts
        .get("--structural-depth")
        .map(|s| s.parse::<u8>())
        .transpose()?
        .unwrap_or(0);
    let n = opts
        .get("--repeat")
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(5);
    if n == 0 {
        return Err("--repeat must be positive".into());
    }
    let tmp = tempfile::tempdir()?;
    let path = if let Some(path) = opts.get("--program") {
        if ["--case", "--source", "--bake-source", "--catalog"]
            .iter()
            .any(|k| opts.contains_key(*k))
        {
            return Err("use a program or a source".into());
        }
        PathBuf::from(path)
    } else {
        let input = inputs(&opts)?;
        let path = tmp.path().join("compiled");
        compile(
            &input.source,
            input.training.as_deref(),
            input.catalog.as_ref(),
            &path,
            &opts,
        )?;
        path.join("program.json")
    };
    if let Some(reference) = opts.get("--reference-program") {
        if stage_digests(
            Path::new(reference),
            #[cfg(feature = "structural-depth")]
            0,
        )? != stage_digests(
            &path,
            #[cfg(feature = "structural-depth")]
            depth,
        )? {
            return Err("per-Run reference mismatch".into());
        }
    }
    let p = program(&path)?;
    let mut fingerprint = None;
    let mut times = vec![];
    let mut counts = J::Null;
    for _ in 0..n {
        let mut e = Executor::new(Graph::new(p.clone())?, false);
        #[cfg(feature = "structural-depth")]
        e.configure_depth(depth, false)?;
        let start = Instant::now();
        e.execute()?;
        times.push(start.elapsed().as_secs_f64() * 1000.);
        let mut state = e.graph.export()?;
        state["rows"]
            .as_array_mut()
            .ok_or("missing rows")?
            .sort_by_cached_key(J::to_string);
        let digest = eggfuse_cell_runtime::checksum::digest(&state);
        if fingerprint.as_ref().is_some_and(|prior| prior != &digest) {
            return Err("benchmark final states differ".into());
        }
        fingerprint = Some(digest);
        let r = e.report();
        let g = &e.graph.stats;
        let visits = g.residual_candidates
            + g.direct_candidates
            + r["multi_stage"]["stats"]["merge_candidate_rows"]
                .as_u64()
                .unwrap()
            + r["stage_spine"]["stats"]["pivot_candidates"]
                .as_u64()
                .unwrap()
            + r["entry_schedule"]["stats"]["predicate_checks"]
                .as_u64()
                .unwrap();
        counts = json!({"all_candidate_visits":visits,"free_join":r["free_join"],"consumer_seed_deliveries":r["entry_schedule"]["stats"]["consumer_seed_deliveries"],"empty_join_skips":r["sieve"]["stats"]["empty_join_skips"],"register_answers":g.register_answers,"single_writes":g.single_writes,
            "semijoin":r["semijoin"],"view_consumers":r["view_consumers"],"view_storage":r["view_storage"],"publication":r["publication"],"entry_row_reads":r["entry_schedule"]["stats"]["row_decodes"],
            "stage_spine":r["stage_spine"]["stats"],"neighbor_context":r["neighbor_context"],"local_context":r["local_context"],
            "handoff_closed":g.handoff_closed,"handoff_closed_fallback":g.handoff_closed_fallback,"handoff_deep":g.handoff_deep,"handoff_lets":g.handoff_lets});
        #[cfg(feature = "structural-depth")]
        {
            counts["structural_depth"] = e.depth_report()["stats"].clone();
        }
    }
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    println!(
        "{}",
        json!({"backend":"standalone-cell-cpu","program_sha256":p.artifact["sha256"],"inference_ms":times,
        "median_ms":(sorted[(n-1)/2]+sorted[n/2])/2.,"counts":counts,"final_state_sha256":fingerprint})
    );
    Ok(())
}
pub(super) fn stages(path: &Path) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&stage_digests(
            path,
            #[cfg(feature = "structural-depth")]
            0
        )?)?
    );
    Ok(())
}
fn stage_digests(
    path: &Path,
    #[cfg(feature = "structural-depth")] depth: u8,
) -> Result<Vec<String>> {
    let mut e = Executor::new(Graph::new(program(path)?)?, true);
    #[cfg(feature = "structural-depth")]
    e.configure_depth(depth, false)?;
    let mut hashes = vec![];
    let sorted = |mut state: J| -> J {
        state["rows"]
            .as_array_mut()
            .unwrap()
            .sort_by_cached_key(J::to_string);
        state
    };
    hashes.push(eggfuse_cell_runtime::checksum::digest(&sorted(
        e.graph.export()?,
    )));
    while !e.tasks.is_empty() {
        let i = e.steps.len();
        e.execute_until(Some(i + 1))?;
        if e.steps.len() == i {
            break;
        }
        let step = &e.steps[i];
        let schedule = [
            "ordinal",
            "ruleset",
            "path",
            "iterations",
            "epoch",
            "logical_matches",
            "changed",
        ]
        .map(|k| &step[k]);
        let state = sorted(e.snapshots.pop().ok_or("missing graph snapshot")?);
        let bindings = e.binding_audit.pop().ok_or("missing binding snapshot")?;
        hashes.push(eggfuse_cell_runtime::checksum::digest(&json!([
            schedule, state, bindings
        ])));
    }
    Ok(hashes)
}
fn program(path: &Path) -> Result<Program> {
    let mut value = read(path)?;
    let claimed = value
        .as_object_mut()
        .ok_or("invalid program")?
        .remove("sha256")
        .ok_or("missing program digest")?;
    if claimed != eggfuse_cell_runtime::checksum::artifact_digest(&value) {
        return Err("program checksum mismatch".into());
    }
    value["sha256"] = claimed;
    Ok(Program::parse(value)?)
}
