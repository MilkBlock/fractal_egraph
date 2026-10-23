//! Attach source-checked services without executing the source setup twice.
use crate::{Result, parameterized_cell::{Runtime, boundary}, program_io, stable_choice};
use egglog::{EGraph, ast::Command};
use serde_json::{Value as Json, json};
use std::{path::Path, time::Instant};

pub(crate) fn prepare(source: &Path, policy: &str, layout: Option<&Path>)
    -> Result<(EGraph, Vec<Command>, Json, Box<dyn crate::table_live::SaturationHook>)>
{
    let start = Instant::now();
    let (mut eg, commands, extension) = program_io::load(source)?;
    let split = commands.iter().rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("assisted source needs a main schedule")?;
    let run = match policy {
        "source" => { eg.run_program(commands[..split].to_vec())?; commands[split..].to_vec() },
        "legacy-stable" if extension.is_none() => {
            stable_choice::setup(&mut eg, &commands[..split])?;
            stable_choice::schedule(&mut eg, &commands[split..])?
        },
        "legacy-stable" => return Err("explicit structural-key source cannot also install legacy stable choice".into()),
        _ => return Err("setup-policy expects source or legacy-stable".into()),
    };
    let boundaries = layout.map(|p| boundary::load_file(p, &commands[..split])).transpose()?.unwrap_or_default();
    let runtime = Runtime::attach_joint(eg, &commands[..split], Some(&boundaries))?;
    let (eg, observer) = runtime.detach_joint()?;
    Ok((eg, run.clone(), json!({"setup_seconds":start.elapsed().as_secs_f64(),
        "runtime_extension":extension,"setup_policy":policy,"setup_commands":split,
        "run_commands":run.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "boundary_programs":layout,"protocol":"setup once; source-checked Cell observer; original main schedule"}), Box::new(observer)))
}
