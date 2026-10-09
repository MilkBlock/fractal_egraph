//! Compare complete typed binding relations immediately at each event boundary.
use egglog::EGraph;
use serde_json::{Value, json};
use std::{collections::BTreeSet, error::Error, fs};

fn rows(eg: &EGraph, name: &str) -> Result<(Vec<String>, BTreeSet<Vec<String>>), Box<dyn Error>> {
    let function = eg.get_function(name).ok_or("missing comparison relation")?;
    let schema = function.schema();
    let sorts: Vec<String> = schema.input.iter().map(|s| s.name().to_string()).collect();
    let mut result = BTreeSet::new();
    eg.function_for_each(name, |row| {
        if !row.subsumed {
            result.insert(
                schema
                    .input
                    .iter()
                    .zip(row.vals.iter())
                    .map(|(sort, value)| format!("{:?}", eg.get_canonical_value(*value, sort)))
                    .collect(),
            );
        }
    })?;
    Ok((sorts, result))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: lhs_equivalence MANIFEST.json REPORT.json".into());
    }
    let manifest: Value = serde_json::from_str(&fs::read_to_string(&args[1])?)?;
    EGraph::set_num_threads(1);
    let mut eg = EGraph::default();
    let setup = manifest["setup"].as_str().ok_or("missing setup")?;
    eg.parse_and_run_program(Some(setup.into()), &fs::read_to_string(setup)?)?;
    let mut stages = Vec::new();
    for stage in manifest["stages"].as_array().ok_or("missing stages")? {
        let source = stage["source"].as_str().ok_or("missing source")?;
        eg.parse_and_run_program(Some(source.into()), &fs::read_to_string(source)?)?;
        let original = rows(&eg, stage["original"].as_str().ok_or("missing original")?)?;
        let rewritten = rows(&eg, stage["rewritten"].as_str().ok_or("missing rewritten")?)?;
        if original != rewritten {
            return Err(format!(
                "binding mismatch at {}: original={}, rewritten={}, missing={:?}, extra={:?}",
                source,
                original.1.len(),
                rewritten.1.len(),
                original
                    .1
                    .difference(&rewritten.1)
                    .take(3)
                    .collect::<Vec<_>>(),
                rewritten
                    .1
                    .difference(&original.1)
                    .take(3)
                    .collect::<Vec<_>>()
            )
            .into());
        }
        let touches: Vec<_> = stage["touches"]
            .as_array()
            .ok_or("missing touches")?
            .iter()
            .map(|name| {
                let n = name.as_str().unwrap();
                json!({"relation":n,"rows":eg.get_size(n)})
            })
            .collect();
        stages.push(
            json!({"label":stage["label"],"binding_count":original.1.len(),
            "binding_sorts":original.0,"same_bindings":true,"touches":touches}),
        );
    }
    fs::write(
        &args[2],
        serde_json::to_string_pretty(
            &json!({"scope":"same native egraph, compared before each next event; no timing benchmark", "stages":stages}),
        )? + "\n",
    )?;
    println!("All {} stage comparisons passed", stages.len());
    Ok(())
}
