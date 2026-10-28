//! Inspect or verify Analyze's generic recipe library without the training source.
use egg_layout::{cell_recipe::Program, pipeline::Result};
use serde_json::json;
use std::path::Path;
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 2 {
        return Err("LIBRARY.json OUTPUT.json [UNIT|all]".into());
    }
    egglog::EGraph::set_num_threads(1);
    let p = Program::load(Path::new(&args[0]))?;
    let mut results = vec![];
    let units = if args.get(2).is_some_and(|s| s != "all") {
        vec![args[2].parse::<usize>()?]
    } else {
        p.document["cells"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["program_status"] == "ready" && c["ripen_state"] == "Saturated")
            .map(|c| c["unit"].as_u64().unwrap() as usize)
            .collect()
    };
    for unit in units {
        let validation = p.execute(unit, true, false).map(|_| ());
        let result = match validation.and_then(|_| p.verify_endpoint(unit)) {
            Ok(r) => r,
            Err(e) => json!({"unit":unit,"error":e.to_string()}),
        };
        eprintln!(
            "Cell {unit}: {}",
            if result.get("error").is_none() {
                "passed"
            } else {
                "failed"
            }
        );
        results.push(result);
    }
    let failed = results.iter().filter(|r| r.get("error").is_some()).count();
    let skipped=p.document["cells"].as_array().unwrap().iter().filter(|c|c["program_status"]!="ready"||c["ripen_state"]!="Saturated").map(|c|json!({"unit":c["unit"],"program_status":c["program_status"],"ripen_state":c["ripen_state"],"reason":c["reason"]})).collect::<Vec<_>>();
    std::fs::write(
        &args[1],
        serde_json::to_vec_pretty(
            &json!({"checked":results.len(),"failed":failed,"results":results,"skipped":skipped,"scope":"native isolated replay and terminal endpoint reconstruction; not live GPU acceleration"}),
        )?,
    )?;
    if failed > 0 {
        return Err(format!("{failed} Cell checks failed").into());
    }
    Ok(())
}
