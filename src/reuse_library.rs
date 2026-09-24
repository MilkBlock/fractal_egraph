//! Explicit, offline Bake import. No runtime mutation or planner substitution.
use super::*;
use crate::coarse_smooth::{Apply, CombKind, RelativeBinding, canonical_effect, symbol};
use egglog::ast::{Action, Command, Expr};
use std::path::Path;

/// Preserve selected occurrences and turn cut dependencies into external obligations.
/// This is a conservative recipe, not a minimization of required effects.
pub fn local_recipe(applies: &[Apply], ids: &[usize]) -> Result<Vec<Comb>> {
    if applies.len() != ids.len() {
        return Err("recipe occurrence arity mismatch".into());
    }
    let slots: BTreeMap<_, _> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    if slots.len() != ids.len() {
        return Err("duplicate recipe occurrence".into());
    }
    let mut out = vec![];
    let mut names = BTreeMap::new();
    let mut support: Vec<BTreeSet<crate::coarse_smooth::Effect>> = vec![];
    for (i, a) in applies.iter().enumerate() {
        let parents: Vec<_> = a
            .parents
            .iter()
            .filter_map(|p| slots.get(p).copied())
            .collect();
        if parents.iter().any(|p| *p >= i) {
            return Err("recipe is not topological".into());
        }
        let mut external = a.external.clone();
        let mut binding = vec![];
        for (j, b) in a.binding.iter().enumerate() {
            let local = match b {
                RelativeBinding::ParentPort { parent, output } => {
                    let p = a.parents.get(*parent).ok_or("bad source parent")?;
                    slots.get(p).map(|s| RelativeBinding::ParentPort {
                        parent: parents.iter().position(|x| x == s).unwrap(),
                        output: *output,
                    })
                }
                _ => None,
            };
            binding.push(if let Some(b) = local {
                b
            } else {
                let v = *a.wanted.get(j).ok_or("bad source binding")?;
                let slot = external.iter().position(|x| *x == v).unwrap_or_else(|| {
                    external.push(v);
                    external.len() - 1
                });
                RelativeBinding::External { slot }
            });
        }
        let mut provided: BTreeSet<_> = parents
            .iter()
            .flat_map(|p| support[*p].iter().cloned())
            .collect();
        let mut external_facts = a.external_facts.clone();
        for e in &a.required {
            if !provided.contains(e) && !external_facts.contains(e) {
                external_facts.push(e.clone());
            }
        }
        let coarse = parents.is_empty()
            || !external_facts.is_empty()
            || binding
                .iter()
                .any(|b| matches!(b, RelativeBinding::External { .. }));
        let aliases = a
            .wanted
            .iter()
            .chain(&a.outputs)
            .chain(&external)
            .map(|v| symbol(*v, &mut names))
            .collect();
        let mut effect = |e| canonical_effect(e, &mut names);
        out.push(Comb {
            kind: if coarse {
                CombKind::CoarseRuleComposition
            } else {
                CombKind::SmoothRuleComposition
            },
            rule: a.rule.clone(),
            input_roles: a.input_roles.clone(),
            output_roles: a.output_roles.clone(),
            parents,
            binding,
            aliases,
            input_count: a.wanted.len(),
            output_count: a.outputs.len(),
            external_count: external.len(),
            requirements: a.required.iter().map(&mut effect).collect(),
            effects: a.produced.iter().map(&mut effect).collect(),
            external_facts: external_facts.iter().map(&mut effect).collect(),
        });
        provided.extend(a.produced.iter().cloned());
        support.push(provided);
    }
    Ok(out)
}

