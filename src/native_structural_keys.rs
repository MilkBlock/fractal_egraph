//! Explicit source policy adapter. History retains the rewritten rules and the
//! policy header so offline ripen cannot silently use the original environment.
use super::*;
pub(super) fn enabled(text: &str) -> bool {
    text.starts_with("; egg-layout structural-keys/eggcc-v1\n")
}
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn normalize(name: &str, text: &str) -> Result<String> {
    if enabled(text) {
        return Ok(text.into());
    }
    let mut parser = EGraph::default();
    let mut rules = 0;
    let commands =
        crate::visual_rule::surface_program(parser.parse_program(Some(name.into()), text)?)
            .into_iter()
            .map(|c| {
                let c = crate::visual_rule::normalize(c, rules);
                if matches!(c, Command::Rule { .. }) {
                    rules += 1
                };
                c
            })
            .collect::<Vec<_>>();
    if commands.iter().any(|c| {
        matches!(
            c,
            Command::Include(..) | Command::Push(..) | Command::Pop(..)
        )
    }) {
        return Err("structural-key source policy requires a flat source without push/pop".into());
    }
    let (commands, _) = cell_assisted_egglog::stable_choice::rewrite_ties(&commands)?;
    Ok(format!(
        "{}\n{}\n",
        cell_assisted_egglog::stable_choice::POLICY_HEADER,
        commands
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
pub(super) fn setup(eg: &mut EGraph, commands: &[Command]) -> Result {
    #[cfg(not(target_arch = "wasm32"))]
    {
        cell_assisted_egglog::stable_choice::setup_prepared_full(eg, commands)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (eg, commands);
        Err("structural-key policy currently requires native analysis".into())
    }
}
pub(super) fn schedule(eg: &mut EGraph, commands: &[Command]) -> Result<Vec<Command>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        cell_assisted_egglog::stable_choice::schedule_full(eg, commands)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (eg, commands);
        Err("structural-key policy currently requires native analysis".into())
    }
}
pub(super) fn literal(eg: &EGraph, value: Value) -> Result<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(cell_assisted_egglog::stable_choice::key_literal(eg, value))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (eg, value);
        Err("structural-key policy currently requires native analysis".into())
    }
}
pub(super) fn prefix(text: &str) -> String {
    format!("; egg-layout structural-keys/eggcc-v1\n{text}")
}
// Optional validation artifact, never enabled by ordinary analysis or replay.
pub(super) fn audit_graph(eg: &EGraph) -> Result {
    let Some(path) = std::env::var_os("EGG_LAYOUT_KEY_AUDIT_GRAPH") else {
        return Ok(());
    };
    let graph = eg.serialize(egglog::SerializeConfig::default());
    if !graph.is_complete() {
        return Err("structural-key validation graph was truncated".into());
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    serde_json::to_writer_pretty(&mut out, &graph.egraph)?;
    Ok(())
}

/// A declared empty-LHS, one-shot initializer constructs the input rather than
/// representing an inference phase. Freeze keys after it, as in the snapshot
/// benchmark; do not turn the whole input-building action into a coarse Cell.
pub(super) fn setup_end(commands: &[Command]) -> Result<usize> {
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
        sorts::is_declaration(c)
            || matches!(
                c,
                Command::Rule { .. }
                    | Command::AddRuleset(..)
                    | Command::UnstableCombinedRuleset(..)
            )
    }) {
        return Err("structural-key policy requires declarations and rules before inference".into());
    }
    Ok(i)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initializer_with_singleton_schedule_wrappers_is_setup() {
        let mut eg = EGraph::default();
        let cs=eg.parse_program(None,"(datatype E (N)) (ruleset initialization) (ruleset body) (rule () ((N)) :ruleset initialization) (rule ((N)) ((N)) :ruleset body) (run-schedule (repeat 1 (run initialization))) (run-schedule (saturate body))").unwrap();
        assert_eq!(setup_end(&cs).unwrap(), cs.len() - 1);
    }
}

pub(super) fn opaque(text: &str) -> Result<BTreeSet<String>> {
    let prefix = "; structural-key-opaque/v1 ";
    if let Some(line) = text.lines().find_map(|l| l.strip_prefix(prefix)) {
        Ok(serde_json::from_str(line)?)
    } else {
        Ok(BTreeSet::new())
    }
}
pub(super) fn with_opaque(text: &str, names: &BTreeSet<String>) -> String {
    format!(
        "; egg-layout structural-keys/eggcc-v1\n; structural-key-opaque/v1 {}\n{text}",
        serde_json::to_string(names).unwrap()
    )
}
pub(super) fn setup_opaque(
    eg: &mut EGraph,
    commands: &[Command],
    opaque: &BTreeSet<String>,
) -> Result {
    #[cfg(not(target_arch = "wasm32"))]
    {
        cell_assisted_egglog::stable_choice::setup_prepared_with_opaque(eg, commands, opaque)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (eg, commands, opaque);
        Err("structural-key policy currently requires native analysis".into())
    }
}
