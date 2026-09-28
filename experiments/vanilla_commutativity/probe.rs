// Standalone consumer of pristine egglog used for per-function table sizes.
// It runs a program and prints every command output (including `(print-size)`),
// without tracing, analysis or graph export.
use egglog::EGraph;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    let text = std::fs::read_to_string(&a[0])?;
    let mut eg = EGraph::default();
    let outputs = eg.parse_and_run_program(Some(a[0].clone()), &text)?;
    for o in &outputs {
        println!("{o}");
    }
    Ok(())
}
