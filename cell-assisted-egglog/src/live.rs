//! Native commit events -> versioned Cell frames -> persistent matrix matches
//! -> resumable native Bake -> target-only GPU resume -> original native RHS.
//! The native graph is owned by this loop; no concurrent mutation may race Bake.
use super::*;
use crate::{
    live_bake::{Baker, Job},
    matrix_service::Service,
    program as ir,
};
use egglog::cell_events::{CellCommitFeed, CellProducerTap};
use serde_json::json;

#[derive(Default, Serialize)]
pub struct LiveStats {
    pub native_matches: usize,
    pub producer_events: usize,
    pub row_events: usize,
    pub union_events: usize,
    pub frame_point_reads: usize,
    pub retired_frames: usize,
    pub new_frames: usize,
    pub learned_instances: usize,
    pub learned_frames: usize,
    pub offered_channels: usize,
    pub fragment_occurrences: usize,
    pub unique_fragments: usize,
    pub already_committed_bindings: usize,
    pub matrix_installs: usize,
    pub matrix_advances: usize,
    pub target_resumes: usize,
    pub bake_started: usize,
    pub bake_completed: usize,
    pub bake_cache_hits: usize,
    pub shared_pending_bakes: usize,
    pub bake_polls: usize,
    pub dispatched_bindings: usize,
    pub cancelled_matches: usize,
    pub quiet_rounds: usize,
    pub audit_queries: usize,
    pub audit_uncovered: Vec<usize>,
    pub device: Json,
}
#[derive(Clone)]
struct Frame {
    template: usize,
    values: Vec<Value>,
}
#[derive(Clone)]
struct Fragment {
    cell: usize,
    lens: usize,
    part: usize,
}
struct Task {
    sources: Vec<usize>,
    binding: Vec<ir::Wire>,
    parts: Vec<(usize, usize)>,
    projection: Vec<usize>,
    key: Option<String>,
    installed: bool,
    committed: bool,
    cancelled: bool,
    error: Option<String>,
}
struct Recipe {
    template: usize,
    source_template: usize,
    source_parameters: Vec<usize>,
    partition: Vec<usize>,
    projection: Vec<usize>,
    certificate: Json,
}
struct CachedBake {
    job: Job,
    raw: Vec<u32>,
    recipe: Option<Recipe>,
}

