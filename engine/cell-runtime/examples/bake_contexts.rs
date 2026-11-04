//! Transfer profitable typed query shapes from a paired discovery run.
use eggfuse_cell_runtime::{
    checksum::artifact_digest,
    context_admission::{bake, install},
    program::Program,
};
use std::{env, fs, io::Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [source, off, general, target, out] = args.as_slice() else {
        return Err(
            "expected TRAIN_PROGRAM OFF_REPORT GENERAL_REPORT TARGET_PROGRAM NEW_PROGRAM".into(),
        );
    };
    let read = |p: &str| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        Ok(serde_json::from_slice(&fs::read(p)?)?)
    };
    let program = |p: &str| -> Result<Program, Box<dyn std::error::Error>> {
        let mut raw = read(p)?;
        let claimed = raw
            .as_object_mut()
            .ok_or("invalid artifact")?
            .remove("sha256")
            .ok_or("missing program digest")?;
        if claimed != artifact_digest(&raw) {
            return Err("program digest mismatch".into());
        }
        raw["sha256"] = claimed;
        Ok(Program::parse(raw)?)
    };
    let train = program(source)?;
    let policy = bake(&train, &read(off)?, &read(general)?)?;
    let target = program(target)?;
    let policy = install(&target, policy)?;
    let mut pg = target.artifact;
    pg["context_admission"] = policy;
    pg.as_object_mut().ok_or("invalid target")?.remove("sha256");
    pg["sha256"] = artifact_digest(&pg).into();
    Program::parse(pg.clone())?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?
        .write_all(&serde_json::to_vec(&pg)?)?;
    Ok(())
}
