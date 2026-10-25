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