pub struct LiveSession {
    source: Session,
    tap: CellProducerTap,
    feed: CellCommitFeed,
    fields: Vec<Vec<String>>,
    canonical_templates: Vec<usize>,
    frame_templates: Vec<usize>,
    reflected_instances: usize,
    worker: Service,
    baker: Baker,
    program: ir::Program,
    dirty: bool,
    frames: BTreeMap<usize, Frame>,
    active: BTreeSet<usize>,
    offers: BTreeSet<Vec<(usize, usize, usize)>>,
    identities: BTreeMap<(String, u32), u32>,
    tasks: Vec<Task>,
    bakes: BTreeMap<String, CachedBake>,
    coarse_templates: BTreeMap<String, usize>,
    dispatched: BTreeSet<Vec<Value>>,
    pub milliseconds: u64,
    pub facts: usize,
    pub stats: LiveStats,
}
fn partition(v: &[u32]) -> Vec<usize> {
    (0..v.len())
        .map(|i| v.iter().position(|x| *x == v[i]).unwrap())
        .collect()
}
impl LiveSession {
    pub fn new(mut source: Session, env: &str, worker: &Path) -> Result<Self> {
        if source.stats.epochs != 0 {
            return Err("attach live matrix before input".into());
        }
        if source.parts.iter().any(|p| !p.cell) || !source.sink.guards.is_empty() {
            return Err("live matrix currently needs certified Cell parts covering the whole constructor LHS; external tables/numeric guards stay unsupported".into());
        }
        let baker = Baker::new(env, &source.ruleset, &source.sink.name)?;
        for t in &source.templates {
            for &(i, _) in &t.parameters {
                if !source
                    .eg
                    .cell_sort_is_equality(t.state["values"][i]["sort"].as_str().unwrap())?
                {
                    return Err("live specialization cannot erase a numeric parameter".into());
                }
            }
        }
        let mut canonical = BTreeMap::new();
        let canonical_templates = source
            .templates
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let key = json!([
                    t.state,
                    t.parameters.iter().map(|(i, _)| i).collect::<Vec<_>>(),
                    t.markers
                ])
                .to_string();
                *canonical.entry(key).or_insert(i)
            })
            .collect();
        let fields = source
            .native
            .iter()
            .map(|name| {
                source
                    .templates
                    .iter()
                    .filter(|t| t.producer == *name)
                    .flat_map(|t| t.parameters.iter().map(|(_, v)| v.clone()))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let tap = source.eg.prepare_cell_producers(
            &source.ruleset,
            &source
                .native
                .iter()
                .cloned()
                .zip(fields.clone())
                .collect::<Vec<_>>(),
        )?;
        let mut tables = source
            .sink
            .atoms
            .iter()
            .map(|a| a.function.clone())
            .collect::<BTreeSet<_>>();
        for t in &source.templates {
            for r in t.state["rows"].as_array().unwrap() {
                let op = r["op"].as_str().unwrap();
                if !t.markers.contains(op) {
                    tables.insert(op.into());
                }
            }
        }
        for name in source.eg.get_function_names() {
            let f = source.eg.get_function(&name).unwrap();
            if f.is_constructor() && !f.is_let_binding() {
                tables.insert(name);
            }
        }
        let (feed, bootstrap) = source
            .eg
            .subscribe_cell_commits(&tables.into_iter().collect::<Vec<_>>())?;
        if !bootstrap.rows.is_empty() {
            return Err("live matrix must subscribe before native input".into());
        }
        let templates = source
            .templates
            .iter()
            .map(|t| json!({"catalog_state":t.id,"closure":t.state,"producer":t.producer}))
            .collect();
        let frame_templates = (0..source.templates.len()).collect();
        Ok(Self {
            source,
            frame_templates,
            reflected_instances: 0,
            tap,
            feed,
            fields,
            canonical_templates,
            worker: Service::new(worker)?,
            baker,
            program: ir::Program {
                version: 1,
                bits: 32,
                slots: 1,
                nodes: vec![ir::Node {
                    op: ir::Op::Port,
                    args: vec![],
                    slot: 0,
                }],
                cells: vec![],
                templates,
                conditions: vec![],
                channels: vec![],
                stages: vec![vec![]],
                updates: vec![],
                numeric: None,
                evidence: json!({"source":"native committed rows plus compact producer callbacks; analyze certified templates","scope":"covered positive constructor fragments; CPU keyed proof admission; no global completeness claim"}),
            },
            dirty: false,
            frames: BTreeMap::new(),
            active: BTreeSet::new(),
            offers: BTreeSet::new(),
            identities: BTreeMap::new(),
            tasks: vec![],
            bakes: BTreeMap::new(),
            coarse_templates: BTreeMap::new(),
            dispatched: BTreeSet::new(),
            milliseconds: 50,
            facts: 200_000,
            stats: LiveStats::default(),
        })
    }
    pub fn engine(&self) -> &EGraph {
        &self.source.eg
    }
    pub fn input(&mut self, text: &str) -> Result<()> {
        let cmds = self.source.eg.parse_program(None, text)?;
        if cmds
            .iter()
            .any(|c| !matches!(c, Command::Action(..) | Command::Check(..)))
        {
            return Err("live input must keep the rule environment frozen".into());
        }
        self.source.eg.run_program(cmds)?;
        Ok(())
    }
    pub fn pending_matches(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.key.is_some() && !t.cancelled && !t.committed && t.error.is_none())
            .count()
    }
    pub fn report(&self) -> Json {
        json!({"stats":self.stats,"pending_matches":self.pending_matches(),"failed_matches":self.tasks.iter().filter(|t|t.error.is_some()).count(),"observed_limits":{"time_slice_ms":self.milliseconds,"facts":self.facts},
            "tasks":self.tasks.iter().enumerate().map(|(i,t)|json!({"channel":i,"committed":t.committed,"cancelled":t.cancelled,"error":t.error,"bake_key":t.key})).collect::<Vec<_>>(),
            "bakes":self.bakes.iter().map(|(key,b)|json!({"key":key,"steps":b.job.steps,"stop":b.job.stop,"complete":b.recipe.is_some(),"certificate":b.recipe.as_ref().map(|r|&r.certificate)})).collect::<Vec<_>>(),
            "scope":"actual native producer/commit/RHS loop with persistent MPS matching and automatic local Bake; only covered constructor fragments; no arbitrary .egg or speedup claim"})
    }
    fn observe(&mut self, name: &str, fields: &[String], values: &[Value]) {
        for (tid, t) in self
            .source
            .templates
            .iter()
            .enumerate()
            .filter(|(_, t)| t.producer == name)
        {
            let params = t
                .parameters
                .iter()
                .map(|(_, n)| fields.iter().position(|f| f == n).map(|i| values[i]))
                .collect::<Option<Vec<_>>>();
            if let Some(parameters) = params {
                if self.source.instance_keys.insert((tid, parameters.clone())) {
                    self.source.instances.push(Instance {
                        template: tid,
                        parameters,
                    });
                }
            }
        }
    }
    fn width(&mut self, n: usize) {
        while self.program.nodes.len() < n {
            let slot = self.program.nodes.len();
            self.program.nodes.push(ir::Node {
                op: ir::Op::Port,
                args: vec![],
                slot,
            });
        }
        self.program.slots = self.program.slots.max(n);
    }
    fn refresh(&mut self) -> Result<Vec<usize>> {
        let mut old = BTreeMap::new();
        for i in &self.active {
            let f = &self.frames[i];
            old.insert((f.template, f.values.clone()), *i);
        }
        let mut now = BTreeSet::new();
        let mut resolved = vec![];
        for inst in &self.source.instances {
            if let Some(values) = resolve(
                &self.source.eg,
                &self.source.templates[inst.template],
                &inst.parameters,
                &mut self.stats.frame_point_reads,
            ) {
                resolved.push((self.canonical_templates[inst.template], values));
            }
        }
        for (tid, values) in resolved {
            let key = (tid, values.clone());
            let i = if let Some(i) = old.get(&key) {
                *i
            } else {
                let t = &self.source.templates[tid];
                let mut binding = vec![];
                for (i, v) in values.iter().enumerate() {
                    let key = (
                        t.state["values"][i]["sort"].as_str().unwrap().to_owned(),
                        v.rep(),
                    );
                    let next = u32::try_from(self.identities.len())?;
                    binding.push(*self.identities.entry(key).or_insert(next));
                }
                self.width(binding.len());
                let i = self.program.cells.len();
                self.program.cells.push(ir::Cell {
                    template: self.frame_templates[tid],
                    partition: partition(&binding),
                    binding: Some(binding),
                });
                self.frames.insert(
                    i,
                    Frame {
                        template: tid,
                        values: values.clone(),
                    },
                );
                old.insert(key, i);
                self.dirty = true;
                self.stats.new_frames += 1;
                if self.source.templates[tid].producer == "live-certified-target" {
                    self.stats.learned_frames += 1;
                }
                i
            };
            now.insert(i);
        }
        self.reflected_instances = self.source.instances.len();
        let retired = self.active.difference(&now).copied().collect::<Vec<_>>();
        self.stats.retired_frames += retired.len();
        self.active = now;
        for t in &mut self.tasks {
            if !t.committed && !t.cancelled && t.sources.iter().any(|s| !self.active.contains(s)) {
                t.cancelled = true;
                self.stats.cancelled_matches += 1;
            }
        }
        Ok(retired)
    }
    fn offer(&mut self) -> Result<()> {
        let mut combos = vec![vec![]];
        for part in 0..self.source.parts.len() {
            // Exact projection hashcons, not a representative/template budget.
            // Different Cell proofs of the same typed partial tuple share one
            // matching computation. Owners are refreshed after native changes;
            // if one proof retires another live proof can authorize a new version.
            let mut grouped = BTreeMap::<(Vec<Option<u32>>, Vec<(u32, u32)>), Fragment>::new();
            for &id in &self.active {
                let frame = &self.frames[&id];
                let values = self.program.cells[id].binding.as_ref().unwrap();
                for (li, lens) in self.source.lenses[frame.template][part].iter().enumerate() {
                    self.stats.fragment_occurrences += 1;
                    let binding = lens.binding.iter().map(|v| v.map(|v| values[v])).collect();
                    let mut aliases = lens
                        .aliases
                        .iter()
                        .filter_map(|(a, b)| {
                            let (a, b) = (values[*a], values[*b]);
                            (a != b).then_some((a.min(b), a.max(b)))
                        })
                        .collect::<Vec<_>>();
                    aliases.sort();
                    aliases.dedup();
                    grouped.entry((binding, aliases)).or_insert(Fragment {
                        cell: id,
                        lens: li,
                        part,
                    });
                }
            }
            self.stats.unique_fragments += grouped.len();
            let choices = grouped.into_values().collect::<Vec<_>>();
            let mut next = vec![];
            for prefix in combos {
                for f in &choices {
                    let mut p = prefix.clone();
                    p.push(f.clone());
                    next.push(p)
                }
            }
            combos = next;
        }
        for pieces in combos {
            let key = pieces
                .iter()
                .map(|f| (f.cell, f.part, f.lens))
                .collect::<Vec<_>>();
            if !self.offers.insert(key) {
                continue;
            }
            let mut bindings = vec![vec![]; self.source.sink.slots().len()];
            let mut equalities = vec![];
            for f in &pieces {
                let frame = &self.frames[&f.cell];
                let lens = &self.source.lenses[frame.template][f.part][f.lens];
                for (s, v) in lens.binding.iter().enumerate() {
                    if let Some(v) = v {
                        bindings[s].push(ir::Wire(f.cell, *v));
                    }
                }
                equalities.extend(
                    lens.aliases
                        .iter()
                        .map(|(a, b)| (ir::Wire(f.cell, *a), ir::Wire(f.cell, *b))),
                );
            }
            if bindings.iter().any(|b| b.is_empty()) {
                return Err("fragment partition did not provide the full native binding".into());
            }
            for b in &bindings {
                equalities.extend(b[1..].iter().map(|w| (b[0], *w)));
            }
            let sources = pieces
                .iter()
                .map(|f| f.cell)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let mut parts = vec![];
            let mut raw = vec![];
            let mut offsets = BTreeMap::new();
            for &i in &sources {
                let f = &self.frames[&i];
                offsets.insert(i, raw.len());
                parts.push((f.template, raw.len()));
                raw.extend((0..f.values.len()).map(|slot| ir::Wire(i, slot)));
            }
            let projection = bindings
                .iter()
                .map(|b| offsets[&b[0].0] + b[0].1)
                .collect::<Vec<_>>();
            let shape = json!([parts, projection]).to_string();
            let tid = if let Some(&tid) = self.coarse_templates.get(&shape) {
                tid
            } else {
                let tid = self.program.templates.len();
                self.program.templates.push(json!({"kind":"unbaked-native-context","parts":parts,"binding_projection":projection}));
                self.coarse_templates.insert(shape, tid);
                tid
            };
            self.width(raw.len());
            let target = self.program.cells.len();
            self.program.cells.push(ir::Cell {
                template: tid,
                binding: None,
                partition: (0..raw.len()).collect(),
            });
            let mut reqs = vec![];
            for (left, right) in equalities {
                let i = self.program.conditions.len();
                self.program.conditions.push(ir::Condition { left, right });
                reqs.push(ir::Requirement {
                    condition: i,
                    depth: 0,
                });
            }
            let ci = self.program.channels.len();
            self.program.channels.push(ir::Channel {
                label: format!("{}:{ci}", self.source.sink.name),
                sources: sources.clone(),
                alternatives: vec![reqs],
                target: Some(ir::Target {
                    certified: false,
                    cell: target,
                    bindings: raw,
                    specializations: vec![],
                }),
            });
            self.program.stages[0].push(ci);
            self.tasks.push(Task {
                sources,
                binding: bindings.iter().map(|b| b[0]).collect(),
                parts,
                projection,
                key: None,
                installed: false,
                committed: false,
                cancelled: false,
                error: None,
            });
            self.stats.offered_channels += 1;
            self.dirty = true;
        }
        Ok(())
    }
    fn install(&mut self) -> Result<()> {
        if self.dirty {
            self.worker.install(&self.program)?;
            self.stats.matrix_installs += 1;
            self.dirty = false;
        }
        Ok(())
    }
    fn specialize(&mut self, response: &Json) -> Result<bool> {
        for out in response["outputs"]
            .as_array()
            .ok_or("missing matrix outputs")?
        {
            let ci = out["channel"].as_u64().ok_or("bad channel")? as usize;
            let t = &mut self.tasks[ci];
            if t.committed || t.cancelled || t.error.is_some() || t.installed {
                continue;
            }
            let original = t
                .binding
                .iter()
                .map(|w| self.frames[&w.0].values[w.1])
                .collect::<Vec<_>>();
            if self.dispatched.contains(&original) {
                t.committed = true;
                self.stats.already_committed_bindings += 1;
                continue;
            }
            let raw = out["binding"]
                .as_array()
                .ok_or("bad completed binding")?
                .iter()
                .map(|v| {
                    v.as_u64()
                        .and_then(|v| u32::try_from(v).ok())
                        .ok_or("bad coordinate")
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let key = json!([t.parts, t.projection, partition(&raw)]).to_string();
            t.key = Some(key.clone());
            if !self.bakes.contains_key(&key) {
                match self
                    .baker
                    .start(&self.source.templates, &t.parts, &raw, &t.projection)
                {
                    Ok(job) => {
                        self.bakes.insert(
                            key,
                            CachedBake {
                                job,
                                raw,
                                recipe: None,
                            },
                        );
                        self.stats.bake_started += 1;
                    }
                    Err(e) => {
                        t.error = Some(e.to_string());
                    }
                }
            } else if self.bakes[&key].recipe.is_some() {
                self.stats.bake_cache_hits += 1;
            } else {
                self.stats.shared_pending_bakes += 1;
            }
        }
        let needed = self
            .tasks
            .iter()
            .filter(|t| !t.cancelled && !t.committed && !t.installed && t.error.is_none())
            .filter_map(|t| t.key.clone())
            .collect::<BTreeSet<_>>();
        for key in needed {
            let b = self.bakes.get_mut(&key).unwrap();
            if b.recipe.is_some() {
                continue;
            }
            self.stats.bake_polls += 1;
            match b.job.poll(self.milliseconds, self.facts) {
                Ok(true) => {
                    let partition = partition(&b.raw);
                    let projection = (0..b.raw.len())
                        .filter(|i| partition[*i] == *i)
                        .collect::<Vec<_>>();
                    let certificate = json!({"native_closure":b.job.done,"environment":self.baker.environment(),"opaque_alias_partition":partition,"proof":"entire retained parent states, exact native RHS binding, native saturation; no source-variable numeric erasure"});
                    let template = self.program.templates.len();
                    self.program.templates.push(certificate.clone());
                    let state = b.job.done.as_ref().unwrap()["state"].clone();
                    let raw_ids = b.job.done.as_ref().unwrap()["raw_values"]
                        .as_array()
                        .unwrap();
                    let source_parameters = raw_ids
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| {
                            state["values"][v.as_u64().unwrap() as usize]["literal"].is_null()
                        })
                        .map(|(i, _)| i)
                        .collect::<Vec<_>>();
                    let parameters = source_parameters
                        .iter()
                        .map(|i| (raw_ids[*i].as_u64().unwrap() as usize, format!("raw{i}")))
                        .collect();
                    let markers = state["rows"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|r| {
                            r["op"]
                                .as_str()
                                .filter(|op| op.starts_with("LiveBakeInput"))
                                .map(str::to_owned)
                        })
                        .collect();
                    let mut literals = BTreeMap::new();
                    for (i, v) in state["values"].as_array().unwrap().iter().enumerate() {
                        if let Some(lit) = v["literal"].as_str() {
                            let e =
                                egglog::ast::Parser::default().get_expr_from_string(None, lit)?;
                            literals.insert(i, self.source.eg.eval_expr(&e)?.1);
                        }
                    }
                    let learned = Template {
                        id: template,
                        state,
                        parameters,
                        producer: "live-certified-target".into(),
                        markers,
                        literals,
                    };
                    let lenses = self
                        .source
                        .parts
                        .iter()
                        .map(|part| project(&learned, &self.source.sink, part))
                        .collect();
                    let source_template = self.source.templates.len();
                    self.source.templates.push(learned);
                    self.source.lenses.push(lenses);
                    self.canonical_templates.push(source_template);
                    self.frame_templates.push(template);
                    b.recipe = Some(Recipe {
                        template,
                        source_template,
                        source_parameters,
                        partition,
                        projection,
                        certificate,
                    });
                    self.stats.bake_completed += 1;
                }
                Ok(false) => (),
                Err(e) => {
                    for t in &mut self.tasks {
                        if t.key.as_ref() == Some(&key) {
                            t.error = Some(e.to_string());
                        }
                    }
                }
            }
        }
        let mut added = false;
        for (ci, t) in self.tasks.iter_mut().enumerate() {
            if t.cancelled || t.committed || t.installed || t.error.is_some() {
                continue;
            }
            let Some(r) = t
                .key
                .as_ref()
                .and_then(|k| self.bakes.get(k))
                .and_then(|b| b.recipe.as_ref())
            else {
                continue;
            };
            let cell = self.program.cells.len();
            self.program.cells.push(ir::Cell {
                template: r.template,
                binding: None,
                partition: (0..r.projection.len()).collect(),
            });
            self.program.channels[ci]
                .target
                .as_mut()
                .unwrap()
                .specializations
                .push(ir::Specialization {
                    cell,
                    partition: r.partition.clone(),
                    projection: r.projection.clone(),
                    certificate: r.certificate.clone(),
                });
            t.installed = true;
            self.dirty = true;
            added = true;
        }
        Ok(added)
    }
    fn commit(&mut self, response: &Json) -> Result<usize> {
        let mut rows = BTreeSet::new();
        let mut accepted = vec![];
        for out in response["outputs"].as_array().unwrap() {
            if out["status"] != 3 {
                continue;
            }
            let i = out["channel"].as_u64().unwrap() as usize;
            let t = &self.tasks[i];
            if t.committed || t.cancelled {
                continue;
            }
            if t.sources.iter().any(|s| !self.active.contains(s)) {
                return Err("stale native source lease".into());
            }
            let row = t
                .binding
                .iter()
                .map(|w| self.frames[&w.0].values[w.1])
                .collect::<Vec<_>>();
            let row = self.source.sink.canonicalize(&self.source.eg, &row)?;
            if !self.dispatched.contains(&row) {
                rows.insert(row);
            }
            accepted.push(i);
        }
        let rows = rows.into_iter().collect::<Vec<_>>();
        if !rows.is_empty() {
            // No consumer LHS query here. The original action sink owns native
            // guards, insert, union, merge and rebuild at a commit boundary.
            self.source
                .eg
                .apply_cell_bindings(&self.source.sink, &rows)?;
            let fields = self
                .source
                .sink
                .slots()
                .iter()
                .map(|(n, _)| n.to_string())
                .collect::<Vec<_>>();
            let name = self.source.sink.name.clone();
            for row in &rows {
                self.observe(&name, &fields, row);
                self.dispatched.insert(row.clone());
            }
        }
        for i in accepted {
            let t = &self.tasks[i];
            if let Some(recipe) = t
                .key
                .as_ref()
                .and_then(|k| self.bakes.get(k))
                .and_then(|b| b.recipe.as_ref())
            {
                let raw = self.program.channels[i]
                    .target
                    .as_ref()
                    .unwrap()
                    .bindings
                    .iter()
                    .map(|w| self.frames[&w.0].values[w.1])
                    .collect::<Vec<_>>();
                let parameters = recipe
                    .source_parameters
                    .iter()
                    .map(|p| raw[*p])
                    .collect::<Vec<_>>();
                let tid = recipe.source_template;
                if self.source.instance_keys.insert((tid, parameters.clone())) {
                    self.source.instances.push(Instance {
                        template: tid,
                        parameters,
                    });
                    self.stats.learned_instances += 1;
                }
            }
            self.tasks[i].committed = true;
        }
        self.stats.dispatched_bindings += rows.len();
        Ok(rows.len())
    }
    pub fn round(&mut self) -> Result<bool> {
        if self.milliseconds == 0 || self.facts == 0 {
            return Err("Bake observation limits must be positive".into());
        }
        let report = self
            .source
            .eg
            .step_cell_producers(&self.tap, &self.source.ruleset)?;
        self.stats.native_matches += report.num_matches_per_rule.values().sum::<usize>();
        let delta = self.source.eg.drain_cell_commits(&mut self.feed)?;
        let events = self.tap.drain();
        self.stats.row_events += delta.rows.len();
        self.stats.union_events += delta.unions.len();
        self.stats.producer_events += events.len();
        let changed = !delta.rows.is_empty()
            || !delta.unions.is_empty()
            || !delta.clears.is_empty()
            || !events.is_empty()
            || self.source.instances.len() != self.reflected_instances;
        if !delta.unions.is_empty()
            || !delta.clears.is_empty()
            || delta.rows.iter().any(|r| !r.visible)
        {
            self.dispatched.clear();
        }
        for e in events {
            let name = self.source.native[e.rule].clone();
            let fields = self.fields[e.rule].clone();
            self.observe(&name, &fields, &e.values);
        }
        let retired = if changed {
            let retired = self.refresh()?;
            self.offer()?;
            retired
        } else {
            vec![]
        };
        if self.program.cells.is_empty() {
            if self.source.audit {
                let uncovered = self.source.eg.audit_cell_matches(&self.source.sink)?.len();
                self.stats.audit_queries += 1;
                self.stats.audit_uncovered.push(uncovered);
            }
            return Ok(report.updated);
        }
        let added = self.dirty;
        self.install()?;
        let mut response = if added || !retired.is_empty() {
            self.stats.matrix_advances += 1;
            self.worker.step(false, &retired)?
        } else {
            self.stats.target_resumes += 1;
            self.worker.step(true, &[])?
        };
        if self.specialize(&response)? {
            self.install()?;
            self.stats.target_resumes += 1;
            response = self.worker.step(true, &[])?;
        }
        if self.source.audit {
            let expected = self
                .source
                .eg
                .audit_cell_matches(&self.source.sink)?
                .into_iter()
                .collect::<BTreeSet<_>>();
            let actual = response["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|o| {
                    let t = &self.tasks[o["channel"].as_u64()? as usize];
                    (!t.cancelled).then(|| {
                        t.binding
                            .iter()
                            .map(|w| self.frames[&w.0].values[w.1])
                            .collect::<Vec<_>>()
                    })
                })
                .collect::<BTreeSet<_>>();
            if !actual.is_subset(&expected) {
                return Err("live matrix produced a non-native binding".into());
            }
            self.stats.audit_queries += 1;
            self.stats
                .audit_uncovered
                .push(expected.difference(&actual).count());
        }
        let emitted = self.commit(&response)?;
        self.stats.device = response;
        if !changed && emitted == 0 && self.pending_matches() == 0 {
            self.stats.quiet_rounds += 1;
        }
        Ok(report.updated || emitted > 0)
    }
}
