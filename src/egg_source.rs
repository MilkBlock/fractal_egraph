//! Lossless string printing around the native AST printer; no source parser.
use egglog::ast::{Action, Command, Expr, Literal};
pub fn quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('\"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    )
}
pub fn printed(command: &Command) -> String {
    // Tokens must not collide with user identifiers or literal contents.
    let original = command.to_string();
    let mut prefix = "__egg_source_".to_string();
    while original.contains(&prefix) {
        prefix.push('_');
    }
    let mut replacements = Vec::new();
    let mut c = command.clone().visit_exprs(&mut |e| {
        if let Expr::Lit(span, Literal::String(s)) = e {
            let token = format!("{prefix}literal_{}", replacements.len());
            replacements.push((token.clone(), quote(&s)));
            Expr::Var(span, token)
        } else {
            e
        }
    });
    fn protect(s: &mut String, replacements: &mut Vec<(String, String)>, prefix: &str) {
        let token = format!("{prefix}quoted_{}", replacements.len());
        replacements.push((format!("\"{token}\""), quote(s)));
        *s = token;
    }
    fn misc(c: &mut Command, r: &mut Vec<(String, String)>, prefix: &str) {
        match c {
            Command::Rule { rule } => {
                if !rule.name.is_empty() {
                    protect(&mut rule.name, r, prefix)
                }
                for a in &mut rule.head.0 {
                    if let Action::Panic(_, s) = a {
                        protect(s, r, prefix)
                    }
                }
            }
            Command::Action(Action::Panic(_, s)) => protect(s, r, prefix),
            Command::Fail(_, c) => misc(c, r, prefix),
            Command::Output { file, .. } => protect(file, r, prefix),
            Command::PrintOverallStatistics(_, Some(file)) => protect(file, r, prefix),
            _ => {}
        }
    }
    misc(&mut c, &mut replacements, &prefix);
    let mut text = c.to_string();
    // Longer tokens first to avoid replacing a numeric suffix prefix.
    replacements.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
    for (from, to) in replacements {
        text = text.replace(&from, &to);
    }
    text
}

pub fn schedule_text(s: &egglog::ast::Schedule) -> String {
    let command = printed(&Command::RunSchedule(s.clone()));
    command
        .strip_prefix("(run-schedule ")
        .unwrap()
        .strip_suffix(')')
        .unwrap()
        .to_string()
}
