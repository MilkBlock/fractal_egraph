//! Application-independent roles of a candidate/minimum closure. Discovery is
//! structural; the executor rechecks the complete helper universe before use.
use super::*;
use egglog::ast::{Action, Expr, Fact, Literal, Rule};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub selected: String,
    pub candidate: String,
    pub pair: String,
    pub compare: String,
    pub empty: String,
    pub sentinel: i64,
    pub sets: [String; 3],
}
fn call(e: &Expr) -> Option<(&str, &[Expr])> {
    if let Expr::Call(_, f, a) = e {
        Some((f, a))
    } else {
        None
    }
}
fn variable(e: &Expr) -> Option<&str> {
    if let Expr::Var(_, v) = e {
        Some(v)
    } else {
        None
    }
}
fn mentions(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Call(_, f, xs) => f == name || xs.iter().any(|e| mentions(e, name)),
        _ => false,
    }
}
pub(super) fn signature(rule: &Rule) -> (String, String, String) {
    let mut r = rule.clone();
    let mut vars = BTreeMap::new();
    let mut rename = |e: Expr| match e {
        Expr::Var(span, name) => {
            let next = vars.len();
            Expr::Var(
                span,
                vars.entry(name)
                    .or_insert_with(|| format!("v{next}"))
                    .clone(),
            )
        }
        e => e,
    };
    for f in &mut r.body {
        match f {
            Fact::Eq(_, a, b) => {
                *a = a.clone().visit_exprs(&mut rename);
                *b = b.clone().visit_exprs(&mut rename)
            }
            Fact::Fact(e) => *e = e.clone().visit_exprs(&mut rename),
        }
    }
    for a in &mut r.head.0 {
        match a {
            Action::Set(_, _, args, e) => {
                for a in args {
                    *a = a.clone().visit_exprs(&mut rename)
                }
                *e = e.clone().visit_exprs(&mut rename)
            }
            Action::Union(_, a, b) => {
                *a = a.clone().visit_exprs(&mut rename);
                *b = b.clone().visit_exprs(&mut rename)
            }
            Action::Expr(_, e) | Action::Let(_, _, e) => *e = e.clone().visit_exprs(&mut rename),
            // The source panic is still executed on a negative-cost fallback.
            Action::Panic(_, message) => *message = "source diagnostic".into(),
            Action::Change(..) => (),
        }
    }
    (
        r.ruleset,
        r.body
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        r.head
            .0
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
    )
}
impl Selection {
    pub fn discover(eg: &EGraph, commands: &[Command]) -> Result<Self> {
        let mut found = vec![];
        for c in commands {
            let Command::Function {
                name: selected,
                schema,
                merge: Some(m),
                ..
            } = c
            else {
                continue;
            };
            let Some((compare, args)) = call(m) else {
                continue;
            };
            if schema.input.len() != 1
                || args.len() != 2
                || variable(&args[0]) != Some("old")
                || variable(&args[1]) != Some("new")
            {
                continue;
            }
            if !eg.get_function(compare).is_some_and(|f| f.is_constructor()) {
                continue;
            }
            for c in commands {
                let Command::Rule { rule } = c else { continue };
                let [Action::Set(_, field, keys, rhs)] = rule.head.0.as_slice() else {
                    continue;
                };
                if field != selected || keys.len() != 1 {
                    continue;
                }
                let Some((pair, parts)) = call(rhs) else {
                    continue;
                };
                if parts.len() != 2 {
                    continue;
                }
                let Some((empty, empty_args)) = call(&parts[0]) else {
                    continue;
                };
                let Expr::Lit(_, Literal::Int(sentinel)) = &parts[1] else {
                    continue;
                };
                if !empty_args.is_empty() {
                    continue;
                }
                let [Fact::Fact(entry)] = rule.body.as_slice() else {
                    continue;
                };
                let Some((candidate, inputs)) = call(entry) else {
                    continue;
                };
                if inputs.len() != 2 || inputs[0].to_string() != keys[0].to_string() {
                    continue;
                }
                let producers=commands.iter().filter_map(|c|if let Command::Rule{rule}=c {
                    rule.head.0.iter().any(|a|matches!(a,Action::Expr(_,e) if call(e).is_some_and(|(f,_)|f==candidate))).then_some(rule.ruleset.clone())
                }else{None}).collect::<BTreeSet<_>>();
                let comparisons = commands
                    .iter()
                    .filter_map(|c| {
                        if let Command::Rule { rule } = c {
                            (rule.head.0.iter().any(|a| matches!(a, Action::Union(..)))
                                && rule.body.iter().any(|f| match f {
                                    Fact::Eq(_, a, b) => {
                                        mentions(a, compare) || mentions(b, compare)
                                    }
                                    Fact::Fact(e) => mentions(e, compare),
                                }))
                            .then_some(rule.ruleset.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<BTreeSet<_>>();
                if producers.len() != 1 || comparisons.len() != 1 {
                    continue;
                }
                let r = Self {
                    selected: selected.clone(),
                    candidate: candidate.into(),
                    pair: pair.into(),
                    compare: compare.into(),
                    empty: empty.into(),
                    sentinel: *sentinel,
                    sets: [
                        producers.first().unwrap().clone(),
                        rule.ruleset.clone(),
                        comparisons.first().unwrap().clone(),
                    ],
                };
                if r.sets.iter().collect::<BTreeSet<_>>().len() != 3 {
                    continue;
                }
                // Require the entire template, not just the suggestive initializer.
                if r.validate_helpers(eg, commands).is_ok() && !found.contains(&r) {
                    found.push(r)
                }
            }
        }
        if found.len() != 1 {
            return Err(format!(
                "expected one checked selection closure, discovered {}",
                found.len()
            )
            .into());
        }
        Ok(found.pop().unwrap())
    }
    pub fn helpers(&self) -> Result<Vec<Command>> {
        let mut eg = EGraph::default();
        let mut commands = eg.parse_program(None, include_str!("selection_helpers.egg"))?;
        let names = BTreeMap::from([
            ("Selected", self.selected.as_str()),
            ("Offer", self.candidate.as_str()),
            ("Weighted", self.pair.as_str()),
            ("Choose", self.compare.as_str()),
            ("Missing", self.empty.as_str()),
        ]);
        for c in &mut commands {
            if let Command::Rule { rule } = c {
                rule.ruleset = if rule.ruleset == "update" {
                    self.sets[1].clone()
                } else {
                    self.sets[2].clone()
                };
                let mut rename = |e: Expr| match e {
                    Expr::Call(s, n, a) => Expr::Call(
                        s,
                        names.get(n.as_str()).map(|s| s.to_string()).unwrap_or(n),
                        a,
                    ),
                    Expr::Var(s, n) if n == "sentinel_value" => {
                        Expr::Lit(s, Literal::Int(self.sentinel))
                    }
                    e => e,
                };
                for f in &mut rule.body {
                    match f {
                        Fact::Eq(_, a, b) => {
                            *a = a.clone().visit_exprs(&mut rename);
                            *b = b.clone().visit_exprs(&mut rename)
                        }
                        Fact::Fact(e) => *e = e.clone().visit_exprs(&mut rename),
                    }
                }
                for a in &mut rule.head.0 {
                    match a {
                        Action::Set(_, f, args, e) => {
                            *f = names
                                .get(f.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or(f.clone());
                            for x in args {
                                *x = x.clone().visit_exprs(&mut rename)
                            }
                            *e = e.clone().visit_exprs(&mut rename)
                        }
                        Action::Union(_, a, b) => {
                            *a = a.clone().visit_exprs(&mut rename);
                            *b = b.clone().visit_exprs(&mut rename)
                        }
                        _ => (),
                    }
                }
            }
        }
        Ok(commands)
    }
    pub fn validate_helpers(&self, eg: &EGraph, commands: &[Command]) -> Result<()> {
        let expected = self
            .helpers()?
            .iter()
            .filter_map(|c| {
                if let Command::Rule { rule } = c {
                    Some(signature(rule))
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        let actual = commands
            .iter()
            .filter_map(|c| {
                if let Command::Rule { rule } = c {
                    self.sets[1..]
                        .contains(&rule.ruleset)
                        .then(|| signature(rule))
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        if expected != actual
            || self.sets[1..]
                .iter()
                .map(|s| eg.cell_rule_names(s).map(|xs| xs.len()))
                .collect::<std::result::Result<Vec<_>, _>>()?
                .iter()
                .sum::<usize>()
                != expected.len()
        {
            return Err("selection closure helper universe differs".into());
        }
        Ok(())
    }
    pub fn schedule(&self) -> String {
        format!(
            "(run-schedule (saturate (seq {} (saturate (seq {} (saturate {}))))))",
            self.sets[0], self.sets[1], self.sets[2]
        )
    }
}
