//! CPU control/oracle. Native AST and typechecker are unchanged; all GPU runs
//! happen in a separate backend and only requested checkpoints reach this process.
use egglog::{
    EGraph,
    ast::{Command, Schedule},
};
use serde_json::{Value, json};
mod compact_observer;
use std::{
    collections::BTreeSet,
    io::{self, BufRead, Write},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn schedule(s: &Schedule, sets: &mut BTreeSet<String>, eg: &mut EGraph, assertions: &mut Vec<String>) -> Result<Value> {
    Ok(match s {
        Schedule::Run(span, r) => {
            sets.insert(r.ruleset.clone());
            let mut node=json!({"run":r.ruleset});
            if let Some(facts)=&r.until {
                use egglog::ast::{Rule,Actions,Action,Expr};
                let name=format!("DeviceBackendUntil_{}",assertions.len());
                let set=format!("device-backend-until-{}",assertions.len());
                if eg.get_function(&name).is_some(){return Err("until helper name collision".into());}
                eg.parse_and_run_program(None,&format!("(relation {name} ()) (ruleset {set})"))?;
                eg.run_program(vec![Command::Rule{rule:Rule{span:span.clone(),name:format!("probe-{set}"),ruleset:set.clone(),naive:true,body:facts.clone(),
                    head:Actions::new(vec![Action::Expr(span.clone(),Expr::Call(span.clone(),name.clone(),vec![]))])}}])?;
                sets.insert(set.clone());assertions.push(name);node["until_run"]=json!(set);
            }
            node
        }
        Schedule::Repeat(_, n, s) => json!({"repeat":n,"body":schedule(s,sets,eg,assertions)?}),
        Schedule::Saturate(_, s) => json!({"saturate":schedule(s,sets,eg,assertions)?}),
        Schedule::Sequence(_, ss) => {
            json!({"sequence":ss.iter().map(|s|schedule(s,sets,eg,assertions)).collect::<Result<Vec<_>>>()?})
        }
    })
}
pub fn execute(install: impl Fn(&mut EGraph, &str) -> Result<()>) -> Result<()> {
    EGraph::set_num_threads(1);
    let path = std::env::args().nth(1).ok_or("SOURCE.egg")?;
    let mut eg = EGraph::default();
    let source = std::fs::read_to_string(&path)?;
    install(&mut eg, &source)?;
    let cmds = eg.parse_program(Some(path.clone()), &source)?;
    if std::env::args().any(|a| a == "--native-check") {
        eg.run_program(cmds)?;
        println!("{}",json!({"native_commands_completed":true,"scope":"source checks only; not full graph equivalence"}));
        return Ok(());
    }
    if std::env::args().any(|a|a=="--selected-dag") {
        eg.run_program(cmds)?;
        println!("{}",eg.export_device_program(&[]).map_err(io::Error::other)?);
        return Ok(());
    }
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
    if std::env::args().any(|a|a=="--native-full") {
        let args=std::env::args().collect::<Vec<_>>();
        let target=args.windows(2).find(|a|a[0]=="--export-to").ok_or("--export-to required")?;
        let out=std::path::Path::new(&target[1]);std::fs::create_dir(out)?;
        let anchors=eg.capture_device_anchors().map_err(io::Error::other)?;
        let start=std::time::Instant::now();eg.run_program(cmds[at..].to_vec())?;let seconds=start.elapsed().as_secs_f64();
        serde_json::to_writer(std::io::BufWriter::new(std::fs::File::create(out.join("native-state.json"))?),&eg.device_checkpoint(&anchors).map_err(io::Error::other)?)?;
        let graph=eg.serialize(egglog::SerializeConfig::default());if !graph.is_complete(){return Err("incomplete native graph".into());}
        serde_json::to_writer_pretty(std::io::BufWriter::new(std::fs::File::create(out.join("graph.json"))?),&graph.egraph)?;
        let report=json!({"complete_schedule":true,"native_run_seconds":seconds,"scope":"Original remaining source commands after declaration/fact bootstrap; no GPU or checkpoint replay"});
        std::fs::write(out.join("report.json"),serde_json::to_vec_pretty(&report)?)?;println!("{report}");return Ok(());
    }
    if std::env::args().any(|a|a=="--inventory") {
        println!("{}",eg.device_inventory());
        return Ok(());
    }
    let mut sets = BTreeSet::new();
    let mut schedules = vec![];
    let mut assertions = vec![];
    for c in &cmds[at..] {
        match c {
            Command::RunSchedule(s) => schedules.push(schedule(s, &mut sets, &mut eg, &mut assertions)?),
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
    let args=std::env::args().collect::<Vec<_>>();
    let option=|name:&str|args.windows(2).find(|w|w[0]==name).map(|w|w[1].as_str());
    if let Some(input)=option("--load-rows") {
        let out=std::path::Path::new(option("--export-to").ok_or("--export-to required")?);
        std::fs::create_dir(out)?;
        let rows:Value=serde_json::from_slice(&std::fs::read(input)?)?;
        let imported=eg.import_device_rows_for_extraction(&rows).map_err(io::Error::other)?;
        serde_json::to_writer(std::io::BufWriter::new(std::fs::File::create(out.join("import.json"))?),&imported)?;
        let graph=eg.serialize(egglog::SerializeConfig::default());
        if !graph.is_complete(){return Err("incomplete serialized graph".into());}
        serde_json::to_writer_pretty(std::io::BufWriter::new(std::fs::File::create(out.join("graph.json"))?),&graph.egraph)?;
        let mut extracted=Vec::new();
        if let Some(table)=option("--extract-table") {
            let sort=eg.get_function(table).ok_or("unknown extraction table")?.schema().output.clone();
            let mut roots=BTreeSet::new();
            eg.function_for_each(table,|r|{if !r.subsumed{roots.insert(*r.vals.last().unwrap());}})?;
            let extractor=egglog::extract::Extractor::compute_costs_from_rootsorts(Some(vec![sort]),&eg,egglog::extract::TreeAdditiveCostModel::default());
            for root in roots {
                let mut dag=egglog::TermDag::default();
                if let Some((cost,term))=extractor.extract_best(&eg,&mut dag,root){extracted.push(json!({"value":format!("{root:?}"),"term":dag.to_string(term),"cost":cost}));}
                else{return Err("unextractable requested root".into());}
            }
        }
        println!("{}",json!({"rows":imported["rows"].as_array().unwrap().len(),"classes":imported["identities"].as_array().unwrap().len(),"rules_executed":0,"rebuilds_executed":0,"extracted":extracted}));
        return Ok(());
    }
    let mut p = eg
        .export_device_program(&sets.into_iter().collect::<Vec<_>>())
        .map_err(|e| io::Error::other(e))?;
    let mut anchors = eg.capture_device_anchors().map_err(io::Error::other)?;
    p["schedule"] = json!(schedules);
    p["assertion_tables"] = json!(assertions);
    println!("{}", p);
    io::stdout().flush()?;
    let mut observer:Option<compact_observer::Observer>=None;
    // This native engine is only advanced when the verifier explicitly requests
    // it. Check-off GPU execution leaves it frozen at initialization.
    for line in io::stdin().lock().lines() {
        let q: Value = serde_json::from_str(&line?)?;
        match q["op"].as_str() {
            Some("observe_tables") => {
                let names=q["tables"].as_array().ok_or("table names")?.iter().map(|v|v.as_str().map(str::to_owned).ok_or("table name")).collect::<std::result::Result<Vec<_>,_>>()?;
                observer=Some(compact_observer::Observer::new(&eg,names).map_err(io::Error::other)?);
                println!("{}",observer.as_mut().unwrap().snapshot(&eg).map_err(io::Error::other)?);
            }
            Some("observe_delta") => println!("{}",observer.as_mut().ok_or("observer not configured")?.snapshot(&eg).map_err(io::Error::other)?),
            Some("track_anchors") => {
                anchors = eg.capture_device_anchors().map_err(io::Error::other)?;
                println!("{}", json!({"tracked":true}));
            }
            Some("current_program") => {
                println!("{}", eg.export_device_program(&[]).map_err(io::Error::other)?);
            }
            Some("edit_actions") => {
                let edits = eg.parse_program(None, q["source"].as_str().ok_or("action source")?)?;
                if edits.iter().any(|c| !matches!(c, Command::Action(..))) {
                    return Err("external edit accepts actions only, never rule execution".into());
                }
                eg.run_program(edits)?;
                println!("{}", json!({"applied":true,"rules_executed":0}));
            }
            Some("run") => {
                let started = q["timed"].as_bool().unwrap_or(false).then(std::time::Instant::now);
                let r = eg.step_rules(q["set"].as_str().ok_or("set")?)?;
                println!("{}", json!({"updated":r.updated,"can_stop":r.can_stop,
                    "step_seconds":started.map(|t| t.elapsed().as_secs_f64()),
                    "logical_matches":r.num_matches_per_rule.values().sum::<usize>()}));
            }
            Some("unary_predicates") => {
                let mut result = serde_json::Map::new();
                for name in q["tables"].as_array().ok_or("predicate names")? {
                    let name = name.as_str().ok_or("predicate name")?;
                    let f = eg.get_function(name).ok_or("unknown predicate")?;
                    if f.schema().input.len() != 1 || !f.schema().input[0].is_eq_sort() {
                        return Err("predicate view requires one equality key".into());
                    }
                    let sort = f.schema().input[0].clone();
                    let mut keys = Vec::new();
                    eg.function_for_each(name, |row| {
                        if !row.subsumed {
                            keys.push(format!("{:?}", eg.get_canonical_value(row.vals[0], &sort)));
                        }
                    })?;
                    keys.sort(); result.insert(name.to_owned(), json!(keys));
                }
                println!("{}", Value::Object(result));
            }
            Some("time_closure") => {
                // An explicit diagnostic control: time the unchanged native
                // closure with no intervening per-Run exports. Keep the oracle
                // frozen so later visibility checks use the same starting state.
                let started = std::time::Instant::now();
                let mut control = eg.clone();
                let clone_seconds = started.elapsed().as_secs_f64();
                let started = std::time::Instant::now();
                let mut runs = 0usize;
                let mut matches = 0usize;
                loop {
                    let report = control.step_rules(q["set"].as_str().ok_or("set")?)?;
                    runs += 1;
                    matches += report.num_matches_per_rule.values().sum::<usize>();
                    if !report.updated { break; }
                }
                println!("{}", json!({"seconds":started.elapsed().as_secs_f64(),
                    "clone_seconds":clone_seconds,"runs":runs,"logical_matches":matches,
                    "scope":"native closure only, no intervening query/snapshot export"}));
            }
            Some("time_runs") => {
                // Replay an already verified native schedule without intervening
                // exports or IPC. The caller checks every update flag afterwards.
                let sets = q["sets"].as_array().ok_or("ruleset sequence")?;
                let mut steps = Vec::with_capacity(sets.len());
                let total = std::time::Instant::now();
                for set in sets {
                    let started = std::time::Instant::now();
                    let r = eg.step_rules(set.as_str().ok_or("ruleset name")?)?;
                    let seconds = started.elapsed().as_secs_f64();
                    steps.push(json!({"seconds":seconds,"updated":r.updated,
                        "logical_matches":r.num_matches_per_rule.values().sum::<usize>()}));
                }
                println!("{}", json!({"seconds":total.elapsed().as_secs_f64(),"steps":steps,
                    "scope":"original native Run sequence, no intervening IPC or graph export"}));
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
fn main() -> Result<()> {
    execute(|_, _| Ok(()))
}
