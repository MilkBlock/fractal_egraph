//! Conservative monotone effect summaries over fixed, selected-node interfaces.
//! Equality of summaries ignores timestamps/provenance, not graph facts.
use crate::binding_program::Term;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Fact {
    Node(Term),
    Relation(String, Vec<Term>),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Summary {
    pub entry: BTreeMap<String, Term>,
    pub exit: BTreeMap<String, Term>,
    pub requires: BTreeSet<Fact>,
    pub adds: BTreeSet<Fact>,
    pub equalities: BTreeSet<(Term, Term)>,
}
fn nodes(t: &Term, out: &mut BTreeSet<Fact>) {
    if t.op.starts_with("in:") || t.op.starts_with("literal:") {
        return;
    }
    for a in &t.args {
        nodes(a, out);
    }
    out.insert(Fact::Node(t.clone()));
}
fn pair(a: Term, b: Term) -> (Term, Term) {
    assert_eq!(a.sort, b.sort);
    if a <= b { (a, b) } else { (b, a) }
}
fn substitute(t: &Term, s: &BTreeMap<String, Term>) -> Result<Term, String> {
    if t.op.starts_with("in:") {
        return s
            .get(&t.op)
            .cloned()
            .ok_or_else(|| format!("unbound interface port {}", t.op));
    }
    Ok(Term::new(
        &t.sort,
        &t.op,
        t.args
            .iter()
            .map(|a| substitute(a, s))
            .collect::<Result<_, _>>()?,
    ))
}
fn match_focus(pattern: &Term, actual: &Term, s: &mut BTreeMap<String, Term>) -> bool {
    if pattern.sort != actual.sort {
        return false;
    }
    if pattern.op.starts_with("in:") {
        return match s.get(&pattern.op) {
            Some(old) => old == actual,
            None => {
                s.insert(pattern.op.clone(), actual.clone());
                true
            }
        };
    }
    pattern.op == actual.op
        && pattern.args.len() == actual.args.len()
        && pattern
            .args
            .iter()
            .zip(&actual.args)
            .all(|(p, a)| match_focus(p, a, s))
}
fn subst_fact(f: &Fact, s: &BTreeMap<String, Term>) -> Result<Fact, String> {
    Ok(match f {
        Fact::Node(t) => Fact::Node(substitute(t, s)?),
        Fact::Relation(n, a) => Fact::Relation(
            n.clone(),
            a.iter()
                .map(|t| substitute(t, s))
                .collect::<Result<_, _>>()?,
        ),
    })
}
fn equality_closure(edges: &BTreeSet<(Term, Term)>) -> BTreeSet<(Term, Term)> {
    let mut adjacent = BTreeMap::<Term, BTreeSet<Term>>::new();
    for (a, b) in edges {
        assert_eq!(a.sort, b.sort);
        adjacent.entry(a.clone()).or_default().insert(b.clone());
        adjacent.entry(b.clone()).or_default().insert(a.clone());
    }
    let mut out = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for start in adjacent.keys() {
        if visited.contains(start) {
            continue;
        }
        let mut todo = vec![start.clone()];
        let mut component = BTreeSet::new();
        while let Some(n) = todo.pop() {
            if !component.insert(n.clone()) {
                continue;
            }
            for next in adjacent.get(&n).into_iter().flatten() {
                todo.push(next.clone());
            }
        }
        let root = component.first().unwrap().clone();
        for t in &component {
            if *t != root {
                out.insert((root.clone(), t.clone()));
            }
        }
        visited.extend(component);
    }
    out // canonical spanning stars, not every pair in an equivalence class
}

impl Summary {
    pub fn identity(entry: BTreeMap<String, Term>) -> Self {
        let mut requires = BTreeSet::new();
        for t in entry.values() {
            nodes(t, &mut requires);
        }
        Self {
            exit: entry.clone(),
            entry,
            requires,
            adds: BTreeSet::new(),
            equalities: BTreeSet::new(),
        }
    }
    pub fn swap(entry: BTreeMap<String, Term>, slot: &str) -> Result<Self, String> {
        let mut s = Self::identity(entry);
        let before = s.entry.get(slot).ok_or("unknown focus slot")?.clone();
        if before.op != "Add" || before.args.len() != 2 {
            return Err("swap requires a selected binary Add node".into());
        }
        let after = Term::new(
            &before.sort,
            "Add",
            vec![before.args[1].clone(), before.args[0].clone()],
        );
        nodes(&after, &mut s.adds);
        s.equalities.insert(pair(before, after.clone()));
        s.exit.insert(slot.into(), after);
        Ok(s.normalized())
    }
    pub fn grow(entry: BTreeMap<String, Term>, slot: &str) -> Result<Self, String> {
        let mut s = Self::identity(entry);
        let before = s.entry.get(slot).ok_or("unknown focus slot")?.clone();
        s.requires
            .insert(Fact::Relation("Seen".into(), vec![before.clone()]));
        let after = Term::new(before.sort.clone(), "Step", vec![before]);
        nodes(&after, &mut s.adds);
        s.adds
            .insert(Fact::Relation("Seen".into(), vec![after.clone()]));
        s.exit.insert(slot.into(), after);
        Ok(s.normalized())
    }
    pub fn then(&self, next: &Self) -> Result<Self, String> {
        if self.exit.keys().collect::<Vec<_>>() != next.entry.keys().collect::<Vec<_>>() {
            return Err("different focus interfaces; explicit framing is required".into());
        }
        let mut subst = BTreeMap::new();
        for (slot, p) in &next.entry {
            if !match_focus(p, &self.exit[slot], &mut subst) {
                return Err("next selected-node pattern requires additional matching".into());
            }
        }
        let available: BTreeSet<_> = self.requires.union(&self.adds).cloned().collect();
        let mut result = self.clone();
        for f in &next.requires {
            let f = subst_fact(f, &subst)?;
            if !available.contains(&f) {
                result.requires.insert(f);
            }
        }
        for f in &next.adds {
            result.adds.insert(subst_fact(f, &subst)?);
        }
        for (a, b) in &next.equalities {
            result
                .equalities
                .insert(pair(substitute(a, &subst)?, substitute(b, &subst)?));
        }
        result.exit = next
            .exit
            .iter()
            .map(|(k, v)| Ok((k.clone(), substitute(v, &subst)?)))
            .collect::<Result<_, String>>()?;
        Ok(result.normalized())
    }
    pub fn normalized(mut self) -> Self {
        self.adds.retain(|f| !self.requires.contains(f));
        self.equalities = equality_closure(&self.equalities);
        self
    }
    /// A sufficient structural equality test; false is not a complete inequivalence proof.
    pub fn same_effect_and_focus(&self, other: &Self) -> bool {
        self.clone().normalized() == other.clone().normalized()
    }
    pub fn same_graph_effect(&self, other: &Self) -> bool {
        let mut a = self.clone().normalized();
        let mut b = other.clone().normalized();
        a.exit.clear();
        b.exit.clear();
        a == b
    }
    /// Absorption under the same declared interface, including final focus.
    pub fn absorbs(&self, next: &Self) -> Result<bool, String> {
        Ok(self.then(next)?.same_effect_and_focus(self))
    }
    /// Weaker observation that intentionally forgets which enode is selected.
    pub fn absorbs_graph(&self, next: &Self) -> Result<bool, String> {
        Ok(self.then(next)?.same_graph_effect(self))
    }
    pub fn equivalent_terms(&self, a: &Term, b: &Term) -> bool {
        if a == b {
            return true;
        }
        let edges = equality_closure(&self.equalities);
        let root = |t: &Term| {
            edges
                .iter()
                .find(|(_, child)| child == t)
                .map(|(root, _)| root.clone())
                .unwrap_or_else(|| t.clone())
        };
        root(a) == root(b)
    }
    pub fn all_facts(&self) -> BTreeSet<Fact> {
        self.requires.union(&self.adds).cloned().collect()
    }
    pub fn binding_route(&self) -> Option<Term> {
        fn refs(t: &Term, map: &mut BTreeMap<usize, String>) -> Option<()> {
            if let Some(i) = t.op.strip_prefix("in:") {
                map.insert(i.parse().ok()?, t.sort.clone());
            }
            for a in &t.args {
                refs(a, map)?;
            }
            Some(())
        }
        let mut ins = BTreeMap::new();
        for t in self.entry.values() {
            refs(t, &mut ins)?;
        }
        if ins.keys().copied().ne(0..ins.len()) {
            return None;
        }
        let mut outputs = Vec::new();
        for (slot, t) in &self.exit {
            let entry = &self.entry[slot];
            if t.op != entry.op || t.args.len() != entry.args.len() {
                return None;
            }
            for a in &t.args {
                if !a.op.starts_with("in:") {
                    return None;
                }
                outputs.push(a.clone());
            }
        }
        Some(Term::new(
            "BindingMap",
            "binding-map",
            vec![
                Term::new(
                    "Signature",
                    "inputs",
                    ins.values().map(|s| Term::new("Sort", s, vec![])).collect(),
                ),
                Term::new(
                    "Signature",
                    "outputs",
                    outputs
                        .iter()
                        .map(|a| Term::new("Sort", &a.sort, vec![]))
                        .collect(),
                ),
                Term::new(
                    "Ops",
                    "ops",
                    outputs
                        .into_iter()
                        .enumerate()
                        .map(|(i, t)| Term::new("Assignment", format!("assign:{i}"), vec![t]))
                        .collect(),
                ),
            ],
        ))
    }
    pub fn json(&self) -> Value {
        let facts = |fs: &BTreeSet<Fact>| {
            fs.iter()
                .map(|f| match f {
                    Fact::Node(t) => json!({"node":t.json()}),
                    Fact::Relation(n, a) => {
                        json!({"relation":n,"args":a.iter().map(Term::json).collect::<Vec<_>>()})
                    }
                })
                .collect::<Vec<_>>()
        };
        json!({"entry_focus":self.entry.iter().map(|(k,t)|(k,t.json())).collect::<BTreeMap<_,_>>(),"exit_focus":self.exit.iter().map(|(k,t)|(k,t.json())).collect::<BTreeMap<_,_>>(),"requires":facts(&self.requires),"adds":facts(&self.adds),"equalities":self.equalities.iter().map(|(a,b)|json!([a.json(),b.json()])).collect::<Vec<_>>(),"binding_route":self.binding_route().map(|r|r.json())})
    }
}
pub fn port(i: usize) -> Term {
    Term::new("Math", format!("in:{i}"), vec![])
}
pub fn swap_interface() -> BTreeMap<String, Term> {
    BTreeMap::from([
        (
            "left".into(),
            Term::new("Math", "Add", vec![port(0), port(1)]),
        ),
        (
            "right".into(),
            Term::new("Math", "Add", vec![port(2), port(3)]),
        ),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_swap_effect_algebra() {
        let interface = swap_interface();
        let id = Summary::identity(interface.clone());
        let s = Summary::swap(interface, "left").unwrap();
        let e = s.then(&s).unwrap();
        assert!(!e.same_effect_and_focus(&id));
        assert!(e.then(&e).unwrap().same_effect_and_focus(&e));
        assert!(e.then(&s).unwrap().same_effect_and_focus(&s));
        assert!(s.same_graph_effect(&e));
        assert!(e.absorbs(&e).unwrap());
        assert!(!e.absorbs(&s).unwrap());
        assert!(e.absorbs_graph(&s).unwrap());
        assert!(crate::binding_program::is_identity_route(
            &e.binding_route().unwrap()
        ));
    }
    #[test]
    fn positions_and_monotonicity_do_not_imply_absorption() {
        let i = swap_interface();
        let s = Summary::swap(i.clone(), "left").unwrap();
        let r = Summary::swap(i, "right").unwrap();
        let ss = s.then(&s).unwrap();
        assert!(
            !ss.then(&r)
                .unwrap()
                .then(&r)
                .unwrap()
                .same_effect_and_focus(&ss)
        );
        let g = Summary::grow(BTreeMap::from([("cursor".into(), port(0))]), "cursor").unwrap();
        let g2 = g.then(&g).unwrap();
        assert!(!g2.same_effect_and_focus(&g2.then(&g2).unwrap()));
    }
    #[test]
    fn substitution_is_simultaneous() {
        let s = Summary::swap(swap_interface(), "left").unwrap();
        let s2 = s.then(&s).unwrap();
        assert_eq!(s2.entry, s2.exit);
    }
}

pub struct Closure {
    pub states: Vec<Summary>,
    pub transitions: Vec<(usize, usize, usize)>,
    pub closed: bool,
    pub rejected: Vec<(usize, usize, String)>,
}
/// Explore summary normal forms. Hitting the bound means unknown, not divergence.
pub fn closure(initial: Summary, generators: &[Summary], limit: usize) -> Closure {
    assert!(limit > 0);
    let mut result = Closure {
        states: vec![initial.normalized()],
        transitions: vec![],
        closed: false,
        rejected: vec![],
    };
    let mut cursor = 0;
    while cursor < result.states.len() {
        for (g, generator) in generators.iter().enumerate() {
            match result.states[cursor].then(generator) {
                Ok(next) => {
                    let id = if let Some(i) = result
                        .states
                        .iter()
                        .position(|s| s.same_effect_and_focus(&next))
                    {
                        i
                    } else {
                        if result.states.len() >= limit {
                            return result;
                        }
                        let i = result.states.len();
                        result.states.push(next);
                        i
                    };
                    result.transitions.push((cursor, g, id));
                }
                Err(reason) => result.rejected.push((cursor, g, reason)),
            }
        }
        cursor += 1;
    }
    result.closed = result.rejected.is_empty();
    result
}
#[cfg(test)]
mod closure_tests {
    use super::*;
    #[test]
    fn independent_swaps_form_nine_effect_states() {
        let i = swap_interface();
        let l = Summary::swap(i.clone(), "left").unwrap();
        let r = Summary::swap(i.clone(), "right").unwrap();
        assert!(
            l.then(&r)
                .unwrap()
                .same_effect_and_focus(&r.then(&l).unwrap())
        );
        let c = closure(Summary::identity(i), &[l, r], 16);
        assert!(c.closed);
        assert_eq!(c.states.len(), 9);
        assert_eq!(c.transitions.len(), 18);
    }
    #[test]
    fn a_budget_hit_is_not_a_fixed_point() {
        let i = BTreeMap::from([("cursor".into(), port(0))]);
        let g = Summary::grow(i.clone(), "cursor").unwrap();
        let mut start = Summary::identity(i);
        start
            .requires
            .insert(Fact::Relation("Seen".into(), vec![port(0)]));
        let c = closure(start, &[g], 8);
        assert!(!c.closed);
        assert_eq!(c.states.len(), 8);
    }
}

#[cfg(test)]
mod equality_tests {
    use super::*;
    #[test]
    fn equivalent_edges_normalize_without_quadratic_pair_expansion() {
        let mut a = Summary::identity(BTreeMap::new());
        a.equalities.insert(pair(port(0), port(1)));
        a.equalities.insert(pair(port(1), port(2)));
        let mut b = a.clone();
        b.equalities.remove(&pair(port(1), port(2)));
        b.equalities.insert(pair(port(0), port(2)));
        assert!(a.same_effect_and_focus(&b));
        let a = a.normalized();
        assert_eq!(a.equalities.len(), 2);
        assert!(a.equivalent_terms(&port(1), &port(2)));
    }
}
