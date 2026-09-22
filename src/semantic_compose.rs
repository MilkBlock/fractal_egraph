//! Typed syntactic unification of finite positive rewrite compositions.
//! This is not unification modulo arbitrary e-graph equalities.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Term {
    Var {
        id: usize,
        sort: String,
    },
    App {
        op: String,
        args: Vec<Term>,
        sort: String,
    },
    Lit {
        code: String,
        sort: String,
    },
}
impl Term {
    pub fn sort(&self) -> &str {
        match self {
            Self::Var { sort, .. } | Self::App { sort, .. } | Self::Lit { sort, .. } => sort,
        }
    }
    fn vars(&self, s: &mut BTreeSet<usize>) {
        match self {
            Self::Var { id, .. } => {
                s.insert(*id);
            }
            Self::App { args, .. } => {
                for a in args {
                    a.vars(s)
                }
            }
            _ => {}
        }
    }
    fn shift(&self, n: usize) -> Self {
        match self {
            Self::Var { id, sort } => Self::Var {
                id: id + n,
                sort: sort.clone(),
            },
            Self::App { op, args, sort } => Self::App {
                op: op.clone(),
                args: args.iter().map(|a| a.shift(n)).collect(),
                sort: sort.clone(),
            },
            _ => self.clone(),
        }
    }
    pub fn code(&self) -> String {
        match self {
            Self::Var { id, .. } => format!("v{id}"),
            Self::Lit { code, .. } => code.clone(),
            Self::App { op, args, .. } => format!(
                "({op}{})",
                args.iter()
                    .map(|a| format!(" {}", a.code()))
                    .collect::<String>()
            ),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Guard {
    Equal(Term, Term),
    Predicate(Term),
}
impl Guard {
    fn map(&self, f: impl Fn(&Term) -> Term) -> Self {
        match self {
            Self::Equal(a, b) => Self::Equal(f(a), f(b)),
            Self::Predicate(t) => Self::Predicate(f(t)),
        }
    }
    fn vars(&self, vars: &mut BTreeSet<usize>) {
        match self {
            Self::Equal(a, b) => {
                a.vars(vars);
                b.vars(vars);
            }
            Self::Predicate(t) => t.vars(vars),
        }
    }
    fn code(&self) -> String {
        match self {
            Self::Equal(a, b) => format!("(= {} {})", a.code(), b.code()),
            Self::Predicate(t) => t.code(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub lhs: Term,
    pub rhs: Term,
    pub effects: Vec<(Term, Term)>,
    pub guards: Vec<Guard>,
}
impl Rule {
    pub fn new(name: String, lhs: Term, rhs: Term) -> Result<Self, String> {
        let mut l = BTreeSet::new();
        let mut r = BTreeSet::new();
        lhs.vars(&mut l);
        rhs.vars(&mut r);
        if lhs.sort() != rhs.sort() || !r.is_subset(&l) {
            return Err("rewrite changes sort or has unbound RHS variables".into());
        }
        Ok(Self {
            name,
            lhs: lhs.clone(),
            rhs: rhs.clone(),
            effects: vec![(lhs, rhs)],
            guards: vec![],
        })
    }
    pub fn egg(&self) -> String {
        let guards = self
            .guards
            .iter()
            .map(|g| format!(" {}", g.code()))
            .collect::<String>();
        let effects = self
            .effects
            .iter()
            .map(|(a, b)| format!("(union {} {})", a.code(), b.code()))
            .collect::<Vec<_>>()
            .join("\n    ");
        format!(
            "(rule ((= composition_root {}){})\n  ({}))",
            self.lhs.code(),
            guards,
            effects
        )
    }
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Substitution(pub BTreeMap<usize, Term>);
impl Substitution {
    pub fn apply(&self, t: &Term) -> Term {
        match t {
            Term::Var { id, .. } => self
                .0
                .get(id)
                .map(|v| self.apply(v))
                .unwrap_or_else(|| t.clone()),
            Term::App { op, args, sort } => Term::App {
                op: op.clone(),
                args: args.iter().map(|a| self.apply(a)).collect(),
                sort: sort.clone(),
            },
            _ => t.clone(),
        }
    }
    pub fn unify(&mut self, a: &Term, b: &Term) -> Result<(), String> {
        let mut next = self.clone();
        next.unify_inner(a, b)?;
        *self = next;
        Ok(())
    }
    fn unify_inner(&mut self, a: &Term, b: &Term) -> Result<(), String> {
        let a = self.apply(a);
        let b = self.apply(b);
        if a == b {
            return Ok(());
        }
        if a.sort() != b.sort() {
            return Err("sort mismatch".into());
        }
        match (&a, &b) {
            (Term::Var { id, .. }, t) | (t, Term::Var { id, .. }) => {
                let mut vs = BTreeSet::new();
                t.vars(&mut vs);
                if vs.contains(id) {
                    return Err("occurs check".into());
                }
                self.0.insert(*id, t.clone());
                Ok(())
            }
            (
                Term::App {
                    op: x, args: xs, ..
                },
                Term::App {
                    op: y, args: ys, ..
                },
            ) if x == y && xs.len() == ys.len() => {
                for (x, y) in xs.iter().zip(ys) {
                    self.unify_inner(x, y)?;
                }
                Ok(())
            }
            _ => Err("constructor or literal mismatch".into()),
        }
    }
}
fn at<'a>(t: &'a Term, path: &[usize]) -> Result<&'a Term, String> {
    if path.is_empty() {
        return Ok(t);
    }
    match t {
        Term::App { args, .. } => at(args.get(path[0]).ok_or("invalid position")?, &path[1..]),
        _ => Err("position crosses non-constructor".into()),
    }
}
fn replace(t: &Term, path: &[usize], v: &Term) -> Term {
    if path.is_empty() {
        return v.clone();
    }
    let Term::App { op, args, sort } = t else {
        unreachable!()
    };
    let mut args = args.clone();
    args[path[0]] = replace(&args[path[0]], &path[1..], v);
    Term::App {
        op: op.clone(),
        args,
        sort: sort.clone(),
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Composition {
    pub rule: Rule,
    pub substitution: Substitution,
    pub right_variable_offset: usize,
    pub position: Vec<usize>,
}
pub fn compose(a: &Rule, b: &Rule, position: &[usize]) -> Result<Composition, String> {
    let mut vars = BTreeSet::new();
    a.lhs.vars(&mut vars);
    a.rhs.vars(&mut vars);
    for (x, y) in &a.effects {
        x.vars(&mut vars);
        y.vars(&mut vars);
    }
    for g in &a.guards {
        g.vars(&mut vars);
    }
    let offset = vars.last().map_or(0, |v| v + 1);
    let lhs = b.lhs.shift(offset);
    let rhs = b.rhs.shift(offset);
    let mut substitution = Substitution::default();
    substitution.unify(at(&a.rhs, position)?, &lhs)?;
    let result = substitution.apply(&replace(&a.rhs, position, &rhs));
    let effects = a
        .effects
        .iter()
        .cloned()
        .chain(
            b.effects
                .iter()
                .map(|(x, y)| (x.shift(offset), y.shift(offset))),
        )
        .map(|(x, y)| (substitution.apply(&x), substitution.apply(&y)))
        .collect();
    let guards = a
        .guards
        .iter()
        .cloned()
        .chain(b.guards.iter().map(|g| g.map(|t| t.shift(offset))))
        .map(|g| g.map(|t| substitution.apply(t)))
        .collect();
    let rule = Rule {
        name: format!("{};{}", a.name, b.name),
        lhs: substitution.apply(&a.lhs),
        rhs: result,
        effects,
        guards,
    };
    Ok(Composition {
        rule,
        substitution,
        right_variable_offset: offset,
        position: position.to_vec(),
    })
}

/// Import positive constructor rewrites with bound equality/relation guards via egglog's own parser. Unsupported
/// commands never become fabricated semantic summaries.
pub fn import(source: &str) -> Result<BTreeMap<String, Rule>, String> {
    use egglog::ast::{Command, Expr};
    let mut eg = egglog::EGraph::default();
    let cmds = eg.parse_program(None, source).map_err(|e| e.to_string())?;
    let mut schema = BTreeMap::new();
    for c in &cmds {
        match c {
            Command::Datatype { name, variants, .. } => {
                for v in variants {
                    schema.insert(v.name.clone(), (v.types.clone(), name.clone()));
                }
            }
            Command::Relation { name, inputs, .. } => {
                schema.insert(name.clone(), (inputs.clone(), "Unit".into()));
            }
            _ => {}
        }
    }
    fn term(
        e: &Expr,
        expected: &str,
        schema: &BTreeMap<String, (Vec<String>, String)>,
        vars: &mut BTreeMap<String, (usize, String)>,
    ) -> Result<Term, String> {
        match e {
            Expr::Var(_, name) => {
                let n = vars.len();
                let (id, sort) = vars.entry(name.clone()).or_insert((n, expected.into()));
                if sort != expected {
                    return Err("variable sort mismatch".into());
                }
                Ok(Term::Var {
                    id: *id,
                    sort: sort.clone(),
                })
            }
            Expr::Lit(_, lit) => {
                use egglog::ast::Literal::*;
                let ty = match lit {
                    Int(_) => "i64",
                    Float(_) => "f64",
                    String(_) => "String",
                    Bool(_) => "bool",
                    Unit => "Unit",
                };
                if ty != expected {
                    return Err("literal sort mismatch".into());
                }
                Ok(Term::Lit {
                    code: lit.to_string(),
                    sort: expected.into(),
                })
            }
            Expr::Call(_, op, args) => {
                let (types, sort) = schema.get(op).ok_or("non-constructor expression")?;
                if sort != expected || types.len() != args.len() {
                    return Err("constructor sort/arity mismatch".into());
                }
                Ok(Term::App {
                    op: op.clone(),
                    sort: sort.clone(),
                    args: args
                        .iter()
                        .zip(types)
                        .map(|(a, t)| term(a, t, schema, vars))
                        .collect::<Result<_, _>>()?,
                })
            }
        }
    }
    let mut out = BTreeMap::new();
    let mut ordinal = 0;
    for c in cmds {
        match c {
            Command::Rewrite(_, r, subsumes) => {
                let name = if r.name.is_empty() {
                    format!("R{ordinal}")
                } else {
                    r.name.clone()
                };
                ordinal += 1;
                if subsumes {
                    continue;
                }
                let sort = match &r.lhs {
                    Expr::Call(_, op, _) => schema.get(op).map(|(_, s)| s.clone()),
                    _ => None,
                };
                if let Some(sort) = sort {
                    let mut vars = BTreeMap::new();
                    if let (Ok(lhs), Ok(rhs)) = (
                        term(&r.lhs, &sort, &schema, &mut vars),
                        term(&r.rhs, &sort, &schema, &mut vars),
                    ) {
                        if let Ok(mut rule) = Rule::new(name.clone(), lhs, rhs) {
                            let count = vars.len();
                            let mut good = true;
                            for guard in &r.conditions {
                                use egglog::ast::Fact;
                                let pair = match guard {
                                    Fact::Fact(e) => {
                                        term(e, "Unit", &schema, &mut vars).map(Guard::Predicate)
                                    }
                                    Fact::Eq(_, a, b) => {
                                        let ty = match a {
                                            Expr::Var(_, v) => vars.get(v).map(|(_, s)| s.clone()),
                                            Expr::Call(_, op, _) => {
                                                schema.get(op).map(|(_, s)| s.clone())
                                            }
                                            _ => None,
                                        };
                                        ty.ok_or("unresolved guard sort".to_string()).and_then(
                                            |ty| {
                                                Ok(Guard::Equal(
                                                    term(a, &ty, &schema, &mut vars)?,
                                                    term(b, &ty, &schema, &mut vars)?,
                                                ))
                                            },
                                        )
                                    }
                                };
                                match pair {
                                    Ok(pair) => rule.guards.push(pair),
                                    Err(_) => good = false,
                                }
                            }
                            if good && vars.len() == count {
                                out.insert(name, rule);
                            }
                        }
                    }
                }
            }
            Command::Rule { .. } => ordinal += 1,
            _ => {}
        }
    }
    Ok(out)
}
