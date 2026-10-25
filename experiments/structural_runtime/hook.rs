//! Speculation chooses probe order, never validity or coverage. Native commit
//! events maintain exact projections. Only the surviving witness products grow
//! bindings; predicted negatives go through the same exhaustive shallow checks.
use cell_assisted_egglog::{Result, table_live::SaturationHook};
use egglog::{
    EGraph, Value,
    ast::Schedule,
    cell_assist::{CellActionSink, CellOperand, CellQueryAtom},
    cell_events::{CellCommitBatch, CellCommitFeed},
};
use rustc_hash::FxHashMap as Map;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    time::Instant,
};

#[derive(Clone, Deserialize)]
pub struct Step {
    pub atom: usize,
    pub slot: usize,
    pub model: usize,
}
#[derive(Clone, Deserialize)]
pub struct Plan {
    pub name: String,
    pub ruleset: String,
    pub anchor: usize,
    pub steps: Vec<Step>,
    pub shape: Json,
}
#[derive(Clone, Deserialize)]
pub struct Model {
    pub sort: String,
    pub channel: String,
    pub sort_hash: u64,
}
#[derive(Clone, Deserialize)]
pub struct Token {
    token: Vec<Json>,
    hash: u64,
}
#[derive(Clone, Deserialize)]
pub struct Library {
    pub plans: Vec<Plan>,
    pub models: Vec<Model>,
    vocabulary: Vec<Token>,
    constructors: BTreeMap<String, (Vec<String>, String)>,
}
#[derive(Clone)]
struct Row {
    values: Vec<Value>,
    stamp: u32,
}
type Key = Vec<Value>;
type Node = (String, Value);
type Ref = (usize, Key);
#[derive(Default)]
struct Table {
    rows: Map<Key, Row>,
    deltas: BTreeMap<u32, BTreeSet<Key>>,
}
struct Index {
    atom: CellQueryAtom,
    slot: usize,
    table: usize,
    rows: Map<Value, BTreeMap<Key, Row>>,
    deltas: BTreeMap<u32, BTreeSet<Value>>,
}
struct Query {
    plan: Plan,
    atoms: Vec<CellQueryAtom>,
    anchor: usize,
    indices: Vec<usize>,
    slots: usize,
    ports: Vec<Map<Value, BTreeSet<Key>>>,
}

#[derive(Default, Serialize)]
struct Work {
    calls: usize,
    anchors: usize,
    delta_anchors: usize,
    predicted_positive: usize,
    predicted_negative: usize,
    positive_bindings: usize,
    recovered_bindings: usize,
    false_positive_roots: usize,
    recovered_roots: usize,
    reads: usize,
    witness_rows: usize,
    // One entry per attempted exact predicate; deepest counts each root ONCE.
    entered: Vec<usize>,
    deepest: Vec<usize>,
    rejected: Vec<usize>,
    complement_deepest: Vec<usize>,
    query_seconds: f64,
}
#[derive(Default, Serialize)]
struct Stats {
    bootstrap_rows: usize,
    row_events: usize,
    union_events: usize,
    constructor_events: usize,
    feature_requests: usize,
    feature_hits: usize,
    feature_invalidations: usize,
    score_requests: usize,
    score_hits: usize,
    gpu_batches: usize,
    gpu_rows: usize,
    uploaded_bytes: usize,
    downloaded_bytes: usize,
    gpu_seconds: f64,
    device_roundtrip_seconds: f64,
    maintenance_seconds: f64,
    query_seconds: f64,
    rules: BTreeMap<String, Work>,
}
fn bump(v: &mut Vec<usize>, i: usize) {
    if v.len() <= i {
        v.resize(i + 1, 0)
    }
    v[i] += 1;
}

