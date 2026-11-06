//! Recompile a learned LHS composition catalog for a distinct workload.
use eggfuse_cell_runtime::{iterative::Catalog, program::Program};
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let [source, target, output] = args.as_slice() else {
        return Err("expected LEARNED_PROGRAM TARGET_PROGRAM NEW_OUTPUT".into());
    };
    if Path::new(output).exists() {
        return Err("output exists".into());
    }
    let read = |p: &str| -> Result<Program, Box<dyn std::error::Error>> {
        Ok(Program::parse(serde_json::from_slice(&fs::read(p)?)?)?)
    };
    let source = read(source)?;
    let target = read(target)?;
    let raw = Catalog::transfer_lhs(&source, &target)?;
    fs::write(output, serde_json::to_vec(&raw)?)?;
    println!(
        "{}",
        serde_json::json!({"source":source.artifact["sha256"],"target":target.artifact["sha256"],"output":raw["sha256"],"joins":raw["iterative_bake"]["joins"].as_array().unwrap().len()})
    );
    Ok(())
}
