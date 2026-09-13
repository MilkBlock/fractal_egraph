fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if !(3..=4).contains(&a.len()) {
        return Err("tier1_export input.egg output.json [--compact]".into());
    }
    let compact = a.get(3).is_some_and(|s|s=="--compact");
    let r = if a[1]=="-" { egg_layout::tier1_effects::execute_stream(std::io::stdin().lock(), compact) }
    else { egg_layout::tier1_effects::execute_mode(&std::fs::read_to_string(&a[1])?,compact) }
        .map_err(std::io::Error::other)?;
    if a[2]=="-" { serde_json::to_writer(std::io::stdout().lock(),&r)?; }
    else { std::fs::write(&a[2],serde_json::to_string_pretty(&r)?)?; }
    Ok(())
}
