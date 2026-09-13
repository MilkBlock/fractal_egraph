//! Adapter for the already validated, finite R23 candidate on the native final graph.
use super::*;
use egg_layout::{
    binding_program::Term, effect_program::Fact, fractal_frontier::*,
    trigger_bridge::BindingRelation,
};
fn port(i: usize) -> Term {
    Term::new("Math", format!("in:{i}"), vec![])
}
fn node(op: &str, args: Vec<Term>) -> Term {
    Term::new("Math", op, args)
}
fn candidate() -> Family {
    let (a, b, x) = (port(0), port(1), port(2));
    let d = node("Diff", vec![x.clone(), a.clone()]);
    let i = node("Integral", vec![b.clone(), x.clone()]);
    let lhs = node("Integral", vec![node("Mul", vec![a.clone(), b]), x.clone()]);
    let rhs = node(
        "Sub",
        vec![
            node("Mul", vec![a, i.clone()]),
            node("Integral", vec![node("Mul", vec![d.clone(), i.clone()]), x]),
        ],
    );
    let trigger = BindingRelation {
        sorts: vec!["Math".into(); 5],
        inputs: vec![0, 1, 2],
        outputs: vec![0, 1, 2],
        facts: vec![Fact::Node(lhs.clone())],
        equalities: vec![],
    };
    let mut recurrence = trigger.clone();
    recurrence.outputs = vec![3, 4, 2];
    recurrence.facts.push(Fact::Node(rhs.clone()));
    recurrence.equalities = vec![(lhs, rhs), (port(3), d), (port(4), i)];
    Family {
        name: "R23-residual-integral".into(),
        trigger,
        recurrence,
    }
}
fn key(s: &FamilyState) -> State {
    State {
        family: "R23-residual-integral".into(),
        binding: s
            .binding
            .iter()
            .map(|v| Term::new("Math", format!("native:{v:?}"), vec![]))
            .collect(),
    }
}
struct Frozen<'a> {
    states: &'a [FamilyState],
    index: BTreeMap<State, usize>,
    visited: BTreeSet<usize>,
}
impl Oracle for Frozen<'_> {
    fn canonicalize(&self, s: &State) -> std::result::Result<State, String> {
        Ok(s.clone())
    } // already canonical, one frozen epoch
    fn inspect(&mut self, _: &Family, s: &State) -> std::result::Result<Observation, String> {
        let &id = self
            .index
            .get(s)
            .ok_or("R23 binding missing from frozen native LHS index")?;
        self.visited.insert(id);
        let s = &self.states[id];
        Ok(match s.next {
            Some(next) => Observation {
                transitions: vec![Transition {
                    target: key(&self.states[next]),
                    kind: EdgeKind::Recurrence,
                    witness:
                        "native LHS + RHS rows and union equality, same x, exact residual binding"
                            .into(),
                }],
                pending: None,
                complete: true,
            },
            None => Observation {
                transitions: vec![],
                pending: Some(if s.blocked == "rhs-not-present" {
                    Pending::EnabledUnmaterialized
                } else {
                    Pending::TriggerGap(
                        json!({"missing":s.blocked,"scope":"completion effect, not a failed LHS match"}),
                    )
                }),
                complete: true,
            },
        })
    }
}
pub(super) fn observe(states: &[FamilyState], seeds: &[usize], limit: usize) -> Json {
    let mut oracle = Frozen {
        states,
        index: states
            .iter()
            .enumerate()
            .map(|(i, s)| (key(s), i))
            .collect(),
        visited: BTreeSet::new(),
    };
    let mut f = Frontier::new([candidate()]).unwrap();
    for &i in seeds {
        f.seed(key(&states[i])).unwrap();
    }
    let mut r = f
        .refresh(
            0,
            Limits {
                states: limit,
                edges: limit,
            },
            &mut oracle,
        )
        .unwrap();
    let covered: BTreeSet<_> = oracle
        .visited
        .iter()
        .flat_map(|&i| states[i].lhs.iter().chain(&states[i].rhs))
        .copied()
        .collect();
    r["covered_enodes"] = json!(covered.len());
    r["adapter_scope"] = json!(
        "supplied R23 family over native frozen rows; read-only structural evidence, no committed-event reconstruction or automatic coarse binding search"
    );
    r
}
