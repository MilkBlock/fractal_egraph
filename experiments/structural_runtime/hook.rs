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
    sync::Arc,
    time::Instant,
};

#[path = "coordinates.rs"]
mod coordinates;
#[path = "markers.rs"]
mod markers;

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
#[derive(Default, Serialize)]
struct BlockStats {
    cache_probes: usize,
    reads: usize,
    hits: usize,
    empty_hits: usize,
    invalidations: usize,
    witness_rows_materialized: usize,
}

struct Index {
    atom: CellQueryAtom,
    slot: usize,
    table: usize,
    rows: Map<Value, BTreeMap<Key, Row>>,
    deltas: BTreeMap<u32, BTreeSet<Value>>,
    versions: Map<Value, u64>,
    answers: Map<Value, (u64, Arc<Vec<Row>>)>,
}
impl Index {
    fn cached(&self, v: Value) -> Option<Arc<Vec<Row>>> {
        self.answers.get(&v).map(|(version, rows)| {
            debug_assert_eq!(*version, self.versions.get(&v).copied().unwrap_or(0));
            rows.clone()
        })
    }
    fn touch(&mut self, v: Value) -> bool {
        *self.versions.entry(v).or_default() += 1;
        self.answers.remove(&v).is_some()
    }
    fn answer(&mut self, v: Value, shared: bool, stats: &mut BlockStats) -> Arc<Vec<Row>> {
        let version = self.versions.get(&v).copied().unwrap_or(0);
        stats.reads += 1;
        let answer = Arc::new(
            self.rows
                .get(&v)
                .into_iter()
                .flat_map(|rs| rs.values().cloned())
                .collect::<Vec<_>>(),
        );
        stats.witness_rows_materialized += answer.len();
        if shared {
            self.answers.insert(v, (version, answer.clone()));
        }
        answer
    }
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
    exact_cached_rejections: usize,
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
    blocks: BlockStats,
    unchanged_notifications: usize,
    timestamp_only_changes: usize,
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
    bootstrap_maintenance_seconds: f64,
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
    coordinates: Option<coordinates::Coordinates>,
    markers: Option<markers::Markers>,
    full_rows: Vec<bool>,
    marker_coordinates: bool,
    event_coordinates: bool,
    shared_conditions: bool,
    coordinate_audit: bool,
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
            coordinates: None,
            markers: None,
            full_rows: vec![],
            marker_coordinates: std::env::var("STRUCTURAL_COORDINATES").as_deref() == Ok("markers"),
            event_coordinates: std::env::var("STRUCTURAL_COORDINATES").as_deref() != Ok("legacy"),
            shared_conditions: std::env::var("STRUCTURAL_SHARED_CONDITIONS").as_deref() != Ok("0"),
            coordinate_audit: std::env::var("STRUCTURAL_COORDINATE_AUDIT").as_deref() == Ok("1"),
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
        let mut shared = Map::default();
        for q in self.queries.values_mut() {
            q.anchor = self.ids[&q.atoms[q.plan.anchor].function];
            for step in &q.plan.steps {
                let a = q.atoms[step.atom].clone();
                let mut vars = Map::default();
                let mut columns = vec![];
                for c in &a.columns {
                    columns.push(match c {
                        CellOperand::Slot(s) => {
                            let next = vars.len();
                            json!({"slot":*vars.entry(*s).or_insert(next)})
                        }
                        CellOperand::Constant { text, .. } => json!({"literal":text}),
                    });
                }
                let key = json!([a.function, a.sorts, columns, vars[&step.slot]]).to_string();
                if self.shared_conditions {
                    if let Some(id) = shared.get(&key) {
                        q.indices.push(*id);
                        continue;
                    }
                }
                let id = self.indices.len();
                shared.insert(key, id);
                q.indices.push(id);
                self.indices.push(Index {
                    table: self.ids[&a.function],
                    atom: a,
                    slot: step.slot,
                    rows: Map::default(),
                    deltas: BTreeMap::new(),
                    versions: Map::default(),
                    answers: Map::default(),
                });
            }
        }
        self.full_rows = (0..self.names.len())
            .map(|t| {
                !self.marker_coordinates
                    || self.queries.values().any(|q| q.anchor == t)
                    || self.indices.iter().any(|i| i.table == t)
            })
            .collect();
        if self.marker_coordinates && self.mode == "gpu" {
            self.markers = Some(markers::Markers::new(
                &self.names,
                &self.lib,
                self.coordinate_audit,
            ));
        } else if self.event_coordinates && self.mode == "gpu" {
            self.coordinates = Some(coordinates::Coordinates::new(&self.names, &self.lib));
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
    fn update_indices(&mut self, t: usize, k: &Key, r: &Row, add: bool, structure: bool) {
        for idx in self.indices.iter_mut().filter(|i| i.table == t) {
            if let Some(b) = bind(&idx.atom, &r.values) {
                let v = b[&idx.slot];
                self.stats.blocks.invalidations += usize::from(idx.touch(v));
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
        if !structure {
            return;
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
    fn topology(&mut self, t: usize, k: &Key, r: &Row, add: bool, bootstrap: bool) {
        if self.markers.is_some() {
            return;
        }
        if let Some(c) = &mut self.coordinates {
            if add {
                c.insert(t, k, &r.values)
            } else {
                c.remove(t, k)
            }
        } else {
            let mut d = BTreeSet::new();
            self.graph_change(t, k, r, add, &mut d);
            if !bootstrap {
                self.invalidate(d)
            }
        }
    }
    fn ingest(&mut self, b: CellCommitBatch, bootstrap: bool) {
        let started = Instant::now();
        self.stats.row_events += b.rows.len();
        self.stats.union_events += b.unions.len();
        if self.coordinates.is_none() && self.markers.is_none() && !b.unions.is_empty() {
            self.stats.feature_invalidations += self.tokens.len();
            self.tokens.clear();
        }
        for t in b.clears {
            if let Some(m) = &mut self.markers {
                m.clear(t);
            }
            let old = std::mem::take(&mut self.tables[t].rows);
            for (k, r) in old {
                self.topology(t, &k, &r, false, bootstrap);
                self.update_indices(t, &k, &r, false, true);
            }
        }
        for change in b.rows {
            let t = change.table;
            let k = change.key;
            self.stats.constructor_events +=
                usize::from(self.lib.constructors.contains_key(&self.names[t]));
            let new = if change.visible {
                change.row.map(|mut values| {
                    values.truncate(self.feed.as_ref().unwrap().tables[t].keys + 1);
                    Row {
                        values,
                        stamp: change.timestamp.expect("native visible row version"),
                    }
                })
            } else {
                None
            };
            if let Some(m) = &mut self.markers {
                m.update(t, &k, new.as_ref().map(|r| r.values.as_slice()));
            }
            if !self.full_rows[t] {
                continue;
            }
            if let (Some(old), Some(new)) = (self.tables[t].rows.get(&k), new.as_ref()) {
                if old.values == new.values {
                    if old.stamp == new.stamp {
                        self.stats.unchanged_notifications += 1;
                        continue;
                    }
                    // Timestamps affect semi-naive matching, not structural coordinates.
                    self.stats.timestamp_only_changes += 1;
                    self.update_indices(t, &k, new, true, false);
                    self.tables[t]
                        .deltas
                        .entry(new.stamp)
                        .or_default()
                        .insert(k.clone());
                    self.tables[t].rows.insert(k, new.clone());
                    continue;
                }
            }
            if let Some(old) = self.tables[t].rows.get(&k).cloned() {
                self.topology(t, &k, &old, false, bootstrap);
                self.update_indices(t, &k, &old, false, true);
                self.tables[t].rows.remove(&k);
            }
            if let Some(new) = new {
                self.tables[t].rows.insert(k.clone(), new.clone());
                self.tables[t]
                    .deltas
                    .entry(new.stamp)
                    .or_default()
                    .insert(k.clone());
                self.update_indices(t, &k, &new, true, true);
                self.topology(t, &k, &new, true, bootstrap);
            }
        }
        if let Some(c) = &mut self.coordinates {
            c.commit();
        }
        if let Some(m) = &mut self.markers {
            m.commit();
        }
        let elapsed = started.elapsed().as_secs_f64();
        self.stats.maintenance_seconds += elapsed;
        if bootstrap {
            self.stats.bootstrap_maintenance_seconds += elapsed;
        }
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
        if let Some(m) = &mut self.markers {
            return m.signature(&n.0, n.1, self.coordinate_audit);
        }
        if let Some(c) = &mut self.coordinates {
            let before = c.metrics.hits;
            let sig = c.signature(&n.0, n.1, self.coordinate_audit);
            self.stats.feature_hits += c.metrics.hits - before;
            return sig;
        }
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
    factors: &[Arc<Vec<Row>>],
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
    for row in factors[depth].iter() {
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
        json!({"schema":"live-spine-complement/v1","mode":self.mode,"statistics":self.stats,"shared_conditions":self.shared_conditions,"coordinate_backend":if self.markers.is_some(){"differential-markers"}else if self.coordinates.is_some(){"commit-published"}else{"legacy-or-disabled"},"condition_indices":self.indices.len(),"coordinate_publication":self.coordinates.as_ref().map(|c|&c.metrics),"marker_publication":self.markers.as_ref().map(|m|&m.metrics),"feature_only_full_rows":self.tables.iter().enumerate().filter(|(i,_)|!self.full_rows[*i]).map(|(_,t)|t.rows.len()).sum::<usize>(),"phase_accounting":"query excludes bootstrap ingest; maintenance includes it",
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
        let maintenance_before = self.stats.maintenance_seconds;
        if self.feed.is_none() {
            self.initialize(eg, sinks)?;
        }
        let ids = sinks
            .iter()
            .enumerate()
            .filter(|(_, s)| self.queries.contains_key(&s.name))
            .map(|(i, s)| (i, s.name.clone(), eg.cell_query_watermark(s)))
            .collect::<Vec<_>>();
        // Shared condition blocks are actual (projection,key,version) requests,
        // not a row x plan reservation. Consumers retain their own binding maps.
        struct Block {
            index: usize,
            key: Value,
            model: usize,
            answer: Option<Arc<Vec<Row>>>,
            score: f32,
        }
        let mut blocks = Vec::<Block>::new();
        let mut intern = Map::default();
        let mut roots = Map::default();
        for (_, name, wm) in &ids {
            let q = &self.queries[name];
            let mut rows = vec![];
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
                let Some(binding) = bind(&q.atoms[q.plan.anchor], &row.values) else {
                    continue;
                };
                let mut links = vec![];
                for (step, index) in q.plan.steps.iter().zip(&q.indices) {
                    let value = binding[&step.slot];
                    let k = (*index, value);
                    if self.shared_conditions {
                        if let Some(id) = intern.get(&k) {
                            links.push(*id);
                            continue;
                        }
                    }
                    let id = blocks.len();
                    intern.insert(k, id);
                    links.push(id);
                    let answer = if self.shared_conditions {
                        self.stats.blocks.cache_probes += 1;
                        self.indices[*index].cached(value)
                    } else {
                        None
                    };
                    blocks.push(Block {
                        index: *index,
                        key: value,
                        model: step.model,
                        answer,
                        score: 1.0,
                    });
                }
                rows.push((row.clone(), binding, links));
            }
            roots.insert(name.clone(), rows);
        }
        let mut score_keys = BTreeSet::new();
        if self.mode == "gpu" {
            for rows in roots.values() {
                for (_, _, links) in rows {
                    if links
                        .iter()
                        .any(|i| blocks[*i].answer.as_ref().is_some_and(|a| a.is_empty()))
                    {
                        continue;
                    }
                    for i in links {
                        let block = &blocks[*i];
                        if block.answer.is_none() {
                            score_keys.insert((block.model, block.key));
                        }
                    }
                }
            }
        }
        let scores = if self.mode == "gpu" {
            self.scores_for(&score_keys.into_iter().collect::<Vec<_>>())?
        } else {
            Map::default()
        };
        for block in &mut blocks {
            block.score = scores
                .get(&(block.model, block.key))
                .copied()
                .unwrap_or(if self.mode == "reject-all" { -1.0 } else { 1.0 });
        }
        let mut supplied = vec![];
        for (id, name, wm) in ids {
            let start = Instant::now();
            let q = &self.queries[&name];
            let work = self.stats.rules.entry(name.clone()).or_default();
            work.calls += 1;
            work.anchors += self.tables[q.anchor].rows.len();
            let mut output = BTreeSet::new();
            for (root, b, links) in &roots[&name] {
                work.delta_anchors += 1;
                let mut order = (0..q.plan.steps.len()).collect::<Vec<_>>();
                let value = |i: usize| {
                    let block = &blocks[links[i]];
                    if self.mode == "gpu" {
                        if let Some(answer) = &block.answer {
                            return if answer.is_empty() {
                                f32::MIN
                            } else {
                                f32::MAX
                            };
                        }
                    }
                    block.score
                };
                let positive = order.iter().all(|i| value(*i) >= 0.5);
                work.exact_cached_rejections += usize::from(
                    self.mode == "gpu"
                        && links
                            .iter()
                            .any(|i| blocks[*i].answer.as_ref().is_some_and(|a| a.is_empty())),
                );
                if positive {
                    work.predicted_positive += 1
                } else {
                    work.predicted_negative += 1
                }
                if self.mode == "gpu" {
                    order.sort_by(|a, b| value(*a).total_cmp(&value(*b)).then(a.cmp(b)));
                }
                let mut factors = vec![Arc::new(vec![]); q.plan.steps.len()];
                let mut depth = 0;
                let mut accepted = true;
                for i in order {
                    bump(&mut work.entered, depth);
                    work.reads += 1;
                    depth += 1;
                    let block = &mut blocks[links[i]];
                    let found = if let Some(answer) = &block.answer {
                        self.stats.blocks.hits += 1;
                        self.stats.blocks.empty_hits += usize::from(answer.is_empty());
                        answer.clone()
                    } else {
                        let answer = self.indices[block.index].answer(
                            block.key,
                            self.shared_conditions,
                            &mut self.stats.blocks,
                        );
                        block.answer = Some(answer.clone());
                        answer
                    };
                    if found.is_empty() {
                        accepted = false;
                        bump(&mut work.rejected, depth);
                        break;
                    }
                    factors[i] = found;
                }
                bump(&mut work.deepest, depth);
                if !positive {
                    bump(&mut work.complement_deepest, depth)
                }
                if !accepted {
                    work.false_positive_roots += usize::from(positive);
                    continue;
                }
                work.witness_rows += factors.iter().map(|f| f.len()).sum::<usize>();
                let before = output.len();
                let mut slots = vec![None; q.slots];
                for (s, v) in b {
                    slots[*s] = Some(*v)
                }
                product(q, b, &factors, 0, &mut slots, root.stamp, wm, &mut output);
                if positive {
                    work.positive_bindings += output.len() - before
                } else {
                    work.recovered_roots += usize::from(output.len() > before);
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
        self.stats.query_seconds +=
            started.elapsed().as_secs_f64() - (self.stats.maintenance_seconds - maintenance_before);
        Ok(supplied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cell_assisted_egglog::table_live::Runtime;
    #[test]
    fn condition_blocks_version_negative_and_complete_payloads() {
        let v = Value::new_const;
        let mut i = Index {
            atom: CellQueryAtom {
                function: "P".into(),
                columns: vec![CellOperand::Slot(0), CellOperand::Slot(1)],
                sorts: vec!["T".into(), "T".into()],
            },
            slot: 0,
            table: 0,
            rows: Map::default(),
            deltas: BTreeMap::new(),
            versions: Map::default(),
            answers: Map::default(),
        };
        let mut stats = BlockStats::default();
        assert!(i.answer(v(1), true, &mut stats).is_empty());
        assert!(i.cached(v(1)).unwrap().is_empty());
        assert!(i.touch(v(1)));
        assert!(i.cached(v(1)).is_none());
        i.rows.insert(
            v(1),
            BTreeMap::from([
                (
                    vec![v(1), v(2)],
                    Row {
                        values: vec![v(1), v(2)],
                        stamp: 4,
                    },
                ),
                (
                    vec![v(1), v(3)],
                    Row {
                        values: vec![v(1), v(3)],
                        stamp: 5,
                    },
                ),
            ]),
        );
        let old = i.answer(v(1), true, &mut stats);
        assert_eq!(old.len(), 2);
        i.touch(v(1));
        i.rows.remove(&v(1));
        assert!(i.answer(v(1), true, &mut stats).is_empty());
        assert_eq!(old.len(), 2);
    }
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
