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
