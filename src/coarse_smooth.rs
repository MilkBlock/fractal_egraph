//! Rust-owned, witness-driven coarse/smooth layers. No prefix admission gate.
//! A layer is a reusable dependency interface, not an execution round.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RelativeBinding {
    ParentPort { parent: usize, output: usize },
    External { slot: usize },
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Effect {
    RowFact(usize),
    Equal(usize, usize),
}

/// IDs in values/effects identify typed values or exact row versions, never just
/// current e-class representatives. Rules include their literal/guard semantics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Apply {
    pub event: u64,
    pub rule: String,
    pub input_roles: Vec<String>,
    pub output_roles: Vec<String>,
    pub parents: Vec<usize>,
    pub binding: Vec<RelativeBinding>,
    pub wanted: Vec<usize>,
    pub outputs: Vec<usize>,
    pub external: Vec<usize>,
    pub required: Vec<Effect>,
    pub external_facts: Vec<Effect>,
    pub produced: Vec<Effect>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum CombKind {
    CoarseComb,
    SmoothComb,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Comb {
    pub kind: CombKind,
    pub rule: String,
    pub input_roles: Vec<String>,
    pub output_roles: Vec<String>,
    pub parents: Vec<usize>,
    pub binding: Vec<RelativeBinding>,
    /// Alias pattern across inputs, outputs, external values and effect operands.
    pub aliases: Vec<usize>,
    pub input_count: usize,
    pub output_count: usize,
    pub external_count: usize,
    pub requirements: Vec<Effect>,
    pub effects: Vec<Effect>,
    pub external_facts: Vec<Effect>,
}
#[derive(Clone, Debug, Serialize)]
pub struct CoarseLayer {
    /// Actual coarse apply occurrences, not distinct rule names.
    pub members: Vec<usize>,
    /// Smaller interfaces are retained; a merge never destroys either child.
    pub children: Vec<usize>,
    /// A real smooth consumer that jointly uses every member, absent for seeds.
    pub witness: Option<usize>,
}
#[derive(Clone, Debug, Serialize)]
pub struct SmoothLayer {
    pub coarse_layer: usize,
    pub members: Vec<usize>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Occurrence {
    pub apply: Apply,
    pub comb: usize,
    pub coarse_layer: usize,
    pub smooth_layer: Option<usize>,
    /// A cross-layer dependency forced a new explicit coarse interface.
    pub boundary_restart: bool,
}
#[derive(Default, Serialize)]
pub struct LayerStore {
    pub reuse: crate::comb_reuse::ReuseStore,
    pub combs: Vec<Comb>,
    pub occurrences: Vec<Occurrence>,
    pub coarse_layers: Vec<CoarseLayer>,
    pub smooth_layers: Vec<SmoothLayer>,
    #[serde(skip)]
    comb_index: BTreeMap<Comb, usize>,
    #[serde(skip)]
    coarse_index: BTreeMap<Vec<usize>, usize>,
    #[serde(skip)]
    smooth_index: BTreeMap<usize, usize>,
    #[serde(skip)]
    events: BTreeMap<u64, usize>,
}
impl LayerStore {
    pub fn occurrence(&self, event: u64) -> Option<&Occurrence> {
        self.events.get(&event).map(|i| &self.occurrences[*i])
    }
    /// Structural report, not another complete value/effect history dump.
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "schema":"coarse-smooth-layers/v1", "backend":"rust",
            "scope":"Observed dependency interfaces; shared definitions, occurrence-specific evidence. No prefix/block admission and no arbitrary subtree enumeration. Layer IDs are run-local, not semantic identifiers.",
            "reuse":self.reuse.report(), "combs":self.combs, "coarse_layers":self.coarse_layers, "smooth_layers":self.smooth_layers,
            "occurrences":self.occurrences.iter().map(|o| serde_json::json!({
                "event":o.apply.event,"comb":o.comb,"parents":o.apply.parents,
                "coarse_layer":o.coarse_layer,"smooth_layer":o.smooth_layer,
                "boundary_restart":o.boundary_restart
            })).collect::<Vec<_>>()
        })
    }

    /// Resolve only recorded ports. Equal values cannot substitute for row identity.
    pub fn push(&mut self, apply: Apply) -> Result<usize, String> {
        let id = self.occurrences.len();
        if self.events.contains_key(&apply.event) {
            return Err("duplicate occurrence event".into());
        }
        if apply.parents.iter().copied().collect::<BTreeSet<_>>().len() != apply.parents.len() {
            return Err("duplicate parent: reuse its binding port instead".into());
        }
        if apply.parents.iter().any(|p| *p >= id) {
            return Err("parent is not an earlier occurrence".into());
        }
        if apply.input_roles.len() != apply.wanted.len()
            || apply.output_roles.len() != apply.outputs.len()
        {
            return Err("input/output role arity mismatch".into());
        }
        if apply.binding.len() != apply.wanted.len() {
            return Err("binding arity mismatch".into());
        }
        for (port, expected) in apply.binding.iter().zip(&apply.wanted) {
            let actual = match port {
                RelativeBinding::ParentPort { parent, output } => apply
                    .parents
                    .get(*parent)
                    .and_then(|p| self.occurrences[*p].apply.outputs.get(*output)),
                RelativeBinding::External { slot } => apply.external.get(*slot),
            };
            if actual != Some(expected) {
                return Err(format!("invalid binding at event {}", apply.event));
            }
        }
        // Validate preconditions BEFORE this apply's own effects are admitted.
        for need in &apply.required {
            if !apply.external_facts.contains(need) && !self.supports(&apply.parents, need) {
                return Err(format!(
                    "unprovided requirement at event {}: {need:?}",
                    apply.event
                ));
            }
        }
        let initial_coarse = apply.parents.is_empty()
            || !apply.external_facts.is_empty()
            || apply
                .binding
                .iter()
                .any(|b| matches!(b, RelativeBinding::External { .. }));
        let mut support: BTreeSet<usize> = BTreeSet::new();
        let mut children = BTreeSet::new();
        for p in &apply.parents {
            let layer = self.occurrences[*p].coarse_layer;
            children.insert(layer);
            support.extend(&self.coarse_layers[layer].members);
        }
        // Never hoist C2 over S1 in C1 -> S1 -> C2 merely because a later
        // consumer also reads C1. Start a new interface instead of making a cycle.
        let boundary_restart = !initial_coarse
            && support.len() > 1
            && support.iter().any(|m| {
                let mut todo: Vec<_> = self.occurrences[*m]
                    .apply
                    .parents
                    .iter()
                    .map(|p| (*p, false))
                    .collect();
                let mut seen = BTreeSet::new();
                while let Some((p, crossed_boundary)) = todo.pop() {
                    if !seen.insert((p, crossed_boundary)) {
                        continue;
                    }
                    if crossed_boundary && support.contains(&p) {
                        return true;
                    }
                    let crossed_boundary = crossed_boundary || !support.contains(&p);
                    todo.extend(
                        self.occurrences[p]
                            .apply
                            .parents
                            .iter()
                            .map(|p| (*p, crossed_boundary)),
                    );
                }
                false
            });
        let coarse = initial_coarse || boundary_restart;
        let mut key = normalize(&apply, &self.occurrences, coarse);
        // Dependency-only parents (including union witnesses) are part of the key.
        key.parents.shrink_to_fit();
        let comb = if let Some(i) = self.comb_index.get(&key) {
            *i
        } else {
            let i = self.combs.len();
            self.comb_index.insert(key.clone(), i);
            self.combs.push(key);
            i
        };
        let members: Vec<_> = if coarse {
            vec![id]
        } else {
            support.into_iter().collect()
        };
        let coarse_layer = if let Some(i) = self.coarse_index.get(&members) {
            *i
        } else {
            let i = self.coarse_layers.len();
            self.coarse_index.insert(members.clone(), i);
            self.coarse_layers.push(CoarseLayer {
                members,
                children: if coarse {
                    vec![]
                } else {
                    children.into_iter().collect()
                },
                witness: if coarse { None } else { Some(id) },
            });
            i
        };
        let smooth_layer = if coarse {
            None
        } else {
            let i = *self.smooth_index.entry(coarse_layer).or_insert_with(|| {
                let i = self.smooth_layers.len();
                self.smooth_layers.push(SmoothLayer {
                    coarse_layer,
                    members: vec![],
                });
                i
            });
            self.smooth_layers[i].members.push(id);
            Some(i)
        };
        self.events.insert(apply.event, id);
        self.occurrences.push(Occurrence {
            apply,
            comb,
            coarse_layer,
            smooth_layer,
            boundary_restart,
        });
        let mut reuse = std::mem::take(&mut self.reuse);
        reuse.push(self);
        self.reuse = reuse;
        Ok(id)
    }
    /// Lazy inherited evidence: no transitive effect-set copy in every occurrence.
    pub fn provides(&self, roots: &[usize]) -> BTreeSet<Effect> {
        let mut todo = roots.to_vec();
        let mut seen = BTreeSet::new();
        let mut out = BTreeSet::new();
        while let Some(p) = todo.pop() {
            if !seen.insert(p) {
                continue;
            }
            let a = &self.occurrences[p].apply;
            out.extend(a.produced.iter().cloned());
            out.extend(a.external_facts.iter().cloned());
            todo.extend(&a.parents);
        }
        out
    }
    pub fn supports(&self, roots: &[usize], need: &Effect) -> bool {
        let effects = self.provides(roots);
        match need {
            Effect::RowFact(_) => effects.contains(need),
            Effect::Equal(a, b) => {
                let mut seen = BTreeSet::from([*a]);
                let mut todo = vec![*a];
                while let Some(x) = todo.pop() {
                    if x == *b {
                        return true;
                    }
                    for e in &effects {
                        if let Effect::Equal(u, v) = e {
                            if *u == x && seen.insert(*v) {
                                todo.push(*v);
                            }
                            if *v == x && seen.insert(*u) {
                                todo.push(*u);
                            }
                        }
                    }
                }
                false
            }
        }
    }
}
fn normalize(a: &Apply, occurrences: &[Occurrence], coarse: bool) -> Comb {
    // First use of a parent in ordered binding slots eliminates arbitrary parent
    // list ordering. Unreferenced proof parents are ordered by shared definition.
    let mut order = vec![];
    for b in &a.binding {
        if let RelativeBinding::ParentPort { parent, .. } = b {
            if !order.contains(parent) {
                order.push(*parent);
            }
        }
    }
    let mut rest: Vec<_> = (0..a.parents.len())
        .filter(|p| !order.contains(p))
        .collect();
    rest.sort_by_key(|p| occurrences[a.parents[*p]].comb);
    order.extend(rest);
    let binding = a
        .binding
        .iter()
        .map(|b| match b {
            RelativeBinding::ParentPort { parent, output } => RelativeBinding::ParentPort {
                parent: order.iter().position(|p| p == parent).unwrap(),
                output: *output,
            },
            b => b.clone(),
        })
        .collect();
    let mut values = BTreeMap::new();
    let mut slot = |v: usize| {
        let n = values.len();
        *values.entry(v).or_insert(n)
    };
    let aliases = a
        .wanted
        .iter()
        .chain(&a.outputs)
        .chain(&a.external)
        .map(|v| slot(*v))
        .collect();
    let mut effect = |e: &Effect| match e {
        Effect::RowFact(v) => Effect::RowFact(slot(*v)),
        Effect::Equal(a, b) => Effect::Equal(slot(*a), slot(*b)),
    };
    Comb {
        kind: if coarse {
            CombKind::CoarseComb
        } else {
            CombKind::SmoothComb
        },
        rule: a.rule.clone(),
        input_roles: a.input_roles.clone(),
        output_roles: a.output_roles.clone(),
        parents: order
            .iter()
            .map(|p| occurrences[a.parents[*p]].comb)
            .collect(),
        binding,
        aliases,
        input_count: a.wanted.len(),
        output_count: a.outputs.len(),
        external_count: a.external.len(),
        requirements: a.required.iter().map(&mut effect).collect(),
        effects: a.produced.iter().map(&mut effect).collect(),
        external_facts: a.external_facts.iter().map(&mut effect).collect(),
    }
}
