//! Native AST normalization shared by profiling and visualization export.
use egglog::ast::{Action, Actions, Command, Expr, Fact, Rule};
pub fn normalize(command: Command, index: usize) -> Command {
    match command {
        Command::Rewrite(ruleset, rewrite, subsume) => {
            assert!(!subsume, "subsuming rewrites need explicit action lowering");
            let mut root = "__viz_root".to_string();
            let text = format!("{} {} {:?}", rewrite.lhs, rewrite.rhs, rewrite.conditions);
            while text.contains(&root) {
                root.push('_');
            }
            let span = rewrite.span.clone();
            let root_expr = Expr::Var(span.clone(), root);
            let mut body = vec![Fact::Eq(span.clone(), root_expr.clone(), rewrite.lhs)];
            body.extend(rewrite.conditions);
            Command::Rule {
                rule: Rule {
                    span: span.clone(),
                    body,
                    head: Actions::new(vec![Action::Union(span, root_expr, rewrite.rhs)]),
                    ruleset,
                    name: if rewrite.name.is_empty() {
                        format!("R{index}")
                    } else {
                        rewrite.name
                    },
                    naive: false,
                },
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rewrite_lowering_preserves_results_and_avoids_capture() {
        let source = r#"(datatype E (Var i64) (A E) (B E)) (rewrite (A __viz_root) (B __viz_root)) (A (Var 1)) (run 2) (check (= (A (Var 1)) (B (Var 1))))"#;
        let mut ordinary = egglog::EGraph::default();
        ordinary.parse_and_run_program(None, source).unwrap();
        let mut eg = egglog::EGraph::default();
        let mut i = 0;
        let commands = eg
            .parse_program(None, source)
            .unwrap()
            .into_iter()
            .map(|c| {
                let c = normalize(c, i);
                if matches!(c, Command::Rule { .. }) {
                    i += 1;
                }
                c
            })
            .collect();
        eg.run_program(commands).unwrap();
        for name in ["A", "B", "Var"] {
            assert_eq!(eg.get_size(name), ordinary.get_size(name));
        }
    }
}

/// Source occurrence IDs and stable paths in the normalized, unrenamed rule AST.
pub fn positions(rule: &Rule) -> Vec<(String, String, String)> {
    fn walk(e: &Expr, path: String, out: &mut Vec<(String, String, String)>) {
        if let Expr::Call(span, op, args) = e {
            if let egglog::ast::Span::Egglog(s) = span {
                out.push((
                    format!("{:?}:{}:{}", s.file.name, s.i, s.j),
                    path.clone(),
                    op.clone(),
                ));
            }
            for (i, arg) in args.iter().enumerate() {
                walk(arg, format!("{path}/args/{i}"), out);
            }
        }
    }
    let mut out = vec![];
    for (i, f) in rule.body.iter().enumerate() {
        match f {
            Fact::Eq(_, a, b) => {
                walk(a, format!("body/{i}/expr/0"), &mut out);
                walk(b, format!("body/{i}/expr/1"), &mut out);
            }
            Fact::Fact(e) => walk(e, format!("body/{i}/expr/0"), &mut out),
        }
    }
    for (i, a) in rule.head.0.iter().enumerate() {
        match a {
            Action::Expr(_, e) | Action::Let(_, _, e) => {
                walk(e, format!("head/{i}/expr/0"), &mut out)
            }
            Action::Union(_, a, b) => {
                walk(a, format!("head/{i}/expr/0"), &mut out);
                walk(b, format!("head/{i}/expr/1"), &mut out);
            }
            Action::Set(span, op, args, value) => {
                walk(
                    &Expr::Call(span.clone(), op.clone(), args.clone()),
                    format!("head/{i}/set/0"),
                    &mut out,
                );
                walk(value, format!("head/{i}/expr/1"), &mut out);
            }
            _ => {}
        }
    }
    out
}
pub fn expression_at<'a>(rule: &'a Rule, path: &str) -> Option<&'a Expr> {
    let p: Vec<_> = path.split('/').collect();
    if p.len() < 4 || p[2] != "expr" {
        return None;
    }
    let i = p[1].parse::<usize>().ok()?;
    let j = p[3].parse::<usize>().ok()?;
    let mut e = match p[0] {
        "body" => match rule.body.get(i)? {
            Fact::Eq(_, a, b) => match j {
                0 => a,
                1 => b,
                _ => return None,
            },
            Fact::Fact(e) if j == 0 => e,
            _ => return None,
        },
        "head" => match rule.head.0.get(i)? {
            Action::Set(_, _, _, value) if j == 1 => value,
            Action::Expr(_, e) | Action::Let(_, _, e) if j == 0 => e,
            Action::Union(_, a, b) => match j {
                0 => a,
                1 => b,
                _ => return None,
            },
            _ => return None,
        },
        _ => return None,
    };
    if p[4..].len() % 2 != 0 {
        return None;
    }
    for pair in p[4..].chunks_exact(2) {
        if pair[0] != "args" {
            return None;
        }
        let Expr::Call(_, _, args) = e else {
            return None;
        };
        e = args.get(pair[1].parse::<usize>().ok()?)?;
    }
    Some(e)
}

/// `set` targets are table applications but are not Expr nodes in egglog's AST.
pub fn owned_expression_at(rule: &Rule, path: &str) -> Option<Expr> {
    if let Some(e) = expression_at(rule, path) {
        return Some(e.clone());
    }
    let p: Vec<_> = path.split('/').collect();
    if p.len() < 4 || p[0] != "head" || p[2] != "set" || p[3] != "0" {
        return None;
    }
    let Action::Set(span, op, args, _) = rule.head.0.get(p[1].parse::<usize>().ok()?)? else {
        return None;
    };
    let root = Expr::Call(span.clone(), op.clone(), args.clone());
    let mut e = &root;
    if p[4..].len() % 2 != 0 {
        return None;
    }
    for pair in p[4..].chunks_exact(2) {
        if pair[0] != "args" {
            return None;
        }
        let Expr::Call(_, _, args) = e else {
            return None;
        };
        e = args.get(pair[1].parse::<usize>().ok()?)?;
    }
    Some(e.clone())
}
