//! Alpha-normalization of opaque RipenInput slots, anchored by ordered interfaces.
//! This is a bijective renaming, not erasure of primitive values or obligations.
use crate::pipeline::Result;
use egglog::{
    EGraph,
    ast::{Command, Expr, Literal},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn contract(origin: &Value, source: &str, validation: &str) -> Result<Value> {
    let marker = origin["parameter_marker"]
        .as_str()
        .ok_or("missing opaque-input constructor")?;
    let mut slots = BTreeMap::new();
    let mut rename = |expr: Expr| -> Expr {
        match expr {
            Expr::Call(span, op, mut args) if op == marker && args.len() == 1 => {
                if let Expr::Lit(s, Literal::Int(n)) = &args[0] {
                    let next = slots.len() as i64;
                    let slot = *slots.entry(*n).or_insert(next);
                    args[0] = Expr::Lit(s.clone(), Literal::Int(slot));
                }
                Expr::Call(span, op, args)
            }
            other => other,
        }
    };
    let mut interface = serde_json::Map::new();
    for side in ["lower", "upper"] {
        let terms = origin["reuse_interface_terms"][side]
            .as_array()
            .ok_or("missing ordered interface")?;
        let normalized = terms
            .iter()
            .map(|term| -> Result<String> {
                let expr = egglog::ast::Parser::default()
                    .get_expr_from_string(None, term.as_str().ok_or("invalid interface term")?)?;
                Ok(expr.visit_exprs(&mut rename).to_string())
            })
            .collect::<Result<Vec<_>>>()?;
        interface.insert(side.into(), json!(normalized));
    }
    let mut parser = EGraph::default();
    let mut program = |text: &str| -> Result<Vec<String>> {
        parser
            .parse_program(None, text)?
            .into_iter()
            .map(|command| {
                let normalized = match command {
                    Command::Action(action) => Command::Action(action.visit_exprs(&mut rename)),
                    Command::Check(span, facts) => Command::Check(
                        span,
                        facts
                            .into_iter()
                            .map(|f| f.visit_exprs(&mut rename))
                            .collect(),
                    ),
                    // Rule/environment declarations are invariant: never reinterpret user
                    // constants, variable scopes or rule names as boundary slot numbers.
                    other => other,
                };
                Ok(normalized.to_string())
            })
            .collect()
    };
    let source = program(source)?;
    let validation = program(validation)?;
    Ok(json!({"source":source,"validation":validation,"interface":interface}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn origin(a: i64, b: i64) -> Value {
        json!({"parameter_marker":"RipenInput","reuse_interface_terms":{"lower":[format!("(RipenInput {a})"),format!("(RipenInput {b})")],"upper":[]}})
    }
    #[test]
    fn opaque_slots_rename_but_alias_constants_order_and_obligations_survive() {
        let a = contract(
            &origin(7, 9),
            "(Add (RipenInput 7) (RipenInput 9))",
            "(check (= (Const 7) (Const 7)))",
        )
        .unwrap();
        let b = contract(
            &origin(41, 82),
            "(Add (RipenInput 41) (RipenInput 82))",
            "(check (= (Const 7) (Const 7)))",
        )
        .unwrap();
        assert_eq!(a, b);
        for other in [
            contract(
                &origin(41, 82),
                "(Add (RipenInput 82) (RipenInput 41))",
                "(check (= (Const 7) (Const 7)))",
            )
            .unwrap(),
            contract(
                &origin(41, 41),
                "(Add (RipenInput 41) (RipenInput 41))",
                "(check (= (Const 7) (Const 7)))",
            )
            .unwrap(),
            contract(
                &origin(41, 82),
                "(Add (RipenInput 41) (RipenInput 82))",
                "(check (= (Const 8) (Const 8)))",
            )
            .unwrap(),
        ] {
            assert_ne!(a, other);
        }
        assert_ne!(
            contract(&origin(0, 1), "(A (RipenInput 0))\n(B (RipenInput 1))", "").unwrap(),
            contract(&origin(0, 1), "(B (RipenInput 1))\n(A (RipenInput 0))", "").unwrap()
        );
    }
}

/// Exact sequential factorization, never subtraction of an unordered effect set.
/// Restrict to positive constructor programs; checkpoints retain their positions.
pub(super) fn suffix(parent: &Value, child: &Value) -> Option<Value> {
    if parent["interface"]["lower"] != child["interface"]["lower"] {
        return None;
    }
    let mut result = serde_json::Map::new();
    for field in ["source", "validation"] {
        let prefix = parent[field].as_array()?;
        let full = child[field].as_array()?;
        if prefix.is_empty() || full.len() <= prefix.len() || !full.starts_with(prefix) {
            return None;
        }
        let mut parser = EGraph::default();
        for text in full {
            for command in parser.parse_program(None, text.as_str()?).ok()? {
                use egglog::ast::Action;
                let positive = |a: &Action| {
                    matches!(a, Action::Expr(..) | Action::Let(..) | Action::Union(..))
                };
                match command {
                    Command::Datatype { .. } | Command::Check(..) => {}
                    Command::Rule { rule } if rule.head.0.iter().all(positive) => {}
                    Command::Action(a) if positive(&a) => {}
                    _ => return None,
                }
            }
        }
        result.insert(field.into(), json!(&full[prefix.len()..]));
    }
    result.insert("interface".into(), child["interface"].clone());
    result.insert("kind".into(), json!("positive-sequential-suffix/v1"));
    Some(Value::Object(result))
}

/// Rebase each observed parent's opaque inputs into the child's coordinates, then
/// remove only actions/checks literally certified by those rebased preparations.
/// No subgraph search, partner enumeration, or guessed value correspondence.
pub(super) fn residual(
    parents: &[(&Value, &Value)],
    child: &Value,
    origin: &Value,
) -> Option<Value> {
    if parents.is_empty() {
        return None;
    }
    let marker = origin["parameter_marker"].as_str()?;
    let mut supplied = [
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::new(),
    ];
    let mut wiring = vec![];
    let mut parser = EGraph::default();
    fn split(text: &str, parser: &mut EGraph) -> Option<(bool, Command)> {
        let mut commands = parser.parse_program(None, text).ok()?;
        if commands.len() != 1 {
            return None;
        }
        let command = commands.remove(0);
        use egglog::ast::Action;
        let positive = |a: &Action| matches!(a, Action::Expr(..) | Action::Union(..));
        let effect = match &command {
            Command::Datatype { .. } => false,
            Command::Rule { rule } if rule.head.0.iter().all(positive) => false,
            Command::Action(a) if positive(a) => true,
            Command::Check(..) => true,
            _ => return None, // globals, updates and non-positive schedules need a different proof
        };
        Some((effect, command))
    }
    let mut environment = std::collections::BTreeSet::new();
    for text in child["source"].as_array()? {
        let (effect, _) = split(text.as_str()?, &mut parser)?;
        if !effect {
            environment.insert(text.as_str()?.to_owned());
        }
    }
    for (program, parent_origin) in parents {
        if parent_origin["parameter_marker"].as_str()? != marker {
            return None;
        }
        let raw_lower = parent_origin["reuse_interface_terms"]["lower"].as_array()?;
        let canonical_lower = program["interface"]["lower"].as_array()?;
        if raw_lower.len() != canonical_lower.len() {
            return None;
        }
        let mut replacements = BTreeMap::new();
        let mut ports = vec![];
        for (raw, canonical) in raw_lower.iter().zip(canonical_lower) {
            let token = parent_origin["symbolic_values"]
                .as_array()?
                .iter()
                .find(|v| v["term"] == *raw)?["token"]
                .as_u64()?;
            let term = origin["symbolic_values"]
                .as_array()?
                .iter()
                .find(|v| v["token"].as_u64() == Some(token))?["term"]
                .as_str()?;
            let canonical_term = contract(origin, term, "").ok()?;
            let expr = egglog::ast::Parser::default()
                .get_expr_from_string(None, canonical_term["source"][0].as_str()?)
                .ok()?;
            replacements.insert(canonical.as_str()?.to_owned(), expr);
            ports.push(canonical_term["source"][0].clone());
        }
        wiring.push(json!(ports));
        for (i, field) in ["source", "validation"].iter().enumerate() {
            for text in program[*field].as_array()? {
                let (effect, command) = split(text.as_str()?, &mut parser)?;
                if !effect {
                    if !environment.contains(text.as_str()?) {
                        return None;
                    }
                    continue;
                }
                let mut unmapped = false;
                let mut replace = |e: Expr| {
                    if let Some(mapped) = replacements.get(&e.to_string()) {
                        return mapped.clone();
                    }
                    if matches!(&e, Expr::Call(_, op, _) if op == marker) {
                        unmapped = true;
                    }
                    e
                };
                let rebased = match command {
                    Command::Action(a) => Command::Action(a.visit_exprs(&mut replace)),
                    Command::Check(s, f) => Command::Check(
                        s,
                        f.into_iter().map(|x| x.visit_exprs(&mut replace)).collect(),
                    ),
                    _ => return None,
                };
                if unmapped {
                    return None;
                }
                supplied[i].insert(rebased.to_string());
            }
        }
    }
    let mut out = serde_json::Map::new();
    for (i, field) in ["source", "validation"].iter().enumerate() {
        let full = child[*field].as_array()?;
        let existing: std::collections::BTreeSet<_> =
            full.iter().filter_map(Value::as_str).collect();
        // Exact coverage is the admission proof; never assume a parent's missing
        // action is implied by a merely similar child action.
        if supplied[i].is_empty() || !supplied[i].iter().all(|s| existing.contains(s.as_str())) {
            return None;
        }
        let remaining: Vec<_> = full
            .iter()
            .filter(|v| !supplied[i].contains(v.as_str().unwrap_or("")))
            .cloned()
            .collect();
        out.insert((*field).into(), json!(remaining));
    }
    out.insert("interface".into(), child["interface"].clone());
    out.insert("parent_wiring".into(), json!(wiring));
    out.insert("kind".into(), json!("positive-parent-residual/v1"));
    Some(Value::Object(out))
}

#[cfg(test)]
mod residual_tests {
    use super::*;
    fn origin(slots: &[(u64, u64)]) -> Value {
        json!({"parameter_marker":"RipenInput", "reuse_interface_terms":{"lower":slots.iter().map(|(_,s)|format!("(RipenInput {s})")).collect::<Vec<_>>(),"upper":[]},
            "symbolic_values":slots.iter().map(|(t,s)|json!({"token":t,"term":format!("(RipenInput {s})")})).collect::<Vec<_>>()})
    }
    #[test]
    fn multiple_parents_rebase_by_actual_binding_and_leave_only_residual() {
        let p = origin(&[(10, 7)]);
        let q = origin(&[(20, 9)]);
        let c = origin(&[(10, 41), (20, 82)]);
        let pcode = "(A (RipenInput 7))";
        let qcode = "(B (RipenInput 9))";
        let code = "(A (RipenInput 41)) (B (RipenInput 82)) (C (RipenInput 41) (RipenInput 82))";
        let pp = contract(&p, pcode, pcode).unwrap();
        let qp = contract(&q, qcode, qcode).unwrap();
        let cc = contract(&c, code, code).unwrap();
        let result = residual(&[(&pp, &p), (&qp, &q)], &cc, &c).unwrap();
        assert_eq!(
            result["source"],
            json!(["(C (RipenInput 0) (RipenInput 1))"])
        );
        assert_eq!(
            result["parent_wiring"],
            json!([["(RipenInput 0)"], ["(RipenInput 1)"]])
        );
        // A missing effect, absent binding and subsume cannot be silently removed.
        let missing = contract(&c, "(C (RipenInput 41) (RipenInput 82))", code).unwrap();
        assert!(residual(&[(&pp, &p), (&qp, &q)], &missing, &c).is_none());
        assert!(residual(&[(&pp, &origin(&[(99, 7)]))], &cc, &c).is_none());
        let nonpositive =
            contract(&c, &format!("{code} (subsume (A (RipenInput 41)))"), code).unwrap();
        assert!(residual(&[(&pp, &p), (&qp, &q)], &nonpositive, &c).is_none());
    }
}
