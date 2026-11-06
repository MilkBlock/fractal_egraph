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
            #[cfg(feature="two-run-blocks")]
            "--two-run-blocks",
            #[cfg(feature="port-frames")]
            "--port-frames",
            #[cfg(feature = "deferred-incidence")]
            "--deferred-incidence",
            #[cfg(feature = "rhs-locals")]
            "--rhs-local-width",
            #[cfg(feature = "rhs-locals")]
            "--rhs-local-profile",
            #[cfg(feature = "structural-depth")]
            "--structural-depth",
            #[cfg(feature = "structural-depth")]
            "--structural-catalog",
            "--reference-program",
        ],
    )?;
    #[cfg(feature="two-run-blocks")]
    let two_run=match opts.get("--two-run-blocks").map(String::as_str).unwrap_or("off"){
        "off"=>false,"on"=>true,_=>return Err("--two-run-blocks requires off or on".into())
    };
    #[cfg(feature="port-frames")]
    let ports=match opts.get("--port-frames").map(String::as_str).unwrap_or("off") {
        "off"=>false,"on"=>true,_=>return Err("--port-frames requires off or on".into())
    };
    #[cfg(feature = "deferred-incidence")]
    let deferred = match opts.get("--deferred-incidence").map(String::as_str).unwrap_or("off") {
        "off" => (false, false), "edges" => (true, false), "on" => (true, true),
        _ => return Err("--deferred-incidence requires off, edges or on".into()),
    };
    #[cfg(feature = "structural-depth")]
    let depth = opts
        .get("--structural-depth")
        .map(|s| s.parse::<u8>())
        .transpose()?
        .unwrap_or(0);
    #[cfg(feature = "structural-depth")]
    let catalog = opts
        .get("--structural-catalog")
        .map(|path| read(Path::new(path)))
        .transpose()?;
    #[cfg(feature = "rhs-locals")]
    let width = opts
        .get("--rhs-local-width")
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    #[cfg(feature = "rhs-locals")]
    let local_profile = opts
        .get("--rhs-local-profile")
        .map(|path| read(Path::new(path)))
        .transpose()?;
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
            #[cfg(feature="two-run-blocks")]
            false,
            #[cfg(feature="port-frames")]
            false,
            #[cfg(feature = "deferred-incidence")]
            (false, false),
            #[cfg(feature = "rhs-locals")]
            (0, None),
            #[cfg(feature = "structural-depth")]
            (0, None),
        )? != stage_digests(
            &path,
            #[cfg(feature="two-run-blocks")]
            two_run,
            #[cfg(feature="port-frames")]
            ports,
            #[cfg(feature = "deferred-incidence")]
            deferred,
            #[cfg(feature = "rhs-locals")]
            (width, local_profile.as_ref()),
            #[cfg(feature = "structural-depth")]
            (depth, catalog.as_ref()),
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
        #[cfg(feature="two-run-blocks")]
        e.configure_two_run(two_run);
        #[cfg(feature="port-frames")]
        e.graph.configure_port_frames(ports);
        #[cfg(feature="two-run-blocks")]
        if two_run {e.graph.configure_port_frames(true);}
        #[cfg(feature = "deferred-incidence")]
        {
            e.graph.configure_deferred_incidence(deferred.0);
            e.graph.configure_deferred_payloads(deferred.1);
        }
        #[cfg(feature = "structural-depth")]
        {
            e.configure_depth(depth, false)?;
            if let Some(c) = &catalog {
                e.use_depth_catalog(c.clone())?;
            }
        }
        #[cfg(feature = "rhs-locals")]
        {
            e.configure_rhs_locals(width)?;
            if let Some(p) = &local_profile {
                e.use_rhs_local_profile(p)?;
            }
        }
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
        #[cfg(feature="port-frames")]
        { counts["port_frames"]=e.graph.port_frames_report(); }
        #[cfg(feature = "deferred-incidence")]
        { counts["deferred_incidence"] = r["deferred_incidence"].clone(); }
        #[cfg(feature = "rhs-locals")]
        {
            counts["rhs_locals"] = e.rhs_local_report();
        }
        #[cfg(feature = "structural-depth")]
        {
            let mut depth = e.depth_report();
            counts["structural_depth"] = depth["stats"].take();
            #[cfg(feature = "receipt-slots")]
            {
                counts["receipt_slots"] = depth["receipt_slots"].take();
            }
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
            #[cfg(feature="two-run-blocks")]
            false,
            #[cfg(feature="port-frames")]
            false,
            #[cfg(feature = "deferred-incidence")]
            (false, false),
            #[cfg(feature = "rhs-locals")]
            (0, None),
            #[cfg(feature = "structural-depth")]
            (0, None)
        )?)?
    );
    Ok(())
}
fn stage_digests(
    path: &Path,
    #[cfg(feature="two-run-blocks")] two_run:bool,
    #[cfg(feature="port-frames")] ports:bool,
    #[cfg(feature = "deferred-incidence")] deferred: (bool, bool),
    #[cfg(feature = "rhs-locals")] local: (usize, Option<&J>),
    #[cfg(feature = "structural-depth")] structural: (u8, Option<&J>),
) -> Result<Vec<String>> {
    let mut e = Executor::new(Graph::new(program(path)?)?, true);
    #[cfg(feature="two-run-blocks")]
    e.configure_two_run(two_run);
    #[cfg(feature="port-frames")]
    e.graph.configure_port_frames(ports);
    #[cfg(feature="two-run-blocks")]
    if two_run {e.graph.configure_port_frames(true);}
    #[cfg(feature = "deferred-incidence")]
    {
        e.graph.configure_deferred_incidence(deferred.0);
        e.graph.configure_deferred_payloads(deferred.1);
    }
    #[cfg(feature = "structural-depth")]
    {
        e.configure_depth(structural.0, false)?;
        if let Some(c) = structural.1 {
            e.use_depth_catalog(c.clone())?;
        }
    }
    #[cfg(feature = "rhs-locals")]
    {
        e.configure_rhs_locals(local.0)?;
        if let Some(p) = local.1 {
            e.use_rhs_local_profile(p)?;
        }
    }
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
