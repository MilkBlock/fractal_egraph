use super::*;
use eggfuse_cell_runtime::{executor::Executor, graph::Graph, program::Program};
use std::time::Instant;
fn lane_process(lane: &str) -> Result<Command> {
    let mut cmd = Command::new(std::env::current_exe()?);
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("CELL_CPU_") {
            cmd.env_remove(k);
        }
    }
    cmd.env(
        "CELL_CPU_SHARED_NODE_SCHEDULE",
        if lane == "baseline" { "0" } else { "1" },
    )
    .env("CELL_CPU_SIEVE", if lane == "filtered" { "1" } else { "0" })
    .env(
        "CELL_CPU_JOIN_INTERSECTION",
        if lane == "filtered" { "1" } else { "0" },
    );
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
        ],
    )?;
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
    if !["baseline", "nodes", "filtered"].contains(&lane) {
        return Err("unknown bench lane".into());
    }
    if verify {
        let stages = |name: &str| -> Result<Vec<String>> {
            let output = lane_process(name)?
                .arg("internal-stages")
                .arg(&program)
                .output()?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
            }
            Ok(serde_json::from_slice(&output.stdout)?)
        };
        let baseline = stages("baseline")?;
        let candidate = stages(lane)?;
        if baseline != candidate {
            let first = baseline
                .iter()
                .zip(&candidate)
                .position(|(a, b)| a != b)
                .unwrap_or(baseline.len().min(candidate.len()));
            return Err(format!("graph, schedule or bindings differ at stage {first}").into());
        }
        eprintln!(
            "verified initial graph and {} Run snapshots against baseline",
            baseline.len() - 1
        );
    }
    let status = lane_process(lane)?
        .arg("internal-worker")
        .arg(program)
        .arg(n.to_string())
        .arg(lane)
        .status()?;
    if !status.success() {
        return Err(format!("benchmark worker failed ({status})").into());
    }
    Ok(())
}
pub(super) fn worker(args: Vec<String>) -> Result<()> {
    if args.len() != 3 {
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
        let c = json!({"all_candidate_visits":g.residual_candidates+g.direct_candidates+merged+pivots+predicates,"merge_candidate_rows":merged,"pivot_candidates":pivots,"predicate_checks":predicates,"residual_candidates":g.residual_candidates,"direct_candidates":g.direct_candidates,"runs":e.steps.len()});
        if !counts.is_null() && counts != c {
            return Err("benchmark work differs between repetitions".into());
        }
        counts = c;
    }
    times.sort_by(f64::total_cmp);
    let median = (times[(n - 1) / 2] + times[n / 2]) / 2.;
    println!(
        "{}",
        json!({"backend":"standalone-cell-cpu","lane":args[2],"program_sha256":p.artifact["sha256"],"inference_ms":times,"median_ms":median,"counts":counts,"final_state_sha256":fingerprint})
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
