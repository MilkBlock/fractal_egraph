use cell_assisted_egglog::{stable_choice, table_live::Runtime};
use egglog::{EGraph, ast::Command};
use serde_json::json;
use std::{path::Path, time::Instant};
mod hook;
// The build adapter supplies this module from the existing bench skill checkout.
mod bench;
fn main() -> cell_assisted_egglog::Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 7 {
        return Err("SOURCE LIB WORKER KERNEL OUTPUT MODE CONTROL (MODE=native|exact|gpu|audit; CONTROL=- or saved graph)".into());
    }
    let out = Path::new(&a[4]);
    std::fs::create_dir_all(out)?;
    EGraph::set_num_threads(1);
    egglog::set_probe_counts_enabled(false);
    let mut eg = EGraph::default();
    let c = eg.parse_program(Some(a[0].clone()), &std::fs::read_to_string(&a[0])?)?;
    let last = c
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("no main schedule")?;
    if last + 1 != c.len() {
        return Err("unsupported commands after main schedule".into());
    }
    let setup = Instant::now();
    let mode = a[5].as_str();
    if !["native", "baseline", "exact", "gpu", "audit"].contains(&mode) {return Err("bad mode".into());}
    let joint_enabled=std::env::var("STRUCTURAL_JOINT_SAT").as_deref()==Ok("1") && mode!="native";
    let mut joint:Option<Box<dyn cell_assisted_egglog::table_live::SaturationHook>>=None;
    if joint_enabled {
        let r=cell_assisted_egglog::parameterized_cell::Runtime::new_joint_stable_commands(c[..last].to_vec())?;
        let (engine,observer)=r.detach_joint()?;eg=engine;joint=Some(Box::new(observer));
    }else{stable_choice::setup(&mut eg,&c[..last])?;}
    let commands = stable_choice::schedule(&mut eg, &c[last..])?;
    let base = eg.clone();
    let lib: hook::Library = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let hook = if mode == "native" || mode == "baseline" {
        joint.take()
    } else {
        let gpu = if mode == "exact"
            || matches!(
                std::env::var("STRUCTURAL_CONDITION_EXECUTION").as_deref(),
                Ok("groups" | "ready")
            ) {
            None
        } else {
            Some(hook::Device::new(
                Path::new(&a[2]),
                Path::new(&a[3]),
                Path::new(&a[1]),
            )?)
        };
        let ridge=hook::Hook::new(lib,gpu,if mode=="exact"{"exact"}else{"gpu"});
        Some(if let Some(joint)=joint {
            Box::new(hook::combined::Combined::new(joint,ridge)) as Box<dyn cell_assisted_egglog::table_live::SaturationHook>
        }else{Box::new(ridge) as Box<dyn cell_assisted_egglog::table_live::SaturationHook>})
    };
    // Match the existing full-query bench's common submission protocol. Mixing
    // sorted assisted RHS with fused native RHS leaves transient order-sensitive
    // facts (notably Smaller) dependent on native table traversal order.
    fn collect(s: &egglog::ast::Schedule, out: &mut std::collections::BTreeSet<String>) {
        use egglog::ast::GenericSchedule::*;
        match s {
            Run(_, r) => {
                out.insert(r.ruleset.clone());
            }
            Sequence(_, xs) => {
                for x in xs {
                    collect(x, out)
                }
            }
            Repeat(_, _, s) | Saturate(_, s) => collect(s, out),
        }
    }
    let mut names = std::collections::BTreeSet::new();
    for c in &commands {
        if let Command::RunSchedule(s) = c {
            collect(s, &mut names)
        }
    }
    names.remove("cell-stable-keys");
    let selected = names.into_iter().collect::<Vec<_>>();
    let ordinary_worker = std::env::var_os("STRUCTURAL_QUERY_WORKER").map(std::path::PathBuf::from);
    let worker = if mode == "native" {
        None
    } else {
        ordinary_worker.as_deref()
    };
    let mut r = Runtime::new_full_assisted(
        base.clone(),
        &commands,
        worker,
        false,
        Some(&selected),
        hook,
    )?;
    if mode == "audit" {
        r.enable_cell_query_audit()?;
    }
    let setup_seconds = setup.elapsed().as_secs_f64();
    let started = Instant::now();
    let status = r.run(&commands);
    let seconds = started.elapsed().as_secs_f64();
    let report = r.report();
    std::fs::write(
        out.join("runtime.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    status?;
    if matches!(mode,"gpu"|"audit") && std::env::var("STRUCTURAL_REQUIRE_PREDICTOR").as_deref()==Ok("1") {
        let d=&report["saturation_diagnostics"];
        if d["predictor_loaded"]!=true || d["predictor_executed"]!=true {
            return Err("prediction was required but no ridge GEMM ran; runtime.json is retained; use an explicitly labelled exact/idle control".into());
        }
    }
    let final_eg = r.into_engine();
    let graph = out.join("graph.control.json");
    bench::save_control(&graph, &base, &final_eg)?;
    let serialized = final_eg.serialize(egglog::SerializeConfig::default());
    if !serialized.is_complete() {
        return Err("incomplete extraction graph".into());
    }
    serde_json::to_writer(
        std::io::BufWriter::new(std::fs::File::create(out.join("graph.json"))?),
        &serialized.egraph,
    )?;
    let check = if a[6] != "-" {
        bench::equal_control_except(Path::new(&a[6]), &base, &final_eg, &[])?
    } else {
        json!(null)
    };
    std::fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "scope":"Full saved raytrace Pass 3 schedule; optional joint/loop and ridge providers, explicit ordinary query backend, original native RHS/merge/rebuild. Not pristine upstream.",
        "mode":mode,"joint_services_enabled":joint_enabled,"ordinary_query_worker":worker,"setup_seconds":setup_seconds,"inference_seconds":seconds,"runtime":report,"exact_graph":check,"graph":graph
        }))?,
    )?;
    eprintln!("[structural runtime] {mode}: {seconds:.6}s; graph comparison {check}");
    Ok(())
}
