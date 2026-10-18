//! Continuous binary-table execution over an original native schedule. The GPU
//! owns persistent query indexes/receipts; native egglog owns all original RHS,
//! union, custom merge and rebuild. Explicit tables are not Cell certificates.
use crate::{Result, matrix_service::Service, table_query};
use egglog::{
    EGraph, Value,
    ast::{Command, GenericSchedule, Schedule},
    cell_assist::{CellActionSink, CellOperand},
    cell_events::{CellCommitBatch, CellCommitFeed},
};
use serde::Serialize;
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

type Binding = Vec<Option<Value>>;
type RowKey = (usize, Vec<Value>);
#[derive(Default, Serialize)]
pub struct QueryStats {
    pub dispatches: usize,
    pub quiet_runs: usize,
    pub bootstrap_rows: usize,
    pub seeded_old_rows: usize,
    pub added_rows: usize,
    pub retired_rows: usize,
    pub uploaded_bytes: usize,
    pub first_upload_bytes: usize,
    pub downloaded_bytes: usize,
    pub emitted_bindings: usize,
    pub audit_queries: usize,
    pub prepare_seconds: f64,
    pub roundtrip_seconds: f64,
    pub decode_seconds: f64,
    pub audit_seconds: f64,
    pub acknowledge_seconds: f64,
    pub device_seconds: f64,
    pub index_lookups: u64,
    pub candidate_rows: u64,
    pub peak_device_bytes: u64,
    pub shader_compiles: u64,
}
struct Query {
    sink: CellActionSink,
    device: Service,
    numbers: Vec<usize>,
    epoch: usize,
    ack_count: usize,
    identities: BTreeMap<(String, Value), u32>,
    frames: Vec<(usize, Binding)>,
    active: BTreeMap<RowKey, usize>,
    live: BTreeSet<usize>,
    pending: BTreeMap<RowKey, Option<Vec<Value>>>,
    old_keys: BTreeSet<RowKey>,
    clears: BTreeSet<usize>,
    audit_state: BTreeSet<Vec<Value>>,
    need_ack: bool,
    stats: QueryStats,
}
impl Query {
    fn new(eg: &mut EGraph, set: &str, name: &str, worker: &Path) -> Result<Self> {
        let sink = eg.prepare_cell_action(set, name)?;
        let old_keys = eg
            .cell_query_old_keys(set, &sink)?
            .into_iter()
            .enumerate()
            .flat_map(|(p, keys)| keys.into_iter().map(move |k| (p, k)))
            .collect();
        let (mut config, numbers) = table_query::plan(eg, &sink)?;
        config["pending_only"] = json!(true);
        let mut device = Service::new(worker)?;
        let installed = device.table_init(config)?;
        Ok(Self {
            sink,
            device,
            numbers,
            epoch: 0,
            ack_count: 0,
            identities: BTreeMap::new(),
            frames: vec![],
            active: BTreeMap::new(),
            live: BTreeSet::new(),
            pending: BTreeMap::new(),
            old_keys,
            clears: BTreeSet::new(),
            audit_state: BTreeSet::new(),
            need_ack: false,
            stats: QueryStats {
                shader_compiles: installed["shader_compiles"]
                    .as_u64()
                    .ok_or("missing compile receipt")?,
                ..Default::default()
            },
        })
    }
    fn project(&self, eg: &EGraph, part: usize, row: &[Value]) -> Result<Option<Binding>> {
        let atom = &self.sink.atoms[part];
        let schema = eg
            .get_function(&atom.function)
            .ok_or("missing native table")?
            .schema();
        if row.len() != atom.columns.len() {
            return Err("committed row width".into());
        }
        let mut b = vec![None; self.sink.slots().len()];
        for ((o, v), sort) in atom
            .columns
            .iter()
            .zip(row)
            .zip(schema.input.iter().chain([&schema.output]))
        {
            let v = eg.get_canonical_value(*v, sort);
            match o {
                CellOperand::Constant { value, .. }
                    if eg.get_canonical_value(*value, sort) != v =>
                {
                    return Ok(None);
                }
                CellOperand::Constant { .. } => (),
                CellOperand::Slot(s) => {
                    if b[*s].is_some_and(|a| a != v) {
                        return Ok(None);
                    }
                    b[*s] = Some(v);
                }
            }
        }
        Ok(Some(b))
    }
    fn dispatch(&mut self, eg: &mut EGraph, audit: bool) -> Result<Vec<Vec<Value>>> {
        let tick = Instant::now();
        let old = self.frames.len();
        let mut retired = vec![];
        for part in std::mem::take(&mut self.clears) {
            let keys = self
                .active
                .keys()
                .filter(|(p, _)| *p == part)
                .cloned()
                .collect::<Vec<_>>();
            for key in keys {
                let id = self.active.remove(&key).unwrap();
                self.live.remove(&id);
                retired.push(id);
            }
        }
        let slots = self.sink.slots();
        let mut words = vec![0, 0, 0, 0];
        let mut queued = std::mem::take(&mut self.pending)
            .into_iter()
            .collect::<Vec<_>>();
        queued.sort_by_key(|(key, _)| !self.old_keys.contains(key));
        let mut seeded = 0usize;
        for (key, row) in queued {
            let binding = match row {
                Some(r) => self.project(eg, key.0, &r)?,
                None => None,
            };
            // A committed version can have identical values (delete/reinsert,
            // rebuild retimestamp). Arbitrary native merge effects may depend
            // on that new occurrence; value equality cannot keep its old receipt.
            if !self.active.contains_key(&key) && binding.is_none() {
                continue;
            }
            if let Some(id) = self.active.remove(&key) {
                self.live.remove(&id);
                retired.push(id);
            }
            if let Some(binding) = binding {
                if self.old_keys.contains(&key) {
                    seeded += 1;
                }
                let id = self.frames.len();
                words.push(u32::try_from(key.0)?);
                for (s, v) in binding.iter().enumerate() {
                    let code = if let Some(v) = v {
                        let n = u32::try_from(self.identities.len() + 1)?;
                        *self
                            .identities
                            .entry((slots[s].1.to_string(), *v))
                            .or_insert(n)
                    } else {
                        0
                    };
                    words.push(code);
                }
                for s in &self.numbers {
                    let v = binding[*s].map(|v| eg.value_to_base::<i64>(v)).unwrap_or(0) as u64;
                    words.extend([v as u32, (v >> 32) as u32]);
                }
                self.frames.push((key.0, binding));
                self.live.insert(id);
                self.active.insert(key, id);
            }
        }
        self.old_keys.clear();
        let added = self.frames.len() - old;
        let mut out = vec![];
        self.stats.prepare_seconds += tick.elapsed().as_secs_f64();
        if added > 0 || !retired.is_empty() {
            self.epoch += 1;
            words[..4].copy_from_slice(&[
                u32::try_from(self.epoch)?,
                u32::try_from(added)?,
                u32::try_from(retired.len())?,
                u32::try_from(self.frames.len())?,
            ]);
            for id in &retired {
                words.push(u32::try_from(*id)?);
            }
            let tick = Instant::now();
            let (r, body) = self.device.table_delta_seeded(&words, audit, seeded)?;
            self.stats.roundtrip_seconds += tick.elapsed().as_secs_f64();
            self.stats.device_seconds += r["gpu_seconds"].as_f64().unwrap_or(0.0);
            self.stats.index_lookups += r["index_lookups"].as_u64().ok_or("missing index count")?;
            self.stats.candidate_rows += r["candidate_row_visits"]
                .as_u64()
                .ok_or("missing visit count")?;
            self.stats.peak_device_bytes = self
                .stats
                .peak_device_bytes
                .max(r["matrix_bytes"].as_u64().ok_or("missing device bytes")?);
            if r["epoch"].as_u64() != Some(self.epoch as u64)
                || r["query_only"] != true
                || r["pending_only_supported"] != true
                || r["groups"] != 0
                || r["arithmetic_error"] != false
            {
                return Err("incompatible/failed persistent query worker".into());
            }
            if r["seeded_rows"].as_u64() != Some(seeded as u64) {
                return Err("worker lost initial seminaive watermark".into());
            }
            self.stats.seeded_old_rows += seeded;
            self.stats.dispatches += 1;
            self.stats.added_rows += added;
            self.stats.retired_rows += retired.len();
            if self.epoch == 1 {
                self.stats.bootstrap_rows = added;
                self.stats.first_upload_bytes = words.len() * 4;
            }
            self.stats.uploaded_bytes += 4 * words.len();
            self.stats.downloaded_bytes += 4 * body.len();
            let tick = Instant::now();
            let width = 5 + self.sink.atoms.len();
            let rows = r["audit_rows"].as_u64().ok_or("missing result rows")? as usize;
            if rows.checked_mul(width) != Some(body.len()) {
                return Err("bad live query response size".into());
            }
            if audit {
                self.audit_state.clear();
            }
            for row in body.chunks_exact(width) {
                if row[2] != 1 || row[1] != u32::from(row[0] as usize >= self.ack_count) {
                    return Err("invalid live query receipt".into());
                }
                let mut b = vec![None; slots.len()];
                for (part, id) in row[5..].iter().enumerate() {
                    let id = *id as usize;
                    let (owner, frame) = self.frames.get(id).ok_or("unknown fragment row")?;
                    if *owner != part || !self.live.contains(&id) {
                        return Err("stale fragment source".into());
                    }
                    for (s, v) in frame.iter().enumerate() {
                        if let Some(v) = v {
                            if b[s].is_some_and(|a| a != *v) {
                                return Err("incompatible live binding".into());
                            }
                            b[s] = Some(*v);
                        }
                    }
                }
                let b = b
                    .into_iter()
                    .map(|v| v.ok_or("missing live binding slot"))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                if audit && !self.audit_state.insert(b.clone()) {
                    return Err("duplicate active live binding".into());
                }
                if row[1] == 1 {
                    out.push(b);
                } else if !audit {
                    return Err("worker replayed acknowledged match".into());
                }
            }
            if out.len()
                != r["pending_matches"]
                    .as_u64()
                    .ok_or("missing pending count")? as usize
            {
                return Err("missing pending bindings".into());
            }
            self.ack_count = r["match_records"].as_u64().ok_or("missing match count")? as usize;
            self.need_ack = true;
            self.stats.decode_seconds += tick.elapsed().as_secs_f64();
        } else {
            self.stats.quiet_runs += 1;
        }
        if audit {
            let tick = Instant::now();
            let native = eg
                .audit_cell_matches(&self.sink)?
                .into_iter()
                .collect::<BTreeSet<_>>();
            let frontier = self.frames[old + seeded..]
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            let expected = native
                .iter()
                .filter(|b| {
                    self.sink.atoms.iter().enumerate().any(|(part, atom)| {
                        let mut projected = vec![None; b.len()];
                        for op in &atom.columns {
                            if let CellOperand::Slot(slot) = op {
                                projected[*slot] = Some(b[*slot]);
                            }
                        }
                        frontier.contains(&(part, projected))
                    })
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            let actual = out.iter().cloned().collect::<BTreeSet<_>>();
            if !self.audit_state.is_subset(&native)
                || expected != actual
                || out.len() != actual.len()
            {
                return Err(format!(
                    "live delta query mismatch {}: expected {} vs {}, missing {} extra {}",
                    self.sink.name,
                    expected.len(),
                    actual.len(),
                    expected.difference(&actual).count(),
                    actual.difference(&expected).count()
                )
                .into());
            }
            self.stats.audit_queries += 1;
            self.stats.audit_seconds += tick.elapsed().as_secs_f64();
        }
        let tick = Instant::now();
        out.sort_unstable();
        self.stats.decode_seconds += tick.elapsed().as_secs_f64();
        self.stats.emitted_bindings += out.len();
        Ok(out)
    }
    fn acknowledge(&mut self) -> Result<()> {
        if self.need_ack {
            let t = Instant::now();
            self.device.table_ack(self.epoch)?;
            self.stats.acknowledge_seconds += t.elapsed().as_secs_f64();
            self.need_ack = false;
        }
        Ok(())
    }
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub setup_seconds: f64,
    pub run_seconds: f64,
    pub event_seconds: f64,
    pub native_rhs_seconds: f64,
    pub native_action_search_apply_seconds: f64,
    pub native_insert_merge_seconds: f64,
    pub native_rebuild_seconds: f64,
    pub ruleset_runs: usize,
    pub row_events: usize,
    pub union_events: usize,
    pub bootstrap_events: usize,
    pub bootstrap_union_events: usize,
}
pub struct Runtime {
    eg: EGraph,
    feed: CellCommitFeed,
    queries: Vec<Query>,
    sets: BTreeMap<String, Vec<usize>>,
    routes: Vec<Vec<(usize, usize)>>,
    audit: bool,
    failed: bool,
    pub stats: Stats,
}
fn collect(s: &Schedule, sets: &mut BTreeSet<String>) -> Result<()> {
    match s {
        GenericSchedule::Run(_, r) => {
            if r.until.is_some() {
                return Err("live table benchmark does not support until schedules".into());
            }
            sets.insert(r.ruleset.clone());
        }
        GenericSchedule::Sequence(_, xs) => {
            for s in xs {
                collect(s, sets)?;
            }
        }
        GenericSchedule::Repeat(_, _, s) | GenericSchedule::Saturate(_, s) => collect(s, sets)?,
    }
    Ok(())
}
impl Runtime {
    pub fn new(mut eg: EGraph, commands: &[Command], worker: &Path, audit: bool) -> Result<Self> {
        let tick = Instant::now();
        let mut names = BTreeSet::new();
        for c in commands {
            let Command::RunSchedule(s) = c else {
                return Err("expected schedule-only packet".into());
            };
            collect(s, &mut names)?;
        }
        let mut queries = vec![];
        let mut sets = BTreeMap::new();
        let mut tables = BTreeSet::new();
        for set in names {
            let mut ids = vec![];
            for name in eg.cell_rule_names(&set)? {
                let q = Query::new(&mut eg, &set, &name, worker)?;
                tables.extend(q.sink.atoms.iter().map(|a| a.function.clone()));
                ids.push(queries.len());
                queries.push(q);
            }
            sets.insert(set, ids);
        }
        let (feed, bootstrap) =
            eg.subscribe_cell_commits(&tables.into_iter().collect::<Vec<_>>())?;
        let routes = feed
            .tables
            .iter()
            .map(|t| {
                queries
                    .iter()
                    .enumerate()
                    .flat_map(|(q, x)| {
                        x.sink.atoms.iter().enumerate().filter_map(move |(p, a)| {
                            if a.function == t.name {
                                Some((q, p))
                            } else {
                                None
                            }
                        })
                    })
                    .collect()
            })
            .collect();
        let mut runtime = Self {
            eg,
            feed,
            queries,
            sets,
            routes,
            audit,
            failed: false,
            stats: Stats::default(),
        };
        runtime.stats.bootstrap_events = bootstrap.rows.len();
        runtime.stats.bootstrap_union_events = bootstrap.unions.len();
        runtime.ingest(bootstrap, true);
        runtime.stats.setup_seconds = tick.elapsed().as_secs_f64();
        Ok(runtime)
    }
    pub fn engine(&self) -> &EGraph {
        &self.eg
    }
    pub fn into_engine(self) -> EGraph {
        self.eg
    }
    fn ingest(&mut self, b: CellCommitBatch, bootstrap: bool) {
        self.stats.row_events += b.rows.len();
        self.stats.union_events += b.unions.len();
        for table in b.clears {
            for &(q, p) in &self.routes[table] {
                let q = &mut self.queries[q];
                q.pending.retain(|(part, _), _| *part != p);
                q.clears.insert(p);
            }
        }
        for r in b.rows {
            let row = if r.visible {
                r.row.map(|mut x| {
                    x.truncate(self.feed.tables[r.table].keys + 1);
                    x
                })
            } else {
                None
            };
            for &(q, p) in &self.routes[r.table] {
                if !bootstrap {
                    self.queries[q].old_keys.remove(&(p, r.key.clone()));
                }
                self.queries[q]
                    .pending
                    .insert((p, r.key.clone()), row.clone());
            }
        }
    }
    pub fn input(&mut self, code: &str) -> Result<()> {
        self.eg.parse_and_run_program(None, code)?;
        let b = self.eg.drain_cell_commits(&mut self.feed)?;
        self.ingest(b, false);
        Ok(())
    }
    fn run_set(&mut self, set: &str) -> Result<bool> {
        let ids = self.sets.get(set).ok_or("unregistered ruleset")?.clone();
        let mut matches = vec![];
        for id in &ids {
            matches.push(self.queries[*id].dispatch(&mut self.eg, self.audit)?);
        }
        let tick = Instant::now();
        let batches = ids
            .iter()
            .zip(&matches)
            .map(|(id, b)| (&self.queries[*id].sink, b.as_slice()))
            .collect::<Vec<_>>();
        let report = self.eg.apply_completed_cell_ruleset(&batches)?;
        self.stats.native_rhs_seconds += tick.elapsed().as_secs_f64();
        self.stats.native_action_search_apply_seconds += report
            .search_and_apply_time_per_ruleset
            .values()
            .map(|t| t.as_secs_f64())
            .sum::<f64>();
        self.stats.native_insert_merge_seconds += report
            .merge_time_per_ruleset
            .values()
            .map(|t| t.as_secs_f64())
            .sum::<f64>();
        self.stats.native_rebuild_seconds += report
            .rebuild_time_per_ruleset
            .values()
            .map(|t| t.as_secs_f64())
            .sum::<f64>();
        let tick = Instant::now();
        let b = self.eg.drain_cell_commits(&mut self.feed)?;
        self.ingest(b, false);
        self.stats.event_seconds += tick.elapsed().as_secs_f64();
        for id in ids {
            self.queries[id].acknowledge()?;
        }
        self.stats.ruleset_runs += 1;
        if self.stats.ruleset_runs % 100 == 0 {
            eprintln!(
                "[live tables] {} ruleset runs, {} row events",
                self.stats.ruleset_runs, self.stats.row_events
            );
        }
        Ok(report.updated)
    }
    fn schedule(&mut self, s: &Schedule) -> Result<bool> {
        match s {
            GenericSchedule::Run(_, r) => self.run_set(&r.ruleset),
            GenericSchedule::Sequence(_, xs) => {
                let mut changed = false;
                for x in xs {
                    changed |= self.schedule(x)?;
                }
                Ok(changed)
            }
            GenericSchedule::Repeat(_, n, x) => {
                let mut changed = false;
                for _ in 0..*n {
                    let step = self.schedule(x)?;
                    changed |= step;
                    if !step {
                        break;
                    }
                }
                Ok(changed)
            }
            GenericSchedule::Saturate(_, x) => {
                let mut changed = false;
                loop {
                    let step = self.schedule(x)?;
                    changed |= step;
                    if !step {
                        break;
                    }
                }
                Ok(changed)
            }
        }
    }
    pub fn run(&mut self, commands: &[Command]) -> Result<()> {
        if self.failed {
            return Err("live table session failed; cannot resume unconfirmed commits".into());
        }
        let t = Instant::now();
        let result = (|| {
            for c in commands {
                let Command::RunSchedule(s) = c else {
                    return Err("expected schedule".into());
                };
                self.schedule(s)?;
            }
            Ok(())
        })();
        self.stats.run_seconds += t.elapsed().as_secs_f64();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn report(&self) -> Json {
        json!({"runtime":self.stats,"audit":self.audit,"rules":self.sets.iter().flat_map(|(set,ids)|ids.iter().map(move |id|json!({"ruleset":set,"rule":self.queries[*id].sink.name,"stats":self.queries[*id].stats}))).collect::<Vec<_>>(),
        "scope":"Persistent GPU table queries driving original RHS and schedule. One bootstrap, committed row deltas thereafter; no native full LHS on timed lane. Explicit row fragments, not certified Cell shortcuts."})
    }
}
