//! Deterministic structural tie-breaking for eggcc's native `to-hoist` merge.
//! Keys are finite constructor DAGs, never e-class IDs or a hash-only ordering.
use crate::Result;
use egglog::{
    EGraph, Value,
    ast::{Command, GenericSchedule, Schedule, Span},
    prelude::add_base_sort,
};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::sync::Arc;
mod full;
mod key;
use key::*;
pub mod source;
pub use full::{
    POLICY_HEADER, coverage, initialization_end, key_literal, rewrite_ties, schedule_full,
    setup_full, setup_prepared_full, setup_prepared_with_opaque,
};

pub const RULESET: &str = "cell-stable-keys";
const PREFIX: &str = "CellStructuralKey-";

fn private(eg: &mut EGraph, source: &str) -> Result<()> {
    let mut cmds = eg.parse_program(None, source)?;
    for c in &mut cmds {
        if let Command::Function {
            let_binding,
            hidden,
            ..
        } = c
        {
            *let_binding = true;
            *hidden = true;
        }
    }
    eg.run_program(cmds)?;
    Ok(())
}
fn key_function(sort: &str) -> String {
    format!("{PREFIX}{sort}")
}
fn declare_key(eg: &mut EGraph, sort: &str) -> Result<()> {
    if let Some(f) = eg.get_function(&key_function(sort)) {
        if !f.is_let_binding()
            || f.schema().input.len() != 1
            || f.schema().input[0].name() != sort
            || f.schema().output.name() != SORT
        {
            return Err(format!("structural-key helper name collision for {sort}").into());
        }
        return Ok(());
    }
    private(
        eg,
        &format!(
            "(function {} ({sort}) {SORT} :merge (cell-key-min old new) :unextractable)",
            key_function(sort)
        ),
    )
}
/// Execute the original setup, replacing only the merge of `to-hoist`.
/// Adds the same auxiliary rules to all backends. No core engine modification.
pub fn setup(eg: &mut EGraph, commands: &[Command]) -> Result<()> {
    setup_impl(eg, commands, false, false, &BTreeSet::new())
}
fn setup_impl(
    eg: &mut EGraph,
    commands: &[Command],
    full: bool,
    rewrite: bool,
    opaque: &BTreeSet<String>,
) -> Result<()> {
    if full {
        full::install_base(eg)?;
    }
    let mut found = false;
    for c in commands {
        let mut c = c.clone();
        if let Command::Function { name, merge, .. } = &mut c {
            if name == "to-hoist" {
                if found {
                    return Err("duplicate to-hoist declaration".into());
                }
                found = true;
                if !full {
                    add_base_sort(eg, KeySort, Span::Panic)?;
                }
                let key = eg.get_sort_by_name(SORT).unwrap().clone();
                let expr = eg.get_sort_by_name("Expr").ok_or("missing Expr")?.clone();
                let int = eg.get_sort_by_name("i64").unwrap().clone();
                if !full {
                    eg.add_pure_primitive(
                        Prim {
                            name: "cell-key-min".into(),
                            types: vec![key.clone(); 3],
                            op: Op::Min,
                        },
                        None,
                    );
                    eg.add_pure_primitive(
                        Prim {
                            name: "cell-key-anchor".into(),
                            types: vec![key.clone(); 2],
                            op: Op::Anchor,
                        },
                        None,
                    );
                }
                eg.add_pure_primitive(
                    Prim {
                        name: "cell-hoist-best".into(),
                        types: vec![
                            expr.clone(),
                            expr.clone(),
                            int.clone(),
                            int,
                            key.clone(),
                            key,
                            expr,
                        ],
                        op: Op::Choose,
                    },
                    None,
                );
                declare_key(eg, "Expr")?;
                *merge=Some(egglog::ast::Parser::default().get_expr_from_string(None,"(cell-hoist-best old new (Expr-size old) (Expr-size new) (CellStructuralKey-Expr old) (CellStructuralKey-Expr new))")?);
            }
        }
        if full && matches!(c, Command::Rule { .. }) {
            for sort in ["Term", "IVTRes"] {
                if eg.get_sort_by_name(sort).is_some() {
                    declare_key(eg, sort)?;
                }
            }
            if rewrite {
                let (cs, _) = full::rewrite_ties(&[c])?;
                eg.run_program(cs)?;
            } else {
                eg.run_program(vec![c])?;
            }
        } else {
            eg.run_program(vec![c])?;
        }
    }
    if !found {
        return Err("missing to-hoist declaration".into());
    }
    install_rules(eg, full, opaque)?;
    // Freeze the current input's structural keys once. Subsequent alternatives
    // do not recursively rename the entire input; union still merges keys by min.
    eg.parse_and_run_program(
        None,
        &format!("(run-schedule (saturate {RULESET})) (run cell-stable-freeze 1)"),
    )?;
    Ok(())
}
fn install_rules(eg: &mut EGraph, full: bool, opaque: &BTreeSet<String>) -> Result<()> {
    let functions = eg
        .get_function_names()
        .into_iter()
        .filter_map(|n| {
            let f = eg.get_function(&n)?;
            f.is_constructor()
                .then(|| (n, (f.schema().input.clone(), f.schema().output.clone())))
        })
        .collect::<BTreeMap<_, _>>();
    let mut reachable = if full {
        functions
            .values()
            .map(|(_, output)| output.name().to_string())
            .collect()
    } else {
        BTreeSet::from(["Expr".to_string()])
    };
    loop {
        let before = reachable.len();
        for (inputs, output) in functions.values() {
            if reachable.contains(output.name()) {
                for s in inputs {
                    if s.is_eq_sort() {
                        reachable.insert(s.name().into());
                    }
                }
            }
        }
        if reachable.len() == before {
            break;
        }
    }
    for sort in &reachable {
        if sort != "Expr" {
            declare_key(eg, sort)?;
        }
    }
    eg.parse_and_run_program(None, &format!("(ruleset {RULESET})"))?;
    eg.parse_and_run_program(None, "(ruleset cell-stable-freeze)")?;
    for sort in &reachable {
        let f = key_function(sort);
        eg.parse_and_run_program(None,&format!("(rule ((= k ({f} x))) ((set ({f} x) (cell-key-anchor k))) :ruleset cell-stable-freeze)"))?;
    }
    let key = eg.get_sort_by_name(SORT).unwrap().clone();
    let mut bases = BTreeSet::new();
    for (name, (inputs, output)) in functions {
        if opaque.contains(&name) || !reachable.contains(output.name()) {
            continue;
        }
        if full
            && inputs.iter().any(|s| {
                !s.is_eq_sort() && !["i64", "f64", "String", "bool", "Unit"].contains(&s.name())
            })
        {
            // An unused declaration is harmless. Never substitute allocation
            // IDs for unknown container structure if an instance appears.
            let args = (0..inputs.len())
                .map(|i| format!("x{i}"))
                .collect::<Vec<_>>()
                .join(" ");
            eg.parse_and_run_program(None,&format!("(rule (({name} {args})) ((panic \"structural-key unsupported instantiated constructor: {name}\")) :ruleset {RULESET})"))?;
            continue;
        }
        let mut conditions = Vec::new();
        let mut args = vec![];
        let mut keys = vec![];
        for (i, s) in inputs.iter().enumerate() {
            let x = format!("x{i}");
            args.push(x.clone());
            if s.is_eq_sort() {
                let k = format!("k{i}");
                conditions.push(format!("(= {k} ({} {x}))", key_function(s.name())));
                keys.push(k);
            } else {
                if !["i64", "f64", "String", "bool", "Unit"].contains(&s.name()) {
                    return Err(format!("stable key does not support {}", s.name()).into());
                }
                let p = format!("cell-key-base-{}", s.name());
                if bases.insert(s.name().to_string()) {
                    eg.add_pure_primitive(
                        Prim {
                            name: p.clone(),
                            types: vec![s.clone(), key.clone()],
                            op: Op::Literal(s.name().into()),
                        },
                        None,
                    );
                }
                keys.push(format!("({p} {x})"));
            }
        }
        let p = format!("cell-key-node-{name}");
        eg.add_pure_primitive(
            Prim {
                name: p.clone(),
                types: vec![key.clone(); inputs.len() + 1],
                op: Op::Node(format!("{}:{name}", output.name()).into()),
            },
            None,
        );
        let source = format!(
            "(rule ((= out ({name} {})) {}) ((set ({} out) ({p} {}))) :ruleset {RULESET})",
            args.join(" "),
            conditions.join(" "),
            key_function(output.name()),
            keys.join(" ")
        );
        eg.parse_and_run_program(None, &source)?;
    }
    Ok(())
}
pub fn schedule(eg: &mut EGraph, commands: &[Command]) -> Result<Vec<Command>> {
    let helper = eg.parse_program(None, &format!("(run-schedule (saturate {RULESET}))"))?;
    let Command::RunSchedule(helper) = &helper[0] else {
        unreachable!()
    };
    fn walk(s: &Schedule, helper: &Schedule) -> Schedule {
        match s {
            GenericSchedule::Run(span, r) if r.ruleset == "boundary-analysis" => {
                GenericSchedule::Sequence(span.clone(), vec![helper.clone(), s.clone()])
            }
            GenericSchedule::Sequence(span, xs) => GenericSchedule::Sequence(
                span.clone(),
                xs.iter().map(|x| walk(x, helper)).collect(),
            ),
            GenericSchedule::Repeat(span, n, x) => {
                GenericSchedule::Repeat(span.clone(), *n, Box::new(walk(x, helper)))
            }
            GenericSchedule::Saturate(span, x) => {
                GenericSchedule::Saturate(span.clone(), Box::new(walk(x, helper)))
            }
            _ => s.clone(),
        }
    }
    Ok(commands
        .iter()
        .map(|c| {
            if let Command::RunSchedule(s) = c {
                Command::RunSchedule(walk(s, helper))
            } else {
                c.clone()
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_hash_collision_does_not_make_different_structures_equal() {
        let mut a = Key::new("Expr:A".into(), vec![]);
        let mut b = Key::new("Expr:B".into(), vec![]);
        Arc::get_mut(&mut a.0).unwrap().hash = 0;
        Arc::get_mut(&mut b.0).unwrap().hash = 0;
        assert_ne!(a, b);
        assert!(a < b);
    }
}
