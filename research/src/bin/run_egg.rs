//! Run an unchanged native .egg program; capture outputs and whole-program time.
use egglog::EGraph;
use serde_json::json;
use std::time::Instant;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let path = args
        .get(1)
        .ok_or("usage: run_egg SOURCE.egg [OUTPUT.json]")?;
    let source = std::fs::read_to_string(path)?;
    let mut eg = EGraph::default();
    let start = Instant::now();
    let result = eg.parse_and_run_program(Some(path.clone()), &source);
    let elapsed = start.elapsed().as_secs_f64();
    let (outputs, error) = match result {
        Ok(o) => (o.iter().map(ToString::to_string).collect::<Vec<_>>(), None),
        Err(e) => (vec![], Some(e.to_string())),
    };
    let data = json!({"source":path,"execution":"original native program; no dependency tracing or schedule modification","build":if cfg!(debug_assertions){"debug"}else{"release"},"elapsed_seconds":elapsed,"outputs":outputs,"error":error});
    let text = serde_json::to_string_pretty(&data)?;
    if let Some(out) = args.get(2) {
        std::fs::write(out, text)?;
    } else {
        println!("{text}");
    }
    if let Some(e) = error {
        return Err(e.into());
    }
    Ok(())
}
