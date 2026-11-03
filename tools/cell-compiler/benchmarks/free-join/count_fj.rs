use egglog::{EGraph, ast::Command};
use std::sync::atomic::Ordering;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    EGraph::set_num_threads(1);
    for path in std::env::args().skip(1) {
        let mut eg = EGraph::default();
        let source = std::fs::read_to_string(&path)?;
        let cmds = eg.parse_program(Some(path.clone()), &source)?;
        let start = cmds
            .iter()
            .position(|c| matches!(c, Command::RunSchedule(_)))
            .unwrap();
        eg.run_program(cmds[..start].to_vec())?;
        for c in &egglog_core_relations::FJ_WORK {
            c.store(0, Ordering::Relaxed);
        }
        let commands = cmds[start..].to_vec();
        let timer = std::time::Instant::now();
        eg.run_program(commands)?;
        let ms = timer.elapsed().as_secs_f64() * 1e3;
        println!(
            "{} {ms:.3} ms {:?}",
            path,
            egglog_core_relations::FJ_WORK
                .each_ref()
                .map(|c| c.load(Ordering::Relaxed))
        );
    }
    Ok(())
}
