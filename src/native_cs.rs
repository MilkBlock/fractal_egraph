//! Immutable CS versions and witnessed binary composition references.
use super::*;
use crate::coarse_smooth::LayerStore;
use serde::Serialize;
#[derive(Clone, Serialize)]
pub(super) struct Unit {
    pub coarse_layer: usize,
    pub coarse: Vec<usize>,
    pub smooth: Vec<usize>,
}
impl Unit {
    pub fn members(&self) -> Vec<usize> {
        self.coarse
            .iter()
            .chain(&self.smooth)
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}
#[derive(Clone, Serialize)]
pub(super) struct Composition {
    pub kind: String,
    pub parts: [usize; 2],
    pub anchors: Vec<Json>,
    pub links: Vec<Json>,
}
#[derive(Default)]
pub(super) struct Store {
    pub units: Vec<Unit>,
    pub compositions: Vec<Composition>,
    versions: BTreeMap<(Vec<usize>, Vec<usize>), usize>,
    index: BTreeMap<usize, Vec<usize>>,
    pairs: BTreeSet<(String, usize, usize)>,
    pub skipped: usize,
}
impl Store {
    pub fn members(&self, id: usize) -> Vec<usize> {
        self.compositions[id]
            .parts
            .iter()
            .flat_map(|i| self.units[*i].members())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn discover(&mut self, c: &Captured, layers: &LayerStore) -> Result<Vec<(String, usize)>> {
        let mut admitted = vec![];
        let mut parser = EGraph::default();
        let commands = parser.parse_program(None, &c.datatype)?;
        let ops: BTreeSet<String> = commands
            .iter()
            .flat_map(|cmd| match cmd {
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
                    ops.contains(op) && args.iter().all(|e| positive(e, ops))
                }
                _ => true,
            }
        }
        let safe: Vec<_> = c
            .rules
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
            .collect();
        let mut smooth = BTreeMap::<usize, Vec<usize>>::new();
        for s in &layers.smooth_layers {
            smooth.entry(s.coarse_layer).or_default().extend(&s.members);
        }
        for (coarse_layer, coarse) in layers.coarse_layers.iter().enumerate() {
            let mut c_members = coarse.members.clone();
            c_members.sort();
            c_members.dedup();
            let mut s_members = smooth.get(&coarse_layer).cloned().unwrap_or_default();
            s_members.sort();
            s_members.dedup();
            let key = (c_members.clone(), s_members.clone());
            if self.versions.contains_key(&key) {
                continue;
            }
            if self.units.len() >= 256 || c_members.len() + s_members.len() > 16 {
                self.skipped += 1;
                continue;
            }
            let unit = Unit {
                coarse_layer,
                coarse: c_members,
                smooth: s_members,
            };
            let members = unit.members();
            let id = self.units.len();
            self.versions.insert(key, id);
            self.units.push(unit);
            if !self.units[id].smooth.is_empty() {
                admitted.push(("CSUnit".into(), id));
            }
            let tokens: BTreeSet<_> = members
                .iter()
                .flat_map(|i| {
                    layers.occurrences[*i]
                        .apply
                        .wanted
                        .iter()
                        .chain(&layers.occurrences[*i].apply.outputs)
                })
                .copied()
                .collect();
            let others: BTreeSet<_> = tokens
                .iter()
                .flat_map(|v| self.index.get(v).into_iter().flatten())
                .copied()
                .collect();
            for other in others.into_iter().take(32) {
                if self.units[other].smooth.is_empty() || self.units[id].smooth.is_empty() {
                    continue;
                }
                let a = self.units[other].members();
                let b = &members;
                if a.iter().any(|i| b.contains(i)) || a.len() + b.len() > 16 {
                    continue;
                }
                // Include transitive dependencies through omitted events too.
                fn depends(c: &Captured, from: &[usize], on: &[usize]) -> Option<bool> {
                    let mut seen = BTreeSet::new();
                    let mut todo = from.to_vec();
                    while let Some(i) = todo.pop() {
                        for &p in &c.records[i].parents {
                            if on.contains(&p) {
                                return Some(true);
                            }
                            if seen.insert(p) {
                                todo.push(p);
                            }
                        }
                        if seen.len() > 256 {
                            return None;
                        }
                    }
                    Some(false)
                }
                let (Some(ab), Some(ba)) = (depends(c, &a, b), depends(c, b, &a)) else {
                    self.skipped += 1;
                    continue;
                };
                let mut links = vec![];
                for &consumer in a.iter().chain(b) {
                    let r = &c.records[consumer];
                    for (input, p) in r.ports.iter().enumerate() {
                        if let Port::Parent(parent, output) = p {
                            let producer = r.parents[*parent];
                            if (a.contains(&consumer) && b.contains(&producer))
                                || (b.contains(&consumer) && a.contains(&producer))
                            {
                                links.push(json!({"producer":producer,"output":output,"consumer":consumer,"input":input}));
                            }
                        }
                    }
                }
                let coarse_a: BTreeSet<_> = self.units[other]
                    .coarse
                    .iter()
                    .flat_map(|i| {
                        layers.occurrences[*i]
                            .apply
                            .wanted
                            .iter()
                            .chain(&layers.occurrences[*i].apply.outputs)
                    })
                    .copied()
                    .collect();
                let coarse_b: BTreeSet<_> = self.units[id]
                    .coarse
                    .iter()
                    .flat_map(|i| {
                        layers.occurrences[*i]
                            .apply
                            .wanted
                            .iter()
                            .chain(&layers.occurrences[*i].apply.outputs)
                    })
                    .copied()
                    .collect();
                let anchors: Vec<_> = coarse_a
                    .intersection(&coarse_b)
                    .map(|v| json!({"token":v,"sort":c.pool.values[*v].sort}))
                    .collect();
                let all_coarse: Vec<_> = self.units[other]
                    .coarse
                    .iter()
                    .chain(&self.units[id].coarse)
                    .copied()
                    .collect();
                let all_smooth: Vec<_> = self.units[other]
                    .smooth
                    .iter()
                    .chain(&self.units[id].smooth)
                    .copied()
                    .collect();
                let ccss = !anchors.is_empty()
                    && a.iter().chain(b).all(|i| safe[c.records[*i].rule])
                    && depends(c, &all_coarse, &all_smooth) == Some(false);
                let (kind, parts) = if ccss {
                    // C can precede S; dependencies within C or within S remain
                    // edges of the combined DAG, never an unordered execution.
                    ("CCSS", [other, id])
                } else if ab != ba && !links.is_empty() {
                    ("CSCS", if ba { [other, id] } else { [id, other] })
                } else {
                    self.skipped += 1;
                    continue;
                };
                if self.compositions.iter().filter(|p| p.kind == kind).count() >= 64 {
                    self.skipped += 1;
                    continue;
                }
                if self.pairs.insert((kind.into(), parts[0], parts[1])) {
                    let i = self.compositions.len();
                    self.compositions.push(Composition {
                        kind: kind.into(),
                        parts,
                        anchors,
                        links,
                    });
                    admitted.push((kind.into(), i));
                }
            }
            for token in tokens {
                let entry = self.index.entry(token).or_default();
                if entry.len() == 8 {
                    entry.remove(0);
                }
                entry.push(id);
            }
        }
        Ok(admitted)
    }
    pub fn report(&self) -> Json {
        json!({"units":self.units,"compositions":self.compositions,"skipped":self.skipped,"scope":"CSCS preserves dependency order; CCSS requires positive grounded actions, coarse port intersection and no C dependency on S; original event order is retained for validation"})
    }
}
