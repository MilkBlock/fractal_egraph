//! Streaming debugger for the actual patched egglog runtime, not a match simulator.
use super::*;

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}
fn math(e: &Expr) -> String {
    match e {
        Expr::Var(_, n) => format!("op({})", quote(n)),
        Expr::Lit(_, Literal::Int(n)) => n.to_string(),
        Expr::Lit(_, l) => format!("op({})", quote(&l.to_string())),
        Expr::Call(_, op, args) => {
            let a: Vec<_> = args.iter().map(math).collect();
            match (op.as_str(), a.as_slice()) {
                ("Add" | "+", [a, b]) => format!("({a} + {b})"),
                ("Mul" | "*", [a, b]) => format!("({a} dot {b})"),
                ("Sub" | "-", [a, b]) => format!("({a} - {b})"),
                ("Div" | "/", [a, b]) => format!("frac({a}, {b})"),
                ("Pow", [a, b]) => format!("({a})^({b})"),
                _ => format!("op({})({})", quote(op), a.join(", ")),
            }
        }
    }
}
pub(super) fn render(rule: &Rule) -> Json {
    fn node(e: &Expr, lines: &mut Vec<String>, next: &mut usize) -> usize {
        let id = *next;
        *next += 1;
        let label = match e {
            Expr::Call(_, op, _) => op.clone(),
            _ => e.to_string(),
        };
        lines.push(format!("n{id} [label={}];", quote(&label)));
        if let Expr::Call(_, _, args) = e {
            for (slot, a) in args.iter().enumerate() {
                let child = node(a, lines, next);
                lines.push(format!("n{id} -> n{child} [label=\"{slot}\"] ;"));
            }
        }
        id
    }
    let mut lines = vec!["digraph pattern { rankdir=LR;".into()];
    let mut next = 0;
    lines.push("subgraph cluster_match { label=\"match / pattern\";".into());
    let mut body = vec![];
    for f in &rule.body {
        match f {
            Fact::Eq(_, a, b) => {
                body.push(format!("{} = {}", math(a), math(b)));
                let x = node(a, &mut lines, &mut next);
                let y = node(b, &mut lines, &mut next);
                lines.push(format!("n{x} -> n{y} [label=\"=\",dir=none,style=dashed];"));
            }
            Fact::Fact(e) => {
                body.push(math(e));
                node(e, &mut lines, &mut next);
            }
        }
    }
    lines.push("} subgraph cluster_effects { label=\"effects\";".into());
    let mut head = vec![];
    for a in &rule.head.0 {
        match a {
            Action::Union(_, a, b) => {
                head.push(format!("{} equiv {}", math(a), math(b)));
                let x = node(a, &mut lines, &mut next);
                let y = node(b, &mut lines, &mut next);
                lines.push(format!("n{x} -> n{y} [label=\"union\",color=blue];"));
            }
            Action::Let(_, n, e) => {
                head.push(format!("op({}) := {}", quote(n), math(e)));
                node(e, &mut lines, &mut next);
            }
            Action::Expr(_, e) => {
                head.push(math(e));
                node(e, &mut lines, &mut next);
            }
            _ => {
                head.push(format!("op({})", quote(&a.to_string())));
                lines.push(format!(
                    "n{next} [shape=box,label={}];",
                    quote(&a.to_string())
                ));
                next += 1;
            }
        }
    }
    lines.push("} }".into());
    json!({"typst":format!("#set page(width: auto, height: auto, margin: 12pt)\n$ {} ==> {} $",body.join(" and "),head.join(" quad ")),"dot":lines.join("\n"),"source":rule.to_string()})
}
pub(super) fn location(rule: &Rule) -> (usize, usize) {
    if let Span::Egglog(s) = &rule.span {
        (
            s.file.get_location(s.i).0,
            s.file.get_location(s.j.saturating_sub(1)).0,
        )
    } else {
        (1, 1)
    }
}
// A staged composition preserves every dependency and intermediate effect even when
// native_lower cannot legally flatten it into a single endpoint rewrite.
pub fn parse_check(source: &str) -> Result<Json> {
    let mut eg = EGraph::default();
    let commands = eg.parse_program(None, source)?;
    Ok(json!({"commands": commands.len()}))
}
/// Source ranges come from egglog's parser (including multi-line rules and Unicode).
pub fn patterns(source: &str) -> Result<Json> {
    let mut eg = EGraph::default();
    let mut rows = vec![];
    for command in crate::visual_rule::surface_program(eg.parse_program(None, source)?) {
        // `birewrite` desugars to both directions and the transpiler emits no
        // `add_rule` scope for it; preview its forward rewrite like a plain rewrite.
        let command = match command {
            Command::BiRewrite(ruleset, rewrite) => Command::Rewrite(ruleset, rewrite, false),
            c => c,
        };
        // Keep subsume explicit instead of normalizing it into a union.
        let command = match command {
            Command::Rewrite(_, ref r, true) => {
                return Err(format!("subsuming rewrite preview is unsupported: {}", r.lhs).into());
            }
            c => crate::visual_rule::normalize(c, rows.len()),
        };
        if let Command::Rule { mut rule } = command {
            if rule.name.is_empty() {
                rule.name = format!("R{}", rows.len());
            }
            let (start, end) = location(&rule);
            let mut row = render(&rule);
            row["source_line"] = json!(start);
            row["end_line"] = json!(end);
            row["rule"] = json!(rule.name);
            rows.push(row);
        }
    }
    Ok(json!({"patterns":rows}))
}

pub fn stream(root: &Path, source: &Path, emit: &mut dyn FnMut(Json) -> Result) -> Result {
    stream_source(
        root,
        &source.to_string_lossy(),
        &std::fs::read_to_string(source)?,
        emit,
    )
}
pub fn stream_source(
    root: &Path,
    name: &str,
    text: &str,
    emit: &mut dyn FnMut(Json) -> Result,
) -> Result {
    let out = std::env::var_os("EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            root.join("out/debug-stream").join(format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        });
    let report = super::execute(
        name,
        text,
        None,
        &out,
        None,
        false,
        std::env::var_os("EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT").is_none(),
        true,
        crate::ripen_observation::Limits::from_env()?,
        emit,
    )?;
    emit(json!({"kind":"complete","analysis_status":report["status"]}))
}
