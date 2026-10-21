//! Native execution of generated .egg markers, with full original-schema checks.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod bench;
use cell_assisted_egglog::stable_choice;
use egglog::{EGraph, ast::Command};
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

fn setup(path: &Path) -> Result<(EGraph, Vec<Command>), Box<dyn std::error::Error>> {
    let mut eg = EGraph::default();
    let c = eg.parse_program(
        Some(path.display().to_string()),
        &std::fs::read_to_string(path)?,
    )?;
    let last = c
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("missing schedule")?;
    if last + 1 != c.len() {
        return Err("Pass-3 benchmark expects final main schedule".into());
    }
    stable_choice::setup(&mut eg, &c[..last])?;
    let schedule = stable_choice::schedule(&mut eg, &c[last..])?;
    Ok((eg, schedule))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 5 {
        return Err("ORIGINAL TRANSFORMED MARKERS_JSON CONTROL_DIR NEW_OUTPUT".into());
    }
    EGraph::set_num_threads(1);
    let profile = std::env::var("CELL_SEARCH_COUNTERS").as_deref() == Ok("1");
    egglog::set_probe_counts_enabled(profile);
    let out = Path::new(&a[4]);
    std::fs::create_dir(out)?;
    let cfg: Value = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    let original = std::fs::read(&a[0])? == std::fs::read(&a[1])?;
    let names = if original {
        vec![]
    } else {
        cfg["markers"]
            .as_array()
            .ok_or("missing marker schema")?
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect::<Vec<_>>()
    };
    let tick = Instant::now();
    let (base, _) = setup(Path::new(&a[0]))?;
    // Match the established benchmark: retain the initialized checkpoint and
    // execute a clone. The old native control was collected by that protocol.
    let (frozen_candidate, schedule) = setup(Path::new(&a[1]))?;
    let mut candidate = frozen_candidate.clone();
    for name in &names {
        let f = candidate
            .get_function(name)
            .ok_or("missing marker relation")?;
        if f.schema().output.name() != "Unit" || base.get_function(name).is_some() {
            return Err(format!(
                "invalid auxiliary projection {name}: output={}, preexisting={}",
                f.schema().output.name(),
                base.get_function(name).is_some()
            )
            .into());
        }
    }
    let initial = bench::equal_except(&base, &base, &candidate, &names)?;
    let setup_seconds = tick.elapsed().as_secs_f64();
    let before = egglog::probe_counts()
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let tick = Instant::now();
    let outputs = candidate.run_program(schedule)?;
    let run_seconds = tick.elapsed().as_secs_f64();
    let counters = egglog::probe_counts()
        .into_iter()
        .map(|(k, v)| (k, v - before.get(k).copied().unwrap_or(0)))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut report = egglog_reports::RunReport::default();
    for o in outputs {
        if let egglog::CommandOutput::RunSchedule(r) = o {
            report.union(r);
        }
    }
    let control = Path::new(&a[3]);
    let old: Value = serde_json::from_slice(&std::fs::read(control.join("report.json"))?)?;
    let checked = bench::equal_control_except(
        &control.join("graphs/native.control.json"),
        &base,
        &candidate,
        &names,
    );
    let mut rows = std::collections::BTreeMap::new();
    for name in &names {
        let mut n = 0;
        candidate.function_for_each(name, |_| n += 1)?;
        rows.insert(*name, n);
    }
    let result = json!({"schema":"pass3-egg-markers/v1","backend":"native CPU egglog; generated source rules",
        "profile":profile,"setup_seconds":setup_seconds,"run_seconds":run_seconds,
        "execution_protocol":"retain initialized checkpoint; execute its clone, matching the native control",
        "native_historical":old["native"],"reused_controls":{"directory":control},
        "initial_verification":initial,"verification":checked.as_ref().ok(),
        "verification_error":checked.as_ref().err().map(ToString::to_string),"marker_rows":rows,
        "run_probe_counters":if profile{json!(counters)}else{Value::Null},
        "native":if original {json!({"run_seconds":run_seconds})}else{Value::Null},
        "run_report":report,"extraction_lanes":["native","markers"]});
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&result)?)?;
    let graphs = out.join("graphs");
    std::fs::create_dir(&graphs)?;
    if !original {
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            control.join("graphs/native.json"),
            graphs.join("native.json"),
        )?;
    }
    let graph = candidate.serialize(egglog::SerializeConfig::default());
    if !graph.is_complete() {
        return Err("truncated candidate graph".into());
    }
    let mut graph = serde_json::to_value(graph.egraph)?;
    graph["nodes"]
        .as_object_mut()
        .ok_or("serialized nodes")?
        .retain(|_, v| !v["op"].as_str().is_some_and(|op| names.contains(&op)));
    serde_json::to_writer(
        std::io::BufWriter::new(std::fs::File::create(graphs.join("markers.json"))?),
        &graph,
    )?;
    if original {
        bench::save_control(&graphs.join("native.control.json"), &base, &candidate)?;
        std::fs::copy(graphs.join("markers.json"), graphs.join("native.json"))?;
    }
    checked?;
    println!(
        "{}",
        json!({"run_seconds":run_seconds,"exact_original_schema":true,"profile":profile})
    );
    Ok(())
}
