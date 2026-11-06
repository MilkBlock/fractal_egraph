use eggfuse_cell_runtime::{checksum::artifact_digest, program::Program};
use serde_json::{Value as J, json};
use std::{fs, path::Path};
fn read(p: &str) -> Result<J, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let [training, profile, target, output] = args.as_slice() else {
        return Err("expected TRAIN_PROGRAM SURVEY_REPORT TARGET_PROGRAM NEW_OUTPUT".into());
    };
    if Path::new(output).exists() {
        return Err("output exists".into());
    }
    let a = Program::parse(read(training)?)?;
    let p = Program::parse(read(target)?)?;
    let catalog = a.bake_two_run(&read(profile)?, &p)?;
    let mut raw = p.artifact;
    raw["two_run_blocks"] = catalog;
    raw.as_object_mut().unwrap().remove("sha256");
    raw["sha256"] = json!(artifact_digest(&raw));
    let checked = Program::parse(raw)?;
    fs::write(output, serde_json::to_vec(&checked.artifact)?)?;
    println!("{}", checked.artifact["two_run_blocks"]);
    Ok(())
}
