use egglog::ast::{Expr, Literal, Parser};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pat {
    Var(String),
    App(String, Vec<Pat>),
}
impl fmt::Display for Pat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Var(v) => write!(f, "{v}"),
            Self::App(op, args) => {
                write!(f, "({op}")?;
                for a in args {
                    write!(f, " {a}")?;
                }
                write!(f, ")")
            }
        }
    }
}
impl Pat {
    pub fn parse(s: &str) -> Self {
        fn convert(e: Expr) -> Pat {
            match e {
                Expr::Var(_, v) => Pat::Var(v),
                Expr::Call(_, op, args) => Pat::App(op, args.into_iter().map(convert).collect()),
                Expr::Lit(_, Literal::Unit) => panic!("unit is not an E constructor"),
                _ => panic!("only E constructors and variables are supported"),
            }
        }
        convert(Parser::default().get_expr_from_string(None, s).unwrap())
    }
    fn rename(&self, p: &str) -> Self {
        match self {
            Self::Var(v) => Self::Var(format!("{p}{v}")),
            Self::App(op, a) => Self::App(op.clone(), a.iter().map(|a| a.rename(p)).collect()),
        }
    }
    pub fn vars(&self) -> BTreeSet<String> {
        match self {
            Self::Var(v) => BTreeSet::from([v.clone()]),
            Self::App(_, a) => a.iter().flat_map(Pat::vars).collect(),
        }
    }
    fn size(&self) -> usize {
        match self {
            Self::Var(_) => 1,
            Self::App(_, a) => 1 + a.iter().map(Pat::size).sum::<usize>(),
        }
    }
    pub fn at(&self, path: &[usize]) -> &Self {
        if path.is_empty() {
            self
        } else {
            match self {
                Self::App(_, a) => a[path[0]].at(&path[1..]),
                _ => panic!("bad path"),
            }
        }
    }
    fn replace(&self, path: &[usize], with: &Self) -> Self {
        if path.is_empty() {
            return with.clone();
        }
        match self {
            Self::App(op, a) => {
                let mut a = a.clone();
                a[path[0]] = a[path[0]].replace(&path[1..], with);
                Self::App(op.clone(), a)
            }
            _ => panic!("bad path"),
        }
    }
    fn paths(&self) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        if let Self::App(_, a) = self {
            out.push(vec![]);
            for (i, t) in a.iter().enumerate() {
                for p in t.paths() {
                    let mut q = vec![i];
                    q.extend(p);
                    out.push(q);
                }
            }
        }
        out
    }
}
type Sub = BTreeMap<String, Pat>;
fn subst(p: &Pat, s: &Sub) -> Pat {
    match p {
        Pat::Var(v) => s.get(v).map(|x| subst(x, s)).unwrap_or_else(|| p.clone()),
        Pat::App(op, a) => Pat::App(op.clone(), a.iter().map(|a| subst(a, s)).collect()),
    }
}
fn unify(a: &Pat, b: &Pat, s: &mut Sub) -> bool {
    let a = subst(a, s);
    let b = subst(b, s);
    if a == b {
        return true;
    }
    match (a, b) {
        (Pat::Var(v), p) | (p, Pat::Var(v)) => {
            if p.vars().contains(&v) {
                false
            } else {
                s.insert(v, p);
                true
            }
        }
        (Pat::App(f, a), Pat::App(g, b)) => {
            f == g && a.len() == b.len() && a.iter().zip(&b).all(|(a, b)| unify(a, b, s))
        }
    }
}
#[derive(Clone, Debug)]
pub struct Rule {
    pub name: String,
    pub lhs: Pat,
    pub rhs: Pat,
}
impl Rule {
    pub fn new(name: &str, l: &str, r: &str) -> Self {
        let lhs = Pat::parse(l);
        let rhs = Pat::parse(r);
        assert!(rhs.vars().is_subset(&lhs.vars()));
        Self {
            name: name.into(),
            lhs,
            rhs,
        }
    }
    pub fn command(&self, ruleset: &str) -> String {
        format!(
            "(rewrite {} {} :ruleset {ruleset} :name \"{}\")",
            self.lhs, self.rhs, self.name
        )
    }
}
#[derive(Clone, Debug)]
pub struct Candidate {
    pub first: usize,
    pub second: usize,
    pub path: Vec<usize>,
    pub rule: Rule,
    pub middle: Pat,
}
pub fn candidates(rules: &[Rule]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, a) in rules.iter().enumerate() {
        for (j, b) in rules.iter().enumerate() {
            let al = a.lhs.rename("a_");
            let ar = a.rhs.rename("a_");
            let bl = b.lhs.rename("b_");
            let br = b.rhs.rename("b_");
            for path in ar.paths() {
                let mut s = Sub::new();
                if !unify(ar.at(&path), &bl, &mut s) {
                    continue;
                }
                let lhs = subst(&al, &s);
                let middle = subst(&ar, &s);
                let rhs = middle.replace(&path, &subst(&br, &s));
                if lhs == rhs
                    || lhs.size() > 32
                    || rhs.size() > 32
                    || !rhs.vars().is_subset(&lhs.vars())
                {
                    continue;
                }
                // Canonical names across LHS, witness and RHS, with variables renamed apart before unification.
                let mut names = Sub::new();
                fn name(p: &Pat, n: &mut Sub) {
                    match p {
                        Pat::Var(v) => {
                            let len = n.len();
                            n.entry(v.clone()).or_insert(Pat::Var(format!("v{len}")));
                        }
                        Pat::App(_, a) => {
                            for x in a {
                                name(x, n)
                            }
                        }
                    }
                }
                name(&lhs, &mut names);
                name(&middle, &mut names);
                name(&rhs, &mut names);
                let lhs = subst(&lhs, &names);
                let middle = subst(&middle, &names);
                let rhs = subst(&rhs, &names);
                if !seen.insert((lhs.clone(), rhs.clone())) {
                    continue;
                }
                out.push(Candidate {
                    first: i,
                    second: j,
                    path,
                    rule: Rule {
                        name: format!("shortcut_{}", out.len()),
                        lhs,
                        rhs,
                    },
                    middle,
                });
            }
        }
    }
    out
}
pub fn rules(case: &str) -> Vec<Rule> {
    if case == "chain" {
        return (0..8)
            .map(|i| {
                Rule::new(
                    &format!("chain{i}"),
                    &format!("(S{i} x)"),
                    &format!("(S{} x)", i + 1),
                )
            })
            .collect();
    }
    vec![
        Rule::new(
            "distribute",
            "(Mul x (Add y z))",
            "(Add (Mul x y) (Mul x z))",
        ),
        Rule::new("mul-one", "(Mul x (One))", "x"),
        Rule::new("mul-zero", "(Mul x (Zero))", "(Zero)"),
        Rule::new("add-zero", "(Add x (Zero))", "x"),
        Rule::new("zero-add", "(Add (Zero) x)", "x"),
        Rule::new("double-neg", "(Neg (Neg x))", "x"),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occurs_check() {
        let mut s = Sub::new();
        assert!(!unify(&Pat::parse("x"), &Pat::parse("(F x)"), &mut s));
    }
    #[test]
    fn repeated_variable_rejects_incompatible_match() {
        let mut s = Sub::new();
        assert!(!unify(
            &Pat::parse("(F x x)"),
            &Pat::parse("(F (A) (B))"),
            &mut s
        ));
    }
    #[test]
    fn chain_composes() {
        let r = rules("chain");
        let c = candidates(&r);
        assert!(c.iter().any(|c|c.rule.lhs.to_string()=="(S0 v0)"&&c.rule.rhs.to_string()=="(S2 v0)"));
        assert_eq!(c.len(), 7);
    }
    #[test]
    fn nested_overlap_specializes_input() {
        let r = rules("algebra");
        let c = candidates(&r);
        assert!(c.iter().any(|c| c.path == vec![0]
            && c.rule.lhs.to_string().contains("(One)")
            && c.first == 0
            && c.second == 1));
    }
}
