use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Cargo starts a bench harness in its package directory. The repository
    // aliases and frozen suite resolve user paths from the repository root.
    std::env::set_current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
    eggfuse_cell_compiler::cli(true)
}
