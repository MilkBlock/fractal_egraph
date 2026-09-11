use crate::binding_program::Term;
use crate::effect_program::{Fact, Summary};
use std::collections::{BTreeMap, BTreeSet};
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
pub trait FixtureOps: Sized {
    fn swap(entry: BTreeMap<String, Term>, slot: &str) -> Result<Self, String>;
    fn grow(entry: BTreeMap<String, Term>, slot: &str) -> Result<Self, String>;
}
impl FixtureOps for Summary {
    fn swap(entry: BTreeMap<String, Term>, slot: &str) -> Result<Self, String> {
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
    fn grow(entry: BTreeMap<String, Term>, slot: &str) -> Result<Self, String> {
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
