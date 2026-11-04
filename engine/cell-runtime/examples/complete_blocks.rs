//! Specialize the existing catalog without changing any block choices.
use eggfuse_cell_runtime::{checksum::artifact_digest, iterative::Catalog, program::Program};
use std::{env, fs};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [src, dst, mode @ ..] = args.as_slice() else {
        return Err("expected ITERATIVE_PROGRAM NEW_PROGRAM".into());
    };
    let p = Program::parse(serde_json::from_slice(&fs::read(src)?)?)?;
    let mut plan = p.artifact["iterative_bake"].clone();
    let reuse = match mode {
        [] => true,
        [s] if s == "--dispatch-only" => false,
        _ => return Err("optional flag is --dispatch-only".into()),
    };
    Catalog::emit_switch(&p, &mut plan, reuse)?;
    let mut artifact = p.artifact;
    artifact["iterative_bake"] = plan;
    artifact.as_object_mut().unwrap().remove("sha256");
    artifact["sha256"] = artifact_digest(&artifact).into();
    Program::parse(artifact.clone())?;
    use std::io::Write;
    let bytes = serde_json::to_vec(&artifact)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?
        .write_all(&bytes)?;
    Ok(())
}
