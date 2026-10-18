use cell_assisted_egglog::{Part, Result, Session, environment, matrix};
use egglog::{EGraph, cell_assist::CellOperand};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};

fn args() -> Result<BTreeMap<String, String>> {
    let mut a = std::env::args().skip(1);
    let mut out = BTreeMap::new();
    while let Some(k) = a.next() {
        if !k.starts_with("--") {
            return Err(format!("expected --option, got {k}").into());
        }
        out.insert(k, a.next().ok_or("missing option value")?);
    }
    Ok(out)
}
fn required<'a>(a: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    Ok(a.get(key).ok_or_else(|| format!("missing {key}"))?.as_str())
}
fn snapshot(eg: &EGraph) -> Result<Json> {
    let mut rows = vec![];
    let names = eg.get_function_names();
    let eq: std::collections::BTreeSet<_> = names
        .iter()
        .filter_map(|n| eg.get_function(n))
        .filter(|f| f.is_constructor())
        .map(|f| f.schema().output.name().to_owned())
        .collect();
    for name in names {
        if name.starts_with('@') {
            continue;
        }
        let f = eg.get_function(&name).unwrap();
        if f.is_let_binding() {
            continue;
        }
        eg.function_for_each(&name,|r|{
            let args=r.vals.iter().zip(&f.schema().input).map(|(v,t)|{
                let value=if eq.contains(t.name()){format!("{}:{:?}",t.name(),eg.get_canonical_value(*v,t))}else{
                    match t.name(){"i64"=>eg.value_to_base::<i64>(*v).to_string(),"bool"=>eg.value_to_base::<bool>(*v).to_string(),"String"=>eg.value_to_base::<egglog::sort::S>(*v).to_string(),"Unit"=>"()".into(),other=>panic!("unsupported snapshot sort {other}")}
                };json!([t.name(),value])
            }).collect::<Vec<_>>();
            rows.push(json!({"op":name,"sort":f.schema().output.name(),"args":args,"result":format!("{}:{:?}",f.schema().output.name(),eg.get_canonical_value(*r.vals.last().unwrap(),&f.schema().output)),"subsumed":r.subsumed}));
        })?;
    }
    Ok(json!(rows))
}
fn run() -> Result<()> {
    let a = args()?;
    if a.get("--mode").map(String::as_str) == Some("matrix") {
        let report = PathBuf::from(required(&a, "--output")?);
        let program = match (a.get("--program"), a.get("--analysis")) {
            (Some(input), None) => serde_json::from_slice(&std::fs::read(input)?)?,
            (None, Some(dir)) => {
                let p = cell_assisted_egglog::analyze_program::load(Path::new(dir))?;
                std::fs::write(
                    report.with_extension("program.json"),
                    serde_json::to_vec_pretty(&p)?,
                )?;
                p
            }
            _ => return Err("matrix mode requires exactly one of --program or --analysis".into()),
        };
        let checked = program.check()?;
        match a.get("--backend").map(String::as_str).unwrap_or("cpu") {
            "cpu" => std::fs::write(&report, serde_json::to_vec_pretty(&checked.execute())?)?,
            "metal" => {
                let lowered = report.with_extension("lowered.json");
                std::fs::write(&lowered, serde_json::to_vec(&checked.lower()?)?)?;
                let status = std::process::Command::new(required(&a, "--gpu-worker")?)
                    .arg(&lowered)
                    .arg(&report)
                    .status()?;
                if !status.success() {
                    return Err("matrix GPU worker failed".into());
                }
            }
            _ => return Err("matrix backend must be cpu or metal".into()),
        }
        println!("{}", report.display());
        return Ok(());
    }
    let dir = PathBuf::from(required(&a, "--analysis")?);
    let ruleset = a.get("--ruleset").map(String::as_str).unwrap_or("");
    let consumer = required(&a, "--consumer")?;
    let mode = a.get("--mode").map(String::as_str).unwrap_or("cpu");
    let start = Instant::now();
    let (env, _) = environment(&dir)?;
    if mode == "inspect" {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &env)?;
        let sink = eg.prepare_cell_action(ruleset, consumer)?;
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"guards":format!("{:?}",sink.guards),"slots":sink.slots(),"atoms":sink.atoms.iter().enumerate().map(|(i,x)|json!({"id":i,"table":x.function,"columns":x.columns.iter().map(|c|match c{CellOperand::Slot(i)=>json!({"slot":i}),CellOperand::Constant{text,..}=>json!({"literal":text})}).collect::<Vec<_>>()})).collect::<Vec<_>>()})
            )?
        );
        return Ok(());
    }
    let rounds: usize = required(&a, "--rounds")?.parse()?;
    let input = std::fs::read_to_string(required(&a, "--input")?)?;
    let (report, graph) = if mode == "off" {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &env)?;
        let setup = start.elapsed().as_secs_f64();
        let t = Instant::now();
        eg.parse_and_run_program(None, &input)?;
        let input_time = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let mut matches = 0;
        for _ in 0..rounds {
            let r = eg.step_rules(ruleset)?;
            matches += r.num_matches_per_rule.values().sum::<usize>();
        }
        let elapsed = t.elapsed().as_secs_f64();
        (
            json!({"mode":mode,"setup_seconds":setup,"input_seconds":input_time,"run_seconds":elapsed,"native_matches":matches,"scope":"fork native rules, no Cell assistance or tracing"}),
            snapshot(&eg)?,
        )
    } else {
        let native = required(&a, "--native")?
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let parts: Vec<Part> = serde_json::from_str(required(&a, "--parts")?)?;
        let mut session = Session::new(&dir, ruleset, consumer, native, parts)?;
        session.set_audit(a.get("--audit").is_some_and(|v| v == "true"));
        let description = session.describe();
        if mode == "resident" {
            let mut resident = cell_assisted_egglog::resident::ResidentSession::new(
                session,
                Path::new(required(&a, "--gpu-worker")?),
            )?;
            let setup = start.elapsed().as_secs_f64();
            let t = Instant::now();
            resident.input(&input)?;
            let input_time = t.elapsed().as_secs_f64();
            let t = Instant::now();
            let mut progress = vec![];
            for _ in 0..rounds {
                progress.push(resident.round()?);
            }
            let elapsed = t.elapsed().as_secs_f64();
            (
                json!({"mode":mode,"setup_seconds":setup,"input_seconds":input_time,"run_seconds":elapsed,"work_progress":progress,"plan":resident.describe(),"stats":resident.stats,"scope":"resident GPU Cell proof maintenance and delta matching; native RHS and commits"}),
                snapshot(resident.engine())?,
            )
        } else {
            let mut backend: Box<dyn matrix::Matcher> = match mode {
                "cpu" => Box::new(matrix::Cpu),
                "gpu" => Box::new(matrix::Metal::new(Path::new(required(
                    &a,
                    "--gpu-worker",
                )?))?),
                _ => return Err("mode must be off, cpu, gpu, resident or inspect".into()),
            };
            let setup = start.elapsed().as_secs_f64();
            let t = Instant::now();
            session.input(&input)?;
            let input_time = t.elapsed().as_secs_f64();
            let t = Instant::now();
            let mut progress = vec![];
            for _ in 0..rounds {
                progress.push(session.round(&mut *backend)?);
            }
            let elapsed = t.elapsed().as_secs_f64();
            (
                json!({"mode":mode,"setup_seconds":setup,"input_seconds":input_time,"run_seconds":elapsed,"semantic_progress":progress,"stats":session.stats,"plan":description,"scope":"analyze-covered positive Cell fragments; uncovered instances remain pending, not failed matches; no claim of arbitrary-rule completeness"}),
                snapshot(session.engine())?,
            )
        }
    };
    if let Some(path) = a.get("--dump") {
        std::fs::write(path, serde_json::to_vec(&graph)?)?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("cell-assisted-egglog: {e}");
        std::process::exit(1)
    }
}
