//! Small real eggcc snapshot audit. Fresh processes vary threads and rule
//! registration order; each lane runs once. This is not a speedup benchmark.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::stable_choice;
use egglog::{EGraph, ast::Command};
use serde_json::{Value as Json, json};
use std::{path::Path, process::Command as Process, time::Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.first().is_some_and(|x| x == "--lane") {
        return lane(&a[1..]);
    }
    if a.len() != 2 {
        return Err("SOURCE.egg NEW_OUTPUT".into());
    }
    let out = Path::new(&a[1]);
    std::fs::create_dir(out)?;
    std::fs::create_dir(out.join("graphs"))?;
    let executable = std::env::current_exe()?;
    let mut results = vec![];
    for (name, full, threads, reverse) in [
        ("original", false, 1, false),
        ("full", true, 1, false),
        ("repeat", true, 1, false),
        ("threads4", true, 4, false),
        ("reverse-rules", true, 1, true),
    ] {
        let dir = out.join(name);
        std::fs::create_dir(&dir)?;
        let log = std::fs::File::create(dir.join("run.log"))?;
        let status = Process::new(&executable)
            .arg("--lane")
            .arg(&a[0])
            .arg(&dir)
            .arg(if full { "full" } else { "original" })
            .arg(threads.to_string())
            .arg(if reverse { "reverse" } else { "normal" })
            .arg(if full && name != "full" {
                out.join("full/control.json")
            } else {
                out.join("original/control.json")
            })
            .env("RAYON_NUM_THREADS", threads.to_string())
            .stdout(log.try_clone()?)
            .stderr(log)
            .status()?;
        let packet = dir.join("result.json");
        let r = if packet.exists() {
            serde_json::from_slice::<Json>(&std::fs::read(packet)?)?
        } else {
            json!({"failed":true,"exit_code":status.code(),"log":dir.join("run.log")})
        };
        println!(
            "{name}: {}",
            json!({"run_seconds":r["run_seconds"],"error":r["error"],"match_reference":r["comparison"].is_object(),"coverage":r["all_constructor_classes_keyed"]})
        );
        results.push(json!({"lane":name,"result":r}));
        std::fs::write(
            out.join("results.json"),
            serde_json::to_vec_pretty(
                &json!({"source":a[0],"lanes":results,"determinism_checks_passed":results.iter().filter(|x|["repeat","threads4","reverse-rules"].contains(&x["lane"].as_str().unwrap())).count()==3 && results.iter().filter(|x|x["lane"]!="original").all(|x|x["result"]["all_constructor_classes_keyed"]==true && x["result"]["error"].is_null()) && results.iter().filter(|x|["repeat","threads4","reverse-rules"].contains(&x["lane"].as_str().unwrap())).all(|x|x["result"]["comparison"].is_object()),"scope":"Independent replays of one complete frozen eggcc Pass input and schedule; not a new three-pass production chain. Different runs are correctness perturbations, not samples for a timing average."}),
            )?,
        )?;
    }
    Ok(())
}
fn lane(a: &[String]) -> Result<()> {
    let source = &a[0];
    let out = Path::new(&a[1]);
    let full = a[2] == "full";
    EGraph::set_num_threads(a[3].parse()?);
    egglog::set_probe_counts_enabled(false);
    let started = Instant::now();
    let mut eg = EGraph::default();
    let mut cs = eg.parse_program(Some(source.clone()), &std::fs::read_to_string(source)?)?;
    let last = cs
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("missing main schedule")?;
    if last + 1 != cs.len() {
        return Err("expected full snapshot ending with schedule".into());
    }
    if a[4] == "reverse" {
        // Reverse only contiguous rule declarations. Facts and all scheduled
        // operations keep their original order; no rule is dropped.
        let mut i = 0;
        while i < last {
            if !matches!(
                cs[i],
                Command::Rule { .. } | Command::Rewrite(..) | Command::BiRewrite(..)
            ) {
                i += 1;
                continue;
            }
            let start = i;
            i += 1;
            while i < last
                && matches!(
                    cs[i],
                    Command::Rule { .. } | Command::Rewrite(..) | Command::BiRewrite(..)
                )
            {
                i += 1
            }
            cs[start..i].reverse();
        }
    }
    let (rewritten, edits) = stable_choice::rewrite_ties(&cs)?;
    std::fs::write(
        out.join("policy-rules.egg"),
        format!(
            "; Selection rewrites only, not a standalone program: setup_full installs key primitives/helper rules; schedule_full installs their maintenance boundaries.\n{}\n",
            rewritten
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )?;
    let setup = if full {
        stable_choice::setup_full(&mut eg, &cs[..last])
    } else {
        eg.run_program(cs[..last].to_vec())
            .map(|_| ())
            .map_err(Into::into)
    };
    if let Err(e) = setup {
        std::fs::write(
            out.join("result.json"),
            serde_json::to_vec_pretty(&json!({"error":e.to_string(),"stage":"setup"}))?,
        )?;
        return Ok(());
    }
    let commands = if full {
        stable_choice::schedule_full(&mut eg, &cs[last..])?
    } else {
        cs[last..].to_vec()
    };
    let setup_seconds = started.elapsed().as_secs_f64();
    let base = eg.clone();
    let tick = Instant::now();
    let run = eg.run_program(commands);
    let elapsed = tick.elapsed().as_secs_f64();
    let error = run.as_ref().err().map(ToString::to_string);
    let coverage = if full {
        stable_choice::coverage(&eg)?
    } else {
        Json::Null
    };
    let all_keyed = full
        && coverage
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x["missing"] == 0);
    let reference = Path::new(&a[5]);
    let comparison = if reference.is_file() {
        Some(compare::equal_control(reference, &base, &eg))
    } else {
        None
    };
    let mut packet = json!({"mode":a[2],"threads":a[3],"registration":a[4],"setup_seconds":setup_seconds,"run_seconds":elapsed,"error":error,"edits":edits,"coverage":coverage,"all_constructor_classes_keyed":all_keyed,"comparison":comparison.as_ref().and_then(|x|x.as_ref().ok()),"comparison_error":comparison.as_ref().and_then(|x|x.as_ref().err()).map(ToString::to_string),"native_rows_including_private_helpers":eg.num_tuples()});
    if reference.is_file() {
        let diagnostic = compare::equal_control_except(reference, &base, &eg, &["Smaller"]);
        packet["diagnostic_without_smaller"] = json!({"passed":diagnostic.is_ok(),"error":diagnostic.as_ref().err().map(ToString::to_string),"not_a_correctness_gate":true});
    }
    let cert = compare::save_control(&out.join("control.json"), &base, &eg);
    if let Err(e) = cert {
        packet["control_error"] = json!(e.to_string());
    }
    let graph = eg.serialize(egglog::SerializeConfig::default());
    if graph.is_complete() {
        let mut f = std::io::BufWriter::new(std::fs::File::create(
            out.parent().unwrap().join("graphs").join(format!(
                "{}.json",
                out.file_name().unwrap().to_string_lossy()
            )),
        )?);
        serde_json::to_writer_pretty(&mut f, &graph.egraph)?;
    } else {
        packet["export_error"] = json!("truncated graph");
    }
    std::fs::write(out.join("result.json"), serde_json::to_vec_pretty(&packet)?)?;
    Ok(())
}
