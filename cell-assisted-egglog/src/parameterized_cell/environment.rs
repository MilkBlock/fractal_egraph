//! Portable, referenced global definitions, never a snapshot of training rows.
use super::*;
use egglog::ast::{Action, Expr, Fact};

fn global_names(commands: &[Command]) -> BTreeSet<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            Command::Action(Action::Let(_, n, _)) => Some(n.clone()),
            Command::Function {
                name,
                schema,
                let_binding: true,
                ..
            } if schema.input.is_empty() => Some(name.clone()),
            _ => None,
        })
        .collect()
}
fn initializer<'a>(c: &'a Command, globals: &BTreeSet<String>) -> Option<(&'a str, &'a Expr)> {
    match c {
        Command::Action(Action::Let(_, n, e)) => Some((n, e)),
        Command::Action(Action::Set(_, n, args, e)) if args.is_empty() && globals.contains(n) => {
            Some((n, e))
        }
        _ => None,
    }
}
fn references(
    e: &Expr,
    globals: &BTreeSet<String>,
    locals: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    match e {
        Expr::Var(_, n) if globals.contains(n) && !locals.contains(n) => {
            out.insert(n.clone());
        }
        Expr::Call(_, n, args) => {
            if args.is_empty() && globals.contains(n) {
                out.insert(n.clone());
            }
            for a in args {
                references(a, globals, locals, out);
            }
        }
        _ => (),
    }
}
fn required(commands: &[Command], globals: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for c in commands {
        let mut locals = BTreeSet::new();
        if let Command::Function { merge: Some(e), .. } = c {
            references(
                e,
                globals,
                &BTreeSet::from(["old".into(), "new".into()]),
                &mut out,
            );
        }
        let Command::Rule { rule } = c else { continue };
        for f in &rule.body {
            match f {
                Fact::Eq(_, a, b) => {
                    references(a, globals, &locals, &mut out);
                    references(b, globals, &locals, &mut out);
                }
                Fact::Fact(e) => references(e, globals, &locals, &mut out),
            }
        }
        for a in &rule.head.0 {
            let _ = a.clone().visit_exprs(&mut |e| {
                // visit_exprs already traverses children. Inspect each node
                // once rather than walking every nested subtree again.
                match &e {
                    Expr::Var(_, n) if globals.contains(n) && !locals.contains(n) => {
                        out.insert(n.clone());
                    }
                    Expr::Call(_, n, args) if args.is_empty() && globals.contains(n) => {
                        out.insert(n.clone());
                    }
                    _ => (),
                }
                e
            });
            match a {
                Action::Let(_, n, _) => {
                    locals.insert(n.clone());
                }
                Action::Set(_, n, args, _) | Action::Change(_, _, n, args)
                    if args.is_empty() && globals.contains(n) =>
                {
                    out.insert(n.clone());
                }
                _ => (),
            }
        }
    }
    out
}

/// Select the dependency closure of globals referenced by the retained rules.
/// Initializers after a Run cannot be moved into startup; later Set updates are
/// input events, not replacement definitions of the initial environment.
pub fn referenced_initializers(source: &[Command], rules: &[Command]) -> Result<Vec<Command>> {
    let globals = global_names(source);
    let first_run = source
        .iter()
        .position(|c| matches!(c, Command::RunSchedule(_)))
        .unwrap_or(source.len());
    let mut definitions = BTreeMap::<String, (usize, &Command, &Expr)>::new();
    let mut duplicates = BTreeSet::new();
    for (i, c) in source.iter().enumerate().take(first_run) {
        if let Some((n, e)) = initializer(c, &globals) {
            if definitions.insert(n.into(), (i, c, e)).is_some() {
                duplicates.insert(n.to_string());
            }
        }
    }
    let mut needed = required(rules, &globals);
    let mut pending = needed.iter().cloned().collect::<Vec<_>>();
    while let Some(n) = pending.pop() {
        if duplicates.contains(&n) {
            return Err(format!("global {n} has multiple startup definitions").into());
        }
        let (_, _, e) = definitions.get(&n).ok_or_else(|| {
            format!("referenced global {n} has no initializer before the first Run")
        })?;
        let mut deps = BTreeSet::new();
        references(e, &globals, &BTreeSet::new(), &mut deps);
        for d in deps {
            if needed.insert(d.clone()) {
                pending.push(d);
            }
        }
    }
    let mut ordered = needed.iter().map(|n| definitions[n]).collect::<Vec<_>>();
    ordered.sort_by_key(|x| x.0);
    Ok(ordered.into_iter().map(|(_, c, _)| c.clone()).collect())
}

/// A file may initialize only referenced globals using constructor/literal
/// expressions and earlier globals. Arbitrary actions, table reads, schedules
/// and unused "let training = ..." payloads remain rejected.
pub(super) fn validate(commands: &[Command]) -> Result<Vec<String>> {
    if commands.iter().all(super::artifact::declarations_only) {
        return Ok(vec![]);
    }
    let globals = global_names(commands);
    let expected = referenced_initializers(commands, commands)?;
    let actions = commands
        .iter()
        .filter(|c| matches!(c, Command::Action(_)))
        .collect::<Vec<_>>();
    if actions.len() != expected.len()
        || actions
            .iter()
            .zip(&expected)
            .any(|(a, b)| a.to_string() != b.to_string())
    {
        return Err(
            "transfer environment contains non-global or unreferenced training input".into(),
        );
    }
    let mut types = EGraph::default();
    types.run_program(
        commands
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    Command::Datatype { .. }
                        | Command::Datatypes { .. }
                        | Command::Sort { .. }
                        | Command::Constructor { .. }
                )
            })
            .cloned()
            .collect(),
    )?;
    let ctors = types
        .get_function_names()
        .into_iter()
        .filter(|n| types.get_function(n).is_some_and(|f| f.is_constructor()))
        .collect::<BTreeSet<_>>();
    fn closed(e: &Expr, ctors: &BTreeSet<String>, ready: &BTreeSet<String>) -> bool {
        match e {
            Expr::Lit(..) => true,
            Expr::Var(_, n) => ready.contains(n),
            Expr::Call(_, n, args) => {
                (ctors.contains(n) && args.iter().all(|a| closed(a, ctors, ready)))
                    || (args.is_empty() && ready.contains(n))
            }
        }
    }
    let mut ready = BTreeSet::new();
    for c in commands {
        if super::artifact::declarations_only(c) {
            continue;
        }
        let (n, e) =
            initializer(c, &globals).ok_or("unsupported executable transfer environment")?;
        if !closed(e, &ctors, &ready) {
            return Err(format!(
                "global {n} needs a constructor/literal definition independent of training tables"
            )
            .into());
        }
        ready.insert(n.to_string());
    }
    Ok(ready.into_iter().collect())
}
