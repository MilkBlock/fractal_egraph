#[allow(dead_code)]
#[path = "tier0_probe.rs"]
mod probe;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 3 {
        return Err("coverage_breadth source.egg output.json".into());
    }
    probe::coverage_breadth(&a[1], &a[2])?;
    Ok(())
}
