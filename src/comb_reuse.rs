//! Online, producer-anchored dictionary coding of composition wiring.
//! This is an alternative metadata representation, not skipped tier0 execution.
use crate::coarse_smooth::{Effect, LayerStore, RelativeBinding};
use crate::coarse_smooth::{canonical_effect as rename, symbol};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{Hash, Hasher};

const MAX_MEMBERS: usize = 16;
const MAX_CANDIDATES: usize = 8;
const MAX_TEMPLATES: usize = 4096;
const PROBATION_LIMIT: usize = 128;
const PROBATION_UNITS: usize = 8192;
const CUT_ROOT_LIMIT: usize = 256;
#[derive(Clone)]
struct Probation {
    pattern: Pattern,
    first: usize,
    last: usize,
    credit: usize,
    count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Step {
    pub schema: usize,
    pub wiring: Vec<Wire>,
    pub internal_parents: Vec<usize>,
    pub aliases: Vec<usize>,
    pub requires: Vec<Effect>,
    pub effects: Vec<Effect>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
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
    keys: HashMap<u64, Vec<usize>>,
    #[serde(skip)]
    probation: Vec<Probation>,
    #[serde(skip)]
    active_by_template: Vec<BTreeSet<usize>>,
    #[serde(skip)]
    dirty_templates: BTreeSet<usize>,
    #[serde(skip)]
    dirty_queue: VecDeque<usize>,
    pub probation_evictions: usize,
    pub local_rotations: usize,
    pub cut_budget_stops: usize,
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
    displaced: BTreeSet<usize>,
    spill: Vec<usize>,
    spill_cost: usize,
}
impl ReuseStore {
    fn lookup(&self, p: &Pattern) -> Option<usize> {
        self.keys
            .get(&fingerprint(p))?
            .iter()
            .copied()
            .find(|t| self.templates[*t].pattern == *p)
    }
    fn activate(&mut self, u: usize) {
        let t = self.uses[u].template;
        self.active_uses.insert(u);
        self.active_by_template[t].insert(u);
        if self.dirty_templates.insert(t) {
            self.dirty_queue.push_back(t);
        }
    }
    fn deactivate(&mut self, u: usize) {
        let t = self.uses[u].template;
        self.active_uses.remove(&u);
        self.active_by_template[t].remove(&u);
        if self.dirty_templates.insert(t) {
            self.dirty_queue.push_back(t);
        }
    }
    /// Resolve legacy/display references without changing physical provenance.
    pub fn physical(&self, r: &Reference) -> Option<Reference> {
        Some(match r {
            Reference::UsePort {
                instance,
                member,
                output,
            } => Reference::ResidualPort {
                event: *self.uses.get(*instance)?.members.get(*member)?,
                output: *output,
            },
            Reference::UseContext { instance, member } => {
                Reference::ResidualContext(*self.uses.get(*instance)?.members.get(*member)?)
            }
            r => r.clone(),
        })
    }
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
            // Local boundary rotation: permit cutting through a prior macro.
            if std::env::var_os("EGG_LAYOUT_DISABLE_ROTATIONS").is_none() {
                regions.insert(BTreeSet::from([*p, i]));
            }
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
            if let Some(t) = self.lookup(&c.pattern) {
                self.matches += 1;
                self.templates[t].matches += 1;
                let gain = c.old_cost.saturating_sub(c.new_cost + c.spill_cost);
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
            for old in &c.displaced {
                self.deactivate(*old);
            }
            for e in &c.spill {
                self.owner[*e] = None;
            }
            self.local_rotations += usize::from(!c.spill.is_empty());
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
            self.activate(u);
            self.selected_wiring_cost -= gain;
        }
        // At most the bounded observed candidates enter the dictionary. Existing
        // equivalent definitions may gain a smaller recipe referencing prior T's.
        for c in candidates {
            if let Some(t) = self.lookup(&c.pattern) {
                let old = &mut self.templates[t];
                old.observed += 1;
                if c.atoms.len() < old.atoms.len() {
                    old.atoms = c.atoms;
                }
            } else {
                let eager = std::env::var_os("EGG_LAYOUT_EAGER_DICTIONARY").is_some();
                let credit = c.old_cost.saturating_sub(c.new_cost + c.spill_cost);
                let cost = definition_cost(&c.pattern);
                let mut first = i;
                let mut count = 1;
                let mut total_credit = credit;
                if !eager {
                    if let Some(k) = self.probation.iter().position(|p| p.pattern == c.pattern) {
                        let p = &mut self.probation[k];
                        p.last = i;
                        p.count += 1;
                        p.credit += credit;
                        if p.credit < cost {
                            continue;
                        }
                        let p = self.probation.swap_remove(k);
                        first = p.first;
                        count = p.count;
                        total_credit = p.credit;
                    } else {
                        if credit == 0 || cost > PROBATION_UNITS {
                            continue;
                        }
                        while self.probation.len() >= PROBATION_LIMIT
                            || self
                                .probation
                                .iter()
                                .map(|p| definition_cost(&p.pattern))
                                .sum::<usize>()
                                + cost
                                > PROBATION_UNITS
                        {
                            let k = self
                                .probation
                                .iter()
                                .enumerate()
                                .min_by_key(|(_, p)| p.last)
                                .unwrap()
                                .0;
                            self.probation.swap_remove(k);
                            self.probation_evictions += 1;
                        }
                        self.probation.push(Probation {
                            pattern: c.pattern,
                            first: i,
                            last: i,
                            credit,
                            count: 1,
                        });
                        continue;
                    }
                }
                if self.templates.len() >= MAX_TEMPLATES {
                    self.budget_stops += 1;
                    continue;
                }
                let t = self.templates.len();
                self.keys
                    .entry(fingerprint(&c.pattern))
                    .or_default()
                    .push(t);
                self.active_by_template.push(BTreeSet::new());
                self.templates.push(Template {
                    pattern: c.pattern,
                    atoms: c.atoms,
                    learned_at: first,
                    observed: count,
                    matches: 0,
                    admission_credit: if eager { 0 } else { total_credit },
                });
            }
        }
        if (i + 1) % 64 == 0 && std::env::var_os("EGG_LAYOUT_DISABLE_RECUT").is_none() {
            self.repartition(store);
        }
    }
    /// Incremental dictionary-aware cut over affected decomposition roots.
    /// Exact additive DP inside each root; bounded coordinate search across the
    /// shared dictionary. A high-fanout update is skipped, never called O(log n).
    pub fn repartition(&mut self, store: &LayerStore) {
        let count = self.dirty_queue.len().min(8);
        let trials: Vec<_> = self.dirty_queue.drain(..count).collect();
        self.cut_passes += 1;
        for t in trials {
            self.dirty_templates.remove(&t);
            if self.active_by_template[t].len() > CUT_ROOT_LIMIT {
                self.cut_budget_stops += 1;
                continue;
            }
            let roots: Vec<_> = self.active_by_template[t].iter().copied().collect();
            if roots.is_empty() {
                continue;
            }
            fn dp(
                r: &ReuseStore,
                s: &LayerStore,
                u: usize,
                banned: usize,
                memo: &mut BTreeMap<usize, (usize, bool)>,
            ) -> usize {
                if let Some((cost, _)) = memo.get(&u) {
                    return *cost;
                }
                let split: usize = r.uses[u]
                    .parts
                    .iter()
                    .map(|p| match p {
                        Part::Residual(e) => raw_cost(&s.occurrences[*e].apply),
                        Part::Use(v) => dp(r, s, *v, banned, memo),
                    })
                    .sum();
                let take = r.uses[u].template != banned && r.uses[u].wiring_cost < split;
                let cost = if take { r.uses[u].wiring_cost } else { split };
                memo.insert(u, (cost, take));
                cost
            }
            let old: usize = roots.iter().map(|u| self.uses[*u].wiring_cost).sum();
            let mut memo = BTreeMap::new();
            let new: usize = roots
                .iter()
                .map(|u| dp(self, store, *u, t, &mut memo))
                .sum();
            self.cut_visits += memo.len();
            let mut selected = BTreeSet::new();
            let mut todo = roots.clone();
            while let Some(u) = todo.pop() {
                if memo[&u].1 {
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
            let mut counts = BTreeMap::<usize, usize>::new();
            for u in &selected {
                *counts.entry(self.uses[*u].template).or_default() += 1;
            }
            let added_dict: usize = counts
                .keys()
                .filter(|t| self.active_by_template[**t].is_empty())
                .map(|t| definition_cost(&self.templates[*t].pattern))
                .sum();
            let removed_dict = definition_cost(&self.templates[t].pattern);
            if new + added_dict >= old + removed_dict {
                continue;
            }
            self.cut_removed_uses += roots.len();
            for u in roots {
                self.deactivate(u);
                for e in &self.uses[u].members {
                    self.owner[*e] = None;
                }
            }
            for u in selected {
                self.activate(u);
                for (m, e) in self.uses[u].members.iter().enumerate() {
                    self.owner[*e] = Some((u, m));
                }
            }
            self.selected_wiring_cost = self.selected_wiring_cost - old + new;
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
        let displaced: BTreeSet<_> = members
            .iter()
            .filter_map(|e| self.owner[*e].map(|(u, _)| u))
            .collect();
        let spill: Vec<_> = displaced
            .iter()
            .flat_map(|u| self.uses[*u].members.iter().copied())
            .filter(|e| !set.contains(e))
            .collect();
        let spill_cost = spill
            .iter()
            .map(|e| raw_cost(&store.occurrences[*e].apply))
            .sum();
        let old_cost = displaced
            .iter()
            .map(|u| self.uses[*u].wiring_cost)
            .sum::<usize>()
            + members
                .iter()
                .filter(|e| self.owner[**e].is_none())
                .map(|e| raw_cost(&store.occurrences[*e].apply))
                .sum::<usize>();
        for (m, event) in members.iter().enumerate() {
            if let Some((u, _)) = self.owner[*event]
                .filter(|(u, _)| self.uses[*u].members.iter().all(|e| set.contains(e)))
            {
                if used.insert(u) {
                    atoms.push(Atom::Use {
                        template: self.uses[u].template,
                        members: self.uses[u].members.iter().map(|e| local[e]).collect(),
                    });
                    parts.push(Part::Use(u));
                }
            } else {
                atoms.push(Atom::Apply(m));
                parts.push(Part::Residual(*event));
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
            displaced,
            spill,
            spill_cost,
        }
    }
    /// Replay the compact wiring against immutable occurrence evidence, without
    /// executing rules. This checks ports, sharing, requirements and union effects.
    pub fn verify(&self, store: &LayerStore) -> Result<(), String> {
        let mut owners = vec![None; store.occurrences.len()];
        let mut reverse = vec![BTreeSet::new(); self.templates.len()];
        let mut selected_cost = 0;
        for id in &self.active_uses {
            let u = self.uses.get(*id).ok_or("invalid active Use")?;
            reverse
                .get_mut(u.template)
                .ok_or("invalid template")?
                .insert(*id);
            selected_cost += u.wiring_cost;
            for (m, e) in u.members.iter().enumerate() {
                if owners
                    .get_mut(*e)
                    .ok_or("invalid member")?
                    .replace((*id, m))
                    .is_some()
                {
                    return Err("overlapping preferred Uses".into());
                }
            }
        }
        if owners != self.owner || reverse != self.active_by_template {
            return Err("cover/reverse index drift".into());
        }
        selected_cost += owners
            .iter()
            .zip(&store.occurrences)
            .filter(|(o, _)| o.is_none())
            .map(|(_, o)| raw_cost(&o.apply))
            .sum::<usize>();
        if selected_cost != self.selected_wiring_cost {
            return Err("selected cost drift".into());
        }
        for (uid, u) in self.uses.iter().enumerate() {
            let t = &self.templates[u.template];
            if t.learned_at >= u.completed_at {
                return Err("Use learned from its own future".into());
            }
            let members: BTreeSet<_> = u.members.iter().copied().collect();
            if members.len() != u.members.len()
                || members.iter().any(|e| *e >= store.occurrences.len())
            {
                return Err("invalid Use members".into());
            }
            let root = *u.members.get(t.pattern.root).ok_or("invalid Use root")?;
            // Reconstruct the same canonical interface from immutable applies.
            // Only pattern/ports/cost matter here, never the current cut's parts.
            let expected = self.candidate(store, &members, root);
            for e in &u.members {
                let a = &store.occurrences[*e].apply;
                let schema = &self.schemas[self.event_schema[*e]];
                if a.rule != schema.rule
                    || a.input_roles != schema.input_roles
                    || a.output_roles != schema.output_roles
                {
                    return Err("Use source/role mismatch".into());
                }
            }
            let resolve = |refs: &[Reference]| -> Result<Vec<Reference>, String> {
                refs.iter().map(|r| {
                    if matches!(r, Reference::UsePort{instance,..}|Reference::UseContext{instance,..} if *instance >= uid) {
                        return Err("cyclic Use reference".into());
                    }
                    self.physical(r).ok_or_else(|| "invalid Use reference".into())
                }).collect()
            };
            if expected.pattern != t.pattern || expected.members != u.members {
                return Err("Use pattern/alias/effect mismatch".into());
            }
            if resolve(&u.inputs)? != expected.inputs
                || resolve(&u.contexts)?.into_iter().collect::<BTreeSet<_>>()
                    != expected.contexts.into_iter().collect()
            {
                return Err("boundary/context provenance changed".into());
            }
            if u.wiring_cost != expected.new_cost {
                return Err("Use wiring cost drift".into());
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
        let probation_units: usize = self
            .probation
            .iter()
            .map(|p| definition_cost(&p.pattern))
            .sum();
        let candidate_index_units: usize = self
            .templates
            .iter()
            .map(|t| definition_cost(&t.pattern))
            .sum::<usize>()
            + probation_units;
        let roots: Vec<_> = self
            .owner
            .iter()
            .enumerate()
            .filter_map(|(i, o)| o.is_none().then_some(Part::Residual(i)))
            .chain(self.active_uses.iter().copied().map(Part::Use))
            .collect();
        serde_json::json!({"scope":"Online exact dictionary coding of witnessed composition wiring. Candidate and convex-cover heuristic, not globally optimal. Native payload/effect history remains retained; units are model costs, not bytes or tier0 speedups.",
            "mode":if self.layer_restricted{"coarse_frontier_restricted"}else{"interface_only"},"schemas":self.schemas,"templates":self.templates,"uses":self.uses,"residuals":self.residuals,"roots":roots,
            "stats":{"probation_templates":self.probation.len(),"probation_units":probation_units,"probation_evictions":self.probation_evictions,"local_rotations":self.local_rotations,"cut_budget_stops":self.cut_budget_stops,"cut_passes":self.cut_passes,"cut_visits":self.cut_visits,"cut_removed_uses":self.cut_removed_uses,"events":self.owner.len(),"covered_events":covered,"residual_events":self.owner.len()-covered,"active_uses":self.active_uses.len(),"confirmed_uses":self.uses.len(),"templates":self.templates.len(),"used_templates":used_templates.len(),"candidate_queries":self.candidate_queries,"matches":self.matches,"rejected_overlap":self.rejected_overlap,"rejected_nonconvex":self.rejected_nonconvex,"budget_stops":self.budget_stops,"continuations_through_use":self.continuations_through_use,"interior_port_reads":self.interior_port_reads,"raw_wiring_units":self.raw_wiring_cost,"selected_wiring_units":self.selected_wiring_cost,"used_dictionary_units":dictionary_units,"candidate_index_units":candidate_index_units,"net_selected_codec_units":self.raw_wiring_cost as i64-self.selected_wiring_cost as i64-dictionary_units as i64,"net_units_including_candidate_index":self.raw_wiring_cost as i64-self.selected_wiring_cost as i64-candidate_index_units as i64}})
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

fn fingerprint(p: &Pattern) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprint_collision_never_establishes_template_equality() {
        let mut r = ReuseStore::default();
        let p = Pattern {
            steps: vec![],
            root: 0,
            inputs: 1,
        };
        let q = Pattern {
            inputs: 2,
            ..p.clone()
        };
        r.templates.push(Template {
            pattern: p,
            atoms: vec![],
            learned_at: 0,
            observed: 1,
            matches: 0,
            admission_credit: 0,
        });
        // Deliberately put a different pattern in q's fingerprint bucket.
        r.keys.insert(fingerprint(&q), vec![0]);
        assert_eq!(r.lookup(&q), None);
    }
}