#[derive(Serialize, Deserialize)]
struct BodyRecord {
    graph: SaturatedRuleComposition,
    report: Value,
}
#[derive(Serialize, Deserialize)]
struct EntryRecord {
    enclosure: EnclosureId,
    source: String,
    origin: Value,
}
#[derive(Serialize, Deserialize)]
pub struct Library {
    schema: String,
    bodies: Vec<BodyRecord>,
    enclosures: Vec<EnclosureDescriptor>,
    entries: Vec<EntryRecord>,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            schema: "rip-sat-library/v1".into(),
            bodies: vec![],
            enclosures: vec![],
            entries: vec![],
        }
    }
}
impl Library {
    /// Rebuild all indexes through the checked API, without replaying runtime metrics.
    pub fn instantiate(&self) -> Result<ReuseGraph> {
        if self.schema != "rip-sat-library/v1" {
            return Err("unsupported reuse library schema".into());
        }
        let mut graph = ReuseGraph::default();
        for (i, b) in self.bodies.iter().enumerate() {
            let id = graph.intern_body(RipSatEgraph::from_native_export(
                b.graph.clone(),
                &b.report,
            )?)?;
            if id.id != RipSatId(i) || !id.inserted {
                return Err("library contains duplicate bodies".into());
            }
        }
        for (i, d) in self.enclosures.iter().enumerate() {
            if graph.register_enclosure(d.clone(), Admission::InitialBake)? != EnclosureId(i) {
                return Err("library contains duplicate descriptors".into());
            }
        }
        for e in &self.entries {
            let d = graph
                .enclosure(e.enclosure)
                .ok_or("unknown entry descriptor")?;
            if e.source.is_empty() || e.origin["reuse_recipe"].is_null() {
                return Err("entry lacks source or origin recipe".into());
            }
            let resolved = resolve_interface(
                &e.origin["reuse_interface_terms"],
                graph.body(d.body).unwrap().graph(),
            )?;
            if resolved != d.interface {
                return Err("entry interface disagrees with descriptor".into());
            }
            let origin: Vec<Comb> = serde_json::from_value(e.origin["reuse_recipe"].clone())?;
            let mut check = d.clone();
            check.origin = origin;
            if graph.register_enclosure(check, Admission::InitialBake)? != e.enclosure {
                return Err("entry recipe disagrees with descriptor".into());
            }
        }
        Ok(graph)
    }
    pub fn load(path: &Path) -> Result<Self> {
        let mut encoded: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        if encoded["schema"] == "rip-sat-library/v2" {
            let mut previous = vec![];
            for b in encoded["bodies"]
                .as_array_mut()
                .ok_or("missing stored bodies")?
            {
                let storage: super::storage::StoredState =
                    serde_json::from_value(b["storage"].take())?;
                let state = storage.expand(&previous)?;
                b["graph"] = serde_json::to_value(&state)?;
                b.as_object_mut().unwrap().remove("storage");
                previous.push(state);
            }
            encoded["schema"] = json!("rip-sat-library/v1");
        }
        let library: Self = serde_json::from_value(encoded)?;
        library.instantiate()?;
        Ok(library)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.instantiate()?;
        // Refuse accidental baseline/library replacement.
        use std::io::Write;
        let mut previous = vec![];
        let mut bodies = vec![];
        for b in &self.bodies {
            let storage = super::storage::StoredState::select(&b.graph, &previous)?;
            bodies.push(json!({"storage":storage,"report":b.report}));
            previous.push(b.graph.clone());
        }
        let data = serde_json::to_vec_pretty(
            &json!({"schema":"rip-sat-library/v2","bodies":bodies,"enclosures":self.enclosures,"entries":self.entries}),
        )?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&data)?;
        Ok(())
    }
    pub fn counts(&self) -> Value {
        json!({"bodies":self.bodies.len(),"enclosures":self.enclosures.len(),"entries":self.entries.len()})
    }
    /// Import completed cells only. Caller reports unfinished/unsupported cells explicitly.
    fn import_cell(&mut self, graph: &mut ReuseGraph, path: &Path) -> Result<()> {
        let report: Value = serde_json::from_slice(&std::fs::read(path.join("ripen.json"))?)?;
        let state: SaturatedRuleComposition = serde_json::from_slice(&std::fs::read(
            path.join("saturated-rule-composition.json"),
        )?)?;
        let source = std::fs::read_to_string(path.join("entry.egg"))?;
        self.admit_native(graph, state, &report, source, Admission::InitialBake)?;
        Ok(())
    }
    pub(crate) fn admit_native(
        &mut self,
        graph: &mut ReuseGraph,
        state: SaturatedRuleComposition,
        report: &Value,
        source: String,
        admission: Admission,
    ) -> Result<(EnclosureId, bool)> {
        let body = RipSatEgraph::from_native_export(state.clone(), &report)?;
        let origin = &report["origin"];
        let recipe: Vec<Comb> = serde_json::from_value(origin["reuse_recipe"].clone())?;
        let terms = &origin["reuse_interface_terms"];
        let interface = resolve_interface(terms, &state)?;
        let upper = interface.upper;
        let lower = interface.lower;
        // Validate before mutating the library. The graph is only an import index.
        let mut validator = ReuseGraph::default();
        let temporary = validator.intern_body(body.clone())?.id;
        validator.register_enclosure(
            EnclosureDescriptor {
                body: temporary,
                interface: Interface {
                    upper: upper.clone(),
                    lower: lower.clone(),
                },
                origin: recipe.clone(),
            },
            Admission::InitialBake,
        )?;
        let interned = graph.intern_body(body)?;
        let descriptor = EnclosureDescriptor {
            body: interned.id,
            interface: Interface {
                upper: upper.iter().map(|i| interned.value_map[*i]).collect(),
                lower: lower.iter().map(|i| interned.value_map[*i]).collect(),
            },
            origin: recipe,
        };
        let enclosure = graph.register_enclosure(descriptor, admission)?;
        if interned.inserted {
            self.bodies.push(BodyRecord {
                graph: state,
                report: json!({"ripen":report["ripen"],"checks":report["checks"],"native_stop_reason":report["native_stop_reason"],"saturated_rule_composition":report["saturated_rule_composition"]}),
            });
        }
        if enclosure.0 == self.enclosures.len() {
            self.enclosures
                .push(graph.enclosure(enclosure).unwrap().clone());
        }
        if !self
            .entries
            .iter()
            .any(|e| e.enclosure == enclosure && e.source == source && e.origin == *origin)
        {
            self.entries.push(EntryRecord {
                enclosure,
                source,
                origin: origin.clone(),
            });
        }
        Ok((enclosure, !interned.inserted))
    }
}
fn resolve_interface(terms: &Value, state: &SaturatedRuleComposition) -> Result<Interface> {
    let mut parser = egglog::EGraph::default();
    let mut ports = |side: &str| -> Result<Vec<usize>> {
        terms[side]
            .as_array()
            .ok_or("missing interface terms")?
            .iter()
            .map(|s| {
                let term = s.as_str().ok_or("invalid interface term")?;
                let cmds = parser.parse_program(None, &format!("(let reuse_port {term})"))?;
                let [Command::Action(Action::Let(_, _, expr))] = cmds.as_slice() else {
                    return Err("invalid interface expression".into());
                };
                resolve(expr, state)
            })
            .collect()
    };
    Ok(Interface {
        upper: ports("upper")?,
        lower: ports("lower")?,
    })
}
fn resolve(expr: &Expr, state: &SaturatedRuleComposition) -> Result<usize> {
    let candidates: BTreeSet<_> = match expr {
        Expr::Lit(_, lit) => state
            .values
            .iter()
            .enumerate()
            .filter(|(_, v)| v.literal.as_deref() == Some(lit.to_string().as_str()))
            .map(|(i, _)| i)
            .collect(),
        Expr::Var(_, name) => state.ports.get(name).copied().into_iter().collect(),
        Expr::Call(_, op, args) => {
            let args = args
                .iter()
                .map(|e| resolve(e, state))
                .collect::<Result<Vec<_>>>()?;
            state
                .rows
                .iter()
                .filter(|r| r.op == *op && r.args == args)
                .map(|r| r.result)
                .collect()
        }
    };
    if candidates.len() != 1 {
        return Err(format!("interface term is absent or ambiguous: {expr}").into());
    }
    Ok(*candidates.first().unwrap())
}

