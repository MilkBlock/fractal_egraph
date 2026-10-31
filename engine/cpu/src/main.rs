use egglog::{
    CommandOutput, EGraph,
    ast::{Command, Schedule},
    cpu_cells::{CpuCells, schedule_json, staged::StagedCpuCells},
};
use serde_json::json;
use std::{path::PathBuf, time::Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args.iter().any(|s| s == "--help") {
        println!(
            "eggfuse-cpu --source SOURCE.egg [--prepare CORE.json | --plan PLAN.json] [--output DIR] [--mode native|cell|staged] [--stage-audit] [--stage-archive DIR] [--no-state-export] [--brief-report]"
        );
        return Ok(());
    }
    let option = |k: &str| args.windows(2).find(|v| v[0] == k).map(|v| v[1].clone());
    let mode = option("--mode").unwrap_or_else(|| "cell".into());
    if mode != "native" && mode != "cell" && mode != "staged" {
        return Err("mode must be native, cell or staged".into());
    }
    let src = option("--source").ok_or("--source required")?;
    let source = std::fs::read_to_string(&src)?;
    let boot = Instant::now();
    EGraph::set_num_threads(1);
    let mut eg = EGraph::default();
    let commands = eg.parse_program(Some(src), &source)?;
    let at = commands
        .iter()
        .position(|c| matches!(c, Command::RunSchedule(_)))
        .unwrap_or(commands.len());
    let mut outputs = eg
        .run_program(commands[..at].to_vec())?
        .into_iter()
        .filter(|o| !matches!(o, CommandOutput::RunSchedule(_)))
        .map(|o| o.to_string())
        .collect::<Vec<_>>();
    let bootstrap_seconds = boot.elapsed().as_secs_f64();
    let mut core = None;
    if mode != "native" || option("--prepare").is_some() {
        // Compilation fixes the rule environment. Ordinary input facts/checks
        // between schedules are supported; schema changes require recompile.
        if commands[at..].iter().any(|c| {
            matches!(
                c,
                Command::Push(..)
                    | Command::Pop(..)
                    | Command::Include(..)
                    | Command::Rule { .. }
                    | Command::Rewrite(..)
                    | Command::BiRewrite(..)
                    | Command::Sort { .. }
                    | Command::Datatype { .. }
                    | Command::Datatypes { .. }
                    | Command::Constructor { .. }
                    | Command::Function { .. }
                    | Command::Relation { .. }
                    | Command::AddRuleset(..)
                    | Command::UnstableCombinedRuleset(..)
            )
        }) {
            return Err("CPU Cell compilation requires declarations/rules before the first schedule; use native mode for a changing rule environment".into());
        }
        let schedules = commands[at..]
            .iter()
            .filter_map(|c| {
                if let Command::RunSchedule(s) = c {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let mut sets = std::collections::BTreeSet::new();
        fn names(s: &Schedule, out: &mut std::collections::BTreeSet<String>) {
            match s {
                Schedule::Run(_, r) => {
                    out.insert(r.ruleset.clone());
                }
                Schedule::Sequence(_, ss) => {
                    for s in ss {
                        names(s, out)
                    }
                }
                Schedule::Repeat(_, _, s) | Schedule::Saturate(_, s) => names(s, out),
            }
        }
        for s in &schedules {
            names(s, &mut sets)
        }
        let mut c = eg
            .export_rule_program(&sets.into_iter().collect::<Vec<_>>())
            .map_err(std::io::Error::other)?;
        c["schedule"] = json!(schedules.iter().map(schedule_json).collect::<Vec<_>>());
        if let Some(path) = option("--prepare") {
            std::fs::write(path, serde_json::to_vec(&c)?)?;
            return Ok(());
        }
        core = Some(c);
    }
    let out = PathBuf::from(option("--output").ok_or("--output required")?);
    std::fs::create_dir(&out)?;
    let export_state = !args.iter().any(|s| s == "--no-state-export");
    let anchors = if export_state {
        eg.capture_device_anchors()
    } else {
        Err("snapshot export disabled".into())
    };
    let setup = Instant::now();
    let mut cells = if mode == "cell" {
        let plan = option("--plan").ok_or("--plan required in cell mode")?;
        Some(CpuCells::compile(
            &mut eg,
            core.as_ref().unwrap(),
            &serde_json::from_slice(&std::fs::read(plan)?)?,
        )?)
    } else {
        None
    };
    let mut staged = if mode == "staged" {
        let plan = option("--plan").ok_or("--plan required in staged mode")?;
        Some(StagedCpuCells::compile(
            &mut eg,
            core.as_ref().unwrap(),
            &serde_json::from_slice(&std::fs::read(plan)?)?,
            args.iter().any(|s| s == "--stage-audit"),
        )?)
    } else {
        None
    };
    if let Some(path) = option("--stage-archive") {
        staged
            .as_mut()
            .ok_or("--stage-archive requires --mode staged")?
            .archive(&eg, std::path::Path::new(&path), core.as_ref().unwrap())?;
    }
    let setup_seconds = setup.elapsed().as_secs_f64();
    let start = Instant::now();
    let mut inference_seconds = 0.0;
    let mut other_command_seconds = 0.0;
    let mut native_report = eg.get_overall_run_report().clone();
    for c in &commands[at..] {
        let t = Instant::now();
        if let (Some(staged), Command::RunSchedule(s)) = (&mut staged, c) {
            native_report.union(staged.run(&mut eg, s)?);
        } else if let (Some(cells), Command::RunSchedule(s)) = (&mut cells, c) {
            native_report.union(cells.run(&mut eg, s)?);
        } else {
            if let Some(staged) = &mut staged {
                staged.invalidate();
            }
            for o in eg.run_program(vec![c.clone()])? {
                if let CommandOutput::RunSchedule(r) = o {
                    native_report.union(r)
                } else {
                    outputs.push(o.to_string())
                }
            }
        }
        if matches!(c, Command::RunSchedule(_)) {
            inference_seconds += t.elapsed().as_secs_f64()
        } else {
            other_command_seconds += t.elapsed().as_secs_f64()
        }
    }
    if let Some(staged) = &mut staged {
        staged.invalidate();
    }
    let execution_seconds = start.elapsed().as_secs_f64();
    let snapshot = match anchors {
        Ok(a) => eg.device_checkpoint(&a),
        Err(e) => Err(e),
    };
    let snapshot_error = snapshot.as_ref().err().cloned();
    let full_snapshot = snapshot.is_ok();
    if let Ok(s) = snapshot {
        std::fs::write(out.join("state.json"), serde_json::to_vec(&s)?)?;
    }
    let report = json!({"mode":mode,"inference_seconds":inference_seconds,"execution_seconds":execution_seconds,"other_command_seconds":other_command_seconds,"bootstrap_seconds":bootstrap_seconds,"setup_seconds":setup_seconds,"full_typed_snapshot":full_snapshot,"snapshot_error":snapshot_error,"source_rows":eg.num_tuples(),"source_outputs":outputs,"native_report":if args.iter().any(|s|s=="--brief-report"){serde_json::Value::Null}else{json!(native_report)},"cells":cells.as_ref().map(CpuCells::report),"staged":staged.as_ref().map(StagedCpuCells::report),"scope":"actual local egglog runtime; native tables, hashcons, merge, union, rebuild and extraction; inference includes Cell receipt maintenance"});
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    Ok(())
}
