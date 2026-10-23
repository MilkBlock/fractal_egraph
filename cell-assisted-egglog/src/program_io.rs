//! Public-API-only harness shared verbatim with the immutable upstream build.
use egglog::{ArcSort, EGraph, Value, ast::Command};
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, path::Path, time::Instant};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn prepare(source: &Path) -> Result<(EGraph, Vec<Command>, Json)> {
    let start = Instant::now();
    let mut eg = EGraph::default();
    let commands = eg.parse_program(
        Some(source.display().to_string()),
        &std::fs::read_to_string(source)?,
    )?;
    // Match the existing Pass-3 protocol: initialization is native in every
    // lane; the last/main schedule and following observations are the run region.
    let split = commands
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .unwrap_or(0);
    eg.run_program(commands[..split].to_vec())?;
    let run = commands[split..].to_vec();
    let metadata = json!({"setup_seconds":start.elapsed().as_secs_f64(),"setup_commands":split,
        "run_commands":run.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "protocol":"native setup before last schedule; retain checkpoint and execute clone; original source policy"});
    Ok((eg, run, metadata))
}
fn value(eg: &EGraph, v: Value, s: &ArcSort) -> Result<Json> {
    let v = if s.is_eq_sort() {
        json!({"eq":format!("{:?}",eg.get_canonical_value(v,s))})
    } else {
        let x = match s.name() {
            "i64" => eg.value_to_base::<i64>(v).to_string(),
            "f64" => format!("{:016x}", eg.value_to_base::<egglog::sort::F>(v).to_bits()),
            "bool" => eg.value_to_base::<bool>(v).to_string(),
            "String" => eg.value_to_base::<egglog::sort::S>(v).to_string(),
            "Unit" => "()".into(),
            x => return Err(format!(
                "graph comparison cannot encode sort {x}; execution is separate from verification"
            )
            .into()),
        };
        json!({"base":x})
    };
    Ok(json!([s.name(), v]))
}
pub fn snapshot(eg: &EGraph) -> Result<Json> {
    let mut tables = BTreeMap::new();
    let mut rows = vec![];
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        if name.starts_with('@') || f.is_let_binding() {
            continue;
        }
        tables.insert(
            name.clone(),
            json!({"input":f.schema().input.iter().map(|s|s.name()).collect::<Vec<_>>(),
            "output":f.schema().output.name()}),
        );
        let mut failure = None;
        eg.function_for_each(&name, |r| {
            let values = r
                .vals
                .iter()
                .zip(f.schema().input.iter().chain([&f.schema().output]))
                .map(|(v, s)| value(eg, *v, s))
                .collect::<Result<Vec<_>>>();
            match values {
                Ok(mut vs) => {
                    let result = vs.pop().unwrap();
                    rows.push(json!([name, vs, result, r.subsumed]));
                }
                Err(e) => failure = Some(e),
            }
        })?;
        if let Some(e) = failure {
            return Err(e);
        }
    }
    Ok(json!({"schema":"program-graph/v1","tables":tables,"rows":rows}))
}
pub fn save(out: &Path, eg: &EGraph, mut report: Json) -> Result<()> {
    match snapshot(eg) {
        Ok(graph) => {
            report["snapshot_complete"] = json!(true);
            report["table_rows"] = json!(graph["rows"].as_array().unwrap().len());
            serde_json::to_writer(
                std::io::BufWriter::new(std::fs::File::create(out.join("graph.json"))?),
                &graph,
            )?;
        }
        Err(e) => {
            report["snapshot_complete"] = json!(false);
            report["snapshot_error"] = json!(e.to_string());
        }
    }
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
pub fn native(source: &Path, out: &Path, backend: &str, provenance: Json) -> Result<()> {
    std::fs::create_dir(out)?;
    let prepared = prepare(source);
    let (frozen, commands, setup) = match prepared {
        Ok(x) => x,
        Err(e) => {
            std::fs::write(
                out.join("report.json"),
                serde_json::to_vec_pretty(
                    &json!({"backend":backend,"complete":false,"phase":"setup","error":e.to_string(),"provenance":provenance}),
                )?,
            )?;
            return Err(e);
        }
    };
    let mut eg = frozen.clone();
    let start = Instant::now();
    let execution = eg.run_program(commands);
    let elapsed = start.elapsed().as_secs_f64();
    let outputs = execution
        .as_ref()
        .ok()
        .map(|xs| xs.iter().map(ToString::to_string).collect::<Vec<_>>());
    save(
        out,
        &eg,
        json!({"schema":"program-run/v1","backend":backend,"complete":execution.is_ok(),
        "phase":"run","error":execution.as_ref().err().map(ToString::to_string),"setup":setup,
        "run_seconds":elapsed,"outputs":outputs,"provenance":provenance,
        "cell_assistance":false,"source_rewritten_by_runner":false}),
    )?;
    execution?;
    Ok(())
}
