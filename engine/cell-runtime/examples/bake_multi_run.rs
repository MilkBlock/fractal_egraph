//! Compile all closed port edges from the checked whole program, without PGO.
use eggfuse_cell_runtime::{checksum::artifact_digest, program::Program};
use serde_json::{Value as J, json};
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        return Err("expected PROGRAM NEW_OUTPUT".into());
    };
    if Path::new(output).exists() {
        return Err("output exists".into());
    }
    let p = Program::parse(serde_json::from_slice(&fs::read(input)?)?)?;
    let catalog = p.compile_multi_run();
    let edges = catalog["entries"].as_array().unwrap().len();
    let results = catalog["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["results"].as_array().is_some_and(|r| !r.is_empty()))
        .count();
    let mut raw: J = p.artifact;
    raw["multi_run_blocks"] = catalog;
    raw.as_object_mut().unwrap().remove("sha256");
    raw["sha256"] = json!(artifact_digest(&raw));
    let checked = Program::parse(raw)?;
    fs::write(output, serde_json::to_vec(&checked.artifact)?)?;
    println!(
        "{}",
        json!({"edges":edges,"edges_with_results":results,"program_sha256":checked.artifact["sha256"]})
    );
    Ok(())
}
