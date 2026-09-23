//! Registers observed CS interfaces and consumer connections. No partner search.
use super::*;
use crate::coarse_smooth::LayerStore;
use serde::Serialize;
#[derive(Clone, Serialize)]
pub(super) struct Unit {
    pub coarse_layer: usize,
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
        _boundary: usize,
    ) -> Result<Vec<(String, usize)>> {
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
                coarse_layer: layer,
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
            });
            let depth = parts
                .iter()
                .map(|i| self.units[*i].depth)
                .max()
                .unwrap_or(0)
                + 1;
            if let Some(id) = self.intern(Unit {
                coarse_layer: occurrence.coarse_layer,
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
        json!({"units":units,"compositions":self.compositions,"scope":"observed consumer hyperedges; exact member interning; no all-pairs discovery or limits"})
    }
}
