//! Counterfactual unit attribution. Time only schedule execution and verify
//! the endpoint and control trace for every observation outside the timer.
use super::*;
use eggfuse_cell_runtime::{executor::Executor, graph::Graph};
use std::{process::Command, time::Instant};

pub(super) fn sample(path: &Path, enabled: bool) -> Result<()> {
    let program = Program::parse(load(path)?)?;
    let mut e = Executor::new(Graph::new(program)?, false);
    e.compositions_enabled = enabled;
    let clock = Instant::now();
    e.execute_until(None)?;
    let seconds = clock.elapsed().as_secs_f64();
    let r = e.report();
    let c = &r["compositions"]["stats"];
    let g = &e.graph.stats;
    let count = |j: &J| j.as_u64().ok_or("missing work counter");
    let visits = g.direct_candidates
        + g.residual_candidates
        + count(&r["multi_stage"]["stats"]["merge_candidate_rows"])?
        + count(&r["stage_spine"]["stats"]["pivot_candidates"])?
        + count(&r["entry_schedule"]["stats"]["predicate_checks"])?;
    let mut state = Sha256::new();
    e.graph.write_export(&mut state)?;
    let mut stages = Sha256::new();
    for s in &e.steps {
        serde_json::to_writer(
            &mut stages,
            &[
                "ordinal",
                "path",
                "iterations",
                "ruleset",
                "changed",
                "epoch",
                "logical_matches",
                "executed_bindings",
            ]
            .map(|k| &s[k]),
        )?;
    }
    let mut j = json!({"ms":seconds*1000.0,"visits":visits,"lookups":e.graph.key_lookup_calls.get(),
        "requests":g.constructor_requests,"cells":g.cells_created,"state_sha256":format!("{:x}",state.finalize()),
        "schedule_sha256":format!("{:x}",stages.finalize()),"logical_bindings":g.bindings});
    for key in [
        "baked_guard_misses",
        "attempts",
        "root_steps",
        "bindings",
        "cross_run_applications",
        "known_atoms",
        "constructor_reuses",
        "port_reads",
        "rejected",
        "unavailable",
        "entry_seconds",
        "match_seconds",
    ] {
        j[key] = c[key].clone();
    }
    println!("{}", serde_json::to_string(&j)?);
    Ok(())
}

fn one_sample(exe: &Path, file: &Path, enabled: bool) -> Result<J> {
    let mut cmd = Command::new(exe);
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("CELL_CPU_") {
            cmd.env_remove(k);
        }
    }
    let o = cmd
        .env("CELL_CPU_SIEVE", "1")
        .env("CELL_CPU_JOIN_INTERSECTION", "1")
        .env("CELL_CPU_SHARED_NODE_SCHEDULE", "1")
        .arg("sample")
        .arg(file)
        .arg(if enabled { "1" } else { "0" })
        .output()?;
    if !o.status.success() {
        return Err(format!("sampling failed: {}", String::from_utf8_lossy(&o.stderr)).into());
    }
    let v: J = serde_json::from_slice(&o.stdout)?;
    Ok(v)
}