pub struct Device {
    child: Child,
}
impl Device {
    pub fn new(worker: &Path, kernel: &Path, lib: &Path) -> Result<Self> {
        let mut child = Command::new(worker)
            .arg(kernel)
            .arg(lib)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut ready = [0; 4];
        child.stdout.as_mut().unwrap().read_exact(&mut ready)?;
        if &ready != b"RDY1" {
            return Err("bad predictor handshake".into());
        }
        Ok(Self { child })
    }
    fn predict(&mut self, requests: &[(usize, Vec<u64>)]) -> Result<(Vec<f32>, f64, usize, usize)> {
        let mut b = vec![];
        b.extend((requests.len() as u32).to_le_bytes());
        for (m, ts) in requests {
            b.extend((*m as u32).to_le_bytes());
            b.extend((ts.len() as u32).to_le_bytes());
            for t in ts {
                b.extend(t.to_le_bytes())
            }
        }
        let input = self.child.stdin.as_mut().unwrap();
        input.write_all(&(b.len() as u32).to_le_bytes())?;
        input.write_all(&b)?;
        input.flush()?;
        let out = self.child.stdout.as_mut().unwrap();
        let mut head = [0; 12];
        out.read_exact(&mut head)?;
        let n = u32::from_le_bytes(head[..4].try_into()?) as usize;
        if n != requests.len() {
            return Err("GPU response arity changed".into());
        }
        let dt = f64::from_le_bytes(head[4..].try_into()?);
        let mut bytes = vec![0; 4 * n];
        out.read_exact(&mut bytes)?;
        let scores = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        if scores.iter().any(|s| !s.is_finite()) {
            return Err("nonfinite GPU score".into());
        }
        Ok((scores, dt, b.len() + 4, 12 + bytes.len()))
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        if let Some(mut i) = self.child.stdin.take() {
            let _ = i.write_all(&0u32.to_le_bytes());
        }
        let _ = self.child.wait();
    }
}

