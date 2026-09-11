//! Typed binding relations and lossless, syntactic component extraction.
//! Constructor terms denote witnessed materialized values, not injective functions.
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Term {
    pub sort: String,
    pub op: String,
    pub args: Vec<Term>,
}
impl Term {
    pub fn new(sort: impl Into<String>, op: impl Into<String>, args: Vec<Self>) -> Self {
        Self {
            sort: sort.into(),
            op: op.into(),
            args,
        }
    }
    pub fn size(&self) -> usize {
        1 + self.args.iter().map(Self::size).sum::<usize>()
    }
    pub fn json(&self) -> Value {
        json!({"sort":self.sort,"op":self.op,"args":self.args.iter().map(Self::json).collect::<Vec<_>>()})
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Pattern {
    Hole(usize, String),
    Node(String, String, Vec<Pattern>),
}
impl Pattern {
    pub fn size(&self) -> usize {
        match self {
            Self::Hole(..) => 1,
            Self::Node(_, _, a) => 1 + a.iter().map(Self::size).sum::<usize>(),
        }
    }
    pub fn json(&self) -> Value {
        match self {
            Self::Hole(i, s) => json!({"hole":i,"sort":s}),
            Self::Node(s, o, a) => {
                json!({"sort":s,"op":o,"args":a.iter().map(Self::json).collect::<Vec<_>>()})
            }
        }
    }
}
pub fn anti_unify(a: &Term, b: &Term) -> Option<Pattern> {
    fn go(a: &Term, b: &Term, holes: &mut BTreeMap<(Term, Term), usize>) -> Option<Pattern> {
        if a.sort != b.sort {
            return None;
        }
        if a.op == b.op && a.args.len() == b.args.len() {
            Some(Pattern::Node(
                a.sort.clone(),
                a.op.clone(),
                a.args
                    .iter()
                    .zip(&b.args)
                    .map(|(x, y)| go(x, y, holes))
                    .collect::<Option<_>>()?,
            ))
        } else {
            let n = holes.len();
            let i = *holes.entry((a.clone(), b.clone())).or_insert(n);
            Some(Pattern::Hole(i, a.sort.clone()))
        }
    }
    go(a, b, &mut BTreeMap::new())
}
fn matches(p: &Pattern, t: &Term, env: &mut BTreeMap<usize, Term>) -> bool {
    match p {
        Pattern::Hole(i, s) => {
            s == &t.sort
                && match env.get(i) {
                    Some(old) => old == t,
                    None => {
                        env.insert(*i, t.clone());
                        true
                    }
                }
        }
        Pattern::Node(s, o, a) => {
            s == &t.sort
                && o == &t.op
                && a.len() == t.args.len()
                && a.iter().zip(&t.args).all(|(p, t)| matches(p, t, env))
        }
    }
}
fn instantiate(p: &Pattern, args: &[Term]) -> Term {
    match p {
        Pattern::Hole(i, s) => {
            assert_eq!(&args[*i].sort, s);
            args[*i].clone()
        }
        Pattern::Node(s, o, a) => Term::new(s, o, a.iter().map(|p| instantiate(p, args)).collect()),
    }
}
fn replace(p: &Pattern, t: &Term, id: usize) -> (Term, usize) {
    let mut env = BTreeMap::new();
    if matches(p, t, &mut env) {
        let args: Vec<_> = env.into_values().collect();
        let call = Term::new(&t.sort, format!("call:F{id}"), args);
        if call.size() < t.size() {
            return (call, 1);
        }
    }
    let mut uses = 0;
    let args = t
        .args
        .iter()
        .map(|a| {
            let (r, n) = replace(p, a, id);
            uses += n;
            r
        })
        .collect();
    (Term::new(&t.sort, &t.op, args), uses)
}
pub fn expand(t: &Term, defs: &[Pattern]) -> Term {
    let args: Vec<_> = t.args.iter().map(|a| expand(a, defs)).collect();
    if let Some(id) = t.op.strip_prefix("call:F") {
        let id: usize = id.parse().unwrap();
        expand(&instantiate(&defs[id], &args), &defs[..id])
    } else {
        Term::new(&t.sort, &t.op, args)
    }
}
/// Deterministic bounded candidate generation. Cost unit is an AST node, not bytes.
pub fn learn(corpus: &[Term], max_defs: usize) -> (Vec<Pattern>, Vec<Term>, Vec<Value>) {
    learn_with_policy(corpus, max_defs, true)
}
pub fn learn_with_policy(
    corpus: &[Term],
    max_defs: usize,
    meaningful: bool,
) -> (Vec<Pattern>, Vec<Term>, Vec<Value>) {
    fn wiring(p: &Pattern) -> bool {
        fn scan(p: &Pattern, holes: &mut BTreeMap<usize, usize>, refs: &mut usize) -> bool {
            match p {
                Pattern::Hole(i, s) => {
                    if !["Sort", "Signature", "Ops", "BindingMap"].contains(&s.as_str()) {
                        *holes.entry(*i).or_default() += 1;
                    }
                    false
                }
                Pattern::Node(_, op, args) => {
                    if op.starts_with("in:") || op.starts_with("out:") || op.starts_with("assign:")
                    {
                        *refs += 1;
                    }
                    let mut result = op.starts_with("materialized:");
                    for a in args {
                        result |= scan(a, holes, refs);
                    }
                    result
                }
            }
        }
        if matches!(p,Pattern::Node(sort,_,_)if sort=="Sort"||sort=="Signature") {
            return false;
        }
        let mut holes = BTreeMap::new();
        let mut refs = 0;
        let constructor = scan(p, &mut holes, &mut refs);
        constructor || refs >= 2 && has_input(p) || holes.values().any(|n| *n > 1)
    }
    fn has_input(p: &Pattern) -> bool {
        match p {
            Pattern::Hole(..) => false,
            Pattern::Node(_, op, a) => {
                op.starts_with("in:") || op.starts_with("out:") || a.iter().any(has_input)
            }
        }
    }

    let mut trees = corpus.to_vec();
    let mut defs = Vec::new();
    let mut log = Vec::new();
    for _ in 0..max_defs {
        fn visit(t: &Term, set: &mut std::collections::BTreeSet<Term>) {
            if t.size() >= 3 {
                set.insert(t.clone());
            }
            for a in &t.args {
                visit(a, set);
            }
        }
        let mut set = std::collections::BTreeSet::new();
        for t in &trees {
            visit(t, &mut set);
        }
        let terms: Vec<_> = set.into_iter().collect();
        let mut candidates = std::collections::BTreeSet::new();
        let mut attempts = 0;
        'outer: for (i, a) in terms.iter().enumerate() {
            for b in &terms[i..] {
                if a.sort != b.sort || a.op != b.op || a.args.len() != b.args.len() {
                    continue;
                }
                attempts += 1;
                if attempts > 20000 {
                    break 'outer;
                }
                if let Some(p) = anti_unify(a, b) {
                    if p.size() > 2 && (!meaningful || wiring(&p)) {
                        candidates.insert(p);
                    }
                }
            }
        }
        let before: usize = trees.iter().map(Term::size).sum();
        let mut best = None;
        let mut gain = 0;
        for p in candidates {
            let rewritten: Vec<_> = trees.iter().map(|t| replace(&p, t, defs.len())).collect();
            let uses: usize = rewritten.iter().map(|(_, n)| *n).sum();
            if uses < 2 {
                continue;
            }
            let after: usize =
                rewritten.iter().map(|(t, _)| t.size()).sum::<usize>() + 1 + p.size();
            if after < before && before - after > gain {
                gain = before - after;
                best = Some((p, rewritten, uses, after));
            }
        }
        let Some((p, rewritten, uses, after)) = best else {
            break;
        };
        log.push(json!({"id":format!("F{}",defs.len()),"uses_in_unique_map_corpus":uses,"ast_node_gain_after_definition_cost":gain,"before_corpus_nodes":before,"after_corpus_plus_new_definition_nodes":after,"definition_nodes":1+p.size(),"candidate_pair_attempts":attempts.min(20000)}));
        defs.push(p);
        trees = rewritten.into_iter().map(|(t, _)| t).collect();
        for (old, new) in corpus.iter().zip(&trees) {
            assert_eq!(*old, expand(new, &defs));
        }
    }
    (defs, trees, log)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn leaf(n: &str) -> Term {
        Term::new("Math", n, vec![])
    }
    #[test]
    fn shared_holes_preserve_alias_constraints() {
        let a = Term::new("Math", "materialized:Add", vec![leaf("a"), leaf("a")]);
        let b = Term::new("Math", "materialized:Add", vec![leaf("b"), leaf("b")]);
        let p = anti_unify(&a, &b).unwrap();
        assert!(!matches(
            &p,
            &Term::new("Math", "materialized:Add", vec![leaf("a"), leaf("b")]),
            &mut BTreeMap::new()
        ));
    }
    #[test]
    fn type_mismatch_is_not_generalized() {
        assert!(anti_unify(&leaf("x"), &Term::new("i64", "x", vec![])).is_none());
    }
    #[test]
    fn learned_calls_expand_exactly() {
        let corpus: Vec<_> = (0..6)
            .map(|i| {
                Term::new(
                    "Map",
                    "long-map",
                    vec![
                        Term::new(
                            "Math",
                            "materialized:Add",
                            vec![leaf("a"), leaf(&format!("x{i}"))],
                        ),
                        Term::new("Math", "materialized:Mul", vec![leaf("a"), leaf("b")]),
                    ],
                )
            })
            .collect();
        let (d, c, _) = learn(&corpus, 4);
        assert!(!d.is_empty());
        for (a, b) in corpus.iter().zip(&c) {
            assert_eq!(*a, expand(b, &d));
        }
    }
}

/// Conservative normalization: e-class equality does not imply constructor injectivity.
pub fn normalize_equations(equations: &[(Term, Term)]) -> (BTreeMap<usize, Term>, Vec<Term>) {
    fn eq(a: Term, b: Term) -> Term {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        Term::new("Constraint", "Eq", vec![a, b])
    }
    let mut assignments = BTreeMap::<usize, Term>::new();
    let mut residual = std::collections::BTreeSet::new();
    for (p, c) in equations {
        assert_eq!(p.sort, c.sort);
        if let Some(i) = c.op.strip_prefix("out:") {
            let i = i.parse::<usize>().unwrap();
            if let Some(old) = assignments.get(&i) {
                if old != p {
                    residual.insert(eq(old.clone(), p.clone()));
                }
            } else {
                assignments.insert(i, p.clone());
            }
        } else if p != c {
            residual.insert(eq(p.clone(), c.clone()));
        }
    }
    (assignments, residual.into_iter().collect())
}
fn route(t: &Term) -> Option<(Vec<String>, Vec<String>, Vec<usize>)> {
    if t.sort != "BindingMap" || t.op != "binding-map" || t.args.len() != 3 {
        return None;
    }
    if t.args[0].sort != "Signature"
        || t.args[0].op != "inputs"
        || t.args[1].sort != "Signature"
        || t.args[1].op != "outputs"
        || t.args[2].sort != "Ops"
        || t.args[2].op != "ops"
    {
        return None;
    }
    if t.args[0]
        .args
        .iter()
        .chain(&t.args[1].args)
        .any(|s| s.sort != "Sort" || !s.args.is_empty())
    {
        return None;
    }
    let ins: Vec<_> = t.args[0].args.iter().map(|a| a.op.clone()).collect();
    let outs: Vec<_> = t.args[1].args.iter().map(|a| a.op.clone()).collect();
    if ins.iter().chain(&outs).any(|s| s == "context") {
        return None;
    }
    let mut map = vec![None; outs.len()];
    for a in &t.args[2].args {
        let i = a.op.strip_prefix("assign:")?.parse::<usize>().ok()?;
        if a.sort != "Assignment" || a.args.len() != 1 || !a.args[0].args.is_empty() {
            return None;
        }
        let j = a.args[0].op.strip_prefix("in:")?.parse::<usize>().ok()?;
        if i >= outs.len()
            || j >= ins.len()
            || outs[i] != ins[j]
            || a.args[0].sort != outs[i]
            || map[i].is_some()
        {
            return None;
        }
        map[i] = Some(j);
    }
    Some((ins, outs, map.into_iter().collect::<Option<_>>()?))
}
/// Compose only total port routing. Materialization, residual reads and guards are
/// intentionally excluded; this proves a wiring law, not an egraph-state rewrite.
pub fn compose_routes(a: &Term, b: &Term) -> Option<Term> {
    let (inputs, middle, ar) = route(a)?;
    let (required, outputs, br) = route(b)?;
    if middle != required {
        return None;
    }
    let ops = outputs
        .iter()
        .zip(br)
        .enumerate()
        .map(|(i, (s, j))| {
            Term::new(
                "Assignment",
                format!("assign:{i}"),
                vec![Term::new(s, format!("in:{}", ar[j]), vec![])],
            )
        })
        .collect();
    Some(Term::new(
        "BindingMap",
        "binding-map",
        vec![
            Term::new(
                "Signature",
                "inputs",
                inputs
                    .iter()
                    .map(|s| Term::new("Sort", s, vec![]))
                    .collect(),
            ),
            Term::new(
                "Signature",
                "outputs",
                outputs
                    .iter()
                    .map(|s| Term::new("Sort", s, vec![]))
                    .collect(),
            ),
            Term::new("Ops", "ops", ops),
        ],
    ))
}
pub fn is_identity_route(t: &Term) -> bool {
    route(t).is_some_and(|(a, b, r)| a == b && r.iter().enumerate().all(|(i, j)| i == *j))
}
#[cfg(test)]
mod relation_tests {
    use super::*;
    fn input(i: usize) -> Term {
        Term::new("Math", format!("in:{i}"), vec![])
    }
    #[test]
    fn equal_constructors_are_not_decomposed() {
        let a = Term::new("Math", "materialized:Add", vec![input(0), input(1)]);
        let b = Term::new(
            "Math",
            "materialized:Add",
            vec![
                Term::new("Math", "out:0", vec![]),
                Term::new("Math", "out:1", vec![]),
            ],
        );
        let (assign, guards) = normalize_equations(&[(a, b)]);
        assert!(assign.is_empty());
        assert_eq!(guards.len(), 1);
    }
    #[test]
    fn repeated_destination_preserves_equality_guard() {
        let c = Term::new("Math", "out:0", vec![]);
        let (a, g) = normalize_equations(&[(input(0), c.clone()), (input(1), c)]);
        assert_eq!(a.len(), 1);
        assert_eq!(g.len(), 1);
    }
    #[test]
    fn swap_composed_with_swap_is_wiring_identity() {
        let sig = |name| {
            Term::new(
                "Signature",
                name,
                vec![Term::new("Sort", "Math", vec![]); 2],
            )
        };
        let swap = Term::new(
            "BindingMap",
            "binding-map",
            vec![
                sig("inputs"),
                sig("outputs"),
                Term::new(
                    "Ops",
                    "ops",
                    vec![
                        Term::new("Assignment", "assign:0", vec![input(1)]),
                        Term::new("Assignment", "assign:1", vec![input(0)]),
                    ],
                ),
            ],
        );
        assert!(is_identity_route(&compose_routes(&swap, &swap).unwrap()));
    }
}
