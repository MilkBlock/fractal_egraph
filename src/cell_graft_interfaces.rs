//! Conditional Cell edges, separate from chronology and from runtime code emission.
//! For a positive constructor/equality environment, a rebased parent preparation
//! contained in a child can be saturated first: closure is monotone and idempotent.
//! Preserve the remaining ordered replay, including every check and union.
use egglog::{
    EGraph,
    ast::{Action, Command, Expr, Fact},
};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Prepared {
    environment: Vec<String>,
    commands: Vec<Command>,
    validation: Vec<Command>,
    ports: Vec<(String, u64, String)>,
    values: BTreeMap<u64, (String, String)>,
    members: BTreeSet<u64>,
    injections: Json,
    opaque: BTreeSet<String>,
}
fn expr(e: &Expr, constructors: &BTreeSet<String>) -> bool {
    match e {
        Expr::Call(_, op, args) => {
            constructors.contains(op) && args.iter().all(|e| expr(e, constructors))
        }
        Expr::Var(..) | Expr::Lit(..) => true,
    }
}
fn fact(f: &Fact, constructors: &BTreeSet<String>) -> bool {
    match f {
        Fact::Eq(_, a, b) => expr(a, constructors) && expr(b, constructors),
        Fact::Fact(e) => expr(e, constructors),
    }
}
fn action(a: &Action, constructors: &BTreeSet<String>) -> bool {
    match a {
        Action::Expr(_, e) => expr(e, constructors),
        Action::Union(_, a, b) => expr(a, constructors) && expr(b, constructors),
        _ => false,
    }
}
// A double fail is an immediate positive check, not an absence test. Do not
// erase it or move it before its original position in the replay.
fn check(c: &Command, constructors: &BTreeSet<String>) -> bool {
    match c {
        Command::Check(_, fs) => fs.iter().all(|f| fact(f, constructors)),
        Command::Fail(_, a) => {
            matches!(a.as_ref(), Command::Fail(_, b) if matches!(b.as_ref(), Command::Check(..)) && check(b,constructors))
        }
        _ => false,
    }
}
fn map(c: &Command, replace: &mut impl FnMut(Expr) -> Expr) -> Option<Command> {
    Some(match c {
        Command::Action(a) => Command::Action(a.clone().visit_exprs(replace)),
        Command::Check(s, fs) => Command::Check(
            s.clone(),
            fs.iter().cloned().map(|f| f.visit_exprs(replace)).collect(),
        ),
        Command::Fail(s, c) => Command::Fail(s.clone(), Box::new(map(c, replace)?)),
        _ => return None,
    })
}
impl Prepared {
    pub fn new(origin: &Json, source: &str, validation: &str) -> Option<Self> {
        // Symbolic guard lowering is a different semantics, not this proof.
        if origin
            .get("symbolic_lowering")
            .is_some_and(|x| !x.is_null())
        {
            return None;
        }
        let mut eg = EGraph::default();
        let parsed = eg.parse_program(None, source).ok()?;
        let validation = eg.parse_program(None, validation).ok()?;
        let mut constructors = BTreeSet::new();
        for c in &parsed {
            match c {
                Command::Datatype { variants, .. } => {
                    constructors.extend(variants.iter().map(|v| v.name.clone()))
                }
                Command::Constructor { name, .. } => {
                    constructors.insert(name.clone());
                }
                _ => {}
            }
        }
        let mut environment = vec![];
        let mut commands = vec![];
        for c in parsed {
            match &c {
                Command::Datatype { .. }
                | Command::Constructor { .. }
                | Command::AddRuleset(..) => environment.push(c.to_string()),
                Command::Sort {
                    presort_and_args: None,
                    ..
                } => environment.push(c.to_string()),
                Command::Rule { rule }
                    if rule.body.iter().all(|f| fact(f, &constructors))
                        && rule.head.0.iter().all(|a| action(a, &constructors)) =>
                {
                    environment.push(c.to_string())
                }
                Command::Action(a) if action(a, &constructors) => commands.push(c),
                _ if check(&c, &constructors) => commands.push(c),
                _ => return None, // set, subsume, primitives, functions, negative tests, etc.
            }
        }
        // The preparation API returns a validation program including the same
        // declarations and parameter initialization, not only bare checks.
        let validation: Vec<_> = validation
            .into_iter()
            .filter(|c| !environment.contains(&c.to_string()))
            .collect();
        if !validation.iter().all(|c| {
            check(c, &constructors) || matches!(c, Command::Action(a) if action(a,&constructors))
        }) {
            return None;
        }
        let values: BTreeMap<_, _> = origin["symbolic_values"]
            .as_array()?
            .iter()
            .map(|v| {
                Some((
                    v["token"].as_u64()?,
                    (
                        v["term"].as_str()?.to_owned(),
                        v["sort"].as_str()?.to_owned(),
                    ),
                ))
            })
            .collect::<Option<_>>()?;
        let ports = origin["reuse_interface_terms"]["lower"]
            .as_array()?
            .iter()
            .map(|p| {
                let term = p.as_str()?;
                let (token, (_, sort)) = values.iter().find(|(_, (t, _))| t == term)?;
                Some((term.to_owned(), *token, sort.clone()))
            })
            .collect::<Option<Vec<_>>>()?;
        let members = origin["members"]
            .as_array()?
            .iter()
            .map(Json::as_u64)
            .collect::<Option<_>>()?;
        let mut opaque: BTreeSet<String> = origin["parameter_markers"]
            .as_object()
            .into_iter()
            .flat_map(|m| m.values())
            .filter_map(Json::as_str)
            .map(str::to_owned)
            .collect();
        if let Some(marker) = origin["parameter_marker"].as_str() {
            opaque.insert(marker.to_owned());
        }
        Some(Self {
            opaque,
            environment,
            commands,
            validation,
            ports,
            values,
            members,
            injections: origin["staged_injections"].clone(),
        })
    }
    pub fn extension(&self, parents: &[(usize, &Self)]) -> Option<Json> {
        if parents.is_empty() {
            return None;
        }
        let text: Vec<_> = self.commands.iter().map(ToString::to_string).collect();
        let checks: BTreeSet<_> = text
            .iter()
            .cloned()
            .chain(self.validation.iter().map(ToString::to_string))
            .collect();
        let mut removed = BTreeSet::new();
        let mut bindings = vec![];
        let mut inherited = BTreeSet::<u64>::new();
        for (unit, p) in parents {
            if p.environment != self.environment
                || !p.members.is_subset(&self.members)
                || p.members == self.members
            {
                return None;
            }
            let mut substitutions = BTreeMap::new();
            let mut ports = vec![];
            for (from, token, sort) in &p.ports {
                let (to, to_sort) = self.values.get(token)?;
                if sort != to_sort {
                    return None;
                }
                let value = egglog::ast::Parser::default()
                    .get_expr_from_string(None, to)
                    .ok()?;
                substitutions.insert(from.clone(), value);
                ports.push(json!({"from":from,"to":to,"sort":sort,"history_token":token}));
            }
            let mut unmapped = false;
            let mut replace = |e: Expr| {
                if let Some(mapped) = substitutions.get(&e.to_string()) {
                    return mapped.clone();
                }
                if matches!(&e, Expr::Call(_,op,_) if p.opaque.contains(op)) {
                    unmapped = true;
                }
                e
            };
            let mut cursor = 0;
            for command in &p.commands {
                let rebased = map(command, &mut replace)?.to_string();
                let next = text[cursor..].iter().position(|s| *s == rebased)? + cursor;
                removed.insert(next);
                cursor = next + 1;
            }
            for command in &p.validation {
                if !checks.contains(&map(command, &mut replace)?.to_string()) {
                    return None;
                }
            }
            if unmapped {
                return None;
            }
            inherited.extend(&p.members);
            bindings.push(json!({"parent":unit,"ports":ports}));
        }
        let residual: Vec<_> = text
            .into_iter()
            .enumerate()
            .filter_map(|(i, s)| (!removed.contains(&i)).then_some(s))
            .collect();
        let injections: Vec<_> = self
            .injections
            .as_array()?
            .iter()
            .filter(|s| {
                !s["before_record"]
                    .as_u64()
                    .is_some_and(|r| inherited.contains(&r))
            })
            .collect();
        Some(
            json!({"kind":"positive-replay-extension/v1", "parent_bindings":bindings,
            "external_injections":injections, "ordered_replay":residual,
            "validation":self.validation.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "scope":"same constructor/equality environment; typed history-bound substitution; parent replay is an ordered subsequence; every remaining external fact, check, action and union retained; not an emitted runtime rule"}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn origin(a: u64, b: u64, members: Vec<usize>) -> Json {
        json!({"parameter_marker":"Input","reuse_interface_terms":{"lower":[format!("(Input {a})"),format!("(Input {b})")]},
            "symbolic_values":[{"token":10,"sort":"T","term":format!("(Input {a})")},{"token":11,"sort":"T","term":format!("(Input {b})")}],
            "members":members,"staged_injections":[]})
    }
    const ENV: &str = "(datatype T (Input i64) (Add T T) (Left T) (Right T)) (ruleset r)";
    #[test]
    fn ports_checks_and_unions_survive_renaming() {
        let p = Prepared::new(
            &origin(7, 9, vec![0]),
            &format!("{ENV} (Add (Input 7) (Input 9))"),
            "",
        )
        .unwrap();
        let source = format!(
            "{ENV} (Add (Input 41) (Input 82)) (union (Input 41) (Input 82)) (fail (fail (check (= (Input 41) (Input 82))))) (Right (Input 82))"
        );
        let c = Prepared::new(&origin(41, 82, vec![0, 2]), &source, "").unwrap();
        let proof = c.extension(&[(0, &p)]).unwrap();
        assert_eq!(proof["parent_bindings"][0]["ports"][0]["to"], "(Input 41)");
        assert_eq!(
            proof["ordered_replay"],
            json!([
                "(union (Input 41) (Input 82))",
                "(fail (fail (check (= (Input 41) (Input 82)))))",
                "(Right (Input 82))"
            ])
        );
        let swapped = Prepared::new(&origin(82, 41, vec![0, 2]), &source, "").unwrap();
        assert!(
            swapped.extension(&[(0, &p)]).is_none(),
            "same ops with opposite binding is not the same parent"
        );
        let mut wrong_sort = origin(41, 82, vec![0, 2]);
        wrong_sort["symbolic_values"][0]["sort"] = json!("Other");
        assert!(
            Prepared::new(&wrong_sort, &source, "")
                .unwrap()
                .extension(&[(0, &p)])
                .is_none()
        );
    }
    #[test]
    fn all_parents_and_all_opaque_ports_are_required() {
        let po = origin(7, 9, vec![0]);
        let p = Prepared::new(&po, &format!("{ENV} (Left (Input 7))"), "").unwrap();
        let q = Prepared::new(
            &origin(7, 9, vec![1]),
            &format!("{ENV} (Right (Input 9))"),
            "",
        )
        .unwrap();
        let co = origin(41, 82, vec![0, 1, 2]);
        let source =
            format!("{ENV} (Left (Input 41)) (Right (Input 82)) (Add (Input 41) (Input 82))");
        let child = Prepared::new(&co, &source, "").unwrap();
        assert_eq!(
            child.extension(&[(0, &p), (1, &q)]).unwrap()["parent_bindings"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let missing = Prepared::new(
            &co,
            &format!("{ENV} (Left (Input 41)) (Add (Input 41) (Input 82))"),
            "",
        )
        .unwrap();
        assert!(missing.extension(&[(0, &p), (1, &q)]).is_none());
        let mut incomplete = origin(41, 82, vec![0]);
        incomplete["reuse_interface_terms"]["lower"] = json!([]);
        let unbound = Prepared::new(&incomplete, &format!("{ENV} (Left (Input 41))"), "").unwrap();
        assert!(child.extension(&[(0, &unbound)]).is_none());
        let other_environment =
            Prepared::new(&co, &format!("{source} (ruleset another)"), "").unwrap();
        assert!(other_environment.extension(&[(0, &p)]).is_none());
    }
    #[test]
    fn sibling_effects_environment_and_nonpositive_programs_are_not_dropped() {
        let p = Prepared::new(
            &origin(0, 1, vec![0, 1]),
            &format!("{ENV} (Add (Input 0) (Input 1)) (Left (Input 0))"),
            "",
        )
        .unwrap();
        let c = Prepared::new(
            &origin(0, 1, vec![0, 2]),
            &format!("{ENV} (Add (Input 0) (Input 1)) (Right (Input 1))"),
            "",
        )
        .unwrap();
        assert!(c.extension(&[(0, &p)]).is_none());
        for bad in [
            "(subsume (Add (Input 0) (Input 1)))",
            "(fail (check (Left (Input 0))))",
            "(rule ((Add x y) (= z (+ 1 2))) ((Left x)) :ruleset r)",
        ] {
            assert!(Prepared::new(&origin(0, 1, vec![0]), &format!("{ENV} {bad}"), "").is_none());
        }
    }
}
