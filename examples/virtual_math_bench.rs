//! Same native workload, one packing boundary. Verification is a separate process
//! mode so a second engine/state dump does not inflate measured time or peak RSS.
use egg_layout::{
    pipeline::Result,
    reuse_graph::{library::Library, native_storage},
    saturated_rule_composition::{self as state, Row, SaturatedRuleComposition, Vertex},
};
use egglog::{
    EGraph,
    ast::{Command, GenericSchedule},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Instant};
fn run(source: &Path, library: &Path, mode: &str) -> Result<(EGraph, Value)> {
    let start = Instant::now();
    let text = std::fs::read_to_string(source)?;
    let tick = Instant::now();
    let lib = if mode == "native" {
        None
    } else {
        Some(Library::load(library)?)
    };
    let load_seconds = tick.elapsed().as_secs_f64();
    let mut eg = EGraph::default();
    let commands = eg.parse_program(Some(source.display().to_string()), &text)?;
    let mut rounds = 0;
    let mut round_times = vec![];
    let mut intervention_seconds = 0.;
    let mut packed = Value::Null;
    let mut boundary_tables = Value::Null;
    for command in commands {
        if let Command::RunSchedule(GenericSchedule::Repeat(_, n, inner)) = &command {
            if !matches!(inner.as_ref(), GenericSchedule::Run(..)) {
                return Err("benchmark requires simple run schedule".into());
            }
            for _ in 0..*n {
                let tick = Instant::now();
                eg.run_program(vec![Command::RunSchedule((**inner).clone())])?;
                round_times.push(tick.elapsed().as_secs_f64());
                rounds += 1;
                if rounds == 1 {
                    boundary_tables = json!(
                        eg.get_function_names()
                            .into_iter()
                            .map(|n| {
                                let size = eg.get_size(&n);
                                (n, size)
                            })
                            .collect::<BTreeMap<_, _>>()
                    );
                    if let Some(lib) = &lib {
                        let tick = Instant::now();
                        packed = if mode == "virtual" {
                            native_storage::pack_library(&mut eg, lib)?
                        } else {
                            native_storage::match_library(&mut eg, lib)?
                        };
                        intervention_seconds = tick.elapsed().as_secs_f64();
                    }
                }
            }
        } else {
            eg.run_program(vec![command])?;
        }
    }
    if rounds != 3 {
        return Err("benchmark must execute exactly three rounds".into());
    }
    let internal_seconds = start.elapsed().as_secs_f64();
    let tables: BTreeMap<_, _> = eg
        .get_function_names()
        .into_iter()
        .map(|n| (n.to_string(), eg.get_size(&n)))
        .collect();
    let report = json!({"mode":mode,"internal_seconds":internal_seconds,"library_load_seconds":load_seconds,"match_and_pack_seconds":intervention_seconds,"round_seconds":round_times,"rounds":rounds,"tables":tables,"boundary_tables":boundary_tables,"intervention":packed,"final_storage":native_storage::storage_report(&eg)});
    Ok((eg, report))
}
fn snapshot(eg: &EGraph) -> Result<SaturatedRuleComposition> {
    let mut ids = BTreeMap::new();
    let mut values = vec![];
    let mut rows = vec![];
    let mut hidden = vec![];
    let mut names = eg.get_function_names();
    names.sort();
    for name in names {
        let f = eg.get_function(&name).unwrap();
        let schema = f.schema();
        let sorts = schema
            .input
            .iter()
            .chain([&schema.output])
            .collect::<Vec<_>>();
        let mut raw = vec![];
        eg.function_for_each(&name, |r| raw.push((r.vals.to_vec(), r.subsumed)))?;
        for (row, subsumed) in raw {
            let mut cols = vec![];
            for (v, sort) in row.iter().zip(&sorts) {
                let key = (sort.name().to_string(), eg.get_canonical_value(*v, sort));
                let id = if let Some(id) = ids.get(&key) {
                    *id
                } else {
                    let literal = match sort.name() {
                        "Math" => None,
                        "i64" => Some(eg.value_to_base::<i64>(*v).to_string()),
                        "String" => Some(serde_json::to_string(
                            &eg.value_to_base::<egglog::sort::S>(*v).to_string(),
                        )?),
                        "bool" => Some(eg.value_to_base::<bool>(*v).to_string()),
                        "Unit" => Some("()".into()),
                        _ => return Err("unsupported mini3 snapshot sort".into()),
                    };
                    let id = values.len();
                    ids.insert(key, id);
                    values.push(Vertex {
                        sort: sort.name().into(),
                        literal,
                    });
                    id
                };
                cols.push(id);
            }
            let result = cols.pop().ok_or("empty row")?;
            if subsumed {
                hidden.push(rows.len());
            }
            rows.push(Row {
                op: name.clone(),
                args: cols,
                result,
            });
        }
    }
    Ok(SaturatedRuleComposition {
        version: 2,
        local_ids: vec![],
        scope: json!({"benchmark":"math-mini3-exact-state"}),
        values,
        rows,
        subsumed_rows: hidden,
        ports: BTreeMap::new(),
    })
}
fn main() -> Result {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 || !["native", "match", "virtual", "verify"].contains(&args[0].as_str()) {
        return Err("usage: virtual_math_bench MODE SOURCE.egg LIBRARY.json".into());
    }
    if args[0] == "verify" {
        let (a, _) = run(Path::new(&args[1]), Path::new(&args[2]), "native")?;
        let a = snapshot(&a)?;
        let mut results = vec![];
        for mode in ["match", "virtual"] {
            let (b, report) = run(Path::new(&args[1]), Path::new(&args[2]), mode)?;
            let b = snapshot(&b)?;
            let comparison = state::compare(&a, &b)?;
            if !matches!(comparison, state::Comparison::Equivalent { .. }) {
                return Err(format!("{mode} differs from native: {comparison:?}").into());
            }
            results.push(json!({"mode":mode,"comparison":comparison,"rows":b.rows.len(),"values":b.values.len(),"intervention":report["intervention"],"final_storage":report["final_storage"]}));
        }
        println!(
            "{}",
            json!({"verification":results,"scope":"all final table rows, literals, equality and visibility; verification excluded from timing/RSS runs"})
        );
    } else {
        let tick = Instant::now();
        let (eg, mut report) = run(Path::new(&args[1]), Path::new(&args[2]), &args[0])?;
        drop(eg);
        report["worker_seconds_including_drop"] = json!(tick.elapsed().as_secs_f64());
        println!("{report}");
    }
    Ok(())
}
