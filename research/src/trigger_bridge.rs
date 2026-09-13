//! Typed conjunctive binding interfaces and conditional, positive-effect bridges.
//! Ports use `in:N`; equality never implies constructor injectivity.
use crate::{
    binding_program::Term,
    effect_program::{Fact, Summary},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingRelation {
    pub sorts: Vec<String>,
    pub inputs: Vec<usize>,
    pub outputs: Vec<usize>,
    pub facts: Vec<Fact>,
    pub equalities: Vec<(Term, Term)>,
}

fn map_term(t: &Term, ports: &BTreeMap<usize, Term>) -> Term {
    if let Some(i) =
        t.op.strip_prefix("in:")
            .and_then(|s| s.parse::<usize>().ok())
    {
        return ports.get(&i).cloned().unwrap_or_else(|| t.clone());
    }
    Term::new(
        &t.sort,
        &t.op,
        t.args.iter().map(|a| map_term(a, ports)).collect(),
    )
}
fn map_fact(f: &Fact, ports: &BTreeMap<usize, Term>) -> Fact {
    match f {
        Fact::Node(t) => Fact::Node(map_term(t, ports)),
        Fact::Relation(n, args) => {
            Fact::Relation(n.clone(), args.iter().map(|t| map_term(t, ports)).collect())
        }
    }
}
fn fact_terms(f: &Fact) -> Vec<&Term> {
    match f {
        Fact::Node(t) => vec![t],
        Fact::Relation(_, args) => args.iter().collect(),
    }
}
fn variables(t: &Term, out: &mut BTreeSet<usize>) {
    if let Some(i) = t.op.strip_prefix("in:").and_then(|s| s.parse().ok()) {
        out.insert(i);
    }
    for a in &t.args {
        variables(a, out);
    }
}
fn check_term(t: &Term, sorts: &[String]) -> Result<(), String> {
    if let Some(i) = t.op.strip_prefix("in:") {
        let i: usize = i.parse().map_err(|_| "invalid port")?;
        if sorts.get(i) != Some(&t.sort) || !t.args.is_empty() {
            return Err("ill-typed port".into());
        }
    }
    for a in &t.args {
        check_term(a, sorts)?;
    }
    Ok(())
}
impl BindingRelation {
    pub fn validate(&self) -> Result<(), String> {
        for i in self.inputs.iter().chain(&self.outputs) {
            if *i >= self.sorts.len() {
                return Err("interface port out of range".into());
            }
        }
        for f in &self.facts {
            for t in fact_terms(f) {
                check_term(t, &self.sorts)?;
            }
        }
        for (a, b) in &self.equalities {
            check_term(a, &self.sorts)?;
            check_term(b, &self.sorts)?;
            if a.sort != b.sort {
                return Err("equality sort mismatch".into());
            }
        }
        Ok(())
    }
    /// Relational composition: existentially retain intermediate ports and join
    /// the interface with equalities. Repeated ports need not be injective.
    pub fn then(&self, next: &Self) -> Result<Self, String> {
        self.validate()?;
        next.validate()?;
        if self.outputs.len() != next.inputs.len() {
            return Err("interface arity mismatch".into());
        }
        let offset = self.sorts.len();
        let ports = next
            .sorts
            .iter()
            .enumerate()
            .map(|(i, s)| (i, Term::new(s, format!("in:{}", offset + i), vec![])))
            .collect();
        let mut out = self.clone();
        out.sorts.extend(next.sorts.clone());
        out.outputs = next.outputs.iter().map(|i| i + offset).collect();
        out.facts
            .extend(next.facts.iter().map(|f| map_fact(f, &ports)));
        out.equalities.extend(
            next.equalities
                .iter()
                .map(|(a, b)| (map_term(a, &ports), map_term(b, &ports))),
        );
        for (&a, &b) in self.outputs.iter().zip(&next.inputs) {
            if self.sorts[a] != next.sorts[b] {
                return Err("interface sort mismatch".into());
            }
            out.equalities.push((
                Term::new(&self.sorts[a], format!("in:{a}"), vec![]),
                ports[&b].clone(),
            ));
        }
        Ok(out)
    }
    /// Number ports by interface and clause occurrence, independent of original IDs.
    /// Clause reordering is deliberately not normalized by this syntactic key.
    pub fn canonical_key(&self) -> Result<String, String> {
        self.validate()?;
        fn visit(t: &Term, order: &mut Vec<usize>) {
            if let Some(i) = t.op.strip_prefix("in:").and_then(|s| s.parse().ok()) {
                if !order.contains(&i) {
                    order.push(i);
                }
            }
            for a in &t.args {
                visit(a, order);
            }
        }
        let mut order = vec![];
        for &i in self.inputs.iter().chain(&self.outputs) {
            if !order.contains(&i) {
                order.push(i);
            }
        }
        for f in &self.facts {
            for t in fact_terms(f) {
                visit(t, &mut order);
            }
        }
        for (a, b) in &self.equalities {
            visit(a, &mut order);
            visit(b, &mut order);
        }
        let ports: BTreeMap<_, _> = order
            .iter()
            .enumerate()
            .map(|(n, &i)| (i, Term::new(&self.sorts[i], format!("in:{n}"), vec![])))
            .collect();
        let interface = |ids: &[usize]| ids.iter().map(|i| ports[i].json()).collect::<Vec<_>>();
        Ok(json!({"inputs":interface(&self.inputs),"outputs":interface(&self.outputs),
            "facts":self.facts.iter().map(|f| fact_json(&map_fact(f,&ports))).collect::<Vec<_>>(),
            "equalities":self.equalities.iter().map(|(a,b)|json!([map_term(a,&ports).json(),map_term(b,&ports).json()])).collect::<Vec<_>>()}).to_string())
    }
    /// Evaluate a supplied partial assignment. Unassigned variables remain explicit;
    /// this does not enumerate joins or select a witness for an existential variable.
    pub fn gap(&self, graph: &Summary, binding: &BTreeMap<usize, Term>) -> Result<Gap, String> {
        self.validate()?;
        for (&i, t) in binding {
            if self.sorts.get(i) != Some(&t.sort) {
                return Err("binding sort mismatch".into());
            }
            check_term(t, &[])?; // supplied values are ground, not another port namespace
        }
        let mut gap = Gap {
            facts: vec![],
            equalities: vec![],
            unbound: BTreeSet::new(),
        };
        for &i in &self.outputs {
            if !binding.contains_key(&i) {
                gap.unbound.insert(i);
            }
        }
        for &i in &self.inputs {
            if !binding.contains_key(&i) {
                gap.unbound.insert(i);
            }
        }
        for f in &self.facts {
            let f = map_fact(f, binding);
            let mut vars = BTreeSet::new();
            for t in fact_terms(&f) {
                variables(t, &mut vars);
            }
            if !vars.is_empty() || !has_fact(graph, &f) {
                gap.facts.push(f);
            }
            gap.unbound.extend(vars);
        }
        for (a, b) in &self.equalities {
            let (a, b) = (map_term(a, binding), map_term(b, binding));
            let mut vars = BTreeSet::new();
            variables(&a, &mut vars);
            variables(&b, &mut vars);
            if !vars.is_empty() || !graph.equivalent_terms(&a, &b) {
                gap.equalities.push((a, b));
            }
            gap.unbound.extend(vars);
        }
        Ok(gap)
    }
}
fn has_fact(graph: &Summary, wanted: &Fact) -> bool {
    graph
        .all_facts()
        .iter()
        .any(|actual| match (actual, wanted) {
            (Fact::Node(a), Fact::Node(b)) => {
                a.sort == b.sort
                    && a.op == b.op
                    && a.args.len() == b.args.len()
                    && a.args
                        .iter()
                        .zip(&b.args)
                        .all(|(a, b)| graph.equivalent_terms(a, b))
            }
            (Fact::Relation(a, x), Fact::Relation(b, y)) => {
                a == b
                    && x.len() == y.len()
                    && x.iter().zip(y).all(|(a, b)| graph.equivalent_terms(a, b))
            }
            _ => false,
        })
}
fn fact_json(f: &Fact) -> Value {
    match f {
        Fact::Node(t) => json!({"node":t.json()}),
        Fact::Relation(n, args) => {
            json!({"relation":n,"args":args.iter().map(Term::json).collect::<Vec<_>>()})
        }
    }
}
pub struct Gap {
    pub facts: Vec<Fact>,
    pub equalities: Vec<(Term, Term)>,
    pub unbound: BTreeSet<usize>,
}
impl Gap {
    pub fn satisfied(&self) -> bool {
        self.facts.is_empty() && self.equalities.is_empty() && self.unbound.is_empty()
    }
    pub fn json(&self) -> Value {
        json!({"missing_facts":self.facts.iter().map(fact_json).collect::<Vec<_>>(),"missing_equalities":self.equalities.iter().map(|(a,b)|json!([a.json(),b.json()])).collect::<Vec<_>>(),"unbound":self.unbound,"satisfied":self.satisfied()})
    }
}

pub struct BridgeAction {
    pub name: String,
    /// Ground positive summary supplied by the caller, not a certified runtime event.
    pub effect: Summary,
}
pub struct BridgeSearch {
    pub plan: Option<Vec<usize>>,
    pub final_state: Summary,
    pub explored: usize,
    pub status: &'static str,
}
impl BridgeSearch {
    pub fn json(
        &self,
        trigger: &BindingRelation,
        binding: &BTreeMap<usize, Term>,
        actions: &[BridgeAction],
    ) -> Result<Value, String> {
        Ok(json!({
            "status": self.status,
            "plan": self.plan.as_ref().map(|p| p.iter().map(|&i| &actions[i].name).collect::<Vec<_>>()),
            "explored_states": self.explored,
            "relation_key": trigger.canonical_key()?,
            "remaining_gap": trigger.gap(&self.final_state, binding)?.json(),
            "state_contract": self.final_state.json(),
            "scope": "supplied ground positive-effect contracts; no native mutation certification"
        }))
    }
}
/// Breadth-first search over a supplied finite set of ground positive actions.
/// Never promote missing preconditions into assumed initial facts.
pub fn search(
    graph: Summary,
    trigger: &BindingRelation,
    binding: &BTreeMap<usize, Term>,
    actions: &[BridgeAction],
    limit: usize,
) -> Result<BridgeSearch, String> {
    if limit == 0 {
        return Err("positive search budget required".into());
    }
    trigger.gap(&graph, binding)?;
    for state in std::iter::once(&graph).chain(actions.iter().map(|a| &a.effect)) {
        for f in state.requires.iter().chain(&state.adds) {
            for t in fact_terms(f) {
                check_term(t, &[])?;
            }
        }
        for (a, b) in state.required_equalities.iter().chain(&state.equalities) {
            check_term(a, &[])?;
            check_term(b, &[])?;
            if a.sort != b.sort {
                return Err("equality sort mismatch".into());
            }
        }
        if !state.entry.is_empty() || !state.exit.is_empty() {
            return Err(
                "bridge actions require explicit ground contracts without implicit focus routing"
                    .into(),
            );
        }
    }
    let graph = graph.normalized();
    if !trigger.gap(&graph, binding)?.unbound.is_empty() {
        return Ok(BridgeSearch {
            plan: None,
            final_state: graph,
            explored: 1,
            status: "binding_unresolved",
        });
    }
    let mut queue = VecDeque::from([(graph.clone(), vec![])]);
    let mut seen = vec![graph.clone()];
    let mut truncated = false;
    while let Some((state, plan)) = queue.pop_front() {
        if trigger.gap(&state, binding)?.satisfied() {
            return Ok(BridgeSearch {
                plan: Some(plan),
                final_state: state,
                explored: seen.len(),
                status: "found_in_supplied_symbolic_actions",
            });
        }
        for (i, action) in actions.iter().enumerate() {
            let a = &action.effect;
            if !a.requires.iter().all(|f| has_fact(&state, f))
                || !a
                    .required_equalities
                    .iter()
                    .all(|(x, y)| state.equivalent_terms(x, y))
            {
                continue;
            }
            let mut next = state.clone();
            next.adds.extend(a.adds.clone());
            next.equalities.extend(a.equalities.clone());
            next = next.normalized();
            if seen.iter().any(|old| old.same_effect_and_focus(&next)) {
                continue;
            }
            if seen.len() == limit {
                truncated = true;
                continue;
            }
            seen.push(next.clone());
            let mut path = plan.clone();
            path.push(i);
            queue.push_back((next, path));
        }
    }
    Ok(BridgeSearch {
        plan: None,
        final_state: graph,
        explored: seen.len(),
        status: if truncated {
            "budget_unknown"
        } else {
            "no_plan_in_supplied_actions"
        },
    })
}
