#[allow(dead_code)]
#[path = "tier0_probe.rs"]
mod probe;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("r23_intervention source.egg output.json".into());
    }
    probe::r23_intervention(&args[1], &args[2])
}
