//! Stream outer saturation exits and finite Runs, not changed internal rounds.
use eggfuse_cell_runtime::{executor::Executor,graph::Graph,program::Program};
use serde_json::json;
use std::io::{BufWriter,Write};
fn main()->Result<(),Box<dyn std::error::Error>> {
    let path=std::env::args().nth(1).ok_or("program path")?;
    let p=Program::parse(serde_json::from_slice(&std::fs::read(path)?)?)?;
    let mut e=Executor::new(Graph::new(p)?,false);let mut out=BufWriter::new(std::io::stdout().lock());
    serde_json::to_writer(&mut out,&json!({"kind":"initial","state":e.graph.export()?}))?;out.write_all(b"\n")?;out.flush()?;
    while !e.tasks.is_empty() {
        let n=e.steps.len();e.execute_until(Some(n+1))?;
        for event in e.take_semantic_audit(){serde_json::to_writer(&mut out,&event)?;out.write_all(b"\n")?;out.flush()?;}
    }
    serde_json::to_writer(&mut out,&json!({"kind":"summary","local":e.report()["local_saturation"],"runs":e.steps.len()}))?;
    out.write_all(b"\n")?;out.flush()?;Ok(())
}