pub(super) fn run(
    mode: &str,
    catalog: &Path,
    base: &Path,
    repeats: usize,
    out: &Path,
) -> Result<()> {
    if !matches!(mode, "isolated" | "marginal" | "joint" | "audit-library")
        || repeats == 0
        || (mode == "audit-library" && repeats != 1)
        || out.exists()
    {
        return Err(
            "expected isolated, marginal or joint with positive repetitions, or audit-library with one observation, and a new output path"
                .into(),
        );
    }
    let tmp = tempfile::tempdir()?;
    let mut lanes = vec![("base".to_string(), base.to_owned())];
    let library = load(catalog)?;
    if mode != "isolated" {
        let p = tmp.path().join("all.json");
        portfolio(catalog, base, &p, None)?;
        lanes.push(("all".into(), p));
    }
    for u in rows(&library["classes"])? {
        if u.get("rejection").is_some() || mode == "joint" {
            continue;
        }
        let class = id(&u["class"])?;
        let p = tmp.path().join(format!("variant-{class}.json"));
        let label = if matches!(mode, "marginal" | "audit-library") {
            portfolio(catalog, base, &p, Some(class))?;
            format!("without-{class}")
        } else {
            let unit = catalog
                .parent()
                .ok_or("catalog directory")?
                .join(format!("class-{class}/unit.json"));
            let v = load(&unit)?;
            if v["sha256"] != u["sha256"] || v["class"] != u["class"] {
                return Err("catalog fingerprint differs".into());
            }
            link(&unit, base, &p)?;
            class.to_string()
        };
        lanes.push((label, p));
    }
    let exe = std::env::current_exe()?;
    let mut records = BTreeMap::<String, Vec<J>>::new();
    let mut reference = None;
    let active = library["classes"]
        .as_array()
        .is_some_and(|v| v.iter().any(|c| c.get("sha256").is_some()));
    let mut record = |label: &str, v: J, keep: bool| -> Result<()> {
        let tag = json!([
            v["state_sha256"],
            v["schedule_sha256"],
            v["logical_bindings"]
        ]);
        if reference.get_or_insert(tag.clone()) != &tag {
            return Err(format!("state or schedule differs for {label}").into());
        }
        if keep {
            records.entry(label.to_owned()).or_default().push(v);
        }
        Ok(())
    };
    if mode == "audit-library" {
        // These are work counters only. Bound memory consumption for large graphs.
        for chunk in lanes.chunks(3) {
            let results = std::thread::scope(|scope| {
                let handles = chunk
                    .iter()
                    .map(|(label, path)| {
                        let exe = &exe;
                        scope.spawn(move || {
                            one_sample(exe, path, label != "base" && active)
                                .map_err(|e| e.to_string())
                        })
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|h| h.join().map_err(|_| "audit worker panicked".to_owned())?)
                    .collect::<std::result::Result<Vec<_>, String>>()
            })?;
            for ((label, _), mut v) in chunk.iter().zip(results) {
                for k in ["ms", "entry_seconds", "match_seconds"] {
                    v.as_object_mut().unwrap().remove(k);
                }
                record(label, v, true)?;
            }
        }
    } else {
        for block in 0..=repeats {
            let mut order = (0..lanes.len()).collect::<Vec<_>>();
            order.rotate_left(block % lanes.len());
            if (block / lanes.len()) % 2 != 0 {
                order.reverse()
            }
            for i in order {
                let (label, file) = &lanes[i];
                record(label, one_sample(&exe, file, i != 0 && active)?, block != 0)?;
            }
        }
    }
    save(
        out,
        &json!({"schema":"eggfuse-unit-attribution/v1","mode":mode,"training":library["training_sha256"],"program":load(base)?["sha256"],"samples":records}),
    )
}

/// Attribute deterministic work, not noisy single-run timings. Positive means
/// the class saves work, except applications which count additional executions.
pub(super) fn shapley(catalog: &Path, program: &Path, out: &Path) -> Result<()> {
    let meta = load(catalog)?;
    let classes = rows(&meta["classes"])?
        .iter()
        .filter(|r| r.get("sha256").is_some())
        .map(|r| id(&r["class"]))
        .collect::<Result<Vec<_>>>()?;
    let n = classes.len();
    if n > 8 || out.exists() {
        return Err("exact attribution supports up to eight classes and a new output file".into());
    }
    let exe = std::env::current_exe()?;
    let mut costs = vec![];
    let mut tag = None;
    let metrics = [
        "visits",
        "lookups",
        "cells",
        "root_steps",
        "cross_run_applications",
    ];
    for mask in 0..(1usize << n) {
        let tmp = tempfile::tempdir()?;
        let file = tmp.path().join("coalition.json");
        portfolio_filter(catalog, program, &file, |id| {
            classes
                .iter()
                .enumerate()
                .any(|(i, c)| *c == id && mask & (1 << i) != 0)
        })?;
        let sample = one_sample(&exe, &file, mask != 0)?;
        let v = json!([
            sample["state_sha256"],
            sample["schedule_sha256"],
            sample["logical_bindings"]
        ]);
        if tag.get_or_insert(v.clone()) != &v {
            return Err(format!("coalition {mask} semantics differ").into());
        }
        costs.push(metrics.map(|k| sample[k].as_i64().expect("work counter")));
    }
    let mut values = BTreeMap::new();
    for (i, class) in classes.iter().enumerate() {
        let mut phi = [0.; 5];
        for mask in 0..costs.len() {
            if mask & (1 << i) != 0 {
                continue;
            }
            let k = mask.count_ones() as usize;
            let choose = (0..k).fold(1usize, |v, j| v * (n - 1 - j) / (j + 1));
            for m in 0..phi.len() {
                phi[m] += (costs[mask][m] - costs[mask | (1 << i)][m]) as f64 / (n * choose) as f64;
            }
        }
        phi[4] = -phi[4];
        values.insert(class, phi);
    }
    save(
        out,
        &json!({"schema":"eggfuse-exact-work-attribution/v1","program":load(program)?["sha256"],"classes_in_mask_order":classes,"columns":metrics,"units":"savings for visits, lookups, cells and root steps; additional cross-Run applications","coalition_work":costs,"contributions":values,"semantics":tag}),
    )
}
