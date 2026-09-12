fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 3 {
        return Err("tier1_export input.egg output.json".into());
    }
    let r = egg_layout::tier1_effects::execute(&std::fs::read_to_string(&a[1])?)
        .map_err(std::io::Error::other)?;
    std::fs::write(&a[2], serde_json::to_string_pretty(&r)?)?;
    Ok(())
}
