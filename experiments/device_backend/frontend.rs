//! CPU control/oracle. Native AST and typechecker are unchanged; all GPU runs
//! happen in a separate backend and only requested checkpoints reach this process.
use egglog::{
    EGraph,
    ast::{Command, Schedule},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{self, BufRead, Write},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn schedule(s: &Schedule, sets: &mut BTreeSet<String>) -> Result<Value> {
    Ok(match s {
        Schedule::Run(_, r) => {
            if r.until.is_some() {
                return Err("until predicates not supported by device control yet".into());
            }
            sets.insert(r.ruleset.clone());
            json!({"run":r.ruleset})
        }
        Schedule::Repeat(_, n, s) => json!({"repeat":n,"body":schedule(s,sets)?}),
        Schedule::Saturate(_, s) => json!({"saturate":schedule(s,sets)?}),
        Schedule::Sequence(_, ss) => {
            json!({"sequence":ss.iter().map(|s|schedule(s,sets)).collect::<Result<Vec<_>>>()?})
        }
    })
}
fn main() -> Result<()> {
    EGraph::set_num_threads(1);
    let path = std::env::args().nth(1).ok_or("SOURCE.egg")?;
    let mut eg = EGraph::default();
    let cmds = eg.parse_program(Some(path.clone()), &std::fs::read_to_string(path)?)?;
    let at = cmds
        .iter()
        .position(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("missing schedule")?;
    // No hidden native inference through include/fail/custom commands during
    // bootstrap. The device path owns every scheduled rule execution.
    for c in &cmds[..at] {
        if !matches!(
            c,
            Command::Sort { .. }
                | Command::Datatype { .. }
                | Command::Datatypes { .. }
                | Command::Constructor { .. }
                | Command::Function { .. }
                | Command::Relation { .. }
                | Command::AddRuleset(..)
                | Command::UnstableCombinedRuleset(..)
                | Command::Rule { .. }
                | Command::Rewrite(..)
                | Command::BiRewrite(..)
                | Command::Action(..)
        ) {
            return Err(
                "device bootstrap accepts declarations and facts only; flatten includes first"
                    .into(),
            );
        }
    }
    eg.run_program(cmds[..at].to_vec())?;
    if std::env::args().any(|a|a=="--inventory") {
        println!("{}",eg.device_inventory());
        return Ok(());
    }
    let mut sets = BTreeSet::new();
    let mut schedules = vec![];
    let mut assertions = vec![];
    for c in &cmds[at..] {
        match c {
            Command::RunSchedule(s) => schedules.push(schedule(s, &mut sets)?),
            Command::Check(_, facts) => {
                let name = format!("DeviceBackendCheck_{}", assertions.len());
                let set = format!("device-backend-check-{}", assertions.len());
                if eg.get_function(&name).is_some() {
                    return Err("assertion helper name collision".into());
                }
                eg.parse_and_run_program(None,&format!("(relation {name} ()) (ruleset {set}) (rule ({}) (({name})) :ruleset {set})",facts.iter().map(|f|f.to_string()).collect::<Vec<_>>().join(" ")))?;
                sets.insert(set.clone());
                schedules.push(json!({"run":set,"assert":true}));
                assertions.push(name);
            }
            _ => return Err("only schedules and checks may follow initialization".into()),
        }
    }
    let mut p = eg
        .export_device_program(&sets.into_iter().collect::<Vec<_>>())
        .map_err(|e| io::Error::other(e))?;
    let anchors = eg.capture_device_anchors().map_err(io::Error::other)?;
    p["schedule"] = json!(schedules);
    p["assertion_tables"] = json!(assertions);
    println!("{}", p);
    io::stdout().flush()?;
    // This native engine is only advanced when the verifier explicitly requests
    // it. Check-off GPU execution leaves it frozen at initialization.
    for line in io::stdin().lock().lines() {
        let q: Value = serde_json::from_str(&line?)?;
        match q["op"].as_str() {
            Some("run") => {
                let r = eg.step_rules(q["set"].as_str().ok_or("set")?)?;
                println!("{}", json!({"updated":r.updated,"can_stop":r.can_stop}));
            }
            Some("snapshot") => println!(
                "{}",
                eg.device_checkpoint(&anchors).map_err(io::Error::other)?
            ),
            Some("quit") => break,
            _ => return Err("invalid oracle command".into()),
        }
        io::stdout().flush()?;
    }
    Ok(())
}
