//! Declaration metadata only; execution still uses egglog's original AST/order.
use egglog::ast::{Command, Subdatatypes, Variant};
use std::collections::BTreeSet;
pub(super) fn equality_sorts(commands: &[Command]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for c in commands {
        match c {
            Command::Datatype { name, .. }
            | Command::Sort {
                name,
                presort_and_args: None,
                ..
            } => {
                names.insert(name.clone());
            }
            Command::Datatypes { datatypes, .. } => {
                for (_, name, kind) in datatypes {
                    if matches!(kind, Subdatatypes::Variants(_)) {
                        names.insert(name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    names
}
pub(super) fn constructors(commands: &[Command]) -> Vec<(String, Variant)> {
    let mut out = vec![];
    for c in commands {
        match c {
            Command::Datatype { name, variants, .. } => {
                out.extend(variants.iter().cloned().map(|v| (name.clone(), v)))
            }
            Command::Datatypes { datatypes, .. } => {
                for (_, name, kind) in datatypes {
                    if let Subdatatypes::Variants(v) = kind {
                        out.extend(v.iter().cloned().map(|v| (name.clone(), v)));
                    }
                }
            }
            Command::Constructor {
                span,
                name,
                schema,
                cost,
                unextractable,
                ..
            } => out.push((
                schema.output.clone(),
                Variant {
                    span: span.clone(),
                    name: name.clone(),
                    types: schema.input.clone(),
                    cost: cost.clone(),
                    unextractable: *unextractable,
                },
            )),
            _ => {}
        }
    }
    out
}
pub(super) fn is_declaration(c: &Command) -> bool {
    matches!(
        c,
        Command::Datatype { .. }
            | Command::Datatypes { .. }
            | Command::Sort { .. }
            | Command::Constructor { .. }
            | Command::Function { .. }
            | Command::Relation { .. }
    )
}
