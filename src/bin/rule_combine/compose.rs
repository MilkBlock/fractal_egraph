use egglog::ast::{Expr, Literal, Parser};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pat {
    Var(String),
    Lit(Literal),
    App(String, Vec<Pat>),
}
impl fmt::Display for Pat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Var(v) => write!(f, "{v}"),
            Self::Lit(v) => write!(f, "{v}"),
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
                Expr::Lit(_, literal) => Pat::Lit(literal),
            }
        }
        convert(Parser::default().get_expr_from_string(None, s).unwrap())
    }
    fn rename(&self, p: &str) -> Self {
        match self {
            Self::Var(v) => Self::Var(format!("{p}{v}")),
            Self::Lit(_) => self.clone(),
            Self::App(op, a) => Self::App(op.clone(), a.iter().map(|a| a.rename(p)).collect()),
        }
    }
    pub fn vars(&self) -> BTreeSet<String> {
        match self {
            Self::Var(v) => BTreeSet::from([v.clone()]),
            Self::Lit(_) => BTreeSet::new(),
            Self::App(_, a) => a.iter().flat_map(Pat::vars).collect(),
        }
    }
    fn size(&self) -> usize {
        match self {
            Self::Var(_) | Self::Lit(_) => 1,
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
        Pat::Lit(_) => p.clone(),
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
        _ => false,
    }
}
#[derive(Clone, Debug)]
pub struct Rule {
    pub name: String,
    pub lhs: Pat,
    pub rhs: Pat,
    pub conditions: Vec<Pat>,
    pub stages: Vec<Pat>,
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
            conditions: vec![],
            stages: vec![],
        }
    }
    pub fn guarded(name: &str, lhs: &str, rhs: &str, conditions: &[&str]) -> Self {
        let mut rule = Self::new(name, lhs, rhs);
        rule.conditions = conditions.iter().map(|s| Pat::parse(s)).collect();
        assert!(
            rule.conditions
                .iter()
                .all(|p| p.vars().is_subset(&rule.lhs.vars()))
        );
        rule
    }
    pub fn command(&self, ruleset: &str) -> String {
        let conditions = self
            .conditions
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        if self.stages.is_empty() {
            let guard = if conditions.is_empty() {
                String::new()
            } else {
                format!(" :when ({conditions})")
            };
            return format!(
                "(rewrite {} {}{} :ruleset {ruleset} :name {:?})",
                self.lhs, self.rhs, guard, self.name
            );
        }
        let mut root = "_shortcut_root".to_string();
        while self.lhs.vars().contains(&root) {
            root.push('_');
        }
        let actions = self
            .stages
            .iter()
            .chain(std::iter::once(&self.rhs))
            .map(|stage| format!("(union {root} {stage})"))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "(rule ((= {root} {}) {conditions}) ({actions}) :ruleset {ruleset} :name {:?})",
            self.lhs, self.name
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
                let conditions: Vec<_> = a
                    .conditions
                    .iter()
                    .map(|p| subst(&p.rename("a_"), &s))
                    .chain(b.conditions.iter().map(|p| subst(&p.rename("b_"), &s)))
                    .collect();
                // Preserve intermediate evaluation, including primitive failure.
                // These shortcuts reduce repeated matching, not intermediate storage.
                let stages: Vec<_> = a
                    .stages
                    .iter()
                    .map(|p| subst(&p.rename("a_"), &s))
                    .chain(std::iter::once(middle.clone()))
                    .chain(
                        b.stages
                            .iter()
                            .map(|p| middle.replace(&path, &subst(&p.rename("b_"), &s))),
                    )
                    .collect();
                if lhs == rhs
                    || lhs.size() > 32
                    || rhs.size() > 32
                    || !rhs.vars().is_subset(&lhs.vars())
                    || conditions
                        .iter()
                        .chain(&stages)
                        .any(|p| !p.vars().is_subset(&lhs.vars()))
                {
                    continue;
                }
                // Canonical names across LHS, witness and RHS, with variables renamed apart before unification.
                let mut names = Sub::new();
                fn name(p: &Pat, n: &mut Sub) {
                    match p {
                        Pat::Lit(_) => {}
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
                for p in conditions.iter().chain(&stages) {
                    name(p, &mut names);
                }
                let lhs = subst(&lhs, &names);
                let middle = subst(&middle, &names);
                let rhs = subst(&rhs, &names);
                if !seen.insert((lhs.clone(), rhs.clone(), conditions.clone(), stages.clone())) {
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
                        conditions: conditions.iter().map(|p| subst(p, &names)).collect(),
                        stages: stages.iter().map(|p| subst(p, &names)).collect(),
                    },
                    middle,
                });
            }
        }
    }
    out
}
/// Compose a producer into a proper subtree of a consumer's LHS.
/// Example: mul-fold feeds a Const child of add-fold.
/// Kept separate from the legacy E-only observation matcher.
pub fn contextual_candidates(rules: &[Rule]) -> Vec<Candidate> {
    let mut result = Vec::new();
    for (i, a) in rules.iter().enumerate() {
        for (j, b) in rules.iter().enumerate() {
            let al = a.lhs.rename("producer_");
            let ar = a.rhs.rename("producer_");
            let bl = b.lhs.rename("consumer_");
            let br = b.rhs.rename("consumer_");
            for path in bl.paths().into_iter().filter(|p| !p.is_empty()) {
                let mut sub = Sub::new();
                if !unify(&ar, bl.at(&path), &mut sub) {
                    continue;
                }
                let left = Rule {
                    name: "lifted".into(),
                    lhs: subst(&bl.replace(&path, &al), &sub),
                    rhs: subst(&bl, &sub),
                    conditions: a
                        .conditions
                        .iter()
                        .map(|p| subst(&p.rename("producer_"), &sub))
                        .collect(),
                    stages: a
                        .stages
                        .iter()
                        .map(|p| subst(&bl.replace(&path, &p.rename("producer_")), &sub))
                        .collect(),
                };
                let right = Rule {
                    name: "consumer".into(),
                    lhs: subst(&bl, &sub),
                    rhs: subst(&br, &sub),
                    conditions: b
                        .conditions
                        .iter()
                        .map(|p| subst(&p.rename("consumer_"), &sub))
                        .collect(),
                    stages: b
                        .stages
                        .iter()
                        .map(|p| subst(&p.rename("consumer_"), &sub))
                        .collect(),
                };
                for mut candidate in candidates(&[left, right])
                    .into_iter()
                    .filter(|c| c.first == 0 && c.second == 1 && c.path.is_empty())
                {
                    candidate.first = i;
                    candidate.second = j;
                    candidate.rule.name = format!("context_shortcut_{}", result.len());
                    result.push(candidate);
                }
            }
        }
    }
    result
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

#[cfg(test)]
mod typed_tests {
    use super::*;
    use egglog::EGraph;
    #[test]
    fn literal_types_and_strings_are_preserved() {
        assert_ne!(Pat::parse("1"), Pat::parse("1.0"));
        assert_ne!(Pat::parse("1"), Pat::parse("\"1\""));
        assert_eq!(Pat::parse("(Const 0)").to_string(), "(Const 0)");
    }
    #[test]
    fn native_contextual_constant_folding_preserves_guards_and_overflow() {
        let rules = [
            Rule::new("mul", "(Mul (Const a) (Const b))", "(Const (* a b))"),
            Rule::guarded(
                "add",
                "(Add (Const a) (Const b))",
                "(Const (+ a b))",
                &["(>= b 0)"],
            ),
        ];
        let candidate = contextual_candidates(&rules)
            .into_iter()
            .find(|c| c.first == 0 && c.second == 1)
            .unwrap();
        for (seed, check, should_fail) in [
            (
                "(Add (Mul (Const 2) (Const 3)) (Const 20))",
                "(check (= seed (Const 26)))",
                false,
            ),
            (
                "(Add (Mul (Const 2) (Const 3)) (Const -1))",
                "(fail (check (= seed (Const 5))))",
                false,
            ),
            (
                "(Add (Mul (Const 9223372036854775807) (Const 2)) (Const 0))",
                "",
                true,
            ),
        ] {
            let mut eg = EGraph::default();
            eg.parse_and_run_program(
                None,
                "(datatype E (Const i64) (Mul E E) (Add E E)) (ruleset shortcut)",
            )
            .unwrap();
            eg.parse_and_run_program(None, &candidate.rule.command("shortcut"))
                .unwrap();
            eg.parse_and_run_program(None, &format!("(let seed {seed})"))
                .unwrap();
            let run = eg.parse_and_run_program(None, "(run shortcut 1)");
            assert_eq!(run.is_err(), should_fail, "{run:?}");
            if !should_fail {
                eg.parse_and_run_program(None, check).unwrap();
            }
        }
    }
}
