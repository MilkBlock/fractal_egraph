use crate::pipeline::Result;
use egglog::EGraph;
use serde_json::{Value, json};
#[path = "frontend_control.rs"]
mod control;
pub fn core(name: &str, text: &str) -> Result<Value> {
    let mut eg = EGraph::default();
    crate::structural_key::source::install(&mut eg, text)?;
    let commands = eg.parse_program(Some(name.into()), text)?;
    let at = control::bootstrap(&mut eg, &commands)?;
    let (sets, schedules, assertions) = control::controls(&mut eg, &commands[at..])?;
    let mut core = eg
        .export_device_program(&sets.into_iter().collect::<Vec<_>>())
        .map_err(std::io::Error::other)?;
    core["schedule"] = json!(schedules);
    core["assertion_tables"] = json!(assertions);
    crate::structural_key::source::attach_device_runtime(&mut core, text)?;
    Ok(core)
}