pub struct Hook {
    lib: Library,
    device: Option<Device>,
    feed: Option<CellCommitFeed>,
    names: Vec<String>,
    ids: Map<String, usize>,
    tables: Vec<Table>,
    queries: Map<String, Query>,
    indices: Vec<Index>,
    outputs: Map<Node, BTreeSet<Ref>>,
    inputs: Map<Node, BTreeSet<(usize, Key, usize)>>,
    tokens: Map<Node, Vec<usize>>,
    scores: Map<(usize, Vec<usize>), f32>,
    stats: Stats,
    mode: String,
}
impl Hook {
    pub fn new(lib: Library, device: Option<Device>, mode: &str) -> Self {
        Self {
            lib,
            device,
            feed: None,
            names: vec![],
            ids: Map::default(),
            tables: vec![],
            queries: Map::default(),
            indices: vec![],
            outputs: Map::default(),
            inputs: Map::default(),
            tokens: Map::default(),
            scores: Map::default(),
            stats: Stats::default(),
            mode: mode.into(),
        }
    }
    fn initialize(&mut self, eg: &mut EGraph, sinks: &[&CellActionSink]) -> Result<()> {
        for p in self.lib.plans.clone() {
            let Some(s) = sinks.iter().find(|s| s.name == p.name) else {
                continue;
            };
            if !s.guards.is_empty() || s.native_residuals || !eg.cell_query_is_seminaive(s) {
                return Err("model query contract changed".into());
            }
            let shape_atoms = p.shape["atoms"].as_array().ok_or("missing atoms")?;
            if s.atoms.len() != shape_atoms.len() {
                return Err("query arity changed".into());
            }
            for (a, b) in s.atoms.iter().zip(shape_atoms) {
                let cols = a
                    .columns
                    .iter()
                    .map(|c| match c {
                        CellOperand::Slot(s) => json!({"Slot":s}),
                        CellOperand::Constant { text, .. } => json!({"Literal":text}),
                    })
                    .collect::<Vec<_>>();
                if json!(cols) != b["columns"]
                    || json!(a.sorts) != b["sorts"]
                    || a.function != b["table"]
                {
                    return Err("model/native wiring mismatch".into());
                }
            }
            let shared: BTreeSet<_> = s
                .atoms
                .iter()
                .flat_map(|a| a.columns.iter())
                .filter_map(|c| {
                    if let CellOperand::Slot(s) = c {
                        Some(*s)
                    } else {
                        None
                    }
                })
                .collect();
            if shared.len() != s.slot_count() {
                return Err("unbound native slot".into());
            }
            let vars = |a: &CellQueryAtom| {
                a.columns
                    .iter()
                    .filter_map(|c| {
                        if let CellOperand::Slot(s) = c {
                            Some(*s)
                        } else {
                            None
                        }
                    })
                    .collect::<BTreeSet<_>>()
            };
            let anchor = s.atoms.get(p.anchor).ok_or("invalid anchor")?;
            let av = vars(anchor);
            let mut uses = BTreeMap::<usize, usize>::new();
            for a in &s.atoms {
                for v in vars(a) {
                    *uses.entry(v).or_default() += 1;
                }
            }
            if uses.iter().any(|(v, n)| *n > 1 && !av.contains(v)) {
                return Err("anchor does not cover shared bindings".into());
            }
            let mut coverage = BTreeSet::from([p.anchor]);
            for step in &p.steps {
                let atom = s.atoms.get(step.atom).ok_or("invalid residual atom")?;
                let bound = vars(atom)
                    .intersection(&av)
                    .copied()
                    .collect::<BTreeSet<_>>();
                if bound != BTreeSet::from([step.slot])
                    || !coverage.insert(step.atom)
                    || step.model >= self.lib.models.len()
                {
                    return Err("residual projection contract is incomplete".into());
                }
                if s.slots()[step.slot].1 != self.lib.models[step.model].sort {
                    return Err("predictor key type mismatch".into());
                }
            }
            if coverage.len() != s.atoms.len() {
                return Err("model omitted an LHS atom".into());
            }
            self.queries.insert(
                s.name.clone(),
                Query {
                    plan: p.clone(),
                    atoms: s.atoms.clone(),
                    anchor: 0,
                    indices: vec![],
                    slots: s.slot_count(),
                    ports: (0..p.steps.len()).map(|_| Map::default()).collect(),
                },
            );
        }
        if self.queries.is_empty() {
            return Err("no exported plan matched live rules".into());
        }
        let mut names: BTreeSet<_> = self
            .queries
            .values()
            .flat_map(|q| q.atoms.iter().map(|a| a.function.clone()))
            .collect();
        // Only constructors named in the actual feature program can affect it.
        // Keep ALL their rows/alternatives, not all unrelated IR constructors.
        let mut mentioned = BTreeSet::new();
        if self.mode == "gpu" {
            for v in &self.lib.vocabulary {
                mentioned.insert(v.token[1].as_str().unwrap().to_string());
                if v.token[0] == "path" || v.token[0] == "path2" {
                    mentioned.insert(v.token[3].as_str().unwrap().to_string());
                }
                if v.token[0] == "path2" {
                    mentioned.insert(v.token[5].as_str().unwrap().to_string());
                }
            }
        }
        self.lib.constructors.retain(|k, _| mentioned.contains(k));
        names.extend(self.lib.constructors.keys().cloned());
        self.names = names.into_iter().collect();
        self.ids = self
            .names
            .iter()
            .enumerate()
            .map(|(i, s)| (s.clone(), i))
            .collect();
        self.tables = (0..self.names.len()).map(|_| Table::default()).collect();
        for q in self.queries.values_mut() {
            q.anchor = self.ids[&q.atoms[q.plan.anchor].function];
            for step in &q.plan.steps {
                let a = q.atoms[step.atom].clone();
                q.indices.push(self.indices.len());
                self.indices.push(Index {
                    table: self.ids[&a.function],
                    atom: a,
                    slot: step.slot,
                    rows: Map::default(),
                    deltas: BTreeMap::new(),
                });
            }
        }
        let (feed, b) = eg.subscribe_cell_commits(&self.names)?;
        self.feed = Some(feed);
        self.stats.bootstrap_rows = b.rows.len();
        self.ingest(b, true);
        Ok(())
    }
    fn graph_change(
        &mut self,
        t: usize,
        key: &Key,
        row: &Row,
        add: bool,
        dirty: &mut BTreeSet<Node>,
    ) {
        let Some((sorts, out)) = self.lib.constructors.get(&self.names[t]) else {
            return;
        };
        let result = (out.clone(), *row.values.last().unwrap());
        dirty.insert(result.clone());
        let rf = (t, key.clone());
        if add {
            self.outputs.entry(result).or_default().insert(rf);
        } else if let Some(x) = self.outputs.get_mut(&result) {
            x.remove(&rf);
        }
        for (i, s) in sorts.iter().enumerate() {
            // An incoming-role change affects this child's own port features,
            // not every ancestor sharing the child. Ancestor propagation is
            // needed only for the changed constructor result above.
            let node = (s.clone(), row.values[i]);
            self.stats.feature_invalidations += usize::from(self.tokens.remove(&node).is_some());
            let rf = (t, key.clone(), i);
            if add {
                self.inputs.entry(node).or_default().insert(rf);
            } else if let Some(x) = self.inputs.get_mut(&node) {
                x.remove(&rf);
            }
        }
    }
    fn parents(&self, n: &Node) -> Vec<Node> {
        self.inputs
            .get(n)
            .into_iter()
            .flatten()
            .filter_map(|(t, k, _)| {
                let row = self.tables[*t].rows.get(k)?;
                let (_, out) = self.lib.constructors.get(&self.names[*t])?;
                Some((out.clone(), *row.values.last()?))
            })
            .collect()
    }
    fn invalidate(&mut self, roots: BTreeSet<Node>) {
        let mut all = roots.clone();
        let mut frontier = roots;
        for _ in 0..2 {
            let next = frontier
                .iter()
                .flat_map(|n| self.parents(n))
                .collect::<BTreeSet<_>>();
            all.extend(next.clone());
            frontier = next;
        }
        for n in all {
            self.stats.feature_invalidations += usize::from(self.tokens.remove(&n).is_some());
        }
    }
    fn update_indices(&mut self, t: usize, k: &Key, r: &Row, add: bool) {
        for idx in self.indices.iter_mut().filter(|i| i.table == t) {
            if let Some(b) = bind(&idx.atom, &r.values) {
                let v = b[&idx.slot];
                if add {
                    idx.rows.entry(v).or_default().insert(k.clone(), r.clone());
                    idx.deltas.entry(r.stamp).or_default().insert(v);
                } else if let Some(rows) = idx.rows.get_mut(&v) {
                    rows.remove(k);
                    if rows.is_empty() {
                        idx.rows.remove(&v);
                    }
                }
            }
        }
        for q in self.queries.values_mut().filter(|q| q.anchor == t) {
            if let Some(b) = bind(&q.atoms[q.plan.anchor], &r.values) {
                for (s, port) in q.plan.steps.iter().zip(&mut q.ports) {
                    let v = b[&s.slot];
                    if add {
                        port.entry(v).or_default().insert(k.clone());
                    } else if let Some(keys) = port.get_mut(&v) {
                        keys.remove(k);
                        if keys.is_empty() {
                            port.remove(&v);
                        }
                    }
                }
            }
        }
    }
    fn ingest(&mut self, b: CellCommitBatch, bootstrap: bool) {
        let started = Instant::now();
        self.stats.row_events += b.rows.len();
        self.stats.union_events += b.unions.len();
        // On union, all affected table keys are supplied by native rebuild. Drop
        // feature memoization as well: these approximate scores never certify IDs.
        if !b.unions.is_empty() {
            self.stats.feature_invalidations += self.tokens.len();
            self.tokens.clear();
        }
        for t in b.clears {
            let old = std::mem::take(&mut self.tables[t].rows);
            for (k, r) in old {
                let mut d = BTreeSet::new();
                self.graph_change(t, &k, &r, false, &mut d);
                self.invalidate(d);
                self.update_indices(t, &k, &r, false);
            }
        }
        for change in b.rows {
            let t = change.table;
            let k = change.key;
            if let Some(old) = self.tables[t].rows.get(&k).cloned() {
                let mut d = BTreeSet::new();
                self.graph_change(t, &k, &old, false, &mut d);
                if !bootstrap {
                    self.invalidate(d)
                }
                self.update_indices(t, &k, &old, false);
                self.tables[t].rows.remove(&k);
            }
            if change.visible {
                if let Some(mut values) = change.row {
                    values.truncate(self.feed.as_ref().unwrap().tables[t].keys + 1);
                    let r = Row {
                        values,
                        stamp: change.timestamp.expect("native visible row version"),
                    };
                    self.tables[t].rows.insert(k.clone(), r.clone());
                    self.tables[t]
                        .deltas
                        .entry(r.stamp)
                        .or_default()
                        .insert(k.clone());
                    self.update_indices(t, &k, &r, true);
                    let mut d = BTreeSet::new();
                    self.graph_change(t, &k, &r, true, &mut d);
                    if !bootstrap {
                        self.invalidate(d)
                    }
                }
            }
            self.stats.constructor_events +=
                usize::from(self.lib.constructors.contains_key(&self.names[t]));
        }
        self.stats.maintenance_seconds += started.elapsed().as_secs_f64();
    }
    fn head(&self, n: &Node, op: &str) -> bool {
        self.outputs
            .get(n)
            .is_some_and(|rs| rs.iter().any(|(t, _)| self.names[*t] == op))
    }
    fn has_token(&self, n: &Node, token: &[Json]) -> bool {
        let kind = token[0].as_str().unwrap();
        let op = token[1].as_str().unwrap();
        if kind == "root" {
            return self.head(n, op);
        }
        if kind == "port" {
            return self.inputs.get(n).is_some_and(|r| {
                r.iter().any(|(t, _, i)| {
                    self.names[*t] == op && *i == token[2].as_u64().unwrap() as usize
                })
            });
        }
        self.outputs
            .get(n)
            .into_iter()
            .flatten()
            .filter(|(t, _)| self.names[*t] == op)
            .any(|(t, k)| {
                let r = &self.tables[*t].rows[k];
                let (sorts, _) = &self.lib.constructors[op];
                let i = token[2].as_u64().unwrap() as usize;
                let child = (sorts[i].clone(), r.values[i]);
                match kind {
                    "return-alias" => child == *n,
                    "alias" => {
                        let j = token[3].as_u64().unwrap() as usize;
                        child == (sorts[j].clone(), r.values[j])
                    }
                    "path" => self.head(&child, token[3].as_str().unwrap()),
                    "path2" => {
                        let middle = token[3].as_str().unwrap();
                        let j = token[4].as_u64().unwrap() as usize;
                        let leaf = token[5].as_str().unwrap();
                        self.outputs
                            .get(&child)
                            .into_iter()
                            .flatten()
                            .filter(|(t, _)| self.names[*t] == middle)
                            .any(|(t, k)| {
                                let rr = &self.tables[*t].rows[k];
                                let (ss, _) = &self.lib.constructors[middle];
                                self.head(&(ss[j].clone(), rr.values[j]), leaf)
                            })
                    }
                    _ => false,
                }
            })
    }
    fn signature(&mut self, n: Node) -> Vec<usize> {
        self.stats.feature_requests += 1;
        if let Some(s) = self.tokens.get(&n) {
            self.stats.feature_hits += 1;
            return s.clone();
        }
        let s = (0..self.lib.vocabulary.len())
            .filter(|i| self.has_token(&n, &self.lib.vocabulary[*i].token))
            .collect::<Vec<_>>();
        self.tokens.insert(n, s.clone());
        s
    }
    fn scores_for(&mut self, keys: &[(usize, Value)]) -> Result<Map<(usize, Value), f32>> {
        let mut routing = vec![];
        let mut pending: BTreeMap<(usize, Vec<usize>), usize> = BTreeMap::new();
        for &(m, v) in keys {
            let sig = self.signature((self.lib.models[m].sort.clone(), v));
            let k = (m, sig);
            self.stats.score_requests += 1;
            if self.scores.contains_key(&k) {
                self.stats.score_hits += 1;
            } else {
                let next = pending.len();
                pending.entry(k.clone()).or_insert(next);
            }
            routing.push(((m, v), k));
        }
        if !pending.is_empty() {
            let mut ordered = pending.into_iter().collect::<Vec<_>>();
            ordered.sort_by_key(|(_, i)| *i);
            let req = ordered
                .iter()
                .map(|((m, s), _)| {
                    let mut hashes = vec![self.lib.models[*m].sort_hash];
                    hashes.extend(s.iter().map(|i| self.lib.vocabulary[*i].hash));
                    (*m, hashes)
                })
                .collect::<Vec<_>>();
            let started = Instant::now();
            let (answers, dt, up, down) = self
                .device
                .as_mut()
                .ok_or("missing GPU predictor")?
                .predict(&req)?;
            self.stats.device_roundtrip_seconds += started.elapsed().as_secs_f64();
            self.stats.gpu_batches += 1;
            self.stats.gpu_rows += req.len();
            self.stats.gpu_seconds += dt;
            self.stats.uploaded_bytes += up;
            self.stats.downloaded_bytes += down;
            for ((k, _), v) in ordered.into_iter().zip(answers) {
                self.scores.insert(k, v);
            }
        }
        Ok(routing
            .into_iter()
            .map(|(k, s)| (k, self.scores[&s]))
            .collect())
    }
}
fn bind(atom: &CellQueryAtom, row: &[Value]) -> Option<BTreeMap<usize, Value>> {
    let mut b = BTreeMap::new();
    for (c, v) in atom.columns.iter().zip(row) {
        match c {
            CellOperand::Constant { value, .. } if value != v => return None,
            CellOperand::Slot(s) => {
                if b.insert(*s, *v).is_some_and(|old| old != *v) {
                    return None;
                }
            }
            _ => (),
        }
    }
    Some(b)
}
fn product(
    q: &Query,
    base: &BTreeMap<usize, Value>,
    factors: &[Vec<Row>],
    depth: usize,
    b: &mut Vec<Option<Value>>,
    stamp: u32,
    watermark: u32,
    out: &mut BTreeSet<Vec<Value>>,
) {
    if depth == factors.len() {
        if stamp >= watermark {
            out.insert(
                b.iter()
                    .map(|v| v.expect("complete original binding"))
                    .collect(),
            );
        }
        return;
    }
    let step = &q.plan.steps[depth];
    for row in &factors[depth] {
        let local = bind(&q.atoms[step.atom], &row.values).unwrap();
        let mut added = vec![];
        let mut ok = true;
        for (s, v) in local {
            if b[s].is_some_and(|old| old != v) {
                ok = false;
                break;
            }
            if b[s].is_none() {
                b[s] = Some(v);
                added.push(s)
            }
        }
        if ok {
            product(
                q,
                base,
                factors,
                depth + 1,
                b,
                stamp.max(row.stamp),
                watermark,
                out,
            )
        }
        for s in added {
            b[s] = None;
        }
    }
}
impl SaturationHook for Hook {
    fn handles(&self, _: &Schedule) -> bool {
        false
    }
    fn try_boundary(&mut self, _: &mut EGraph, _: &Schedule) -> Result<Option<Json>> {
        Ok(None)
    }
    fn diagnostics(&self) -> Json {
        json!({"schema":"live-spine-complement/v1","mode":self.mode,"statistics":self.stats,
        "scope":"GPU scores only reorder exact CPU checks. Every eligible anchor retained; complete native bindings and timestamp frontier. No Cell proof or sat-skip credit."})
    }
    fn take_commits(&mut self, eg: &mut EGraph) -> Result<(Vec<String>, Vec<CellCommitBatch>)> {
        let Some(feed) = self.feed.as_mut() else {
            return Ok((vec![], vec![]));
        };
        let b = eg.drain_cell_commits(feed)?;
        self.ingest(b.clone(), false);
        Ok((self.names.clone(), vec![b]))
    }
    fn query_cells(
        &mut self,
        eg: &mut EGraph,
        set: &str,
        sinks: &[&CellActionSink],
    ) -> Result<Vec<(usize, Vec<Vec<Value>>)>> {
        if set != "always-run" {
            return Ok(vec![]);
        }
        let started = Instant::now();
        if self.feed.is_none() {
            self.initialize(eg, sinks)?;
        }
        let ids = sinks
            .iter()
            .enumerate()
            .filter(|(_, s)| self.queries.contains_key(&s.name))
            .map(|(i, s)| (i, s.name.clone(), eg.cell_query_watermark(s)))
            .collect::<Vec<_>>();
        let mut score_keys = BTreeSet::new();
        let mut roots = Map::default();
        for (_, name, wm) in &ids {
            let q = &self.queries[name];
            let mut rows = vec![];
            // Reverse subscriptions route changed residual keys to anchors. This
            // does not read predicate membership or enumerate witness products.
            let mut changed = self.tables[q.anchor]
                .deltas
                .range(*wm..)
                .flat_map(|(_, ks)| ks.iter().cloned())
                .collect::<BTreeSet<_>>();
            for (idx, port) in q.indices.iter().zip(&q.ports) {
                for (_, vs) in self.indices[*idx].deltas.range(*wm..) {
                    for v in vs {
                        if let Some(keys) = port.get(v) {
                            changed.extend(keys.iter().cloned());
                        }
                    }
                }
            }
            for key in changed {
                let Some(row) = self.tables[q.anchor].rows.get(&key) else {
                    continue;
                };
                let Some(b) = bind(&q.atoms[q.plan.anchor], &row.values) else {
                    continue;
                };
                if self.mode == "gpu" {
                    for s in &q.plan.steps {
                        score_keys.insert((s.model, b[&s.slot]));
                    }
                }
                rows.push((row.clone(), b));
            }
            roots.insert(name.clone(), rows);
        }
        let scores = if self.mode == "gpu" {
            self.scores_for(&score_keys.into_iter().collect::<Vec<_>>())?
        } else {
            Map::default()
        };
        let mut supplied = vec![];
        for (id, name, wm) in ids {
            let start = Instant::now();
            let q = &self.queries[&name];
            let work = self.stats.rules.entry(name.clone()).or_default();
            work.calls += 1;
            work.anchors += self.tables[q.anchor].rows.len();
            let mut output = BTreeSet::new();
            for (root, b) in &roots[&name] {
                work.delta_anchors += 1;
                let mut order = (0..q.plan.steps.len()).collect::<Vec<_>>();
                let value = |i: usize| {
                    scores
                        .get(&(q.plan.steps[i].model, b[&q.plan.steps[i].slot]))
                        .copied()
                        .unwrap_or(if self.mode == "reject-all" { -1.0 } else { 1.0 })
                };
                let positive = order.iter().all(|i| value(*i) >= 0.5);
                if positive {
                    work.predicted_positive += 1
                } else {
                    work.predicted_negative += 1
                }
                if self.mode == "gpu" {
                    order.sort_by(|a, b| value(*a).total_cmp(&value(*b)).then(a.cmp(b)));
                }
                let mut factors = vec![vec![]; q.plan.steps.len()];
                let mut depth = 0;
                let mut accepted = true;
                for i in order {
                    bump(&mut work.entered, depth);
                    work.reads += 1;
                    depth += 1;
                    let s = &q.plan.steps[i];
                    let found = self.indices[q.indices[i]].rows.get(&b[&s.slot]);
                    if found.is_none_or(|f| f.is_empty()) {
                        accepted = false;
                        bump(&mut work.rejected, depth);
                        break;
                    }
                    factors[i] = found.unwrap().values().cloned().collect();
                }
                bump(&mut work.deepest, depth);
                if !positive {
                    bump(&mut work.complement_deepest, depth)
                }
                if !accepted {
                    work.false_positive_roots += usize::from(positive);
                    continue;
                }
                work.witness_rows += factors.iter().map(Vec::len).sum::<usize>();
                let before = output.len();
                let mut slots = vec![None; q.slots];
                for (s, v) in b {
                    slots[*s] = Some(*v)
                }
                product(q, b, &factors, 0, &mut slots, root.stamp, wm, &mut output);
                if positive {
                    work.positive_bindings += output.len() - before
                } else {
                    work.recovered_roots += 1;
                    work.recovered_bindings += output.len() - before;
                }
            }
            work.query_seconds += start.elapsed().as_secs_f64();
            supplied.push((id, output.into_iter().collect()));
        }
        // Every exported query belongs to this same Run; retain only deltas
        // that some original query watermark can still need.
        let oldest = sinks
            .iter()
            .filter(|s| self.queries.contains_key(&s.name))
            .map(|s| eg.cell_query_watermark(s))
            .min()
            .unwrap_or(0);
        for table in &mut self.tables {
            table.deltas = table.deltas.split_off(&oldest);
        }
        for index in &mut self.indices {
            index.deltas = index.deltas.split_off(&oldest);
        }
        self.stats.query_seconds += started.elapsed().as_secs_f64();
        Ok(supplied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cell_assisted_egglog::table_live::Runtime;
    fn fixture(function: bool, alias: bool) -> Result<(Runtime, Vec<egglog::ast::Command>)> {
        let decl = if function {
            "(function P (Term) Term :merge new)"
        } else {
            "(relation P (Term Term))"
        };
        let lhs = if function { "(= z (P x))" } else { "(P x z)" };
        let root = if alias { "(Root x x)" } else { "(Root x y)" };
        let y = if alias { "x" } else { "y" };
        let src = format!(
            "(datatype Term (N i64) (Root Term Term)) {decl} (relation Q (Term Term)) (relation F (Term Term Term)) (ruleset always-run) (rule ((= r {root}) {lhs} (Q {y} w)) ((F r z w)) :ruleset always-run :name \"test\")"
        );
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &src)?;
        let s = eg.prepare_cell_action("always-run", "test")?;
        let ai = s.atoms.iter().position(|a| a.function == "Root").unwrap();
        let mut models = vec![];
        let mut steps = vec![];
        let av = s.atoms[ai]
            .columns
            .iter()
            .filter_map(|c| {
                if let CellOperand::Slot(i) = c {
                    Some(*i)
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        for (i, a) in s.atoms.iter().enumerate() {
            if i == ai {
                continue;
            };
            let slot = a
                .columns
                .iter()
                .filter_map(|c| {
                    if let CellOperand::Slot(i) = c {
                        Some(*i)
                    } else {
                        None
                    }
                })
                .find(|s| av.contains(s))
                .unwrap();
            steps.push(Step {
                atom: i,
                slot,
                model: models.len(),
            });
            models.push(Model {
                sort: "Term".into(),
                channel: a.function.clone(),
                sort_hash: 1,
            });
        }
        let atoms=s.atoms.iter().map(|a|json!({"table":a.function,"sorts":a.sorts,"columns":a.columns.iter().map(|c|match c{CellOperand::Slot(s)=>json!({"Slot":s}),CellOperand::Constant{text,..}=>json!({"Literal":text})}).collect::<Vec<_>>()})).collect::<Vec<_>>();
        let lib = Library {
            plans: vec![Plan {
                name: "test".into(),
                ruleset: "always-run".into(),
                anchor: ai,
                steps,
                shape: json!({"atoms":atoms}),
            }],
            models,
            vocabulary: vec![],
            constructors: BTreeMap::from([
                ("N".into(), (vec!["i64".into()], "Term".into())),
                (
                    "Root".into(),
                    (vec!["Term".into(), "Term".into()], "Term".into()),
                ),
            ]),
        };
        let cmds = eg.parse_program(None, "(run always-run 1)")?;
        let selected = vec!["always-run".into()];
        let mut r = Runtime::new_full_assisted(
            eg,
            &cmds,
            None,
            false,
            Some(&selected),
            Some(Box::new(Hook::new(lib, None, "reject-all"))),
        )?;
        r.enable_cell_query_audit()?;
        Ok((r, cmds))
    }
    #[test]
    fn complement_all_negative_preserves_products_and_late_facts() -> Result<()> {
        EGraph::set_num_threads(1);
        let (mut r, c) = fixture(false, false)?;
        r.input("(Root (N 1) (N 2)) (P (N 1) (N 3)) (P (N 1) (N 4))")?;
        r.run(&c)?;
        r.input("(Q (N 2) (N 5)) (Q (N 2) (N 6))")?;
        r.run(&c)?;
        r.run(&c)?;
        let j = r.report();
        assert_eq!(
            j["saturation_diagnostics"]["statistics"]["rules"]["test"]["recovered_bindings"],
            4
        );
        Ok(())
    }
    #[test]
    fn union_subsume_and_alias_use_current_native_frontier() -> Result<()> {
        EGraph::set_num_threads(1);
        let (mut r, c) = fixture(false, true)?;
        r.input("(Root (N 1) (N 2)) (P (N 1) (N 3)) (Q (N 2) (N 4))")?;
        r.run(&c)?;
        r.input("(union (N 1) (N 2))")?;
        r.run(&c)?;
        r.input("(subsume (Root (N 1) (N 2))) (Q (N 2) (N 5))")?;
        r.run(&c)?;
        assert!(
            r.report()["saturation_diagnostics"]["statistics"]["union_events"]
                .as_u64()
                .unwrap()
                > 0
        );
        Ok(())
    }
    #[test]
    fn function_replacement_does_not_reuse_old_witness() -> Result<()> {
        EGraph::set_num_threads(1);
        let (mut r, c) = fixture(true, false)?;
        r.input("(Root (N 1) (N 2)) (set (P (N 1)) (N 3)) (Q (N 2) (N 4))")?;
        r.run(&c)?;
        r.input("(set (P (N 1)) (N 5))")?;
        r.run(&c)?;
        r.run(&c)?;
        assert_eq!(
            r.report()["saturation_diagnostics"]["statistics"]["rules"]["test"]["recovered_bindings"],
            2
        );
        Ok(())
    }
}
