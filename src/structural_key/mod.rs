//! Explicit source-declared structural-key extension.
use crate::pipeline::Result;
use egglog::ast::Command;
mod key;
pub mod source;
pub fn initialization_end(commands: &[Command]) -> Result<usize> {
    use egglog::ast::GenericSchedule;
    let mut i = commands
        .iter()
        .position(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("structural-key analysis needs a schedule")?;
    while let Some(Command::RunSchedule(s)) = commands.get(i) {
        fn single(s: &egglog::ast::Schedule) -> &egglog::ast::Schedule {
            match s {
                GenericSchedule::Repeat(_, 1, x) => single(x),
                GenericSchedule::Sequence(_, xs) if xs.len() == 1 => single(&xs[0]),
                other => other,
            }
        }
        let step = single(s);
        let GenericSchedule::Run(_, r) = step else {
            break;
        };
        if r.until.is_some() {
            break;
        }
        let rules = commands[..i]
            .iter()
            .filter_map(|c| {
                if let Command::Rule { rule } = c {
                    (rule.ruleset == r.ruleset).then_some(rule)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if rules.is_empty() || rules.iter().any(|r| !r.body.is_empty()) {
            break;
        }
        i += 1;
    }
    if i == commands.len() {
        return Err(
            "structural-key analysis needs an inference schedule after initialization".into(),
        );
    }
    if commands[i..].iter().any(|c| {
        matches!(
            c,
            Command::Sort { .. }
                | Command::Datatype { .. }
                | Command::Datatypes { .. }
                | Command::Constructor { .. }
                | Command::Function { .. }
                | Command::Relation { .. }
        ) || matches!(
            c,
            Command::Rule { .. } | Command::AddRuleset(..) | Command::UnstableCombinedRuleset(..)
        )
    }) {
        return Err(
            "structural-key policy requires declarations and rules before inference".into(),
        );
    }
    Ok(i)
}
