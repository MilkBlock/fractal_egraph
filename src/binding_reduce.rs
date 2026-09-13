//! Compile a topologically ordered, pure binding interface using egglog's parser.
//! Local names disappear into parameter slots and shared expression definitions.
//! This is endpoint algebra, not execution of tier-0 actions or guard inference.
use egglog::ast::{Expr, Literal, Parser};
use std::collections::{BTreeMap, BTreeSet};

pub enum Definition<'a> {
    Let {
        name: &'a str,
        expression: &'a str,
    },
    /// Site identifies a recursive call within this DAG, not a global event.
    Recur {
        name: &'a str,
        site: i64,
        arguments: Vec<&'a str>,
    },
}

fn values(xs: impl IntoIterator<Item = String>) -> String {
    let xs: Vec<_> = xs.into_iter().collect();
    xs.into_iter()
        .rev()
        .fold("(BXNil)".into(), |tail, x| format!("(BXCons {x} {tail})"))
}

fn expression(src: &str, env: &BTreeMap<String, usize>) -> Result<String, String> {
    fn lower(e: &Expr, env: &BTreeMap<String, usize>) -> Result<String, String> {
        Ok(match e {
            Expr::Var(_, n) => format!(
                "(BParam {})",
                env.get(n)
                    .ok_or_else(|| format!("undefined or forward binding: {n}"))?
            ),
            Expr::Lit(_, Literal::Int(n)) => format!("(BInt {n})"),
            Expr::Lit(..) => return Err("only integer literals are supported".into()),
            Expr::Call(_, op, args) => {
                let xs = args
                    .iter()
                    .map(|a| lower(a, env))
                    .collect::<Result<Vec<_>, _>>()?;
                // Explicit endpoint algebra vocabulary; unknown calls remain opaque.
                // Never infer mathematical semantics from a tier-0 constructor name.
                if ["EAdd", "ESub", "EMul", "EDiv", "EPow"].contains(&op.as_str()) {
                    if xs.len() != 2 {
                        return Err(format!("{op} requires two arguments"));
                    }
                    format!("(B{} {} {})", &op[1..], xs[0], xs[1])
                } else {
                    format!(
                        "(BApply {} {})",
                        serde_json::to_string(op).unwrap(),
                        values(xs)
                    )
                }
            }
        })
    }
    let e = Parser::default()
        .get_expr_from_string(None, src)
        .map_err(|e| e.to_string())?;
    lower(&e, env)
}

/// Returns a ground egglog expression. `effects` are opaque terms, not actions.
/// Each definition may reference all preceding definitions and input parameters.
/// Multiple outputs need not share an arithmetic root; their order is preserved.
pub fn compile(
    inputs: &[&str],
    definitions: &[Definition<'_>],
    outputs: &[&str],
    effects: &[&str],
) -> Result<String, String> {
    let mut env = BTreeMap::new();
    for (i, name) in inputs.iter().enumerate() {
        if env.insert((*name).to_owned(), i).is_some() {
            return Err(format!("duplicate input: {name}"));
        }
    }
    let mut sites = BTreeSet::new();
    let mut bindings = vec![];
    for def in definitions {
        let (name, value) = match def {
            Definition::Let {
                name,
                expression: src,
            } => (*name, expression(src, &env)?),
            Definition::Recur {
                name,
                site,
                arguments,
            } => {
                if *site < 0 || !sites.insert(*site) {
                    return Err("recursive sites must be distinct nonnegative IDs; reuse the binding for sharing".into());
                }
                let args = arguments
                    .iter()
                    .map(|a| expression(a, &env))
                    .collect::<Result<Vec<_>, _>>()?;
                (*name, format!("(BRecur {site} {})", values(args)))
            }
        };
        if env.contains_key(name) {
            return Err(format!("binding redefinition: {name}"));
        }
        bindings.push(value);
        for index in env.values_mut() {
            *index += 1;
        }
        env.insert(name.to_owned(), 0);
    }
    let roots = |xs: &[&str]| {
        xs.iter()
            .map(|s| expression(s, &env))
            .collect::<Result<Vec<_>, _>>()
            .map(values)
    };
    let result = format!("(BDag {} {})", roots(outputs)?, roots(effects)?);
    Ok(bindings
        .into_iter()
        .rev()
        .fold(result, |tail, value| format!("(BLet {value} {tail})")))
}
