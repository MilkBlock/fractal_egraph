//! Online, producer-anchored dictionary coding of composition wiring.
//! This is an alternative metadata representation, not skipped tier0 execution.
use crate::coarse_smooth::{Effect, LayerStore, RelativeBinding};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

const MAX_MEMBERS: usize = 16;
const MAX_CANDIDATES: usize = 8;
const MAX_TEMPLATES: usize = 4096;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Wire {
    Local { member: usize, output: usize },
    Input(usize),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Schema {
    pub rule: String,
    pub input_roles: Vec<String>,
    pub output_roles: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Step {
    pub schema: usize,
    pub wiring: Vec<Wire>,
    pub internal_parents: Vec<usize>,
    pub aliases: Vec<usize>,
    pub requires: Vec<Effect>,
    pub effects: Vec<Effect>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Pattern {
    pub steps: Vec<Step>,
    pub root: usize,
    pub inputs: usize,
}
#[derive(Clone, Debug, Serialize)]
pub enum Atom {
    Apply(usize),
    Use {
        template: usize,
        members: Vec<usize>,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct Template {
    pub pattern: Pattern,
    pub atoms: Vec<Atom>,
    pub learned_at: usize,
    pub observed: usize,
    pub matches: usize,
    /// Historical opportunity credit, not already realized compression savings.
    pub admission_credit: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Reference {
    ResidualPort {
        event: usize,
        output: usize,
    },
    UsePort {
        instance: usize,
        member: usize,
        output: usize,
    },
    External(usize),
    ExternalEffect(Effect),
    ResidualContext(usize),
    UseContext {
        instance: usize,
        member: usize,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct Residual {
    pub event: usize,
    pub binding: Vec<Reference>,
    pub contexts: Vec<Reference>,
    pub coarse: bool,
}
#[derive(Clone, Debug, Serialize)]
pub enum Part {
    Residual(usize),
    Use(usize),
}
#[derive(Clone, Debug, Serialize)]
pub struct Use {
    pub template: usize,
    pub completed_at: usize,
    pub members: Vec<usize>,
    pub inputs: Vec<Reference>,
    pub contexts: Vec<Reference>,
    pub parts: Vec<Part>,
    pub wiring_cost: usize,
}
#[derive(Clone, Default, Serialize)]
pub struct ReuseStore {
    pub schemas: Vec<Schema>,
    #[serde(skip)]
    schema_index: BTreeMap<Schema, usize>,
    #[serde(skip)]
    event_schema: Vec<usize>,
    pub templates: Vec<Template>,
    pub uses: Vec<Use>,
    pub residuals: Vec<Residual>,
    /// Preferred disjoint cover. An interior output remains addressable.
    pub owner: Vec<Option<(usize, usize)>>,
    pub active_uses: BTreeSet<usize>,
    pub candidate_queries: usize,
    pub matches: usize,
    pub rejected_overlap: usize,
    pub rejected_nonconvex: usize,
    pub budget_stops: usize,
    pub continuations_through_use: usize,
    pub interior_port_reads: usize,
    pub raw_wiring_cost: usize,
    pub selected_wiring_cost: usize,
    pub layer_restricted: bool,
    pub cut_passes: usize,
    pub cut_visits: usize,
    pub cut_removed_uses: usize,
    #[serde(skip)]
    keys: BTreeMap<Pattern, usize>,
}
struct Candidate {
    pattern: Pattern,
    members: Vec<usize>,
    inputs: Vec<Reference>,
    contexts: Vec<Reference>,
    atoms: Vec<Atom>,
    parts: Vec<Part>,
    old_cost: usize,
    new_cost: usize,
}
impl ReuseStore {
    fn value_ref(&self, event: usize, output: usize) -> Reference {
        // Historical name retained in the JSON protocol. Identity is physical,
        // independent of whether this occurrence is currently a residual or Use.
        Reference::ResidualPort { event, output }
    }
    fn context_ref(&self, event: usize) -> Reference {
        Reference::ResidualContext(event)
    }
    /// Resolve a stable physical output to its current presentation owner.
    pub fn locate(&self, event: usize, output: usize) -> Reference {
        match self.owner[event] {
            Some((instance, member)) => Reference::UsePort {
                instance,
                member,
                output,
            },
            None => Reference::ResidualPort { event, output },
        }
    }
    fn atom_members(&self, event: usize) -> Vec<usize> {
        self.owner[event]
            .map(|(u, _)| self.uses[u].members.clone())
            .unwrap_or_else(|| vec![event])
    }
    /// Called after LayerStore has validated the current binding and preconditions.
    /// Matching occurs before learning any pattern containing this new event.
    pub fn push(&mut self, store: &LayerStore) {
        let i = self.residuals.len();
        assert_eq!(store.occurrences.len(), i + 1);
        let a = &store.occurrences[i].apply;
        let schema = Schema {
            rule: a.rule.clone(),
            input_roles: a.input_roles.clone(),
            output_roles: a.output_roles.clone(),
        };
        let sid = if let Some(id) = self.schema_index.get(&schema) {
            *id
        } else {
            let id = self.schemas.len();
            self.schemas.push(schema.clone());
            self.schema_index.insert(schema, id);
            id
        };
        self.event_schema.push(sid);
        let binding: Vec<_> = a
            .binding
            .iter()
            .map(|p| match p {
                RelativeBinding::ParentPort { parent, output } => {
                    self.value_ref(a.parents[*parent], *output)
                }
                RelativeBinding::External { slot } => Reference::External(a.external[*slot]),
            })
            .collect();
        for b in &a.binding {
            if let RelativeBinding::ParentPort { parent, .. } = b {
                if let Some((u, m)) = self.owner[a.parents[*parent]] {
                    self.continuations_through_use += 1;
                    self.interior_port_reads +=
                        usize::from(m != self.templates[self.uses[u].template].pattern.root);
                }
            }
        }
        let contexts = a.parents.iter().map(|p| self.context_ref(*p)).collect();
        self.owner.push(None);
        self.residuals.push(Residual {
            event: i,
            binding,
            contexts,
            coarse: a.parents.is_empty()
                || !a.external_facts.is_empty()
                || a.binding
                    .iter()
                    .any(|b| matches!(b, RelativeBinding::External { .. })),
        });
        self.raw_wiring_cost += raw_cost(a);
        self.selected_wiring_cost += raw_cost(a);
        let mut regions = BTreeSet::new();
        // Individual producer continuations retain other parents as explicit inputs.
        for p in a.parents.iter().take(4) {
            let mut set: BTreeSet<_> = self.atom_members(*p).into_iter().collect();
            set.insert(i);
            regions.insert(set);
        }
        // Also try joint continuations and a small dependency neighbourhood,
        // expanding completed Uses atomically, never their entire prior history.
        let mut set = BTreeSet::from([i]);
        for _ in 0..3 {
            let ps: Vec<_> = set
                .iter()
                .flat_map(|m| store.occurrences[*m].apply.parents.iter().copied())
                .filter(|p| !set.contains(p))
                .collect();
            if ps.is_empty() {
                break;
            }
            for p in ps {
                set.extend(self.atom_members(p));
                if set.len() > MAX_MEMBERS {
                    break;
                }
            }
            if set.len() > MAX_MEMBERS {
                self.budget_stops += 1;
                break;
            }
            regions.insert(set.clone());
        }
        let mut allowed = BTreeSet::new();
        if self.layer_restricted {
            let mut todo = vec![i];
            while let Some(e) = todo.pop() {
                if !allowed.insert(e) {
                    continue;
                }
                if allowed.len() > 4096 {
                    self.budget_stops += 1;
                    break;
                }
                if store.occurrences[e].smooth_layer.is_some() {
                    todo.extend(&store.occurrences[e].apply.parents);
                }
            }
        }
        let mut candidates = vec![];
        for set in regions.into_iter().take(MAX_CANDIDATES) {
            if set.len() < 2 || set.len() > MAX_MEMBERS {
                self.budget_stops += 1;
                continue;
            }
            if self.layer_restricted && !set.is_subset(&allowed) {
                continue;
            }
            if set.iter().any(|m| {
                self.owner[*m]
                    .is_some_and(|(u, _)| self.uses[u].members.iter().any(|e| !set.contains(e)))
            }) {
                self.rejected_overlap += 1;
                continue;
            }
            match convex(store, &set) {
                Some(true) => {}
                Some(false) => {
                    self.rejected_nonconvex += 1;
                    continue;
                }
                None => {
                    self.budget_stops += 1;
                    continue;
                }
            }
            candidates.push(self.candidate(store, &set, i));
        }
        let mut best = None;
        for (j, c) in candidates.iter().enumerate() {
            self.candidate_queries += 1;
            if let Some(t) = self.keys.get(&c.pattern).copied() {
                self.matches += 1;
                self.templates[t].matches += 1;
                let gain = c.old_cost.saturating_sub(c.new_cost);
                self.templates[t].admission_credit += gain;
                if self.templates[t].admission_credit < definition_cost(&self.templates[t].pattern)
                {
                    continue;
                }
                if gain > 0 && best.is_none_or(|(_, _, g)| gain > g) {
                    best = Some((j, t, gain));
                }
            }
        }
        if let Some((j, t, gain)) = best {
            let c = &candidates[j];
            let u = self.uses.len();
            for part in &c.parts {
                if let Part::Use(old) = part {
                    self.active_uses.remove(old);
                }
            }
            self.uses.push(Use {
                template: t,
                completed_at: i,
                members: c.members.clone(),
                inputs: c.inputs.clone(),
                contexts: c.contexts.clone(),
                parts: c.parts.clone(),
                wiring_cost: c.new_cost,
            });
            for (member, event) in c.members.iter().enumerate() {
                self.owner[*event] = Some((u, member));
            }
            self.active_uses.insert(u);
            self.selected_wiring_cost -= gain;
        }
        // At most the bounded observed candidates enter the dictionary. Existing
        // equivalent definitions may gain a smaller recipe referencing prior T's.
        for c in candidates {
            if let Some(t) = self.keys.get(&c.pattern).copied() {
                let old = &mut self.templates[t];
                old.observed += 1;
                if c.atoms.len() < old.atoms.len() {
                    old.atoms = c.atoms;
                }
            } else {
                if self.templates.len() >= MAX_TEMPLATES {
                    self.budget_stops += 1;
                    continue;
                }
                let t = self.templates.len();
                self.keys.insert(c.pattern.clone(), t);
                self.templates.push(Template {
                    pattern: c.pattern,
                    atoms: c.atoms,
                    learned_at: i,
                    observed: 1,
                    matches: 0,
                    admission_credit: 0,
                });
            }
        }
        if (i + 1) % 64 == 0 && std::env::var_os("EGG_LAYOUT_DISABLE_RECUT").is_none() {
            self.repartition(store);
        }
    }
    /// Exact additive cut within existing Use decomposition trees, followed by
    /// bounded dictionary-removal trials. Does not enumerate arbitrary DAG cuts.
    pub fn repartition(&mut self, store: &LayerStore) {
        let roots: Vec<_> = self.active_uses.iter().copied().collect();
        let raw: Vec<_> = store
            .occurrences
            .iter()
            .map(|o| raw_cost(&o.apply))
            .collect();
        let outside: usize = self
            .owner
            .iter()
            .enumerate()
            .filter(|(_, o)| o.is_none())
            .map(|(e, _)| raw[e])
            .sum();
        let evaluate = |disabled: &BTreeSet<usize>| {
            let mut costs = Vec::with_capacity(self.uses.len());
            let mut take = Vec::with_capacity(self.uses.len());
            for u in &self.uses {
                let split: usize = u
                    .parts
                    .iter()
                    .map(|p| match p {
                        Part::Residual(e) => raw[*e],
                        Part::Use(v) => costs[*v],
                    })
                    .sum();
                let yes = !disabled.contains(&u.template) && u.wiring_cost < split;
                take.push(yes);
                costs.push(if yes { u.wiring_cost } else { split });
            }
            let wiring = outside + roots.iter().map(|u| costs[*u]).sum::<usize>();
            let mut selected = BTreeSet::new();
            let mut todo = roots.clone();
            while let Some(u) = todo.pop() {
                if take[u] {
                    selected.insert(u);
                } else {
                    todo.extend(
                        self.uses[u]
                            .parts
                            .iter()
                            .filter_map(|p| if let Part::Use(v) = p { Some(*v) } else { None }),
                    );
                }
            }
            let dict: BTreeSet<_> = selected.iter().map(|u| self.uses[*u].template).collect();
            let total = wiring
                + dict
                    .iter()
                    .map(|t| definition_cost(&self.templates[*t].pattern))
                    .sum::<usize>();
            (total, wiring, selected, dict)
        };
        let original_dict: BTreeSet<_> = roots.iter().map(|u| self.uses[*u].template).collect();
        let original_total = self.selected_wiring_cost
            + original_dict
                .iter()
                .map(|t| definition_cost(&self.templates[*t].pattern))
                .sum::<usize>();
        let mut disabled = BTreeSet::new();
        let mut best = evaluate(&disabled);
        let mut trials: Vec<_> = best.3.iter().copied().collect();
        trials.sort_by_key(|t| std::cmp::Reverse(definition_cost(&self.templates[*t].pattern)));
        let mut visits = self.uses.len();
        for t in trials.into_iter().take(8) {
            disabled.insert(t);
            let next = evaluate(&disabled);
            visits += self.uses.len();
            if next.0 < best.0 {
                best = next;
            } else {
                disabled.remove(&t);
            }
        }
        self.cut_passes += 1;
        self.cut_visits += visits;
        // Shared dictionary charges are not additive; retain the old cut if
        // exposing children has increased its total cost.
        if best.0 > original_total {
            return;
        }
        self.cut_removed_uses += self.active_uses.difference(&best.2).count();
        self.active_uses = best.2;
        self.selected_wiring_cost = best.1;
        self.owner.fill(None);
        for u in &self.active_uses {
            for (m, e) in self.uses[*u].members.iter().enumerate() {
                self.owner[*e] = Some((*u, m));
            }
        }
    }
    fn candidate(&self, store: &LayerStore, set: &BTreeSet<usize>, root: usize) -> Candidate {
        let members = order(store, set, root);
        let local: BTreeMap<_, _> = members.iter().enumerate().map(|(k, i)| (*i, k)).collect();
        let mut aliases = BTreeMap::new();
        let mut parameters = BTreeMap::new();
        let mut inputs = vec![];
        let mut steps = vec![];
        let mut contexts = BTreeSet::new();
        for event in &members {
            let a = &store.occurrences[*event].apply;
            let wiring = a
                .binding
                .iter()
                .enumerate()
                .map(|(slot, b)| {
                    if let RelativeBinding::ParentPort { parent, output } = b {
                        if let Some(member) = local.get(&a.parents[*parent]) {
                            return Wire::Local {
                                member: *member,
                                output: *output,
                            };
                        }
                    }
                    let v = a.wanted[slot];
                    let n = parameters.len();
                    let p = *parameters.entry(v).or_insert_with(|| {
                        inputs.push(match b {
                            RelativeBinding::ParentPort { parent, output } => {
                                self.value_ref(a.parents[*parent], *output)
                            }
                            RelativeBinding::External { .. } => Reference::External(v),
                        });
                        n
                    });
                    Wire::Input(p)
                })
                .collect();
            contexts.extend(
                a.external_facts
                    .iter()
                    .cloned()
                    .map(Reference::ExternalEffect),
            );
            let mut internal_parents = vec![];
            for p in &a.parents {
                if let Some(m) = local.get(p) {
                    internal_parents.push(*m);
                } else {
                    contexts.insert(self.context_ref(*p));
                }
            }
            internal_parents.sort();
            let values = a
                .wanted
                .iter()
                .chain(&a.outputs)
                .map(|v| symbol(*v, &mut aliases))
                .collect();
            steps.push(Step {
                schema: self.event_schema[*event],
                wiring,
                internal_parents,
                aliases: values,
                requires: a.required.iter().map(|e| rename(e, &mut aliases)).collect(),
                effects: a.produced.iter().map(|e| rename(e, &mut aliases)).collect(),
            });
        }
        let mut atoms = vec![];
        let mut parts = vec![];
        let mut used = BTreeSet::new();
        let mut old_cost = 0;
        for (m, event) in members.iter().enumerate() {
            if let Some((u, _)) = self.owner[*event] {
                if used.insert(u) {
                    atoms.push(Atom::Use {
                        template: self.uses[u].template,
                        members: self.uses[u].members.iter().map(|e| local[e]).collect(),
                    });
                    parts.push(Part::Use(u));
                    old_cost += self.uses[u].wiring_cost;
                }
            } else {
                atoms.push(Atom::Apply(m));
                parts.push(Part::Residual(*event));
                old_cost += raw_cost(&store.occurrences[*event].apply);
            }
        }
        let new_cost = 3 + inputs.len() * 3 + contexts.len() + members.len();
        Candidate {
            pattern: Pattern {
                steps,
                root: local[&root],
                inputs: inputs.len(),
            },
            members,
            inputs,
            contexts: contexts.into_iter().collect(),
            atoms,
            parts,
            old_cost,
            new_cost,
        }
    }
    /// Replay the compact wiring against immutable occurrence evidence, without
    /// executing rules. This checks ports, sharing, requirements and union effects.
    pub fn verify(&self, store: &LayerStore) -> Result<(), String> {
        let mut covered = BTreeSet::new();
        for u in &self.active_uses {
            for e in &self.uses[*u].members {
                if !covered.insert(*e) {
                    return Err("overlapping preferred Uses".into());
                }
            }
        }
        for (e, owner) in self.owner.iter().enumerate() {
            if owner.is_none() {
                covered.insert(e);
            } else if !covered.contains(&e) {
                return Err("uncovered owned occurrence".into());
            }
        }
        if covered.len() != self.owner.len() {
            return Err("incomplete preferred cover".into());
        }
        for (uid, u) in self.uses.iter().enumerate() {
            let t = &self.templates[u.template];
            if t.learned_at >= u.completed_at {
                return Err("Use learned from its own future".into());
            }
            let local: BTreeMap<_, _> =
                u.members.iter().enumerate().map(|(m, e)| (*e, m)).collect();
            let mut symbols = BTreeMap::new();
            let resolve = |r: &Reference| -> Option<usize> {
                match r {
                    Reference::External(v) => Some(*v),
                    Reference::ResidualPort { event, output } => store
                        .occurrences
                        .get(*event)?
                        .apply
                        .outputs
                        .get(*output)
                        .copied(),
                    Reference::UsePort {
                        instance,
                        member,
                        output,
                    } if *instance < uid => {
                        let old = self.uses.get(*instance)?;
                        store
                            .occurrences
                            .get(*old.members.get(*member)?)?
                            .apply
                            .outputs
                            .get(*output)
                            .copied()
                    }
                    _ => None,
                }
            };
            for (m, e) in u.members.iter().enumerate() {
                let a = &store.occurrences[*e].apply;
                let step = &t.pattern.steps[m];
                let schema = &self.schemas[step.schema];
                if a.rule != schema.rule
                    || a.input_roles != schema.input_roles
                    || a.output_roles != schema.output_roles
                {
                    return Err("Use source/role mismatch".into());
                }
                for (slot, w) in step.wiring.iter().enumerate() {
                    match w {
                        Wire::Local { member, output } => {
                            let Some(RelativeBinding::ParentPort {
                                parent,
                                output: actual,
                            }) = a.binding.get(slot)
                            else {
                                return Err("missing internal port".into());
                            };
                            if a.parents[*parent] != u.members[*member] || actual != output {
                                return Err("internal producer changed".into());
                            }
                        }
                        Wire::Input(p) => {
                            if resolve(&u.inputs[*p]) != a.wanted.get(slot).copied() {
                                return Err("boundary input changed".into());
                            }
                        }
                    }
                }
                let mut ps: Vec<_> = a
                    .parents
                    .iter()
                    .filter_map(|p| local.get(p).copied())
                    .collect();
                ps.sort();
                if ps != step.internal_parents {
                    return Err("internal dependency changed".into());
                }
                let aliases: Vec<_> = a
                    .wanted
                    .iter()
                    .chain(&a.outputs)
                    .map(|v| symbol(*v, &mut symbols))
                    .collect();
                let requires: Vec<_> = a.required.iter().map(|e| rename(e, &mut symbols)).collect();
                let effects: Vec<_> = a.produced.iter().map(|e| rename(e, &mut symbols)).collect();
                if aliases != step.aliases || requires != step.requires || effects != step.effects {
                    return Err("alias/effect mismatch".into());
                }
            }
            let expected_contexts: BTreeSet<_> = u
                .members
                .iter()
                .flat_map(|e| store.occurrences[*e].apply.parents.iter())
                .filter(|e| !local.contains_key(e))
                .copied()
                .collect();
            let expected_external: BTreeSet<_> = u
                .members
                .iter()
                .flat_map(|e| store.occurrences[*e].apply.external_facts.iter().cloned())
                .collect();
            let mut actual_contexts = BTreeSet::new();
            let mut actual_external = BTreeSet::new();
            for r in &u.contexts {
                match r {
                    Reference::ResidualContext(e) => {
                        actual_contexts.insert(*e);
                    }
                    Reference::UseContext { instance, member } if *instance < uid => {
                        actual_contexts.insert(self.uses[*instance].members[*member]);
                    }
                    Reference::ExternalEffect(e) => {
                        actual_external.insert(e.clone());
                    }
                    _ => return Err("invalid context reference".into()),
                }
            }
            if expected_contexts != actual_contexts || expected_external != actual_external {
                return Err("context/effect evidence changed".into());
            }
            let mut parts = vec![];
            for p in &u.parts {
                match p {
                    Part::Residual(e) => parts.push(*e),
                    Part::Use(old) if *old < uid => parts.extend(&self.uses[*old].members),
                    _ => return Err("cyclic Use".into()),
                }
            }
            parts.sort();
            let mut members = u.members.clone();
            members.sort();
            if parts != members {
                return Err("Use parts do not expand to its members".into());
            }
        }
        Ok(())
    }
    pub fn report(&self) -> serde_json::Value {
        let covered = self.owner.iter().filter(|o| o.is_some()).count();
        let used_templates: BTreeSet<_> = self
            .active_uses
            .iter()
            .map(|u| self.uses[*u].template)
            .collect();
        let dictionary_units: usize = used_templates
            .iter()
            .map(|t| definition_cost(&self.templates[*t].pattern))
            .sum();
        let candidate_index_units: usize = self
            .templates
            .iter()
            .map(|t| definition_cost(&t.pattern))
            .sum();
        let roots: Vec<_> = self
            .owner
            .iter()
            .enumerate()
            .filter_map(|(i, o)| o.is_none().then_some(Part::Residual(i)))
            .chain(self.active_uses.iter().copied().map(Part::Use))
            .collect();
        serde_json::json!({"scope":"Online exact dictionary coding of witnessed composition wiring. Candidate and convex-cover heuristic, not globally optimal. Native payload/effect history remains retained; units are model costs, not bytes or tier0 speedups.",
            "mode":if self.layer_restricted{"coarse_frontier_restricted"}else{"interface_only"},"schemas":self.schemas,"templates":self.templates,"uses":self.uses,"residuals":self.residuals,"roots":roots,
            "stats":{"cut_passes":self.cut_passes,"cut_visits":self.cut_visits,"cut_removed_uses":self.cut_removed_uses,"events":self.owner.len(),"covered_events":covered,"residual_events":self.owner.len()-covered,"active_uses":self.active_uses.len(),"confirmed_uses":self.uses.len(),"templates":self.templates.len(),"used_templates":used_templates.len(),"candidate_queries":self.candidate_queries,"matches":self.matches,"rejected_overlap":self.rejected_overlap,"rejected_nonconvex":self.rejected_nonconvex,"budget_stops":self.budget_stops,"continuations_through_use":self.continuations_through_use,"interior_port_reads":self.interior_port_reads,"raw_wiring_units":self.raw_wiring_cost,"selected_wiring_units":self.selected_wiring_cost,"used_dictionary_units":dictionary_units,"candidate_index_units":candidate_index_units,"net_selected_codec_units":self.raw_wiring_cost as i64-self.selected_wiring_cost as i64-dictionary_units as i64,"net_units_including_candidate_index":self.raw_wiring_cost as i64-self.selected_wiring_cost as i64-candidate_index_units as i64}})
    }
    pub fn replay(store: &LayerStore, layer_restricted: bool) -> Self {
        let mut prefix = LayerStore::default();
        let mut result = Self {
            layer_restricted,
            ..Self::default()
        };
        // Used for the end-to-end admission ablation; the same certified applies
        // and precomputed layer IDs, without recursively constructing another codec.
        for o in &store.occurrences {
            prefix.occurrences.push(o.clone());
            result.push(&prefix);
        }
        result
    }
}
fn definition_cost(p: &Pattern) -> usize {
    p.steps
        .iter()
        .map(|s| {
            4 + s.wiring.len() * 3 + s.internal_parents.len() + s.requires.len() + s.effects.len()
        })
        .sum()
}
fn raw_cost(a: &crate::coarse_smooth::Apply) -> usize {
    4 + a.binding.len() * 3 + a.parents.len() + a.required.len() + a.produced.len()
}
fn symbol(v: usize, map: &mut BTreeMap<usize, usize>) -> usize {
    let n = map.len();
    *map.entry(v).or_insert(n)
}
fn rename(e: &Effect, map: &mut BTreeMap<usize, usize>) -> Effect {
    match e {
        Effect::RowFact(v) => Effect::RowFact(symbol(*v, map)),
        Effect::Equal(a, b) => Effect::Equal(symbol(*a, map), symbol(*b, map)),
    }
}
fn convex(s: &LayerStore, set: &BTreeSet<usize>) -> Option<bool> {
    let min = *set.first()?;
    let mut seen = BTreeSet::new();
    let mut todo = vec![];
    for i in set {
        todo.extend(
            s.occurrences[*i]
                .apply
                .parents
                .iter()
                .filter(|p| !set.contains(p))
                .copied(),
        );
    }
    while let Some(i) = todo.pop() {
        if i < min || !seen.insert(i) {
            continue;
        }
        if seen.len() > 4096 {
            return None;
        }
        if set.contains(&i) {
            return Some(false);
        }
        todo.extend(&s.occurrences[i].apply.parents);
    }
    Some(true)
}
fn order(s: &LayerStore, set: &BTreeSet<usize>, root: usize) -> Vec<usize> {
    fn visit(
        s: &LayerStore,
        set: &BTreeSet<usize>,
        i: usize,
        seen: &mut BTreeSet<usize>,
        out: &mut Vec<usize>,
    ) {
        if !set.contains(&i) || !seen.insert(i) {
            return;
        }
        let a = &s.occurrences[i].apply;
        let mut ps = vec![];
        for b in &a.binding {
            if let RelativeBinding::ParentPort { parent, .. } = b {
                let p = a.parents[*parent];
                if !ps.contains(&p) {
                    ps.push(p);
                }
            }
        }
        let mut extra: Vec<_> = a
            .parents
            .iter()
            .copied()
            .filter(|p| !ps.contains(p))
            .collect();
        extra.sort_by_key(|p| (&s.occurrences[*p].apply.rule, *p));
        ps.extend(extra);
        for p in ps {
            visit(s, set, p, seen, out);
        }
        out.push(i);
    }
    let mut fingerprints = BTreeMap::new();
    for i in set {
        let a = &s.occurrences[*i].apply;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        a.rule.hash(&mut h);
        a.input_roles.hash(&mut h);
        a.output_roles.hash(&mut h);
        for b in &a.binding {
            match b {
                RelativeBinding::ParentPort { parent, output }
                    if set.contains(&a.parents[*parent]) =>
                {
                    fingerprints
                        .get(&a.parents[*parent])
                        .copied()
                        .unwrap_or(0u64)
                        .hash(&mut h);
                    output.hash(&mut h);
                }
                _ => 0u64.hash(&mut h),
            }
        }
        fingerprints.insert(*i, h.finish());
    }
    let mut seen = BTreeSet::new();
    let mut out = vec![];
    visit(s, set, root, &mut seen, &mut out);
    let mut remaining: Vec<_> = set.iter().copied().filter(|i| !seen.contains(i)).collect();
    remaining.sort_by_key(|i| (fingerprints[i], *i));
    for i in remaining {
        visit(s, set, i, &mut seen, &mut out);
    }
    out
}
