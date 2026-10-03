//! Committed apply state DAG. Legacy layer grouping is an explicit comparison mode.
//! Versioned producers and bound-port context are distinct witnesses; no partner search.
use super::*;
use crate::coarse_smooth::LayerStore;
use serde::Serialize;
#[derive(Clone, Serialize)]
pub(super) struct Unit {
    pub coarse_layer: Option<usize>,
    pub coarse: Vec<usize>,
    pub smooth: Vec<usize>,
    pub source_composition: Option<usize>,
    pub depth: usize,
}
impl Unit {
    pub fn members(&self) -> Vec<usize> {
        self.coarse.clone()
    }
}
#[derive(Clone, Serialize)]
pub(super) struct Composition {
    pub kind: String,
    pub forms: Vec<String>,
    pub parts: Vec<usize>,
    pub consumer: usize,
    pub members: Vec<usize>,
    pub boundary: Option<usize>,
    pub rule: Option<usize>,
    pub external_facts: Vec<usize>,
    pub union_effects: Vec<(usize, usize)>,
    pub updated_rows: bool,
    pub producer_parts: Vec<usize>,
    pub context_parts: Vec<usize>,
}
#[derive(Default)]
pub(super) struct Store {
    pub units: Vec<Unit>,
    pub compositions: Vec<Composition>,
    by_members: BTreeMap<Vec<usize>, usize>,
    by_layer: BTreeMap<usize, usize>,
    seen_events: usize,
    seen_layers: usize,
    positive: Option<Vec<bool>>,
    pairs: BTreeSet<(Vec<usize>, usize)>,
    observed_mode: Option<bool>,
    event_states: Vec<Option<usize>>,
    confirmations: usize,
    seen_changes: usize,
    port_frontiers: BTreeMap<usize, BTreeSet<usize>>,
    aliases: BTreeMap<usize, usize>,
}
impl Store {
    fn intern(&mut self, u: Unit) -> Option<usize> {
        let key = u.members();
        if self.by_members.contains_key(&key) {
            return None;
        }
        let id = self.units.len();
        self.by_members.insert(key, id);
        self.units.push(u);
        Some(id)
    }
    pub fn discover(
        &mut self,
        c: &Captured,
        s: &LayerStore,
        boundary: usize,
    ) -> Result<Vec<(String, usize)>> {
        let observed = *self.observed_mode.get_or_insert_with(|| {
            std::env::var("EGG_LAYOUT_COMPOSITION_DISCOVERY").as_deref() != Ok("layers")
        });
        if observed {
            return self.observe(c, boundary);
        }
        if self.positive.is_none() {
            let mut parser = EGraph::default();
            let declarations = parser.parse_program(None, &c.datatype)?;
            let ops: BTreeSet<String> = declarations
                .iter()
                .flat_map(|d| match d {
                    Command::Datatype { variants, .. } => {
                        variants.iter().map(|v| v.name.clone()).collect()
                    }
                    Command::Relation { name, .. } => vec![name.clone()],
                    _ => vec![],
                })
                .collect();
            fn positive(e: &Expr, ops: &BTreeSet<String>) -> bool {
                match e {
                    Expr::Call(_, op, args) => {
                        ops.contains(op) && args.iter().all(|a| positive(a, ops))
                    }
                    _ => true,
                }
            }
            self.positive = Some(
                c.rules
                    .iter()
                    .map(|r| {
                        r.rule.body.iter().all(|f| match f {
                            Fact::Eq(_, a, b) => positive(a, &ops) && positive(b, &ops),
                            Fact::Fact(e) => positive(e, &ops),
                        }) && r.rule.head.0.iter().all(|a| match a {
                            Action::Expr(_, e) | Action::Let(_, _, e) => positive(e, &ops),
                            Action::Union(_, a, b) => positive(a, &ops) && positive(b, &ops),
                            _ => false,
                        })
                    })
                    .collect(),
            );
        }
        let mut added = vec![];
        for layer in self.seen_layers..s.coarse_layers.len() {
            let mut members = s.coarse_layers[layer].members.clone();
            members.sort();
            members.dedup();
            let u = Unit {
                coarse_layer: Some(layer),
                coarse: members.clone(),
                smooth: vec![],
                source_composition: None,
                depth: 0,
            };
            if let Some(id) = self.intern(u) {
                added.push(("CSUnit".into(), id));
            }
            self.by_layer.insert(layer, self.by_members[&members]);
        }
        self.seen_layers = s.coarse_layers.len();
        for smooth in &s.smooth_layers {
            if let Some(&id) = self.by_layer.get(&smooth.coarse_layer) {
                self.units[id].smooth = smooth.members.clone();
            }
        }
        for consumer in self.seen_events..s.occurrences.len() {
            let occurrence = &s.occurrences[consumer];
            let own = self.by_layer[&occurrence.coarse_layer];
            let parts: Vec<_> = occurrence
                .apply
                .parents
                .iter()
                .map(|p| self.by_layer[&s.occurrences[*p].coarse_layer])
                .chain(std::iter::once(own))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            if parts.len() < 2 || !self.pairs.insert((parts.clone(), consumer)) {
                continue;
            }
            // One actual consumer defines one hyperedge. Do not enumerate subsets or
            // pair all units that happen to mention the same constant.
            let roots: BTreeSet<_> = parts
                .iter()
                .flat_map(|i| self.units[*i].coarse.iter().copied())
                .collect();
            let mut members = roots.clone();
            let mut todo = vec![consumer];
            while let Some(i) = todo.pop() {
                members.insert(i);
                for &p in &c.records[i].parents {
                    if !roots.contains(&p) && members.insert(p) {
                        todo.push(p);
                    }
                }
            }
            let id = self.compositions.len();
            let members: Vec<_> = members.into_iter().collect();
            let has_smooth_parent = occurrence
                .apply
                .parents
                .iter()
                .any(|p| s.occurrences[*p].smooth_layer.is_some());
            let mut forms = vec![];
            if has_smooth_parent {
                forms.push("CSCS".to_string());
            }
            if occurrence.smooth_layer.is_some()
                && parts.iter().filter(|p| **p != own).count() > 1
                && members
                    .iter()
                    .all(|i| self.positive.as_ref().unwrap()[c.records[*i].rule])
            {
                forms.push("CCSS".to_string());
            }
            if forms.is_empty() {
                forms.push("CoarseJoin".into());
            }
            self.compositions.push(Composition {
                kind: forms[0].clone(),
                forms,
                parts: parts.clone(),
                consumer,
                members: members.clone(),
                boundary: None,
                rule: None,
                external_facts: vec![],
                union_effects: vec![],
                updated_rows: false,
                producer_parts: vec![],
                context_parts: vec![],
            });
            let depth = parts
                .iter()
                .map(|i| self.units[*i].depth)
                .max()
                .unwrap_or(0)
                + 1;
            if let Some(id) = self.intern(Unit {
                coarse_layer: Some(occurrence.coarse_layer),
                coarse: members,
                smooth: vec![],
                source_composition: Some(id),
                depth,
            }) {
                added.push(("CSUnit".into(), id));
            }
        }
        self.seen_events = s.occurrences.len();
        Ok(added)
    }
    /// Materialize history membership only for replay preparation, never to
    /// enumerate partners. The stored state graph itself contains only deltas.
    pub fn members(&self, unit: usize) -> Vec<usize> {
        let mut members = BTreeSet::new();
        let mut visited = BTreeSet::new();
        let mut todo = vec![unit];
        while let Some(id) = todo.pop() {
            if !visited.insert(id) {
                continue;
            }
            let u = &self.units[id];
            members.extend(u.coarse.iter().copied());
            if let Some(comp) = u.source_composition {
                todo.extend(self.compositions[comp].parts.iter().copied());
            }
        }
        members.into_iter().collect()
    }
    fn canonical(&self, mut token: usize) -> usize {
        while let Some(&p) = self.aliases.get(&token) {
            if p == token {
                break;
            }
            token = p;
        }
        token
    }
    fn observe(&mut self, c: &Captured, boundary: usize) -> Result<Vec<(String, usize)>> {
        let mut added = vec![];
        if self.event_states.len() == c.records.len() && self.seen_changes == c.changes.len() {
            return Ok(added);
        }
        for change in &c.changes[self.seen_changes..] {
            if change.reset {
                self.port_frontiers.clear();
                self.aliases.clear();
            }
            for (a, b) in &change.unions {
                let a = self.canonical(*a);
                let b = self.canonical(*b);
                if a != b {
                    self.aliases.insert(a.max(b), a.min(b));
                }
            }
        }
        self.seen_changes = c.changes.len();
        let mut rebased = BTreeMap::<usize, BTreeSet<usize>>::new();
        for (port, states) in std::mem::take(&mut self.port_frontiers) {
            rebased
                .entry(self.canonical(port))
                .or_default()
                .extend(states);
        }
        self.port_frontiers = rebased;
        let first = self.event_states.len();
        let mut pending = BTreeMap::<usize, BTreeSet<usize>>::new();
        for record in self.event_states.len()..c.records.len() {
            let r = &c.records[record];
            // Match survival or a deduplicated write is not a new state mutation.
            if r.produced.is_empty() && r.unions.is_empty() && !r.updated_rows {
                self.confirmations += 1;
                self.event_states.push(None);
                continue;
            }
            let roots: Vec<_> = r.read_results.iter().flatten().copied().collect();
            // Same-batch peers see the previous frontier; recording order does
            // not invent a causal chain between simultaneous matches.
            let producer_parts: Vec<_> = r
                .parents
                .iter()
                .filter_map(|p| self.event_states.get(*p).copied().flatten())
                .collect();
            let context_parts: Vec<_> = roots
                .iter()
                .flat_map(|r| {
                    self.port_frontiers
                        .get(&self.canonical(*r))
                        .into_iter()
                        .flatten()
                        .copied()
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let parts: Vec<_> = producer_parts
                .iter()
                .chain(&context_parts)
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let prior: BTreeSet<_> = parts.iter().flat_map(|p| self.members(*p)).collect();
            let supplied: BTreeSet<_> = prior
                .iter()
                .flat_map(|i| c.records[*i].required.iter().chain(&c.records[*i].produced))
                .copied()
                .collect();
            let external_facts: Vec<_> = r
                .required
                .iter()
                .filter(|f| !supplied.contains(f))
                .copied()
                .collect();
            let coarse = parts.is_empty() || !external_facts.is_empty() || r.updated_rows;
            let kind = if coarse {
                "ObservedCoarseApply"
            } else {
                "ObservedSmoothApply"
            }
            .to_string();
            let comp = self.compositions.len();
            self.compositions.push(Composition {
                kind: kind.clone(),
                forms: vec![kind],
                parts: parts.clone(),
                consumer: record,
                members: vec![record],
                boundary: Some(boundary),
                rule: Some(r.rule),
                external_facts,
                union_effects: r.unions.clone(),
                updated_rows: r.updated_rows,
                producer_parts,
                context_parts,
            });
            let id = self.units.len();
            let depth = parts
                .iter()
                .map(|p| self.units[*p].depth + 1)
                .max()
                .unwrap_or(0);
            self.units.push(Unit {
                coarse_layer: None,
                coarse: vec![record],
                smooth: vec![],
                source_composition: Some(comp),
                depth,
            });
            for port in r
                .values
                .iter()
                .copied()
                .chain(roots)
                .filter(|t| c.datatype_names.contains(c.pool.values[*t].sort.as_ref()))
            {
                pending.entry(self.canonical(port)).or_default().insert(id);
            }
            self.event_states.push(Some(id));
            added.push(("ObservedState".into(), id));
        }
        for (port, states) in pending {
            let superseded: BTreeSet<_> = states
                .iter()
                .flat_map(|id| {
                    self.units[*id]
                        .source_composition
                        .into_iter()
                        .flat_map(|comp| self.compositions[comp].parts.iter().copied())
                })
                .collect();
            let frontier = self.port_frontiers.entry(port).or_default();
            frontier.retain(|id| !superseded.contains(id));
            frontier.extend(states);
        }
        for r in &c.records[first..] {
            for (a, b) in &r.unions {
                let a = self.canonical(*a);
                let b = self.canonical(*b);
                if a != b {
                    self.aliases.insert(a.max(b), a.min(b));
                }
            }
        }
        let mut merged = BTreeMap::<usize, BTreeSet<usize>>::new();
        for (root, states) in std::mem::take(&mut self.port_frontiers) {
            merged
                .entry(self.canonical(root))
                .or_default()
                .extend(states);
        }
        self.port_frontiers = merged;
        self.seen_events = c.records.len();
        Ok(added)
    }
    pub fn report(&self) -> Json {
        let units: Vec<_> = self
            .units
            .iter()
            .map(|u| {
                let mut v = serde_json::to_value(u).unwrap();
                v["entry_members"] = json!(u.coarse);
                v
            })
            .collect();
        json!({"discovery":if self.observed_mode==Some(true){"committed_apply_dag"}else{"legacy_layers"},"confirmations_without_new_effect":self.confirmations,"members_are_deltas":self.observed_mode==Some(true),"units":units,"compositions":self.compositions,"scope":"one node per witnessed mutation; producer dependencies and external obligations; layer-free discovery; compilation is a separate decision"})
    }
}
