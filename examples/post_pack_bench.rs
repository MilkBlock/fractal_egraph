// Identical public-API harness for pristine egglog and this checkout. No tracing.
use egglog::EGraph;

use serde_json::json;
use std::{collections::BTreeSet, io::Write, time::Instant};
fn id(v: egglog::Value) -> u64 {
    format!("{v:?}")
        .trim_start_matches("Value(")
        .trim_end_matches(')')
        .parse()
        .unwrap()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    let text = std::fs::read_to_string(&a[0])?;
    let start = Instant::now();
    let mut eg = EGraph::default();
    let outputs = eg.parse_and_run_program(Some(a[0].clone()), &text)?;
    let native_seconds = start.elapsed().as_secs_f64();
    let pack_start = Instant::now();
    if let Ok(path) = std::env::var("MEMORY_FIX_LIBRARY") {
        let lib = egg_layout::reuse_graph::library::Library::load(std::path::Path::new(&path))?;
        if std::env::var("MEMORY_FIX_MODE").is_ok_and(|s| s == "cell") {
            let report=egg_layout::reuse_graph::native_storage::compact_library(&mut eg,&lib)?;
            if let Ok(path)=std::env::var("CELL_STORAGE_REPORT") {std::fs::write(path,serde_json::to_vec_pretty(&report)?)?;}
        } else if std::env::var("MEMORY_FIX_MODE").is_ok_and(|s| s == "match") {
            egg_layout::reuse_graph::native_storage::match_library(&mut eg, &lib)?;
        } else {
            egg_layout::reuse_graph::native_storage::pack_library(&mut eg, &lib)?;
        }
    }
    let pack_seconds = pack_start.elapsed().as_secs_f64();
    let seconds = start.elapsed().as_secs_f64();
    let matches = outputs
        .iter()
        .filter_map(|o| {
            if let egglog::CommandOutput::RunSchedule(r) = o {
                Some(r.num_matches_per_rule.values().sum::<usize>())
            } else {
                None
            }
        })
        .sum::<usize>();
    if let Some(path) = a.get(1) {
        let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
        write!(out, "[")?;
        let mut comma = false;
        let mut nodes = 0;
        let mut classes = BTreeSet::new();
        for name in eg.get_function_names() {
            let f = eg.get_function(&name).unwrap();
            if f.schema().output.name() != "Math" {
                continue;
            }
            eg.function_for_each(&name, |r| {
                let args = r
                    .vals
                    .iter()
                    .zip(&f.schema().input)
                    .map(|(v, t)| match t.name() {
                        "Math" => json!(["Math", id(eg.get_canonical_value(*v, t))]),
                        "i64" => json!(["i64", eg.value_to_base::<i64>(*v).to_string()]),
                        "String" => json!([
                            "String",
                            eg.value_to_base::<egglog::sort::S>(*v).to_string()
                        ]),
                        _ => panic!("unsupported benchmark sort"),
                    })
                    .collect::<Vec<_>>();
                let result =
                    id(eg.get_canonical_value(*r.vals.last().unwrap(), &f.schema().output));
                classes.insert(result);
                nodes += 1;
                if comma {
                    write!(out, ",").unwrap();
                }
                comma = true;
                serde_json::to_writer(&mut out, &json!({"op":name,"args":args,"result":result}))
                    .unwrap();
            })?;
        }
        write!(out, "]")?;
        out.flush()?;
        println!(
            "{}",
            json!({"seconds":seconds,"native_seconds":native_seconds,"pack_seconds":pack_seconds,"matches":matches,"enodes":nodes,"eclasses":classes.len(),"verification":true})
        );
    } else {
        drop(outputs);
        drop(eg);
        println!(
            "{}",
            json!({"seconds":seconds,"native_seconds":native_seconds,"pack_seconds":pack_seconds,"through_drop_seconds":start.elapsed().as_secs_f64(),"matches":matches,"verification":false})
        );
    }
    Ok(())
}
