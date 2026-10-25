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
use rustc_hash::{FxHashMap as Map, FxHashSet as Set};
use egglog_numeric_id::NumericId;
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

#[path = "combined.rs"]
pub mod combined;

#[path = "resident.rs"]
mod resident;

#[path = "conditions.rs"]
mod conditions;
#[path = "coordinates.rs"]
mod coordinates;
#[path = "markers.rs"]
mod markers;
#[path = "summaries.rs"]
mod summaries;

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
    #[serde(default)]
    pub excluded_tokens: Vec<usize>,
    #[serde(default)]
    pub reference_model: Option<usize>,
    #[serde(default)]
    pub weights: Vec<u16>,
    #[serde(default)]
    pub bias: f32,
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
    #[serde(default)]
    summary_markers: Vec<summaries::Definition>,
    vocabulary: Vec<Token>,
    constructors: BTreeMap<String, (Vec<String>, String)>,
}
#[derive(Clone)]
struct Row {
    values: Vec<Value>,
    stamp: u32,
}
#[cfg(feature = "shared-rows")]
type StoredRow = Arc<Row>;
#[cfg(not(feature = "shared-rows"))]
type StoredRow = Row;
fn store_row(row: Row) -> StoredRow {
    #[cfg(feature = "shared-rows")]
    {
        Arc::new(row)
    }
    #[cfg(not(feature = "shared-rows"))]
    {
        row
    }
}
type Key = Vec<Value>;
type Node = (String, Value);
type Ref = (usize, Key);
#[derive(Default)]
struct Table {
    rows: Map<Key, StoredRow>,
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

struct Bucket {
    key: Value,
    version: u64,
    pending_live: Option<bool>,
    rows: BTreeMap<Key, StoredRow>,
    answer: Option<(u64, Arc<Vec<StoredRow>>)>,
}
struct Index {
    projection: conditions::Projection,
    slot: usize,
    table: usize,
    directory: Map<Value, usize>,
    buckets: Vec<Bucket>,
    presence: Vec<u64>,
    deltas: BTreeMap<u32, BTreeSet<Value>>,
}
impl Index {
    fn intern(&mut self, key: Value) -> usize {
        if let Some(h) = self.directory.get(&key) {
            return *h;
        }
        let h = self.buckets.len();
        self.directory.insert(key, h);
        if h / 64 == self.presence.len() {
            self.presence.push(0);
        }
        self.buckets.push(Bucket {
            key,
            version: 0,
            pending_live: None,
            rows: BTreeMap::new(),
            answer: None,
        });
        h
    }
    fn occupied(&self, h: usize) -> bool {
        self.presence[h / 64] & (1u64 << (h % 64)) != 0
    }
    fn refresh_presence(&mut self, h: usize) {
        let mask = 1u64 << (h % 64);
        if self.buckets[h].rows.is_empty() {
            self.presence[h / 64] &= !mask;
        } else {
            self.presence[h / 64] |= mask;
        }
    }
    fn cached(&self, h: usize) -> Option<Arc<Vec<StoredRow>>> {
        let b = &self.buckets[h];
        b.answer.as_ref().map(|(version, rows)| {
            debug_assert_eq!(*version, b.version);
            rows.clone()
        })
    }
    fn touch(&mut self, h: usize) -> bool {
        let b = &mut self.buckets[h];
        b.version += 1;
        b.answer.take().is_some()
    }
    fn answer(&mut self, h: usize, shared: bool, stats: &mut BlockStats) -> Arc<Vec<StoredRow>> {
        let b = &mut self.buckets[h];
        stats.reads += 1;
        let answer = Arc::new(b.rows.values().cloned().collect::<Vec<_>>());
        stats.witness_rows_materialized += answer.len();
        if shared {
            b.answer = Some((b.version, answer.clone()));
        }
        answer
    }
}
struct Attachment {
    handles: Vec<usize>,
    ready: bool,
    single: bool,
}
struct Query {
    plan: Plan,
    atoms: Vec<CellQueryAtom>,
    projections: Vec<conditions::Projection>,
    anchor: usize,
    indices: Vec<usize>,
    slots: usize,
    ports: Vec<Map<Value, BTreeSet<Key>>>,
    attachments: Map<Key, Attachment>,
    dirty_roots: Set<Key>,
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
    exact_group_rejected: usize,
}
#[derive(Default, Serialize)]
struct Stats {
    blocks: BlockStats,
    prediction_audit: BTreeMap<String, Confusion>,
    matrix: MatrixStats,
    resident: ResidentStats,
    groups: GroupStats,
    readiness: ReadyStats,
    index_dispatches: usize,
    anchor_dispatches: usize,
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
    constant_score_requests: usize,
    ordering_roots: usize,
    single_condition_roots: usize,
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
#[derive(Default, Serialize)]
struct Confusion {
    tp: usize,
    fp: usize,
    tn: usize,
    fn_: usize,
}
impl Confusion {
    fn record(&mut self, predicted: bool, truth: bool) {
        match (predicted, truth) {
            (true, true) => self.tp += 1,
            (true, false) => self.fp += 1,
            (false, true) => self.fn_ += 1,
            (false, false) => self.tn += 1,
        }
    }
}
#[derive(Default, Serialize)]
struct GroupStats {
    truth_checks: usize,
    truth_index_reads: usize,
    rejected_candidates: usize,
    shared_steps: usize,
    new_steps: usize,
    references: usize,
    no_payload_rejections: usize,
}
#[derive(Default, Serialize)]
struct ReadyStats {
    condition_updates: usize,
    existence_flips: usize,
    unchanged_existence: usize,
    root_rechecks: usize,
    root_truth_changes: usize,
    filtered_candidates: usize,
    accepted_candidates: usize,
    oracle_checks: usize,
    direct_condition_candidates: usize,
    stored_conjunction_candidates: usize,
}
fn bump(v: &mut Vec<usize>, i: usize) {
    if v.len() <= i {
        v.resize(i + 1, 0)
    }
    v[i] += 1;
}

#[derive(Default, Serialize)]
struct MatrixStats {
    requests: u64,
    coordinate_rows: u64,
    requested_cells: u64,
    computed_cells: u64,
    padded_cells: u64,
    blocks: u64,
    multi_column_cells: u64,
    max_columns: u64,
    max_rows: u64,
    single_column_padded_cells: u64,
}
impl MatrixStats {
    fn add(&mut self, v: &[u64; 10]) {
        self.requests += v[0];
        self.coordinate_rows += v[1];
        self.requested_cells += v[2];
        self.computed_cells += v[3];
        self.padded_cells += v[4];
        self.blocks += v[5];
        self.multi_column_cells += v[6];
        self.max_columns = self.max_columns.max(v[7]);
        self.max_rows = self.max_rows.max(v[8]);
        self.single_column_padded_cells += v[9];
    }
}
#[derive(Default, Serialize)]
struct ResidentStats {
    contexts:u64, patches:u64, feature_assignments:u64, cache_hits:u64,
    prefetched_heads_used:u64, computed_heads:u64, unused_heads_current:u64,
    unused_heads_discarded:u64, context_buffer_bytes:u64,
}
impl ResidentStats {
    fn add(&mut self,v:&[u64;9]) {
        self.contexts=v[0];self.patches+=v[1];self.feature_assignments+=v[2];
        self.cache_hits+=v[3];self.prefetched_heads_used+=v[4];self.computed_heads+=v[5];
        self.unused_heads_current=v[6];self.unused_heads_discarded+=v[7];
        self.context_buffer_bytes=v[8];
    }
}
fn packet(requests: &[(usize, Vec<u64>)]) -> Vec<u8> {
    let mut ids = Map::default();
    let mut rows = Vec::new();
    let mut route = Vec::new();
    for (model, ts) in requests {
        let id = *ids.entry(ts).or_insert_with(|| {
            let n = rows.len();
            rows.push(ts);
            n
        });
        route.push((id, *model));
    }
    let mut b = Vec::new();
    b.extend((requests.len() as u32).to_le_bytes());
    b.extend((rows.len() as u32).to_le_bytes());
    for ts in rows {
        b.extend((ts.len() as u32).to_le_bytes());
        for t in ts {
            b.extend(t.to_le_bytes());
        }
    }
    for (row, model) in route {
        b.extend((row as u32).to_le_bytes());
        b.extend((model as u32).to_le_bytes());
    }
    b
}
pub struct Device {
    child: Child,
    resident: Option<resident::Client>,
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
        let resident=if &ready==b"RDC1" {
            Some(resident::Client::new(&serde_json::from_slice::<Library>(&std::fs::read(lib)?)?))
        } else if &ready==b"RDY2" {None} else {return Err("bad predictor handshake".into());};
        Ok(Self { child, resident })
    }
    fn predict(
        &mut self,
        requests: &[(usize, Vec<u64>)],
    ) -> Result<(Vec<f32>, f64, usize, usize, [u64; 10], [u64;9])> {
        if self.resident.is_some(){return Err("resident scores need typed context identities".into());}
        self.exchange(packet(requests), requests.len())
    }
    fn predict_contexts(&mut self, requests:&[(usize,u32,Vec<usize>)]) -> Result<(Vec<f32>,f64,usize,usize,[u64;10],[u64;9])> {
        let b=self.resident.as_mut().ok_or("no resident context store")?.packet(requests)?;
        self.exchange(b,requests.len())
    }
    fn exchange(&mut self, b:Vec<u8>, requested:usize) -> Result<(Vec<f32>,f64,usize,usize,[u64;10],[u64;9])> {
        let input = self.child.stdin.as_mut().unwrap();
        input.write_all(&(b.len() as u32).to_le_bytes())?;
        input.write_all(&b)?;
        input.flush()?;
        let out = self.child.stdout.as_mut().unwrap();
        let mut head = [0; 12];
        out.read_exact(&mut head)?;
        let n = u32::from_le_bytes(head[..4].try_into()?) as usize;
        if n != requested {
            return Err("GPU response arity changed".into());
        }
        let dt = f64::from_le_bytes(head[4..].try_into()?);
        let mut layout = [0u8; 80];
        out.read_exact(&mut layout)?;
        let metrics = std::array::from_fn(|i| {
            u64::from_le_bytes(layout[8 * i..8 * i + 8].try_into().unwrap())
        });
        let mut resident=[0u64;9];
        if self.resident.is_some(){
            let mut raw=[0u8;72];out.read_exact(&mut raw)?;
            for i in 0..9{resident[i]=u64::from_le_bytes(raw[i*8..i*8+8].try_into()?);}
        }
        let mut bytes = vec![0; 4 * n];
        out.read_exact(&mut bytes)?;
        let scores = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        if scores.iter().any(|s| !s.is_finite()) {
            return Err("nonfinite GPU score".into());
        }
        Ok((scores, dt, b.len() + 4, 92 + bytes.len() + if self.resident.is_some(){72}else{0}, metrics, resident))
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
    index_users: Vec<Vec<usize>>,
    anchor_users: Vec<Vec<String>>,
    group_conditions: bool,
    ready_conditions: bool,
    condition_users: Vec<Vec<(String, usize)>>,
    pending_existence: Vec<(usize, usize)>,
    coordinates: Option<coordinates::Coordinates>,
    markers: Option<markers::Markers>,
    summaries: Option<summaries::Summaries>,
    full_rows: Vec<bool>,
    marker_coordinates: bool,
    event_coordinates: bool,
    shared_conditions: bool,
    coordinate_audit: bool,
    fixed_scores: Vec<Option<f32>>,
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
            fixed_scores: lib
                .models
                .iter()
                .map(|m| {
                    (m.weights.len() == 256 && m.weights.iter().all(|w| w & 0x7fff == 0))
                        .then_some(m.bias)
                })
                .collect(),
            lib,
            device,
            feed: None,
            names: vec![],
            ids: Map::default(),
            tables: vec![],
            queries: Map::default(),
            indices: vec![],
            index_users: vec![],
            anchor_users: vec![],
            group_conditions: matches!(
                std::env::var("STRUCTURAL_CONDITION_EXECUTION").as_deref(),
                Ok("groups" | "ready")
            ),
            ready_conditions: std::env::var("STRUCTURAL_CONDITION_EXECUTION").as_deref()
                == Ok("ready"),
            condition_users: vec![],
            pending_existence: vec![],
            coordinates: None,
            markers: None,
            summaries: None,
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
                    projections: s.atoms.iter().map(conditions::Projection::new).collect(),
                    anchor: 0,
                    indices: vec![],
                    slots: s.slot_count(),
                    ports: (0..p.steps.len()).map(|_| Map::default()).collect(),
                    attachments: Map::default(),
                    dirty_roots: Set::default(),
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
        if self.mode == "gpu" && !self.group_conditions {
            for v in &self.lib.vocabulary {
                if v.token[0] == "summary" {
                    continue;
                }
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
        if self.mode == "gpu" && !self.group_conditions {
            for d in &self.lib.summary_markers {
                for a in &d.atoms {
                    names.insert(a["table"].as_str().ok_or("summary table")?.into());
                }
            }
            // Target relations must never enter their own predictive feature.
            for m in &self.lib.models {
                for d in &self.lib.summary_markers {
                    if d.dependencies.contains(&m.channel) {
                        for (i, t) in self.lib.vocabulary.iter().enumerate() {
                            if t.token[0] == "summary"
                                && t.token[1] == d.identity
                                && !m.excluded_tokens.contains(&i)
                            {
                                return Err("target-dependent summary not masked".into());
                            }
                        }
                    }
                }
            }
        }
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
                    projection: conditions::Projection::new(&a),
                    table: self.ids[&a.function],
                    slot: step.slot,
                    directory: Map::default(),
                    buckets: vec![],
                    presence: vec![],
                    deltas: BTreeMap::new(),
                });
            }
        }
        self.index_users = vec![vec![]; self.names.len()];
        for (i, idx) in self.indices.iter().enumerate() {
            self.index_users[idx.table].push(i);
        }
        self.anchor_users = vec![vec![]; self.names.len()];
        self.condition_users = vec![vec![]; self.indices.len()];
        for (name, q) in &self.queries {
            self.anchor_users[q.anchor].push(name.clone());
            if q.indices.len() > 1 {
                for (port, &idx) in q.indices.iter().enumerate() {
                    self.condition_users[idx].push((name.clone(), port));
                }
            }
        }
        self.full_rows = (0..self.names.len())
            .map(|t| {
                !self.marker_coordinates
                    || self.queries.values().any(|q| q.anchor == t)
                    || self.indices.iter().any(|i| i.table == t)
            })
            .collect();
        let mut base_lib = self.lib.clone();
        let first_summary = base_lib
            .vocabulary
            .iter()
            .position(|t| t.token[0] == "summary")
            .unwrap_or(base_lib.vocabulary.len());
        if base_lib.vocabulary[first_summary..]
            .iter()
            .any(|t| t.token[0] != "summary")
        {
            return Err("summary features must follow base vocabulary".into());
        }
        base_lib.vocabulary.truncate(first_summary);
        if self.mode == "gpu" && !self.group_conditions && !self.lib.summary_markers.is_empty() {
            self.summaries = Some(summaries::Summaries::new(&self.names, &self.lib, eg)?);
        }
        if self.marker_coordinates && self.mode == "gpu" && !self.group_conditions {
            self.markers = Some(markers::Markers::new(
                &self.names,
                &base_lib,
                self.coordinate_audit,
            ));
        } else if self.event_coordinates && self.mode == "gpu" && !self.group_conditions {
            self.coordinates = Some(coordinates::Coordinates::new(&self.names, &base_lib));
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
    fn update_indices(&mut self, t: usize, k: &Key, r: &StoredRow, add: bool, structure: bool) {
        for &id in &self.index_users[t] {
            self.stats.index_dispatches += 1;
            let idx = &mut self.indices[id];
            if idx.projection.matches(&r.values) {
                let v = r.values[idx.projection.column(idx.slot)];
                let h = idx.intern(v);
                if self.ready_conditions && idx.buckets[h].pending_live.is_none() {
                    idx.buckets[h].pending_live = Some(!idx.buckets[h].rows.is_empty());
                    self.pending_existence.push((id, h));
                }
                self.stats.blocks.invalidations += usize::from(idx.touch(h));
                if add {
                    idx.buckets[h].rows.insert(k.clone(), r.clone());
                    idx.deltas.entry(r.stamp).or_default().insert(v);
                } else {
                    idx.buckets[h].rows.remove(k);
                }
                idx.refresh_presence(h);
            }
        }
        if !structure {
            return;
        }
        for name in &self.anchor_users[t] {
            self.stats.anchor_dispatches += 1;
            let q = self.queries.get_mut(name).unwrap();
            if q.projections[q.plan.anchor].matches(&r.values) {
                let mut attached = Vec::with_capacity(q.plan.steps.len());
                for ((s, port), &idx) in q.plan.steps.iter().zip(&mut q.ports).zip(&q.indices) {
                    let v = r.values[q.projections[q.plan.anchor].column(s.slot)];
                    if add {
                        attached.push(self.indices[idx].intern(v));
                        port.entry(v).or_default().insert(k.clone());
                    } else if let Some(keys) = port.get_mut(&v) {
                        keys.remove(k);
                        if keys.is_empty() {
                            port.remove(&v);
                        }
                    }
                }
                if add {
                    let single = !attached.is_empty()
                        && q.indices
                            .iter()
                            .zip(&attached)
                            .all(|(i, h)| *i == q.indices[0] && *h == attached[0]);
                    q.attachments.insert(
                        k.clone(),
                        Attachment {
                            handles: attached,
                            ready: false,
                            single,
                        },
                    );
                    if self.ready_conditions && !single {
                        q.dirty_roots.insert(k.clone());
                    }
                } else {
                    q.attachments.remove(k);
                    q.dirty_roots.remove(k);
                }
            }
        }
    }
    fn finish_readiness(&mut self) {
        for (idx, h) in self.pending_existence.drain(..) {
            let b = &mut self.indices[idx].buckets[h];
            let old = b.pending_live.take().unwrap();
            self.stats.readiness.condition_updates += 1;
            if old == !b.rows.is_empty() {
                self.stats.readiness.unchanged_existence += 1;
                continue;
            }
            self.stats.readiness.existence_flips += 1;
            for (name, port) in &self.condition_users[idx] {
                let q = self.queries.get_mut(name).unwrap();
                if let Some(keys) = q.ports[*port].get(&b.key) {
                    q.dirty_roots
                        .extend(keys.iter().filter(|k| !q.attachments[*k].single).cloned());
                }
            }
        }
        for q in self.queries.values_mut() {
            for key in q.dirty_roots.drain() {
                let Some(a) = q.attachments.get_mut(&key) else {
                    continue;
                };
                if a.single {
                    continue;
                }
                let ready = q
                    .indices
                    .iter()
                    .zip(&a.handles)
                    .all(|(&i, &h)| self.indices[i].occupied(h));
                self.stats.readiness.root_rechecks += 1;
                self.stats.readiness.root_truth_changes += usize::from(a.ready != ready);
                a.ready = ready;
            }
        }
    }
    fn topology(&mut self, t: usize, k: &Key, r: &StoredRow, add: bool, bootstrap: bool) {
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
            if let Some(s) = &mut self.summaries {
                s.clear(t);
            }
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
                    store_row(Row {
                        values,
                        stamp: change.timestamp.expect("native visible row version"),
                    })
                })
            } else {
                None
            };
            if let Some(s) = &mut self.summaries {
                s.update(t, &k, new.as_ref().map(|r| r.values.as_slice()));
            }
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
        if self.ready_conditions {
            self.finish_readiness();
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
        let mut result = self.base_signature(n.clone());
        if let Some(s) = &mut self.summaries {
            result.extend(s.signature(&n));
        }
        result
    }
    fn base_signature(&mut self, n: Node) -> Vec<usize> {
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
            .filter(|i| {
                self.lib.vocabulary[*i].token[0] != "summary"
                    && self.has_token(&n, &self.lib.vocabulary[*i].token)
            })
            .collect::<Vec<_>>();
        self.tokens.insert(n, s.clone());
        s
    }
    fn scores_for(&mut self, keys: &[(usize, Value)]) -> Result<Map<(usize, Value), f32>> {
        let mut routing = vec![];
        let mut pending: BTreeMap<(usize, Vec<usize>), (usize,Value,Vec<usize>)> = BTreeMap::new();
        let mut coordinate_cache = Map::default();
        for &(m, v) in keys {
            // Exact simplification of this predictor, not a proof of native
            // predicate truth. CPU verification/completion remains unchanged.
            if let Some(score) = self.fixed_scores[m] {
                self.stats.score_requests += 1;
                self.stats.constant_score_requests += 1;
                let k = (m, vec![]);
                self.scores.insert(k.clone(), score);
                routing.push(((m, v), k));
                continue;
            }
            let node = (self.lib.models[m].sort.clone(), v);
            let mut sig = if let Some(sig) = coordinate_cache.get(&node) {
                Vec::clone(sig)
            } else {
                let sig = self.signature(node.clone());
                coordinate_cache.insert(node, sig.clone());
                sig
            };
            let raw_sig=sig.clone();
            sig.retain(|i| !self.lib.models[m].excluded_tokens.contains(i));
            let k = (m, sig);
            self.stats.score_requests += 1;
            if self.scores.contains_key(&k) {
                self.stats.score_hits += 1;
            } else {
                let next = pending.len();
                pending.entry(k.clone()).or_insert((next,v,raw_sig));
            }
            routing.push(((m, v), k));
        }
        if !pending.is_empty() {
            let mut ordered = pending.into_iter().collect::<Vec<_>>();
            ordered.sort_by_key(|(_, (i,_,_))| *i);
            let req = ordered
                .iter()
                .map(|((m, s), _)| {
                    let mut hashes = vec![self.lib.models[*m].sort_hash];
                    hashes.extend(s.iter().map(|i| self.lib.vocabulary[*i].hash));
                    (*m, hashes)
                })
                .collect::<Vec<_>>();
            let started = Instant::now();
            let device=self.device.as_mut().ok_or("missing GPU predictor")?;
            let (answers, dt, up, down, layout, resident) = if device.resident.is_some() {
                let contexts=ordered.iter().map(|((m,_),(_,v,raw))|(*m,v.rep(),raw.clone())).collect::<Vec<_>>();
                device.predict_contexts(&contexts)?
            }else{device.predict(&req)?};
            self.stats.device_roundtrip_seconds += started.elapsed().as_secs_f64();
            self.stats.matrix.add(&layout);
            self.stats.resident.add(&resident);
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
#[cfg(test)]
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
    factors: &[Arc<Vec<StoredRow>>],
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
        let mut added = vec![];
        let mut ok = true;
        // Index insertion already checked constants and repeated slots.
        // Cross-atom bindings are still checked here on the full witness row.
        for &(s, col) in &q.projections[step.atom].bindings {
            let v = row.values[col];
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
        if self.coordinate_audit {
            if let Some(s) = &self.summaries {
                s.audit();
            }
        }
        json!({"row_storage":if cfg!(feature="shared-rows") {"shared"} else {"owned"},"condition_handles":{"presence_bytes":self.indices.iter().map(|i|i.presence.len()*8).sum::<usize>(),"presence_capacity_bytes":self.indices.iter().map(|i|i.presence.capacity()*8).sum::<usize>(),"total":self.indices.iter().map(|i|i.buckets.len()).sum::<usize>(),"nonempty":self.indices.iter().map(|i|i.buckets.iter().filter(|b|!b.rows.is_empty()).count()).sum::<usize>(),"anchor_connections":self.queries.values().map(|q|q.attachments.values().map(|a|a.handles.len()).sum::<usize>()).sum::<usize>()},"condition_execution":if self.ready_conditions {"ready"}else if self.group_conditions {"groups"} else {"roots"},"summary_publication":self.summaries.as_ref().map(|s|s.report()),"schema":"live-spine-complement/v1",
        "predictor_loaded":self.device.is_some(),
        "predictor_backend":self.device.as_ref().map(|d|if d.resident.is_some(){"resident-response"}else{"packet-coordinates"}),
        "predictor_executed":self.stats.matrix.blocks>0,"mode":self.mode,"statistics":self.stats,"shared_conditions":self.shared_conditions,"coordinate_backend":if self.markers.is_some(){"differential-markers"}else if self.coordinates.is_some(){"commit-published"}else{"legacy-or-disabled"},"condition_indices":self.indices.len(),"coordinate_publication":self.coordinates.as_ref().map(|c|&c.metrics),"marker_publication":self.markers.as_ref().map(|m|&m.metrics),"feature_only_full_rows":self.tables.iter().enumerate().filter(|(i,_)|!self.full_rows[*i]).map(|(_,t)|t.rows.len()).sum::<usize>(),"phase_accounting":"query excludes bootstrap ingest; maintenance includes it",
        "scope":if self.group_conditions {"Exact current projected-condition groups; payloads only for accepted candidates. Every original binding and timestamp frontier retained. No Cell proof or sat-skip credit."}else{"GPU scores only reorder exact CPU checks. Every eligible anchor retained; complete native bindings and timestamp frontier. No Cell proof or sat-skip credit."}})
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
            handle: usize,
            key: Value,
            model: usize,
            answer: Option<Arc<Vec<StoredRow>>>,
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
                let Some(attached) = q.attachments.get(&key) else {
                    continue;
                };
                if self.ready_conditions {
                    let ready = if attached.single {
                        self.stats.readiness.direct_condition_candidates += 1;
                        self.indices[q.indices[0]].occupied(attached.handles[0])
                    } else {
                        self.stats.readiness.stored_conjunction_candidates += 1;
                        attached.ready
                    };
                    if self.coordinate_audit {
                        let truth = q
                            .indices
                            .iter()
                            .zip(&attached.handles)
                            .all(|(&i, &h)| !self.indices[i].buckets[h].rows.is_empty());
                        assert_eq!(ready, truth, "stale event-published readiness");
                        self.stats.readiness.oracle_checks += 1;
                    }
                    if !ready {
                        let w = self.stats.rules.entry(name.clone()).or_default();
                        w.delta_anchors += 1;
                        w.exact_group_rejected += 1;
                        bump(&mut w.deepest, 0);
                        bump(&mut w.rejected, 0);
                        self.stats.groups.no_payload_rejections += 1;
                        self.stats.readiness.filtered_candidates += 1;
                        continue;
                    }
                    self.stats.readiness.accepted_candidates += 1;
                }
                let Some(row) = self.tables[q.anchor].rows.get(&key) else {
                    continue;
                };
                if !q.projections[q.plan.anchor].matches(&row.values) {
                    continue;
                }
                let mut links = vec![];
                for ((step, index), &handle) in
                    q.plan.steps.iter().zip(&q.indices).zip(&attached.handles)
                {
                    let value = self.indices[*index].buckets[handle].key;
                    debug_assert_eq!(
                        value,
                        row.values[q.projections[q.plan.anchor].column(step.slot)]
                    );
                    let k = (*index, handle);
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
                        self.indices[*index].cached(handle)
                    } else {
                        None
                    };
                    blocks.push(Block {
                        index: *index,
                        handle,
                        key: value,
                        model: step.model,
                        answer,
                        score: 1.0,
                    });
                }
                rows.push((row.clone(), links));
            }
            roots.insert(name.clone(), rows);
        }
        let mut score_keys = BTreeSet::new();
        if self.mode == "gpu" && !self.group_conditions {
            for rows in roots.values() {
                for (_, links) in rows {
                    if links
                        .iter()
                        .any(|i| blocks[*i].answer.as_ref().is_some_and(|a| a.is_empty()))
                    {
                        continue;
                    }
                    // With at most one unknown check, a score cannot alter
                    // exact probe order. Do not inflate matrix demand with it.
                    let first = links.iter().find(|i| blocks[**i].answer.is_none());
                    let multiple = links
                        .iter()
                        .any(|i| blocks[*i].answer.is_none() && Some(i) != first);
                    if !multiple {
                        self.stats.single_condition_roots += 1;
                        continue;
                    }
                    self.stats.ordering_roots += 1;
                    for i in links {
                        let block = &blocks[*i];
                        if block.answer.is_none() {
                            score_keys.insert((block.model, block.key));
                        }
                    }
                }
            }
        }
        let primary_keys = score_keys.clone();
        if self.coordinate_audit {
            for &(m, v) in &primary_keys {
                if let Some(r) = self.lib.models[m].reference_model {
                    score_keys.insert((r, v));
                }
            }
        }
        let scores = if self.mode == "gpu" && !self.group_conditions {
            self.scores_for(&score_keys.into_iter().collect::<Vec<_>>())?
        } else {
            Map::default()
        };
        if self.coordinate_audit && self.mode == "gpu" {
            // Diagnostic only: compare both models on precisely the same
            // unknown (model,key) requests at the same native pre-Run state.
            for &(m, v) in &primary_keys {
                let block = blocks.iter().find(|b| b.model == m && b.key == v).unwrap();
                let truth = !self.indices[block.index].buckets[block.handle]
                    .rows
                    .is_empty();
                let has = self.summaries.as_mut().is_some_and(|s| {
                    s.signature(&(self.lib.models[m].sort.clone(), v))
                        .iter()
                        .any(|i| !self.lib.models[m].excluded_tokens.contains(i))
                });
                for (label, mid) in [
                    ("candidate", Some(m)),
                    ("reference", self.lib.models[m].reference_model),
                ] {
                    if let Some(mid) = mid {
                        let pred = scores[&(mid, v)] >= 0.5;
                        for key in [
                            format!("{label}/all"),
                            format!(
                                "{label}/{}",
                                if has {
                                    "with-summary"
                                } else {
                                    "without-summary"
                                }
                            ),
                            format!("{label}/channel/{}", self.lib.models[m].channel),
                        ] {
                            self.stats
                                .prediction_audit
                                .entry(key)
                                .or_default()
                                .record(pred, truth);
                        }
                    }
                }
            }
        }
        for block in &mut blocks {
            block.score = scores
                .get(&(block.model, block.key))
                .copied()
                .unwrap_or(if self.mode == "reject-all" { -1.0 } else { 1.0 });
        }
        let truth = if self.group_conditions && !self.ready_conditions {
            self.stats.groups.truth_checks += blocks.len();
            blocks
                .iter()
                .map(|block| {
                    if let Some(answer) = &block.answer {
                        !answer.is_empty()
                    } else {
                        self.stats.groups.truth_index_reads += 1;
                        self.indices[block.index].occupied(block.handle)
                    }
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let mut groups = conditions::Groups::new();
        let mut supplied = vec![];
        for (id, name, wm) in ids {
            let start = Instant::now();
            let q = &self.queries[&name];
            let work = self.stats.rules.entry(name.clone()).or_default();
            work.calls += 1;
            work.anchors += self.tables[q.anchor].rows.len();
            let mut output = BTreeSet::new();
            for (root, links) in &roots[&name] {
                work.delta_anchors += 1;
                if self.group_conditions {
                    if !self.ready_conditions && !groups.accepts(links, &truth) {
                        work.exact_group_rejected += 1;
                        self.stats.groups.no_payload_rejections += 1;
                        // A group rejection does no per-candidate witness read.
                        bump(&mut work.deepest, 0);
                        bump(&mut work.rejected, 0);
                        continue;
                    }
                    let mut factors = Vec::with_capacity(links.len());
                    for &bid in links {
                        let block = &mut blocks[bid];
                        if block.answer.is_none() {
                            block.answer = Some(self.indices[block.index].answer(
                                block.handle,
                                self.shared_conditions,
                                &mut self.stats.blocks,
                            ));
                        }
                        factors.push(block.answer.as_ref().unwrap().clone());
                    }
                    let n = factors.len();
                    work.reads += n;
                    bump(&mut work.deepest, n);
                    for depth in 0..n {
                        bump(&mut work.entered, depth);
                    }
                    work.witness_rows += factors.iter().map(|f| f.len()).sum::<usize>();
                    let mut slots = vec![None; q.slots];
                    for &(s, col) in &q.projections[q.plan.anchor].bindings {
                        slots[s] = Some(root.values[col]);
                    }
                    let before = output.len();
                    product(q, &factors, 0, &mut slots, root.stamp, wm, &mut output);
                    work.positive_bindings += output.len() - before;
                    continue;
                }
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
                            block.handle,
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
                for &(s, col) in &q.projections[q.plan.anchor].bindings {
                    slots[s] = Some(root.values[col]);
                }
                product(q, &factors, 0, &mut slots, root.stamp, wm, &mut output);
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
        self.stats.groups.rejected_candidates += groups.stats.rejected_candidates;
        self.stats.groups.references += groups.stats.references;
        self.stats.groups.new_steps += groups.stats.new_steps;
        self.stats.groups.shared_steps += groups.stats.reused_steps;
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
    fn row_storage_keeps_values_and_uses_selected_ownership() {
        let a = store_row(Row {
            values: vec![Value::new_const(9)],
            stamp: 7,
        });
        let b = a.clone();
        assert_eq!(a.values, b.values);
        assert_eq!(a.stamp, b.stamp);
        assert_eq!(
            a.values.as_ptr() == b.values.as_ptr(),
            cfg!(feature = "shared-rows")
        );
    }
    #[test]
    fn constant_model_needs_neither_coordinates_nor_gpu() {
        let lib:Library=serde_json::from_value(json!({"plans":[],"models":[{"sort":"T","channel":"P","sort_hash":0,"weights":vec![0u16;256],"bias":0.625}],"vocabulary":[],"constructors":{}})).unwrap();
        let mut h = Hook::new(lib, None, "gpu");
        let scores = h
            .scores_for(&[(0, Value::new_const(1)), (0, Value::new_const(2))])
            .unwrap();
        assert_eq!(scores.len(), 2);
        assert!(scores.values().all(|x| *x == 0.625));
        assert_eq!(h.stats.feature_requests, 0);
        assert_eq!(h.stats.constant_score_requests, 2);
        assert_eq!(h.stats.gpu_rows, 0);
    }
    #[test]
    fn packet_shares_coordinates_without_merging_request_identity() {
        let b = packet(&[
            (2, vec![5, 7]),
            (3, vec![5, 7]),
            (2, vec![5, 7]),
            (2, vec![7, 5]),
        ]);
        let word = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        assert_eq!((word(0), word(4)), (4, 2));
        assert_eq!((word(8), word(28)), (2, 2));
        let route = (48..b.len()).step_by(4).map(word).collect::<Vec<_>>();
        assert_eq!(route, vec![0, 2, 0, 3, 0, 2, 1, 2]);
    }
    #[test]
    fn condition_blocks_version_negative_and_complete_payloads() {
        let v = Value::new_const;
        let test_atom = CellQueryAtom {
            function: "P".into(),
            columns: vec![CellOperand::Slot(0), CellOperand::Slot(1)],
            sorts: vec!["T".into(), "T".into()],
        };
        let mut i = Index {
            projection: conditions::Projection::new(&test_atom),
            slot: 0,
            table: 0,
            directory: Map::default(),
            buckets: vec![],
            presence: vec![],
            deltas: BTreeMap::new(),
        };
        let mut stats = BlockStats::default();
        let h = i.intern(v(1));
        assert!(i.answer(h, true, &mut stats).is_empty());
        assert!(i.cached(h).unwrap().is_empty());
        assert!(i.touch(h));
        assert!(i.cached(h).is_none());
        for n in [2, 3] {
            i.buckets[h].rows.insert(
                vec![v(1), v(n)],
                store_row(Row {
                    values: vec![v(1), v(n)],
                    stamp: n + 2,
                }),
            );
        }
        i.refresh_presence(h);
        assert!(i.occupied(h));
        let old = i.answer(h, true, &mut stats);
        assert_eq!(old.len(), 2);
        i.buckets[h].rows.remove(&vec![v(1), v(2)]);
        i.refresh_presence(h);
        assert!(i.occupied(h));
        i.touch(h);
        i.buckets[h].rows.clear();
        i.refresh_presence(h);
        assert!(!i.occupied(h));
        assert!(i.answer(h, true, &mut stats).is_empty());
        assert_eq!(old.len(), 2);
        assert_eq!(i.intern(v(1)), h);
        let other = i.intern(v(99));
        i.answer(other, true, &mut stats);
        i.touch(h);
        assert!(
            i.cached(other).is_some(),
            "unrelated condition cache must survive"
        );
    }

    fn fixture(function: bool, alias: bool) -> Result<(Runtime, Vec<egglog::ast::Command>)> {
        fixture_mode(function, alias, false)
    }
    fn fixture_mode(
        function: bool,
        alias: bool,
        grouped: bool,
    ) -> Result<(Runtime, Vec<egglog::ast::Command>)> {
        fixture_ready(function, alias, grouped, false)
    }
    fn fixture_ready(
        function: bool,
        alias: bool,
        grouped: bool,
        ready: bool,
    ) -> Result<(Runtime, Vec<egglog::ast::Command>)> {
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
                excluded_tokens: vec![],
                reference_model: None,
                weights: vec![],
                bias: 0.0,
            });
        }
        let atoms=s.atoms.iter().map(|a|json!({"table":a.function,"sorts":a.sorts,"columns":a.columns.iter().map(|c|match c{CellOperand::Slot(s)=>json!({"Slot":s}),CellOperand::Constant{text,..}=>json!({"Literal":text})}).collect::<Vec<_>>()})).collect::<Vec<_>>();
        let lib = Library {
            summary_markers: vec![],
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
        let mut handler = Hook::new(lib, None, "reject-all");
        handler.group_conditions = grouped;
        handler.ready_conditions = ready;
        let mut r = Runtime::new_full_assisted(
            eg,
            &cmds,
            None,
            false,
            Some(&selected),
            Some(Box::new(handler)),
        )?;
        r.enable_cell_query_audit()?;
        Ok((r, cmds))
    }
    #[test]
    fn readiness_tracks_false_true_union_subsume_and_all_witnesses() -> Result<()> {
        EGraph::set_num_threads(1);
        for alias in [false, true] {
            let (mut r, c) = fixture_ready(false, alias, true, true)?;
            r.input("(Root (N 1) (N 2)) (P (N 1) (N 3)) (P (N 1) (N 4))")?;
            r.run(&c)?;
            r.input("(Q (N 2) (N 5)) (Q (N 2) (N 6))")?;
            r.run(&c)?;
            r.input("(union (N 1) (N 2))")?;
            r.run(&c)?;
            r.input("(delete (P (N 1) (N 3))) (delete (P (N 1) (N 4)))")?;
            r.run(&c)?;
            r.input("(P (N 1) (N 7))")?;
            r.run(&c)?;
            r.input("(subsume (Root (N 1) (N 2))) (Q (N 2) (N 8))")?;
            r.run(&c)?;
            r.run(&c)?;
            let stats = &r.report()["saturation_diagnostics"]["statistics"];
            assert!(stats["readiness"]["existence_flips"].as_u64().unwrap() > 0);
            assert_eq!(stats["groups"]["truth_checks"], 0);
            assert_eq!(stats["gpu_rows"], 0);
        }
        Ok(())
    }
    #[test]
    fn payload_changes_keep_readiness_but_preserve_new_native_matches() -> Result<()> {
        EGraph::set_num_threads(1);
        let (mut r, c) = fixture_ready(true, false, true, true)?;
        r.input("(Root (N 1) (N 2)) (set (P (N 1)) (N 3)) (Q (N 2) (N 4))")?;
        r.run(&c)?;
        let old = r.report();
        r.input("(set (P (N 1)) (N 5))")?;
        r.run(&c)?;
        let new = r.report();
        assert_eq!(
            old["saturation_diagnostics"]["statistics"]["readiness"]["root_rechecks"],
            new["saturation_diagnostics"]["statistics"]["readiness"]["root_rechecks"]
        );
        assert_eq!(
            new["saturation_diagnostics"]["statistics"]["rules"]["test"]["positive_bindings"],
            2
        );
        r.run(&c)?;
        assert_eq!(
            r.report()["saturation_diagnostics"]["statistics"]["rules"]["test"]["positive_bindings"],
            2
        );
        Ok(())
    }
    #[test]
    fn group_queries_keep_late_witnesses_union_subsume_and_function_updates() -> Result<()> {
        EGraph::set_num_threads(1);
        for (function, alias) in [(false, false), (false, true), (true, false)] {
            let (mut r, c) = fixture_mode(function, alias, true)?;
            r.input("(Root (N 1) (N 2))")?;
            r.run(&c)?;
            let p = if function {
                "(set (P (N 1)) (N 3))"
            } else {
                "(P (N 1) (N 3)) (P (N 1) (N 4))"
            };
            r.input(p)?;
            r.run(&c)?;
            assert_eq!(
                r.report()["saturation_diagnostics"]["statistics"]["blocks"]["witness_rows_materialized"],
                0
            );
            r.input("(Q (N 2) (N 5)) (Q (N 2) (N 6))")?;
            r.run(&c)?;
            r.input("(union (N 1) (N 2))")?;
            r.run(&c)?;
            if function {
                r.input("(set (P (N 1)) (N 7))")?;
                r.run(&c)?;
            }
            r.input("(subsume (Root (N 1) (N 2))) (Q (N 2) (N 8))")?;
            r.run(&c)?;
            r.run(&c)?;
            let j = r.report();
            assert!(
                j["saturation_diagnostics"]["statistics"]["groups"]["truth_checks"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
            assert_eq!(j["saturation_diagnostics"]["statistics"]["gpu_rows"], 0);
            assert!(
                j["saturation_diagnostics"]["statistics"]["rules"]["test"]["positive_bindings"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
        }
        Ok(())
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
