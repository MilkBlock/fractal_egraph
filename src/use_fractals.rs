//! Finite recurrence evidence between immutable Use(T) instances.
//! No unions, induction claims, or endpoint-only equivalence are inferred here.
use crate::coarse_smooth::{canonical_effect as effect, symbol};
use crate::{
    coarse_smooth::{Effect, LayerStore},
    comb_reuse::{Reference, ReuseStore},
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
const SOURCES_PER_EVENT: usize = 32;
const SOURCES_PER_USE: usize = 64;
const EDGE_LIMIT: usize = 16384;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum BindingLink {
    Return { member: usize, output: usize },
    Carry { input: usize },
    External { slot: usize },
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum ContextLink {
    Produced { member: usize },
    Carry { context: usize },
    Effect { member: usize, effect: usize },
    External { slot: usize, kind: String },
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Transfer {
    pub from_template: usize,
    pub to_template: usize,
    pub inputs: Vec<BindingLink>,
    pub contexts: Vec<ContextLink>,
    pub external_input_contexts: Vec<Option<ContextLink>>,
    /// Canonical equality partition, including concrete source and target values.
    pub aliases: Vec<usize>,
    pub external_effects: Vec<Effect>,
}
#[derive(Clone, Serialize)]
pub struct Transition {
    pub transfer: Transfer,
    pub witnesses: Vec<[usize; 2]>,
    pub fresh_inputs: usize,
    pub fresh_contexts: usize,
}
#[derive(Clone, Serialize)]
pub struct Family {
    pub template: usize,
    pub transitions: Vec<usize>,
    pub instances: Vec<usize>,
    pub edges: Vec<[usize; 3]>, // source Use, target Use, transition
    pub triggers: Vec<usize>,
    pub witness_chain: Vec<usize>,
    pub observed_depth: usize,
    pub branching_instances: Vec<usize>,
    pub external_demand_observed: bool,
    pub status: String,
}
#[derive(Default, Serialize)]
pub struct Analysis {
    pub transitions: Vec<Transition>,
    pub families: Vec<Family>,
    pub uses_examined: usize,
    pub unlinked_uses: usize,
    pub overlap_rejections: usize,
    pub truncated_sources: usize,
    pub truncated_edges: bool,
    pub truncated_paths: bool,
    pub truncated_branches: bool,
    pub scope: String,
}
fn physical(r: &ReuseStore, p: &Reference) -> Reference {
    r.physical(p).expect("validated Use reference")
}
fn event(p: &Reference) -> Option<usize> {
    match p {
        Reference::ResidualPort { event, .. } | Reference::ResidualContext(event) => Some(*event),
        _ => None,
    }
}
fn value(s: &LayerStore, p: &Reference) -> usize {
    match p {
        Reference::ResidualPort { event, output } => s.occurrences[*event].apply.outputs[*output],
        Reference::External(v) => *v,
        _ => unreachable!("Use inputs are value references"),
    }
}
fn transfer(s: &LayerStore, from: usize, to: usize) -> Transfer {
    let r = &s.reuse;
    let a = &r.uses[from];
    let b = &r.uses[to];
    let members: BTreeMap<_, _> = a.members.iter().enumerate().map(|(m, e)| (*e, m)).collect();
    let prev_inputs: Vec<_> = a.inputs.iter().map(|p| physical(r, p)).collect();
    let prev_contexts: Vec<_> = a.contexts.iter().map(|p| physical(r, p)).collect();
    let mut external = BTreeMap::new();
    let inputs = b
        .inputs
        .iter()
        .map(|p| {
            let p = physical(r, p);
            if let Reference::ResidualPort { event, output } = &p {
                if let Some(m) = members.get(event) {
                    return BindingLink::Return {
                        member: *m,
                        output: *output,
                    };
                }
            }
            if let Some(input) = prev_inputs.iter().position(|x| *x == p) {
                return BindingLink::Carry { input };
            }
            let n = external.len();
            BindingLink::External {
                slot: *external.entry(p).or_insert(n),
            }
        })
        .collect();
    let mut extra_contexts = BTreeMap::new();
    let mut contexts: Vec<ContextLink> = b
        .contexts
        .iter()
        .map(|p| {
            let p = physical(r, p);
            if let Reference::ResidualContext(e) = &p {
                if let Some(m) = members.get(e) {
                    return ContextLink::Produced { member: *m };
                }
            }
            if let Some(context) = prev_contexts.iter().position(|x| *x == p) {
                return ContextLink::Carry { context };
            }
            if let Reference::ExternalEffect(e) = &p {
                for (m, i) in a.members.iter().enumerate() {
                    if let Some(k) = s.occurrences[*i].apply.produced.iter().position(|x| x == e) {
                        return ContextLink::Effect {
                            member: m,
                            effect: k,
                        };
                    }
                }
            }
            let kind = match &p {
                Reference::ExternalEffect(Effect::Equal(..)) => "equality",
                Reference::ExternalEffect(Effect::RowFact(_)) => "row",
                _ => "producer-context",
            }
            .to_string();
            let n = extra_contexts.len();
            ContextLink::External {
                slot: *extra_contexts.entry(p).or_insert(n),
                kind,
            }
        })
        .collect();
    let context_map: BTreeMap<_, _> = b
        .contexts
        .iter()
        .map(|p| physical(r, p))
        .zip(contexts.iter().cloned())
        .collect();
    let mut external_input_contexts = vec![None; external.len()];
    for (p, slot) in &external {
        if let Some(e) = event(p) {
            external_input_contexts[*slot] =
                context_map.get(&Reference::ResidualContext(e)).cloned();
        }
    }
    // Proof-context order is not an apply position. Preserve role links above,
    // then remove accidental ordering from concrete event IDs.
    contexts.sort();
    let mut names = BTreeMap::new();
    let mut aliases = vec![];
    for u in [a, b] {
        for p in &u.inputs {
            aliases.push(symbol(value(s, &physical(r, p)), &mut names));
        }
        for e in &u.members {
            for v in &s.occurrences[*e].apply.outputs {
                aliases.push(symbol(*v, &mut names));
            }
        }
    }
    let external_effects = a
        .contexts
        .iter()
        .chain(&b.contexts)
        .filter_map(|p| {
            if let Reference::ExternalEffect(e) = p {
                Some(effect(e, &mut names))
            } else {
                None
            }
        })
        .collect();
    Transfer {
        from_template: a.template,
        to_template: b.template,
        inputs,
        contexts,
        external_input_contexts,
        aliases,
        external_effects,
    }
}
/// Includes historical Uses, not just the compression-selected cut. Each edge
/// is producer-anchored and pairs disjoint instances; the best chain additionally
/// requires all its instances to be mutually disjoint.
pub fn analyze(s: &LayerStore) -> Analysis {
    let r = &s.reuse;
    let mut a=Analysis{uses_examined:r.uses.len(),scope:"Observed Use-template transfers, exact physical ports and alias partitions. All historical Uses considered within budgets; no arbitrary-depth induction, no guard solver or recurrence reduction. One best disjoint witness path per endpoint; alternatives can be missed.".into(),..Default::default()};
    let mut index = vec![vec![]; s.occurrences.len()];
    let mut keys = BTreeMap::new();
    let mut connected = BTreeSet::new();
    let mut edge_count = 0;
    for (to, b) in r.uses.iter().enumerate() {
        let mut sources = BTreeSet::new();
        for p in b.inputs.iter().chain(&b.contexts) {
            if let Some(e) = event(&physical(r, p)) {
                let owners: &Vec<usize> = &index[e];
                a.truncated_sources += owners.len().saturating_sub(SOURCES_PER_EVENT);
                sources.extend(owners.iter().rev().take(SOURCES_PER_EVENT).copied());
            }
        }
        a.truncated_sources += sources.len().saturating_sub(SOURCES_PER_USE);
        for from in sources.into_iter().rev().take(SOURCES_PER_USE) {
            if edge_count == EDGE_LIMIT {
                a.truncated_edges = true;
                break;
            }
            let x = &r.uses[from];
            if x.members.iter().any(|e| b.members.contains(e)) {
                a.overlap_rejections += 1;
                continue;
            }
            let t = transfer(s, from, to);
            // Context-only overlap is not a producer output feeding this binding.
            if !t
                .inputs
                .iter()
                .any(|p| matches!(p, BindingLink::Return { .. }))
                && !t
                    .contexts
                    .iter()
                    .any(|p| matches!(p, ContextLink::Produced { .. } | ContextLink::Effect { .. }))
            {
                continue;
            }
            let n = a.transitions.len();
            let k = *keys.entry(t.clone()).or_insert_with(|| {
                let fresh_inputs = t
                    .inputs
                    .iter()
                    .filter(|p| matches!(p, BindingLink::External { .. }))
                    .count();
                let fresh_contexts = t
                    .contexts
                    .iter()
                    .filter(|p| matches!(p, ContextLink::External { .. }))
                    .count();
                a.transitions.push(Transition {
                    transfer: t,
                    fresh_inputs,
                    fresh_contexts,
                    witnesses: vec![],
                });
                n
            });
            a.transitions[k].witnesses.push([from, to]);
            connected.extend([from, to]);
            edge_count += 1;
        }
        for e in &b.members {
            index[*e].push(to);
        }
    }
    a.unlinked_uses = r.uses.len() - connected.len();
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for (i, t) in a.transitions.iter().enumerate() {
        if t.transfer.from_template == t.transfer.to_template && t.witnesses.len() >= 2 {
            groups.entry(t.transfer.from_template).or_default().push(i);
        }
    }
    for (template, transitions) in groups {
        let mut edges: Vec<_> = transitions
            .iter()
            .flat_map(|t| {
                a.transitions[*t]
                    .witnesses
                    .iter()
                    .map(|[u, v]| [*u, *v, *t])
            })
            .collect();
        edges.sort_by_key(|e| (e[1], e[0], e[2]));
        let instances: BTreeSet<_> = edges.iter().flat_map(|e| [e[0], e[1]]).collect();
        let targets: BTreeSet<_> = edges.iter().map(|e| e[1]).collect();
        let mut paths: BTreeMap<usize, Vec<usize>> =
            instances.iter().map(|u| (*u, vec![*u])).collect();
        let mut children = BTreeMap::<usize, BTreeSet<usize>>::new();
        for [u, v, _] in &edges {
            let p = &paths[u];
            if p.len() >= 64 {
                a.truncated_paths = true;
                continue;
            }
            if p.iter().any(|x| {
                r.uses[*x]
                    .members
                    .iter()
                    .any(|e| r.uses[*v].members.contains(e))
            }) {
                continue;
            }
            if p.len() + 1 > paths[v].len() {
                let mut next = p.clone();
                next.push(*v);
                paths.insert(*v, next);
            }
            children.entry(*u).or_default().insert(*v);
        }
        let chain = paths
            .values()
            .max_by_key(|p| p.len())
            .cloned()
            .unwrap_or_default();
        if chain.len() < 3 {
            continue;
        }
        // Count only pairwise-disjoint successors as actual branching evidence.
        let branching_instances = children
            .into_iter()
            .filter_map(|(u, vs)| {
                let mut accepted: Vec<usize> = vec![];
                if vs.len() > 64 {
                    a.truncated_branches = true;
                }
                for v in vs.into_iter().take(64) {
                    if accepted.iter().all(|x| {
                        r.uses[*x]
                            .members
                            .iter()
                            .all(|e| !r.uses[v].members.contains(e))
                    }) {
                        accepted.push(v);
                    }
                }
                (accepted.len() > 1).then_some(u)
            })
            .collect();
        let external_demand_observed = transitions
            .iter()
            .any(|t| a.transitions[*t].fresh_inputs + a.transitions[*t].fresh_contexts > 0);
        a.families.push(Family {
            template,
            transitions,
            instances: instances.iter().copied().collect(),
            triggers: instances.difference(&targets).copied().collect(),
            edges,
            observed_depth: chain.len(),
            witness_chain: chain,
            branching_instances,
            external_demand_observed,
            status: "observed_transfer_set_recurrence_candidate; unbounded_induction_unknown"
                .into(),
        });
    }
    a
}
