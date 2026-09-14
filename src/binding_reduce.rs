//! Canonical pure DAG compiler. Names resolve in dependency order; serialization
//! follows ordered output/contract roots, independent of the input schedule.
use egglog::ast::{Expr, Literal, Parser};
use std::collections::BTreeMap;

pub enum Definition<'a> {
    Let {
        name: &'a str,
        expression: &'a str,
    },
    Recur {
        name: &'a str,
        callee: &'a str,
        output: i64,
        arguments: Vec<&'a str>,
    },
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    Param(usize),
    Int(i64),
    Text(String),
    Call(String, Vec<usize>),
    Recur(String, i64, Vec<usize>),
}
impl Node {
    fn children(&self) -> &[usize] {
        match self {
            Self::Call(_, xs) | Self::Recur(_, _, xs) => xs,
            _ => &[],
        }
    }
}
#[derive(Default)]
struct Graph {
    nodes: Vec<Node>,
    intern: BTreeMap<Node, usize>,
    names: BTreeMap<String, usize>,
}
impl Graph {
    fn node(&mut self, n: Node) -> usize {
        if let Some(i) = self.intern.get(&n) {
            return *i;
        }
        let i = self.nodes.len();
        self.intern.insert(n.clone(), i);
        self.nodes.push(n);
        i
    }
    fn bind(&mut self, name: &str, i: usize) -> Result<(), String> {
        if self.names.insert(name.into(), i).is_some() {
            return Err(format!("binding redefinition: {name}"));
        }
        Ok(())
    }
    fn expr(&mut self, e: &Expr) -> Result<usize, String> {
        let n = match e {
            Expr::Var(_, name) => {
                return self
                    .names
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("undefined or forward binding: {name}"));
            }
            Expr::Lit(_, Literal::Int(n)) => Node::Int(*n),
            Expr::Lit(_, Literal::String(s)) => Node::Text(s.clone()),
            Expr::Lit(..) => return Err("unsupported literal".into()),
            Expr::Call(_, op, args) => {
                if [
                    "EAdd", "ESub", "EMul", "EDiv", "EPow", "EIAdd", "EISub", "EIMul", "EMax",
                ]
                .contains(&op.as_str())
                    && args.len() != 2
                {
                    return Err(format!("{op} requires two arguments"));
                }
                if op == "EArrayGet" && args.len() != 2 {
                    return Err("EArrayGet requires array and index arguments".into());
                }
                if op == "EExp" && args.len() != 1 {
                    return Err("EExp requires one argument".into());
                }
                Node::Call(
                    op.clone(),
                    args.iter()
                        .map(|a| self.expr(a))
                        .collect::<Result<_, _>>()?,
                )
            }
        };
        Ok(self.node(n))
    }
    fn parse(&mut self, s: &str) -> Result<usize, String> {
        let e = Parser::default()
            .get_expr_from_string(None, s)
            .map_err(|e| e.to_string())?;
        self.expr(&e)
    }
}
fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}
fn list(xs: impl IntoIterator<Item = String>) -> String {
    let mut s = String::new();
    let mut n = 0;
    for x in xs {
        s.push_str("(BXCons ");
        s.push_str(&x);
        s.push(' ');
        n += 1;
    }
    s.push_str("(BXNil)");
    s.extend(std::iter::repeat_n(')', n));
    s
}
pub fn compile(
    inputs: &[&str],
    definitions: &[Definition<'_>],
    outputs: &[&str],
    effects: &[&str],
) -> Result<String, String> {
    compile_contract(inputs, definitions, outputs, &[], effects)
}
/// Input order and output/requirement/effect port order are semantic. Independent
/// definition order and alpha-renaming are not. Only pure expressions are interned.
pub fn compile_contract(
    inputs: &[&str],
    definitions: &[Definition<'_>],
    outputs: &[&str],
    requirements: &[&str],
    effects: &[&str],
) -> Result<String, String> {
    let mut g = Graph::default();
    for (i, name) in inputs.iter().enumerate() {
        let n = g.node(Node::Param(i));
        g.bind(name, n)?;
    }
    for def in definitions {
        let (name, n) = match def {
            Definition::Let { name, expression } => (*name, g.parse(expression)?),
            Definition::Recur {
                name,
                callee,
                output,
                arguments,
            } => {
                if callee.is_empty() || *output < 0 {
                    return Err("recursive callee and nonnegative output port required".into());
                }
                let xs = arguments
                    .iter()
                    .map(|a| g.parse(a))
                    .collect::<Result<_, _>>()?;
                (*name, g.node(Node::Recur((*callee).into(), *output, xs)))
            }
        };
        g.bind(name, n)?;
    }
    let mut roots = vec![];
    for group in [outputs, requirements, effects] {
        roots.push(
            group
                .iter()
                .map(|s| g.parse(s))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    // Ordered DFS postorder is canonical for the reachable, ordered DAG. Explicit
    // stack avoids recursive traversal; no global slot shifting or tree expansion.
    let mut seen = vec![false; g.nodes.len()];
    let mut order = vec![];
    for root in roots.iter().flatten() {
        let mut stack = vec![(*root, false)];
        while let Some((i, done)) = stack.pop() {
            if done {
                order.push(i);
                continue;
            }
            if seen[i] {
                continue;
            }
            seen[i] = true;
            if matches!(g.nodes[i], Node::Param(_)) {
                continue;
            }
            stack.push((i, true));
            for c in g.nodes[i].children().iter().rev() {
                stack.push((*c, false));
            }
        }
    }
    let mut positions = vec![0; g.nodes.len()];
    for (p, i) in order.iter().enumerate() {
        positions[*i] = p;
    }
    let reference = |i: usize, depth: usize| match g.nodes[i] {
        Node::Param(p) => format!("(BParam {})", depth + p),
        _ => format!("(BParam {})", depth - 1 - positions[i]),
    };
    let mut result = String::new();
    for (depth, i) in order.iter().enumerate() {
        let expr = match &g.nodes[*i] {
            Node::Int(n) => format!("(BInt {n})"),
            Node::Text(s) => format!("(BText {})", quoted(s)),
            Node::Recur(c, p, xs) => format!(
                "(BRecur {} {p} {})",
                quoted(c),
                list(xs.iter().map(|i| reference(*i, depth)))
            ),
            Node::Call(op, xs) => {
                if [
                    "EAdd", "ESub", "EMul", "EDiv", "EPow", "EIAdd", "EISub", "EIMul", "EMax",
                ]
                .contains(&op.as_str())
                {
                    format!(
                        "(B{} {} {})",
                        &op[1..],
                        reference(xs[0], depth),
                        reference(xs[1], depth)
                    )
                } else if op == "EArrayGet" {
                    format!(
                        "(BArrayGetValue {} {})",
                        reference(xs[0], depth),
                        reference(xs[1], depth)
                    )
                } else if op == "EExp" {
                    format!("(BExp {})", reference(xs[0], depth))
                } else if op == "@row" {
                    let Some((&first, rest)) = xs.split_first() else {
                        return Err("row requires a table identity".into());
                    };
                    let Node::Text(table) = &g.nodes[first] else {
                        return Err("row table must be a string literal".into());
                    };
                    format!(
                        "(BRow {} {})",
                        quoted(table),
                        list(rest.iter().map(|i| reference(*i, depth)))
                    )
                } else if op == "@column" {
                    if xs.len() != 2 {
                        return Err("column requires index and row".into());
                    };
                    let Node::Int(col) = g.nodes[xs[0]] else {
                        return Err("column must be an integer literal".into());
                    };
                    if col < 0 {
                        return Err("negative column".into());
                    };
                    format!("(BColumn {col} {})", reference(xs[1], depth))
                } else {
                    format!(
                        "(BApply {} {})",
                        quoted(op),
                        list(xs.iter().map(|i| reference(*i, depth)))
                    )
                }
            }
            Node::Param(_) => unreachable!(),
        };
        result.push_str("(BLet ");
        result.push_str(&expr);
        result.push(' ');
    }
    result.push_str(&format!(
        "(BDag {} {} {})",
        list(roots[0].iter().map(|i| reference(*i, order.len()))),
        list(roots[1].iter().map(|i| reference(*i, order.len()))),
        list(roots[2].iter().map(|i| reference(*i, order.len())))
    ));
    result.extend(std::iter::repeat_n(')', order.len()));
    Ok(result)
}
