//! Export the exact fragment queries used by the existing library packer.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    let lib = egg_layout::reuse_graph::library::Library::load(std::path::Path::new(&a[0]))?;
    std::fs::write(&a[1], serde_json::to_vec(&lib.fragment_catalog())?)?;
    Ok(())
}
