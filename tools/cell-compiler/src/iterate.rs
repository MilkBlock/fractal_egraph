//! Execute frozen catalogs from the identical initial state and admit bounded
//! adjacent winners. Only install artifacts after the semantic audit succeeds.
use eggfuse_cell_runtime::{
    checksum::artifact_digest,
    executor::Executor,
    graph::Graph,
    iterative::{Catalog, Limits},
    program::Program,
    transfer_audit,
};
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
type Result<T> = eggfuse_bake::pipeline::Result<T>;
fn save(path: &Path, value: &J) -> Result<()> {
    let mut f = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    serde_json::to_writer(&mut f, value)?;
    f.persist(path)?;
    Ok(())
}
pub(crate) fn run(args: Vec<String>) -> Result<()> {
    let opts = super::options(
        args.into_iter(),
        &[
            "--program",
            "--output",
            "--rounds",
            "--objective",
            "--max-depth",
            "--max-nodes",
            "--new-per-round",
            "--max-units",
            "--candidate-slots",
            "--min-support",
        ],
    )?;
    let path = opts
        .get("--program")
        .ok_or("--program SEED_PROGRAM required")?;
    let output = opts.get("--output").ok_or("--output NEW_DIR required")?;
    let seed = Program::parse(super::read(Path::new(path))?)?;
    let rounds = opts
        .get("--rounds")
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(4usize);
    let native = eggfuse_bake::bake_limits::BakeLimits::from_env()?;
    let mut limits = Limits {
        depth: native.max_depth,
        nodes: native.max_nodes,
        total: native.max_units,
        ..Default::default()
    };
    for (name, slot) in [
        ("--max-depth", &mut limits.depth),
        ("--max-nodes", &mut limits.nodes),
        ("--new-per-round", &mut limits.per_round),
        ("--max-units", &mut limits.total),
        ("--candidate-slots", &mut limits.candidates),
    ] {
        if let Some(s) = opts.get(name) {
            *slot = s.parse()?
        }
    }
    if let Some(s) = opts.get("--min-support") {
        limits.support = s.parse()?
    }
    let handoffs = match opts
        .get("--objective")
        .map(String::as_str)
        .unwrap_or("copies")
    {
        "copies" => false,
        "rhs-lookups" | "lhs-lookups" => true,
        _ => return Err("unknown iterative Bake objective".into()),
    };
    let lhs = opts.get("--objective").is_some_and(|s| s == "lhs-lookups");
    let mut plan = if lhs && !seed.artifact["iterative_bake"].is_null() {
        let mut plan = seed.artifact["iterative_bake"].clone();
        plan["limits"] = json!(limits);
        plan
    } else {
        Catalog::seed(&seed, limits)?
    };
    if lhs {
        Catalog::expand_lhs_seed(&seed, &mut plan)?;
    }
    let out = Path::new(output);
    fs::create_dir(out)?;
    let mut reference = None;
    let mut history = Vec::new();
    let mut best:Option<(u64,u64,usize)>=None;
    for round in 0..=rounds {
        if handoffs {
            Catalog::emit_handoffs(&seed, &mut plan)?;
        }
        if lhs {
            Catalog::emit_lhs(&seed, &mut plan)?;
        }
        let mut artifact = seed.artifact.clone();
        artifact.as_object_mut().unwrap().remove("sha256");
        artifact["iterative_bake"] = plan.clone();
        artifact["sha256"] = artifact_digest(&artifact).into();
        let p = Program::parse(artifact)?;
        let units = p.iterative.entries();
        let registry = eggfuse_bake::native_analyze::register_cpu_blocks(
            &plan,
            &units,
            plan["joins"].as_array().unwrap(),
            eggfuse_bake::bake_limits::BakeLimits {
                max_depth: limits.depth,
                max_nodes: limits.nodes,
                max_units: limits.total,
            },
        )?;
        let mut exec = Executor::new(Graph::new(p)?, false);
        #[cfg(feature = "two-run-blocks")]
        if lhs {
            exec.configure_multi_run(8)?;
        }
        exec.collect_iterative_pairs();
        transfer_audit::take_digests();
        transfer_audit::set_active(true);
        let clock = std::time::Instant::now();
        let run = exec.execute_until(None);
        let inference_ms = clock.elapsed().as_secs_f64() * 1000.;
        transfer_audit::set_active(false);
        run?;
        let rhs = transfer_audit::take_digests();
        let mut hasher = Sha256::new();
        exec.graph.write_export(&mut hasher)?;
        let graph = format!("{:x}", hasher.finalize());
        let schedule = exec
            .steps
            .iter()
            .map(|s| {
                [
                    "ordinal",
                    "ruleset",
                    "path",
                    "iterations",
                    "epoch",
                    "logical_matches",
                    "executed_bindings",
                    "changed",
                ]
                .map(|k| &s[k])
            })
            .collect::<Vec<_>>();
        let stamp = (graph, artifact_digest(&json!(schedule)), rhs);
        if let Some(expected) = &reference {
            if expected != &stamp {
                return Err(format!(
                    "round {round} differs from initial graph, schedule or ordered RHS"
                )
                .into());
            }
        } else {
            reference = Some(stamp.clone())
        }
        let p = &exec.graph.program;
        let cat = &p.iterative;
        let train = exec.iterative_training();
        let admitted = if round < rounds {
            if lhs {
                cat.select_lhs(p, train)
            } else {
                cat.select(p, train)
            }
        } else {
            vec![]
        };
        let stopped = if round == rounds {
            "round_limit"
        } else if units.len() == limits.total {
            "catalog_full"
        } else if admitted.is_empty() {
            "no_profitable_candidate"
        } else {
            "continue"
        };
        let report = exec.report();
        let phases = ["search", "rhs", "repair", "query_update", "delivery_gc"]
            .into_iter()
            .map(|k| {
                (
                    k,
                    exec.steps
                        .iter()
                        .map(|s| s["phases"][k].as_f64().unwrap_or(0.))
                        .sum::<f64>()
                        * 1000.,
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        history.push(json!({"round":round,"units":units.len(),"source_program":p.artifact["sha256"],"work":train.report(),"lhs_completion":report["stage_spine"]["stats"]["iterative_lhs"],"key_lookup_calls":exec.graph.key_lookup_calls.get(),"stats":report["stats"],"inference_ms":inference_ms,"phases_ms":phases,"rhs_completion":{"lets":exec.graph.stats.handoff_lets,"sets":exec.graph.stats.handoff_sets,"closed":exec.graph.stats.handoff_closed,"noop_unions":exec.graph.stats.handoff_noop_unions,"deep":exec.graph.stats.handoff_deep},"admitted":admitted,
            "runs":stamp.2.len(),"final_state_sha256":stamp.0,"schedule_sha256":stamp.1,
            "ordered_rhs_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&stamp.2)?)),"stop":stopped,"resumed_steps":exec.report()["stage_spine"]["stats"]["resumed_steps"]}));
        // Immutable round checkpoints make an interrupted run inspectable.
        let dir = out.join(format!("round-{round}"));
        fs::create_dir(&dir)?;
        save(&dir.join("catalog.json"), &plan)?;
        save(&dir.join("program.json"), &p.artifact)?;
        save(&dir.join("cell-egraph.json"), &registry)?;
        save(&out.join("rounds.json"), &json!(history))?;
        let score=(exec.graph.key_lookup_calls.get(),
            report["stage_spine"]["stats"]["iterative_lhs"]["history_receipts"].as_u64().unwrap_or(0),units.len());
        if !lhs || best.is_none_or(|old|score<old) {
            best=Some(score);
            save(&out.join("program.json"), &p.artifact)?;
            save(&out.join("rhs.json"), &json!(stamp.2))?;
            save(&out.join("expanded.json"), &json!(units))?;
            save(&out.join("selection.json"),&json!({"round":round,"score":score,
                "policy":"minimum actual global key probes, then captured receipts, then catalog units; training time is not a speed claim"}))?;
        }
        eprintln!(
            "round {round}: {} blocks, {} new, {} direct origins",
            units.len(),
            admitted.len(),
            train.direct_origins
        );
        if stopped != "continue" {
            break;
        }
        for (join, _) in admitted {
            plan["joins"].as_array_mut().unwrap().push(json!(join));
        }
    }
    Ok(())
}
