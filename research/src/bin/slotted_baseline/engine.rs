use slotted_egraphs::*;
use std::collections::HashSet;

define_language! {
    pub enum Math {
        Var(Slot) = "Var",
        Num(u32),
        Add(AppliedId, AppliedId) = "Add",
        Mul(AppliedId, AppliedId) = "Mul",
        Neg(AppliedId) = "Neg",
        Lam(Bind<AppliedId>) = "Lam",
        App(AppliedId, AppliedId) = "App",
    }
}
#[derive(Clone, Debug)]
pub enum Term {
    Var(u32),
    Num(u32),
    Add(Box<Term>, Box<Term>),
    Mul(Box<Term>, Box<Term>),
    Neg(Box<Term>),
}
impl Term {
    pub fn text(&self, slots: bool) -> String {
        match self {
            Self::Var(v) => format!("(Var {}{v})", if slots { "$" } else { "" }),
            Self::Num(v) => {
                if slots {
                    v.to_string()
                } else {
                    format!("(Num {v})")
                }
            }
            Self::Add(a, b) => format!("(Add {} {})", a.text(slots), b.text(slots)),
            Self::Mul(a, b) => format!("(Mul {} {})", a.text(slots), b.text(slots)),
            Self::Neg(a) => format!("(Neg {})", a.text(slots)),
        }
    }
}
fn add(a: Term, b: Term) -> Term {
    Term::Add(Box::new(a), Box::new(b))
}
pub fn input(case: &str, i: u32) -> Term {
    let base = i * 4 + 1;
    let leaf = |n| {
        if case == "constants" {
            Term::Num(n)
        } else {
            Term::Var(n)
        }
    };
    if case == "ac" {
        add(
            add(leaf(base), leaf(base + 1)),
            add(leaf(base + 2), leaf(base + 3)),
        )
    } else {
        add(
            Term::Mul(Box::new(leaf(base)), Box::new(leaf(base + 1))),
            Term::Neg(Box::new(leaf(base))),
        )
    }
}
pub fn ac_terms(i: u32) -> Vec<Term> {
    fn trees(xs: &[u32]) -> Vec<Term> {
        if xs.len() == 1 {
            return vec![Term::Var(xs[0])];
        }
        let mut out = Vec::new();
        for k in 1..xs.len() {
            for a in trees(&xs[..k]) {
                for b in trees(&xs[k..]) {
                    out.push(add(a.clone(), b));
                }
            }
        }
        out
    }
    let mut out = Vec::new();
    let xs: Vec<_> = (i * 4 + 1..i * 4 + 5).collect();
    for &a in &xs {
        for &b in &xs {
            for &c in &xs {
                for &d in &xs {
                    if HashSet::from([a, b, c, d]).len() == 4 {
                        out.extend(trees(&[a, b, c, d]));
                    }
                }
            }
        }
    }
    out
}
pub fn rewrites() -> Vec<Rewrite<Math>> {
    vec![
        Rewrite::new("comm", "(Add ?a ?b)", "(Add ?b ?a)"),
        Rewrite::new("assoc", "(Add (Add ?a ?b) ?c)", "(Add ?a (Add ?b ?c))"),
        Rewrite::new("assoc-back", "(Add ?a (Add ?b ?c))", "(Add (Add ?a ?b) ?c)"),
    ]
}
const SETUP: &str = r#"
(datatype E (Var i64) (Num i64) (Add E E) (Mul E E) (Neg E))
(ruleset ac)
(rewrite (Add a b) (Add b a) :ruleset ac)
(rewrite (Add (Add a b) c) (Add a (Add b c)) :ruleset ac)
(rewrite (Add a (Add b c)) (Add (Add a b) c) :ruleset ac)
"#;
pub enum Engine {
    Slotted {
        graph: EGraph<Math>,
        roots: Vec<AppliedId>,
        rules: Vec<Rewrite<Math>>,
    },
    Egglog {
        graph: egglog::EGraph,
        roots: Vec<egglog::Value>,
    },
}
impl Engine {
    pub fn new(mode: &str) -> Self {
        match mode {
            "slotted" => Self::Slotted {
                graph: EGraph::default(),
                roots: Vec::new(),
                rules: rewrites(),
            },
            "egglog" => {
                let mut graph = egglog::EGraph::default();
                graph.parse_and_run_program(None, SETUP).unwrap();
                Self::Egglog {
                    graph,
                    roots: Vec::new(),
                }
            }
            _ => panic!("expected slotted or egglog"),
        }
    }
    pub fn insert(&mut self, t: &Term) {
        match self {
            Self::Slotted { graph, roots, .. } => {
                roots.push(graph.add_expr(RecExpr::parse(&t.text(true)).unwrap()))
            }
            Self::Egglog { graph, roots } => {
                let ast = graph
                    .parser
                    .get_expr_from_string(None, &t.text(false))
                    .unwrap();
                roots.push(graph.eval_expr(&ast).unwrap().1);
            }
        }
    }
    pub fn step(&mut self) -> bool {
        match self {
            Self::Slotted { graph, rules, .. } => apply_rewrites(graph, rules),
            Self::Egglog { graph, .. } => graph.step_rules("ac").unwrap().updated,
        }
    }
    pub fn counts(&self) -> (usize, usize, usize) {
        match self {
            Self::Slotted { graph, roots, .. } => {
                let root_ids: HashSet<_> =
                    roots.iter().map(|r| graph.find_applied_id(r).id).collect();
                (
                    graph.total_number_of_nodes(),
                    graph.ids().len(),
                    root_ids.len(),
                )
            }
            Self::Egglog { graph, roots } => {
                let sort = graph.get_sort_by_name("E").unwrap();
                let mut ids = HashSet::new();
                let mut nodes = 0;
                for name in ["Var", "Num", "Add", "Mul", "Neg"] {
                    graph
                        .function_for_each(name, |row| {
                            nodes += 1;
                            ids.insert(graph.get_canonical_value(*row.vals.last().unwrap(), sort));
                        })
                        .unwrap();
                }
                let root_ids: HashSet<_> = roots
                    .iter()
                    .map(|r| graph.get_canonical_value(*r, sort))
                    .collect();
                (nodes, ids.len(), root_ids.len())
            }
        }
    }
    pub fn root_matches(&self, i: usize, t: &Term) -> bool {
        match self {
            Self::Slotted { graph, roots, .. } => {
                lookup_rec_expr(&RecExpr::parse(&t.text(true)).unwrap(), graph)
                    .is_some_and(|found| graph.eq(&roots[i], &found))
            }
            Self::Egglog { graph, roots } => {
                let sort = graph.get_sort_by_name("E").unwrap();
                plain_lookup(graph, t).is_some_and(|v| {
                    graph.get_canonical_value(v, sort) == graph.get_canonical_value(roots[i], sort)
                })
            }
        }
    }
    pub fn root_maps(&self) -> usize {
        match self {
            Self::Slotted { roots, .. } => roots.iter().map(|r| r.m.len()).sum(),
            Self::Egglog { .. } => 0,
        }
    }
}
fn plain_lookup(eg: &egglog::EGraph, t: &Term) -> Option<egglog::Value> {
    let sort = eg.get_sort_by_name("E").unwrap();
    let v = match t {
        Term::Var(v) => eg.lookup_function("Var", &[eg.base_to_value(i64::from(*v))]),
        Term::Num(v) => eg.lookup_function("Num", &[eg.base_to_value(i64::from(*v))]),
        Term::Add(a, b) => eg.lookup_function("Add", &[plain_lookup(eg, a)?, plain_lookup(eg, b)?]),
        Term::Mul(a, b) => eg.lookup_function("Mul", &[plain_lookup(eg, a)?, plain_lookup(eg, b)?]),
        Term::Neg(a) => eg.lookup_function("Neg", &[plain_lookup(eg, a)?]),
    }?;
    Some(eg.get_canonical_value(v, sort))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_renaming_shares_classes_but_not_free_names() {
        let mut e = Engine::new("slotted");
        e.insert(&input("renamed", 0));
        let first = e.counts();
        e.insert(&input("renamed", 1));
        assert_eq!(e.counts().0, first.0);
        assert_eq!(e.counts().2, 1);
        assert!(e.root_matches(0, &input("renamed", 0)));
        assert!(!e.root_matches(0, &input("renamed", 1)));
        let wrong = add(
            Term::Mul(Box::new(Term::Var(1)), Box::new(Term::Var(2))),
            Term::Neg(Box::new(Term::Var(2))),
        );
        assert!(!e.root_matches(0, &wrong));
    }
    #[test]
    fn constants_are_not_slots() {
        let mut e = Engine::new("slotted");
        e.insert(&input("constants", 0));
        let a = e.counts().0;
        e.insert(&input("constants", 1));
        assert!(e.counts().0 > a);
        assert_eq!(e.counts().2, 2);
        assert!(!e.root_matches(0, &input("constants", 1)));
    }
    #[test]
    fn binder_alpha_equivalence_preserves_capture() {
        let mut e = EGraph::<Math>::default();
        let a = e.add_expr(RecExpr::parse("(Lam $1 (Add (Var $1) (Var $9)))").unwrap());
        let b = e.add_expr(RecExpr::parse("(Lam $2 (Add (Var $2) (Var $9)))").unwrap());
        let c = e.add_expr(RecExpr::parse("(Lam $2 (Add (Var $2) (Var $2)))").unwrap());
        assert!(e.eq(&a, &b));
        assert!(!e.eq(&a, &c));
    }
    #[test]
    fn both_engines_represent_all_120_ac_terms() {
        assert_eq!(ac_terms(0).len(), 120);
        for mode in ["slotted", "egglog"] {
            let mut e = Engine::new(mode);
            for i in 0..2 {
                e.insert(&input("ac", i));
            }
            let mut saturated = false;
            for _ in 0..32 {
                if !e.step() {
                    saturated = true;
                    break;
                }
            }
            assert!(saturated, "{mode} failed to saturate");
            for i in 0..2 {
                for t in ac_terms(i) {
                    assert!(e.root_matches(i as usize, &t), "{mode}: {t:?}");
                }
            }
            assert!(!e.root_matches(0, &input("ac", 1)));
            assert_eq!(e.counts().2, if mode == "slotted" { 1 } else { 2 });
        }
    }
}
