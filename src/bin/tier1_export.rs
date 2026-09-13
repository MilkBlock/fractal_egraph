fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if !(3..=4).contains(&a.len()) {
        return Err("tier1_export input.egg output.json [--compact]".into());
    }
    let r = egg_layout::tier1_effects::execute_mode(&std::fs::read_to_string(&a[1])?,a.get(3).is_some_and(|s|s=="--compact"))
        .map_err(std::io::Error::other)?;
    std::fs::write(&a[2], serde_json::to_string_pretty(&r)?)?;
    Ok(())
}
