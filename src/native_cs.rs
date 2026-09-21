//! Immutable CS versions and witnessed binary composition references.
use super::*;
use crate::coarse_smooth::LayerStore;
use serde::Serialize;
#[derive(Clone, Serialize)]
/// One candidate interface made from recorded applications.
///
/// `coarse` and `smooth` are context-relative member lists. They do not form a
/// proof that the unit is globally minimal, complete, or safe to execute as a
/// new rule; those claims would require checking the original obligations.
pub(super) struct Unit {
    pub coarse_layer: usize,
    pub coarse: Vec<usize>,
    pub smooth: Vec<usize>,
    pub source_composition: Option<usize>,
    pub depth: usize,
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
/// A bounded, witnessed pair of candidate units.
///
/// `anchors` and `links` explain why the pair was inspected. `certificate`
/// records the conditions checked by this implementation; it is not a license
/// to bypass native replay or to reorder tier0 execution.
pub(super) struct Composition {
    pub kind: String,
    pub parts: [usize; 2],
    pub anchors: Vec<Json>,
    pub links: Vec<Json>,
    pub certificate: Json,
}
#[derive(Default)]
pub(super) struct Store {
    pub units: Vec<Unit>,
    pub compositions: Vec<Composition>,
    versions: BTreeMap<(Vec<usize>, Vec<usize>), usize>,
    index: BTreeMap<usize, Vec<usize>>,
    pairs: BTreeSet<(String, usize, usize)>,
    pub skipped: usize,
    registry: crate::closure_contract::Registry,
    seen_occurrences: usize,
    seen_coarse: usize,
    changes_seen: usize,
    smooth: BTreeMap<usize, Vec<usize>>,
    pending_units: Vec<usize>,
    promoted: BTreeSet<usize>,
    pub layer_visits: usize,
    pub pair_checks: usize,
    environment: String,
    safe_rules: Option<Vec<bool>>,
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
    /// Discover candidates from newly observed layers and resource changes.
    ///
    /// Discovery is intentionally conservative and budgeted: omitted pairs are
    /// reported as omitted, not silently interpreted as nonexistent. Every
    /// admitted candidate is replayed later in an isolated native cell.
    pub fn discover(
        &mut self,
        c: &Captured,
        layers: &LayerStore,
        boundary: usize,
    ) -> Result<Vec<(String, usize)>> {
        let mut admitted = vec![];
        let environment = serde_json::to_string(&(
            &c.datatype,
            c.rules
                .iter()
                .map(|r| r.rule.to_string())
                .collect::<Vec<_>>(),
        ))?;
        if !self.environment.is_empty() && self.environment != environment {
            self.registry.apply(&crate::closure_contract::Change {
                reset: true,
                ..Default::default()
            });
            return Err("CS rule environment changed; start a new pipeline instead of reusing old certificates".into());
        }
        self.environment = environment;
        while let Some(change) = c
            .changes
            .get(self.changes_seen)
            .filter(|e| e.boundary <= boundary)
        {
            let wakes = self.registry.apply(change);
            self.pending_units.extend(wakes);
            self.changes_seen += 1;
        }
        let mut ready = std::mem::take(&mut self.pending_units);
        ready.sort();
        ready.dedup();

        if self.safe_rules.is_none() {
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
            self.safe_rules = Some(safe);
        }
        let safe = self.safe_rules.as_ref().unwrap();
        let mut touched: BTreeSet<_> = (self.seen_coarse..layers.coarse_layers.len()).collect();
        for (id, o) in layers
            .occurrences
            .iter()
            .enumerate()
            .skip(self.seen_occurrences)
        {
            touched.insert(o.coarse_layer);
            if o.smooth_layer.is_some() {
                self.smooth.entry(o.coarse_layer).or_default().push(id);
            }
        }
        self.seen_occurrences = layers.occurrences.len();
        self.seen_coarse = layers.coarse_layers.len();
        for coarse_layer in touched {
            self.layer_visits += 1;
            let coarse = &layers.coarse_layers[coarse_layer];
            let mut c_members = coarse.members.clone();
            c_members.sort();
            c_members.dedup();
            let mut s_members = self.smooth.get(&coarse_layer).cloned().unwrap_or_default();
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
                source_composition: None,
                depth: 0,
            };
            let members = unit.members();
            let id = self.units.len();
            self.versions.insert(key, id);
            self.units.push(unit);
            if !self.units[id].smooth.is_empty() {
                admitted.push(("CSUnit".into(), id));
            }
            ready.push(id);
        }
        for id in ready {
            let members = self.units[id].members();
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
            let resources = tokens
                .iter()
                .copied()
                .map(crate::closure_contract::Resource::Token)
                .chain(
                    members
                        .iter()
                        .flat_map(|i| c.rules[c.records[*i].rule].calls.values())
                        .filter_map(|(_, e)| match e {
                            Expr::Call(_, op, _) => {
                                Some(crate::closure_contract::Resource::Table(op.clone()))
                            }
                            _ => None,
                        }),
                );
            if !self.registry.watched(id) {
                self.registry.watch(id, resources);
            }
            let others: BTreeSet<_> = tokens
                .iter()
                .flat_map(|v| self.index.get(v).into_iter().flatten())
                .copied()
                .collect();
            for other in others.into_iter().take(32) {
                // Stale instances may suggest a symbolic candidate, but cannot bypass native revalidation.
                self.pair_checks += 1;
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
                                if seen.len() > 256 {
                                    return None;
                                }
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
                        certificate:json!({"status":if self.registry.valid(parts[0])&&self.registry.valid(parts[1]){"VerifiedWithinRecordedInterface"}else{"RequiresRevalidation"},"environment":0,
                            "parts":[self.registry.stamp(parts[0]),self.registry.stamp(parts[1])],
                            "obligations":a.iter().chain(b).flat_map(|i|{
                                let r=&c.records[*i];let a=&a;r.ports.iter().enumerate().filter_map(move |(input,p)|match p {
                                    Port::External(_)=>Some(json!({"before_record":i,"input":input,"token":r.wanted[input]})),
                                    Port::Parent(parent,_) if !a.contains(&r.parents[*parent])&&!b.contains(&r.parents[*parent])=>Some(json!({"before_record":i,"input":input,"token":r.wanted[input]})),_=>None})
                            }).collect::<Vec<_>>(),
                            "requirements":a.iter().chain(b).map(|i|json!({"record":i,"required":layers.occurrences[*i].apply.required,"external_facts":layers.occurrences[*i].apply.external_facts})).collect::<Vec<_>>(),
                            "outputs":a.iter().chain(b).map(|i|json!({"record":i,"roles":c.records[*i].outputs.iter().map(|o|format!("{o:?}")).collect::<Vec<_>>(),"effects":layers.occurrences[*i].apply.produced})).collect::<Vec<_>>(),
                            "reordering":if kind=="CCSS"{"positive operations; no C depends on S"}else{"dependency direction preserved"},
                            "live_substitution":false,"external_schedule":"original injection stages retained; early availability is not assumed"}),
                    });
                    admitted.push((kind.into(), i));
                }
            }
            for token in tokens {
                let entry = self.index.entry(token).or_default();
                entry.retain(|i| *i != id);
                if entry.len() == 8 {
                    entry.remove(0);
                }
                entry.push(id);
            }
        }
        Ok(admitted)
    }
    pub fn environment(&self) -> &str {
        &self.environment
    }
    pub fn current(&self, id: usize) -> bool {
        self.compositions
            .get(id)
            .is_some_and(|p| p.parts.iter().all(|i| self.registry.valid(*i)))
    }
    /// Add a validated composition descriptor as a deeper candidate unit.
    ///
    /// Promotion extends the analysis graph only. It does not install a macro,
    /// delete the source history, or alter the live tier0 matcher.
    pub fn promote(&mut self, id: usize, proof: &Json) -> bool {
        if proof["checks"] != "passed"
            || proof["saturated_rule_composition"]["status"] != "exported"
            || self.promoted.contains(&id)
            || self.units.len() >= 256
        {
            return false;
        }
        let p = &self.compositions[id];
        let coarse = p
            .parts
            .iter()
            .flat_map(|i| self.units[*i].coarse.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let smooth = p
            .parts
            .iter()
            .flat_map(|i| self.units[*i].smooth.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let depth = p.parts.iter().map(|i| self.units[*i].depth).max().unwrap() + 1;
        let unit = Unit {
            coarse_layer: self.units[p.parts[0]].coarse_layer,
            coarse,
            smooth,
            source_composition: Some(id),
            depth,
        };
        let new = self.units.len();
        self.units.push(unit);
        self.promoted.insert(id);
        self.pending_units.push(new);
        true
    }
    pub fn report(&self) -> Json {
        json!({"units":self.units,"compositions":self.compositions,"skipped":self.skipped,"environment":self.environment,"layer_visits":self.layer_visits,"pair_checks":self.pair_checks,"resource_notifications":self.registry.notifications,"subscriber_wakes":self.registry.wakes,"promotions":self.promoted.len(),"stale_evidence":(0..self.units.len()).filter(|i|!self.registry.valid(*i)).count(),"composition_evidence_current":(0..self.compositions.len()).map(|i|self.current(i)).collect::<Vec<_>>(),"leases":(0..self.units.len()).map(|i|self.registry.stamp(i)).collect::<Vec<_>>(),"scope":"CSCS preserves dependency order; CCSS requires positive grounded actions, coarse port intersection and no C dependency on S; original event order is retained for validation"})
    }
}
