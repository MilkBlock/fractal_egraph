//! Shared native frontend for the interactive oracle and the Cell CPU compiler.
use egglog::{
    EGraph,
    ast::{Command, Schedule},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn bootstrap(eg: &mut EGraph, commands: &[Command]) -> Result<usize> {
    let at = commands
        .iter()
        .position(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("missing schedule")?;
    // No hidden native inference through include/fail/custom commands during
    // bootstrap. The device path owns every scheduled rule execution.
    for c in &commands[..at] {
        if !matches!(
            c,
            Command::Sort { .. }
                | Command::Datatype { .. }
                | Command::Datatypes { .. }
                | Command::Constructor { .. }
                | Command::Function { .. }
                | Command::Relation { .. }
                | Command::AddRuleset(..)
                | Command::UnstableCombinedRuleset(..)
                | Command::Rule { .. }
                | Command::Rewrite(..)
                | Command::BiRewrite(..)
                | Command::Action(..)
        ) {
            return Err(
                "device bootstrap accepts declarations and facts only; flatten includes first"
                    .into(),
            );
        }
    }
    eg.run_program(commands[..at].to_vec())?;
    Ok(at)
}
fn schedule(
    s: &Schedule,
    sets: &mut BTreeSet<String>,
    eg: &mut EGraph,
    assertions: &mut Vec<String>,
) -> Result<Value> {
    Ok(match s {
        Schedule::Run(span, r) => {
            sets.insert(r.ruleset.clone());
            let mut node = json!({"run":r.ruleset});
            if let Some(facts) = &r.until {
                use egglog::ast::{Action, Actions, Expr, Rule};
                let name = format!("DeviceBackendUntil_{}", assertions.len());
                let set = format!("device-backend-until-{}", assertions.len());
                if eg.get_function(&name).is_some() {
                    return Err("until helper name collision".into());
                }
                eg.parse_and_run_program(None, &format!("(relation {name} ()) (ruleset {set})"))?;
                eg.run_program(vec![Command::Rule {
                    rule: Rule {
                        span: span.clone(),
                        name: format!("probe-{set}"),
                        ruleset: set.clone(),
                        naive: true,
                        body: facts.clone(),
                        head: Actions::new(vec![Action::Expr(
                            span.clone(),
                            Expr::Call(span.clone(), name.clone(), vec![]),
                        )]),
                    },
                }])?;
                sets.insert(set.clone());
                assertions.push(name);
                node["until_run"] = json!(set);
            }
            node
        }
        Schedule::Repeat(_, n, s) => json!({"repeat":n,"body":schedule(s,sets,eg,assertions)?}),
        Schedule::Saturate(_, s) => json!({"saturate":schedule(s,sets,eg,assertions)?}),
        Schedule::Sequence(_, ss) => {
            json!({"sequence":ss.iter().map(|s|schedule(s,sets,eg,assertions)).collect::<Result<Vec<_>>>()?})
        }
    })
}
pub fn controls(
    eg: &mut EGraph,
    commands: &[Command],
) -> Result<(BTreeSet<String>, Vec<Value>, Vec<String>)> {
    let mut sets = BTreeSet::new();
    let mut schedules = vec![];
    let mut assertions = vec![];
    for c in commands {
        match c {
            Command::RunSchedule(s) => schedules.push(schedule(s, &mut sets, eg, &mut assertions)?),
            Command::Check(_, facts) => {
                let name = format!("DeviceBackendCheck_{}", assertions.len());
                let set = format!("device-backend-check-{}", assertions.len());
                if eg.get_function(&name).is_some() {
                    return Err("assertion helper name collision".into());
                }
                eg.parse_and_run_program(None,&format!("(relation {name} ()) (ruleset {set}) (rule ({}) (({name})) :ruleset {set})",facts.iter().map(|f|f.to_string()).collect::<Vec<_>>().join(" ")))?;
                sets.insert(set.clone());
                schedules.push(json!({"run":set,"assert":true}));
                assertions.push(name);
            }
            _ => return Err("only schedules and checks may follow initialization".into()),
        }
    }
    Ok((sets, schedules, assertions))
}
