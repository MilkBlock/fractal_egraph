use super::*;
use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
use std::time::Instant;
type ModuleFlags<'a> = BTreeMap<&'a str, (&'static str, &'a str)>;

fn lane_process(lane: &str, modules: &ModuleFlags<'_>) -> Result<Command> {
    let mut cmd = Command::new(std::env::current_exe()?);
    let filtered = matches!(
        lane,
        "filtered" | "free-join" | "free-join-entry" | "free-join-planned"
    );
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("CELL_CPU_") {
            cmd.env_remove(k);
        }
    }
    cmd.env(
        "CELL_CPU_SHARED_NODE_SCHEDULE",
        if lane == "baseline" { "0" } else { "1" },
    )
    .env(
        "CELL_CPU_FREE_JOIN",
        match lane {
            "free-join-planned" => "planned",
            "free-join-entry" => "entry",
            "free-join" => "1",
            _ => "0",
        },
    )
    .env("CELL_CPU_SIEVE", if filtered { "1" } else { "0" })
    .env(
        "CELL_CPU_JOIN_INTERSECTION",
        if filtered { "1" } else { "0" },
    );
    cmd.envs(modules.values().copied());
    Ok(cmd)
}
pub(super) fn run(mut args: Vec<String>) -> Result<()> {
    let verify = args.iter().any(|v| v == "--verify-stages");
    args.retain(|v| v != "--verify-stages");
    let opts = options(
        args.into_iter(),
        &[
            "--case",
            "--source",
            "--bake-source",
            "--program",
            "--repeat",
            "--lane",
            "--modules",
            "--slot-transfer",
        ],
    )?;
    let mut modules = BTreeMap::new();
    if let Some(requested) = opts.get("--modules") {
        for name in requested.split(',') {
            let flag = match name {
                "local-congruence" => ("CELL_CPU_CONGRUENCE", "1"),
                "union-graft" => ("CELL_CPU_UNION_GRAFT", "1"),
                "local-context" => ("CELL_CPU_LOCAL_CONTEXT", "1"),
                "stage-spine" => ("CELL_CPU_STAGE_SPINE", "1"),
                "slot-transfer" => ("CELL_CPU_SLOT_TRANSFER", "1"),
                "range-guards" => ("CELL_CPU_RANGE_GUARDS", "1"),
                "fd-keys" => ("CELL_CPU_FD_KEYS", "1"),
                "scalar-merge" => ("CELL_CPU_SCALAR_MERGE", "1"),
                "static-aliases" => ("CELL_CPU_KNOWN_RESULTS", "static"),
                _ => return Err(format!("unknown bench module {name:?}").into()),
            };
            if modules.insert(name, flag).is_some() {
                return Err(format!("duplicate bench module {name:?}").into());
            }
        }
    }
    if let Some(mode) = opts.get("--slot-transfer") {
        mode.parse::<eggfuse_cell_runtime::graph::SlotTransfer>()?;
        if modules
            .insert("slot-transfer", ("CELL_CPU_SLOT_TRANSFER", mode.as_str()))
            .is_some()
        {
            return Err(
                "select slot transfer using either --slot-transfer or --modules slot-transfer"
                    .into(),
            );
        }
    }
    let n = opts
        .get("--repeat")
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(5);
    if n == 0 {
        return Err("--repeat must be positive".into());
    }
    let dir = tempfile::tempdir()?;
    let program = if let Some(p) = opts.get("--program") {
        if ["--case", "--source", "--bake-source"]
            .iter()
            .any(|k| opts.contains_key(*k))
        {
            return Err("use a program or compile an input".into());
        }
        PathBuf::from(p)
    } else {
        let (s, b) = inputs(&opts)?;
        let out = dir.path().join("compiled");
        compile(&s, b.as_deref(), &out, &opts)?;
        out.join("program.json")
    };
    let lane = opts.get("--lane").map(String::as_str).unwrap_or("nodes");
    if ![
        "baseline",
        "nodes",
        "filtered",
        "free-join",
        "free-join-entry",
        "free-join-planned",
    ]
    .contains(&lane)
    {
        return Err("unknown bench lane".into());
    }
    if verify {
        let stages = |name: &str, flags: &ModuleFlags<'_>| -> Result<Vec<String>> {
            let output = lane_process(name, flags)?
                .arg("internal-stages")
                .arg(&program)
                .output()?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
            }
            Ok(serde_json::from_slice(&output.stdout)?)
        };
        let control = if !modules.is_empty() {
            lane
        } else if lane == "free-join-planned" {
            "free-join-entry"
        } else if lane == "free-join-entry" {
            "free-join"
        } else if lane == "free-join" {
            "filtered"
        } else {
            "baseline"
        };
        let mut control_modules = BTreeMap::new();
        if opts.contains_key("--slot-transfer") {
            control_modules = modules.clone();
            control_modules.remove("slot-transfer");
        }
        let baseline = stages(control, &control_modules)?;
        let candidate = stages(lane, &modules)?;
        if baseline != candidate {
            let first = baseline
                .iter()
                .zip(&candidate)
                .position(|(a, b)| a != b)
                .unwrap_or(baseline.len().min(candidate.len()));
            return Err(format!("graph, schedule or bindings differ at stage {first}").into());
        }
        eprintln!(
            "verified initial graph and {} Run snapshots against {control}",
            baseline.len() - 1
        );
    }
    let status = lane_process(lane, &modules)?
        .arg("internal-worker")
        .arg(program)
        .arg(n.to_string())
        .arg(lane)
        .args(modules.keys())
        .status()?;
    if !status.success() {
        return Err(format!("benchmark worker failed ({status})").into());
    }
    Ok(())
}
pub(super) fn worker(args: Vec<String>) -> Result<()> {
    if args.len() < 3 {
        return Err("invalid worker arguments".into());
    }
    let p = program(Path::new(&args[0]))?;
    let n: usize = args[1].parse()?;
    if n == 0 {
        return Err("worker requires a positive repeat count".into());
    }
    let mut times = vec![];
    let mut fingerprint = None;
    let mut counts = J::Null;
    let mut reverse_keys = Vec::new();
    let mut range_maintenance_ms = Vec::new();
    for _ in 0..n {
        let mut e = Executor::new(Graph::new(p.clone())?, false);
        let start = Instant::now();
        e.execute_until(None)?;
        times.push(start.elapsed().as_secs_f64() * 1000.);
        let mut state = e.graph.export()?;
        state["rows"]
            .as_array_mut()
            .ok_or("missing rows")?
            .sort_by_cached_key(|r| r.to_string());
        let hash = eggfuse_cell_runtime::checksum::digest(&state);
        if fingerprint.as_ref().is_some_and(|f| f != &hash) {
            return Err("benchmark final states differ between repetitions".into());
        }
        fingerprint = Some(hash);
        let g = &e.graph.stats;
        let r = e.report();
        let merged = r["multi_stage"]["stats"]["merge_candidate_rows"]
            .as_u64()
            .ok_or("missing merge counter")?;
        let pivots = r["stage_spine"]["stats"]["pivot_candidates"]
            .as_u64()
            .ok_or("missing pivot counter")?;
        let predicates = r["entry_schedule"]["stats"]["predicate_checks"]
            .as_u64()
            .ok_or("missing predicate counter")?;
        let candidate_visits =
            g.residual_candidates + g.direct_candidates + merged + pivots + predicates;
        let mut c = json!({
            "all_candidate_visits": candidate_visits,
            "merge_candidate_rows": merged,
            "pivot_candidates": pivots,
            "predicate_checks": predicates,
            "residual_candidates": g.residual_candidates,
            "direct_candidates": g.direct_candidates,
            "consumer_seed_deliveries": r["entry_schedule"]["stats"]["consumer_seed_deliveries"],
            "sieve_probes": r["sieve"]["stats"]["probes"],
            "empty_join_skips": r["sieve"]["stats"]["empty_join_skips"],
            "constructor_requests": g.constructor_requests,
            "static_eliminated": e.graph.known_stats.static_eliminated,
            "lazy_prefix_eliminated": e.graph.known_stats.lazy_prefix_eliminated,
            "cells_created": g.cells_created,
            "rhs_frames_avoided": g.rhs_frames_avoided,
            "local_context":r["local_context"],
            "register_seeds":g.register_seeds,"register_answers":g.register_answers,
            "handoff_attempts":g.handoff_attempts, "handoff_lets":g.handoff_lets,
            "handoff_sets":g.handoff_sets, "handoff_deep":g.handoff_deep,
            "handoff_closed":g.handoff_closed, "handoff_closed_fallback":g.handoff_closed_fallback,
            "single_writes":g.single_writes,"transient_writes":g.transient_writes,
            "slot_transfer": g.slot_transfer,
            "slot_transfer_mode": e.graph.slot_transfer,
            "free_join": r["free_join"],
            "runs": e.steps.len()
        });
        if args.len() > 3 {
            let graft = &r["union_graft"];
            let spine = &r["stage_spine"];
            // The historical visit counter omits the rows checked by the graft matcher.
            c["candidate_visits_with_graft_pairs"] = json!(
                candidate_visits
                    + graft["stats"]["candidate_pairs"]
                        .as_u64()
                        .ok_or("missing graft pair counter")?
            );
            c["local_congruence"] = r["local_congruence"].clone();
            c["union_graft"] = json!({
                "enabled": graft["enabled"], "compiled_entries": graft["compiled_entries"],
                "context_row_reads": graft["stats"]["context_row_reads"],
                "routed_seeds": graft["stats"]["routed_seeds"],
                "candidate_pairs": graft["stats"]["candidate_pairs"],
                "bindings": graft["stats"]["bindings"],
            });
            c["stage_spine"] = json!({
                "enabled": spine["enabled"], "capture_reads": spine["stats"]["capture_reads"],
                "resumed_steps": spine["stats"]["resumed_steps"], "cross_stage_steps": spine["stats"]["cross_stage_steps"],
                "max_chain": spine["stats"]["max_chain"],
                "root_attempts": spine["stats"]["root_attempts"],
                "root_handles_created": spine["stats"]["root_handles_created"],
                "pre_resume_port_lookups": spine["stats"]["pre_resume_port_lookups"],
                "contexts_created": spine["stats"]["contexts_created"],
                "captured_anchors": spine["stats"]["captured_anchors"],
                "distinct_anchor_keys": spine["stats"]["distinct_anchor_keys"],
                "anchor_publications": spine["stats"]["anchor_publications"],
                "receipt_hits": spine["stats"]["receipt_hits"],
                "receipt_redirects": spine["stats"]["receipt_redirects"],
                "receipt_rejections": spine["stats"]["receipt_rejections"],
                "alias_query_forwards": spine["stats"]["alias_query_forwards"],
                "alias_effect_forwards": spine["stats"]["alias_effect_forwards"],
                "port_cache_fills": spine["stats"]["port_cache_fills"],
                "routed_spine_seeds": spine["stats"]["routed_spine_seeds"],
                "routed_matcher_seeds": spine["stats"]["routed_matcher_seeds"],
                "mixed_routed_queries": spine["stats"]["mixed_routed_queries"],
            });
        }
        let mut physical = r["physical"].clone();
        if let Some(range) = physical["range"].as_object_mut() {
            let ns = range
                .remove("maintenance_ns")
                .and_then(|v| v.as_u64())
                .ok_or("missing range timer")?;
            range_maintenance_ms.push(ns as f64 / 1e6);
        }
        physical["scalar_merge_calls"] = json!(g.scalar_merge_calls);
        physical["scalar_unchanged"] = json!(g.scalar_unchanged);
        physical["scalar_old_projection"] = json!(g.scalar_old_projection);
        physical["merge_program_calls"] = json!(g.merge_program_calls);
        physical["merge_program_operations"] = json!(g.merge_program_operations);
        c["physical"] = physical;
        // Over-budget attempts stop at the first overflowing hash bucket.
        // Randomized key iteration changes the inspected prefix, but not the plan.
        if args[2] == "free-join-planned" {
            let visits = c["free_join"]
                .as_object_mut()
                .and_then(|stats| stats.remove("reverse_keys"))
                .and_then(|value| value.as_u64())
                .ok_or("missing reverse key counter")?;
            reverse_keys.push(visits);
        }
        if !counts.is_null() && counts != c {
            return Err("benchmark work differs between repetitions".into());
        }
        counts = c;
    }
    if let (Some(min), Some(max)) = (reverse_keys.iter().min(), reverse_keys.iter().max()) {
        counts["free_join"]["reverse_keys_range"] = json!([min, max]);
    }
    times.sort_by(f64::total_cmp);
    let median = (times[(n - 1) / 2] + times[n / 2]) / 2.;
    println!(
        "{}",
        json!({"backend":"standalone-cell-cpu","lane":args[2],"modules_requested":&args[3..],"program_sha256":p.artifact["sha256"],"inference_ms":times,"range_maintenance_ms":range_maintenance_ms,"median_ms":median,"counts":counts,"final_state_sha256":fingerprint})
    );
    Ok(())
}

pub(super) fn stages(path: &Path) -> Result<()> {
    let mut e = Executor::new(Graph::new(program(path)?)?, true);
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
    println!("{}", serde_json::to_string(&hashes)?);
    Ok(())
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
