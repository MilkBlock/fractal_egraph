//! Source policy export: reuse the analyzer branch's three selection rewrites.
use cell_assisted_egglog::{Result, stable_choice};
use egglog::{
    EGraph, ResolvedCall,
    ast::{Action, Command, Expr, Fact, GenericCommand, GenericExpr, Literal},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
#[path = "support/egg_source.rs"]
mod egg_source;
use egg_source::{printed, quote};
fn literals(e: &Expr, out: &mut Vec<serde_json::Value>) {
    if let Expr::Lit(_, v) = e {
        let item = match v {
            Literal::String(s) => {
                Some(json!({"sort":"String","expression":quote(s),"label":format!("String:{s:?}")}))
            }
            Literal::Float(f) => {
                let x = f.0;
                let bits = if x.is_nan() {
                    f64::NAN.to_bits()
                } else if x == 0.0 {
                    0
                } else {
                    x.to_bits()
                };
                Some(
                    json!({"sort":"f64","expression":e.to_string(),"label":format!("f64:{bits:016x}")}),
                )
            }
            _ => None,
        };
        if let Some(x) = item {
            if !out
                .iter()
                .any(|v| v["sort"] == x["sort"] && v["label"] == x["label"])
            {
                out.push(x);
            }
        }
    }
    if let Expr::Call(_, _, xs) = e {
        for x in xs {
            literals(x, out)
        }
    }
}
fn source_literals(c: &Command, out: &mut Vec<serde_json::Value>) {
    fn action(a: &Action, out: &mut Vec<serde_json::Value>) {
        match a {
            Action::Let(_, _, e) | Action::Expr(_, e) => literals(e, out),
            Action::Set(_, _, xs, e) => {
                for x in xs {
                    literals(x, out)
                }
                literals(e, out)
            }
            Action::Union(_, a, b) => {
                literals(a, out);
                literals(b, out)
            }
            Action::Change(_, _, _, xs) => {
                for x in xs {
                    literals(x, out)
                }
            }
            _ => {}
        }
    }
    match c {
        Command::Rule { rule } => {
            for f in &rule.body {
                match f {
                    Fact::Eq(_, a, b) => {
                        literals(a, out);
                        literals(b, out)
                    }
                    Fact::Fact(e) => literals(e, out),
                }
            }
            for a in &rule.head.0 {
                action(a, out)
            }
        }
        Command::Action(a) => action(a, out),
        Command::Function { merge: Some(e), .. } => literals(e, out),
        _ => {}
    }
}
fn validate_resolved(c: &egglog::ast::ResolvedCommand) -> Result<()> {
    fn expr(e: &egglog::ResolvedExpr) -> Result<()> {
        if let GenericExpr::Call(_, head, args) = e {
            if let ResolvedCall::Primitive(p) = head {
                if ["String", "f64"].contains(&p.output().name()) {
                    return Err(format!("portable structural keys require source-literal String/f64 values; primitive {} computes {}",p.name(),p.output().name()).into());
                }
            }
            for a in args {
                expr(a)?
            }
        }
        Ok(())
    }
    use egglog::ast::GenericAction as A;
    fn action(a: &egglog::ast::GenericAction<ResolvedCall, egglog::ResolvedVar>) -> Result<()> {
        match a {
            A::Let(_, _, e) | A::Expr(_, e) => expr(e)?,
            A::Set(_, _, xs, e) => {
                for x in xs {
                    expr(x)?
                }
                expr(e)?
            }
            A::Union(_, a, b) => {
                expr(a)?;
                expr(b)?
            }
            A::Change(_, _, _, xs) => {
                for x in xs {
                    expr(x)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    match c {
        GenericCommand::Rule { rule } => {
            for f in &rule.body {
                match f {
                    egglog::ast::GenericFact::Eq(_, a, b) => {
                        expr(a)?;
                        expr(b)?
                    }
                    egglog::ast::GenericFact::Fact(e) => expr(e)?,
                }
            }
            for a in &rule.head.0 {
                action(a)?
            }
        }
        GenericCommand::Action(a) => action(a)?,
        GenericCommand::Function { merge: Some(e), .. } => expr(e)?,
        _ => {}
    }
    Ok(())
}
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 2 {
        return Err("SOURCE OUTPUT_DIR".into());
    }
    let source = std::fs::read_to_string(&a[0])?;
    let out = Path::new(&a[1]);
    std::fs::create_dir(out)?;
    let mut validator = EGraph::default();
    for c in validator.resolve_program(Some(a[0].clone()), &source)? {
        validate_resolved(&c)?;
    }
    let mut eg = EGraph::default();
    let mut n = 0;
    let commands =
        egg_layout::visual_rule::surface_program(eg.parse_program(Some(a[0].clone()), &source)?)
            .into_iter()
            .map(|c| {
                let c = egg_layout::visual_rule::normalize(c, n);
                if matches!(c, Command::Rule { .. }) {
                    n += 1;
                }
                c
            })
            .collect::<Vec<_>>();
    if commands.iter().any(|c| {
        matches!(
            c,
            Command::Include(..) | Command::Input { .. } | Command::Push(..) | Command::Pop(..)
        )
    }) {
        return Err("portable policy needs flat self-contained source without stack scopes".into());
    }
    let first = stable_choice::initialization_end(&commands)?;
    eg.run_program(commands[..first].to_vec())?;
    let mut constructors = BTreeMap::new();
    let mut sorts = BTreeSet::new();
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        if f.is_constructor() && !f.is_let_binding() && !name.starts_with('@') {
            sorts.insert(f.schema().output.name().to_string());
            constructors.insert(name,json!({"input":f.schema().input.iter().map(|s|s.name()).collect::<Vec<_>>(),"output":f.schema().output.name()}));
        }
    }
    let (policy, rewrites) = stable_choice::rewrite_ties(&commands)?;
    let mut values = Vec::new();
    for c in &commands {
        source_literals(c, &mut values)
    }
    std::fs::write(
        out.join("policy.egg"),
        policy.iter().map(printed).collect::<Vec<_>>().join("\n"),
    )?;
    std::fs::write(
        out.join("metadata.json"),
        serde_json::to_vec_pretty(
            &json!({"commands":policy.iter().map(printed).collect::<Vec<_>>(),"constructors":constructors,"sorts":sorts,"literals":values,"rewrites":rewrites,"origin":"eggcc-structural-keys a89c4cc; reused stable_choice::rewrite_ties"}),
        )?,
    )?;
    Ok(())
}
