//! Export original rule ASTs with source positions using egglog's own parser.
use egglog::ast::{Action, Command, Expr, Fact, Span};
use serde_json::{Value, json};
fn expr(e: &Expr) -> Value {
    match e {
        Expr::Var(_, v) => json!({"var":v}),
        Expr::Lit(_, l) => json!({"literal":l.to_string()}),
        Expr::Call(s, op, args) => {
            let span = if let Span::Egglog(s) = s {
                format!("{:?}:{}:{}", s.file.name, s.i, s.j)
            } else {
                String::new()
            };
            json!({"op":op,"args":args.iter().map(expr).collect::<Vec<_>>(),"span":span})
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let path = args.get(1).ok_or("source.egg output.json")?;
    let mut eg = egglog::EGraph::default();
    let commands = eg.parse_program(Some(path.clone()), &std::fs::read_to_string(path)?)?;
    let mut rules = serde_json::Map::new();
    let mut index = 0;
    for c in commands {
        if let Command::Rule { rule } = egg_layout::visual_rule::normalize(c, index) {
            let body: Vec<_> = rule
                .body
                .iter()
                .map(|f| match f {
                    Fact::Eq(_, a, b) => json!({"eq":[expr(a),expr(b)]}),
                    Fact::Fact(e) => json!({"fact":expr(e)}),
                })
                .collect();
            let head: Vec<_> = rule
                .head
                .0
                .iter()
                .map(|a| match a {
                    Action::Union(_, a, b) => json!({"union":[expr(a),expr(b)]}),
                    _ => json!({"unsupported":a.to_string()}),
                })
                .collect();
            rules.insert(rule.name.clone(), json!({"body":body,"head":head}));
            index += 1;
        }
    }
    std::fs::write(&args[2], serde_json::to_string_pretty(&rules)? + "\n")?;
    Ok(())
}
