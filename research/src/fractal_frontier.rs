//! Bounded, snapshot-backed exploration of supplied recursive rule candidates.
//! Scheduling adjacency is never a dependency and a frontier is not saturation.
use crate::{binding_program::Term, trigger_bridge::BindingRelation};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone)]
pub struct Family {
    pub name: String,
    pub trigger: BindingRelation,
    pub recurrence: BindingRelation,
}
impl Family {
    pub fn validate(&self) -> Result<(), String> {
        self.trigger.validate()?;
        self.recurrence.validate()?;
        let input_sorts = |r: &BindingRelation| {
            r.inputs
                .iter()
                .map(|&i| r.sorts[i].clone())
                .collect::<Vec<_>>()
        };
        let output_sorts: Vec<_> = self
            .recurrence
            .outputs
            .iter()
            .map(|&i| self.recurrence.sorts[i].clone())
            .collect();
        if input_sorts(&self.trigger) != input_sorts(&self.recurrence)
            || input_sorts(&self.recurrence) != output_sorts
        {
            return Err(
                "recursive candidate requires compatible trigger, entry and exit sorts".into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct State {
    pub family: String,
    /// Concrete, typed interface values. Retain aliasing, not only a shape hash.
    pub binding: Vec<Term>,
}
impl State {
    fn json(&self) -> Value {
        json!({"family":self.family,"binding":self.binding.iter().map(Term::json).collect::<Vec<_>>()})
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    Recurrence,
    CoarseBridge,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Transition {
    pub target: State,
    pub kind: EdgeKind,
    /// Adapter-supplied structural witness; NOT a committed causal event ID.
    pub witness: String,
}
#[derive(Clone, Debug)]
pub enum Pending {
    /// Readiness was verified; the required next effect is not in this snapshot.
    EnabledUnmaterialized,
    /// Missing facts/equalities are explicitly retained, and may arrive later.
    TriggerGap(Value),
    EffectGap(Value),
    Unknown(String),
}
#[derive(Clone)]
pub struct Observation {
    pub transitions: Vec<Transition>,
    /// Can coexist with transitions: some branches may still need effects.
    pub pending: Option<Pending>,
    /// Complete only relative to the supplied candidate family and snapshot.
    pub complete: bool,
}
/// Implementations must inspect one stable snapshot, check the supplied family
/// contracts and enumerate only witnessed transitions. No mutation is requested.
pub trait Oracle {
    fn canonicalize(&self, state: &State) -> Result<State, String>;
    fn inspect(&mut self, family: &Family, state: &State) -> Result<Observation, String>;
}
pub struct Limits {
    pub states: usize,
    pub edges: usize,
}
pub struct Frontier {
    families: BTreeMap<String, Family>,
    seeds: BTreeSet<State>,
    last_epoch: Option<u64>,
}
impl Frontier {
    pub fn new(families: impl IntoIterator<Item = Family>) -> Result<Self, String> {
        let mut catalog = BTreeMap::new();
        for f in families {
            f.validate()?;
            if catalog.insert(f.name.clone(), f).is_some() {
                return Err("duplicate family".into());
            }
        }
        Ok(Self {
            families: catalog,
            seeds: BTreeSet::new(),
            last_epoch: None,
        })
    }
    fn validate_state(&self, s: &State) -> Result<(), String> {
        let f = self.families.get(&s.family).ok_or("unknown family")?;
        if s.binding.len() != f.recurrence.inputs.len() {
            return Err("binding arity mismatch".into());
        }
        let mut aliases = BTreeMap::new();
        for (t, &i) in s.binding.iter().zip(&f.recurrence.inputs) {
            if aliases.insert(i, t).is_some_and(|old| old != t) {
                return Err("repeated input port has inconsistent binding".into());
            }
            if t.sort != f.recurrence.sorts[i] {
                return Err("binding sort mismatch".into());
            }
        }
        fn ground(t: &Term) -> bool {
            !t.op.starts_with("in:") && t.args.iter().all(ground)
        }
        if !s.binding.iter().all(ground) {
            return Err("frontier binding must be ground".into());
        }
        Ok(())
    }
    pub fn seed(&mut self, state: State) -> Result<(), String> {
        self.validate_state(&state)?;
        self.seeds.insert(state);
        Ok(())
    }
    /// Rebuild reachable observations from persistent seeds each epoch. This
    /// deliberately revalidates union-affected bindings, edges and trigger gaps.
    /// There is no stale cached 'terminal' state and no forced branch winner.
    pub fn refresh(
        &mut self,
        epoch: u64,
        limits: Limits,
        oracle: &mut impl Oracle,
    ) -> Result<Value, String> {
        if self.last_epoch.is_some_and(|old| epoch < old) {
            return Err("snapshot epoch moved backwards".into());
        }
        let canon = |s: &State, oracle: &dyn Oracle| -> Result<State, String> {
            let c = oracle.canonicalize(s)?;
            if c.family != s.family {
                return Err("canonicalization changed family".into());
            }
            self.validate_state(&c)?;
            Ok(c)
        };
        let mut queue = VecDeque::new();
        let mut discovered = BTreeSet::new();
        for s in &self.seeds {
            let c = canon(s, oracle)?;
            if discovered.insert(c.clone()) {
                queue.push_back(c);
            }
        }
        let mut nodes = vec![];
        let mut edges = BTreeSet::new();
        let mut incomplete = 0;
        let mut pending = 0;
        let mut deferred_edges = 0;
        while nodes.len() < limits.states {
            let Some(s) = queue.pop_front() else { break };
            let observation = oracle.inspect(&self.families[&s.family], &s)?;
            if !observation.complete {
                incomplete += 1;
            }
            let wait = observation.pending.map(|p| {
                pending += 1;
                match p {
                    Pending::EnabledUnmaterialized => json!({"status":"enabled_unmaterialized"}),
                    Pending::TriggerGap(g) => json!({"status":"trigger_gap","gap":g}),
                    Pending::EffectGap(g) => json!({"status":"effect_gap","gap":g}),
                    Pending::Unknown(reason) => json!({"status":"unknown","reason":reason}),
                }
            });
            let before_deferred = deferred_edges;
            for mut t in observation.transitions {
                if t.witness.is_empty() {
                    return Err("transition missing structural witness".into());
                }
                t.target = canon(&t.target, oracle)?;
                if t.kind == EdgeKind::Recurrence && t.target.family != s.family {
                    return Err("recurrence changed family".into());
                }
                if !edges.contains(&(s.clone(), t.clone())) && edges.len() >= limits.edges {
                    deferred_edges += 1;
                    continue;
                }
                if discovered.insert(t.target.clone()) {
                    queue.push_back(t.target.clone());
                }
                edges.insert((s.clone(), t));
            }
            nodes.push(json!({"state":s.json(),"pending":wait,"candidate_enumeration_complete":observation.complete && before_deferred==deferred_edges}));
        }
        self.last_epoch = Some(epoch);
        Ok(
            json!({"epoch":epoch,"states":nodes,"edges":edges.iter().map(|(s,t)|json!({"source":s.json(),"target":t.target.json(),"kind":format!("{:?}",t.kind),"witness":t.witness})).collect::<Vec<_>>(),
            "visited_states":nodes.len(),"witnessed_edges":edges.len(),"pending_states":pending,
            "budget_exhausted":!queue.is_empty() || deferred_edges>0,"deferred_transition_count":deferred_edges,"deferred_states":queue.iter().map(State::json).collect::<Vec<_>>(),
            "incomplete_observations":incomplete,"snapshot_candidate_closure":queue.is_empty() && incomplete==0 && pending==0 && deferred_edges==0,
            "infinite_fractal_certified":false,"scope":"bounded snapshot structural witnesses; not chronological rule runs, committed causality, or a saturation certificate"}),
        )
    }
}