pub fn bake(runs: &[String], output: &Path) -> Result<Value> {
    let mut library = Library::default();
    let mut graph = ReuseGraph::default();
    let mut skipped = vec![];
    for run in runs {
        let cells = Path::new(run).join("saturated-rule-composition/cells");
        let mut paths = std::fs::read_dir(cells)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        for path in paths.into_iter().filter(|p| p.is_dir()) {
            if let Err(e) = library.import_cell(&mut graph, &path) {
                skipped.push(json!({"cell":path,"reason":e.to_string()}));
            }
        }
    }
    library.save(output)?;
    Ok(
        json!({"counts":library.counts(),"skipped":skipped,"scope":"offline native-artifact library; no planner substitution"}),
    )
}

/// Online admission from completed native jobs. No filesystem trace replay.
pub struct Learner {
    pub graph: ReuseGraph,
    library: Library,
    initial: Value,
    admitted: Vec<Value>,
    rejected: Vec<Value>,
}
impl Learner {
    pub fn load(path: &Path) -> Result<Self> {
        let library = Library::load(path)?;
        let graph = library.instantiate()?;
        let initial = library.counts();
        Ok(Self {
            graph,
            library,
            initial,
            admitted: vec![],
            rejected: vec![],
        })
    }
    pub fn observe(
        &mut self,
        job: usize,
        state: SaturatedRuleComposition,
        report: &Value,
        source: String,
    ) {
        match self.library.admit_native(
            &mut self.graph,
            state,
            report,
            source,
            Admission::RuntimeSupplement,
        ) {
            Ok((id, shared)) => self
                .admitted
                .push(json!({"job":job,"enclosure":id,"body_storage_reused":shared})),
            Err(e) => self
                .rejected
                .push(json!({"job":job,"reason":e.to_string()})),
        }
    }
    pub fn report(&self) -> Value {
        json!({"initial":self.initial,"current":self.library.counts(),"new_enclosure_count":self.graph.metrics().new_enclosure_count,"body_storage_reuse_count":self.admitted.iter().filter(|a|a["body_storage_reused"]==true).count(),"admitted":self.admitted,"rejected":self.rejected,"unexpected_break_count":null,"scope":"online completed-ripen admission; live tier0 graft instances not yet observed"})
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.library.save(path)
    }
}
impl Library {
    /// Positive visible fragments only; marker rows are placeholders, not native tables.
    pub fn shared_bodies(&self) -> Result<super::storage::SharedBodies> {
        let mut previous = vec![];
        let mut packed = vec![];
        for b in &self.bodies {
            packed.push(super::storage::StoredState::select(&b.graph, &previous)?);
            previous.push(b.graph.clone());
        }
        super::storage::SharedBodies::new(packed)
    }
    pub fn fragments(&self) -> Vec<super::planner::Query> {
        let mut result = BTreeSet::new();
        for e in &self.entries {
            let b = &self.bodies[self.enclosures[e.enclosure.0].body.0].graph;
            let marker = e.origin["parameter_marker"].as_str().unwrap_or("");
            let atoms: Vec<_> = b
                .rows
                .iter()
                .enumerate()
                .filter(|(i, r)| r.op != marker && !b.subsumed_rows.contains(i))
                .map(|(_, r)| {
                    let mut columns = r.args.clone();
                    if b.values[r.result].sort != "Unit" {
                        columns.push(r.result);
                    }
                    (r.op.clone(), columns)
                })
                .collect();
            if atoms.is_empty() {
                continue;
            }
            let projection = atoms
                .iter()
                .flat_map(|(_, cols)| cols.iter().copied())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            result.insert(super::planner::Query {
                values: b
                    .values
                    .iter()
                    .map(|v| (v.sort.clone(), v.literal.clone()))
                    .collect(),
                atoms,
                projection,
            });
        }
        result.into_iter().collect()
    }
}
