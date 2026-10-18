//! A shared native-row store and one GPU batch per ruleset boundary. Native
//! timestamps define the frontier; output lives only until the original RHS
//! commits. The CPU reference uses cached native seminaive queries and exactly
//! the same sorted commit boundary, including order-sensitive custom merges.
use crate::{Result, family_program, matrix_service::Service};
use egglog::{
    ArcSort, EGraph, Value,
    ast::{Command, GenericSchedule, Schedule},
    cell_assist::{CellActionSink, CellQuery},
    cell_events::{CellCommitBatch, CellCommitFeed},
};
use egglog_numeric_id::NumericId;
use serde::Serialize;
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};
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
    pub prepare_seconds: f64,
    pub roundtrip_seconds: f64,
    pub decode_seconds: f64,
    pub device_seconds: f64,
    pub native_query_seconds: f64,
    pub audit_seconds: f64,
    pub dispatches: usize,
    pub uploaded_bytes: usize,
    pub downloaded_bytes: usize,
    pub first_upload_bytes: usize,
    pub unchanged_notifications: usize,
    pub workers: usize,
    pub shader_compiles: usize,
    pub reused_slots: usize,
    pub peak_live_rows: usize,
    pub output_high_water: usize,
}
#[derive(Default, Serialize)]
struct QueryStats {
    dispatches: usize,
    quiet_runs: usize,
    emitted_bindings: usize,
    audit_queries: usize,
}
struct Row {
    table: usize,
    generation: u32,
    timestamp: u32,
    live: bool,
    values: Vec<Value>,
}
#[derive(Default)]
pub struct Options<'a> {
    pub analysis: Option<&'a Path>,
    /// Keep the same Cell-guided order but disable reuse for a fair ablation.
    pub plan_only: bool,
    pub profile: bool,
}
pub struct Runtime {
    eg: EGraph,
    sinks: Vec<CellActionSink>,
    reference: Vec<CellQuery>,
    sets: BTreeMap<String, Vec<usize>>,
    qstats: Vec<QueryStats>,
    device: Option<Service>,
    feed: Option<CellCommitFeed>,
    sorts: Vec<Vec<ArcSort>>,
    table_ids: Vec<Vec<usize>>,
    rows: Vec<Row>,
    keys: Vec<BTreeMap<Vec<Value>, usize>>,
    free: Vec<usize>,
    pending: BTreeSet<usize>,
    max_stamp: Vec<u32>,
    columns: usize,
    slots: usize,
    epoch: usize,
    live: usize,
    audit: bool,
    cpu: bool,
    failed: bool,
    last_device: Json,
    cell_evidence: Json,
    cell_work: BTreeMap<String, u64>,
    pub stats: Stats,
}
fn collect(s: &Schedule, out: &mut BTreeSet<String>) -> Result<()> {
    match s {
        GenericSchedule::Run(_, r) => {
            if r.until.is_some() {
                return Err("until schedule unsupported".into());
            }
            out.insert(r.ruleset.clone());
        }
        GenericSchedule::Sequence(_, xs) => {
            for s in xs {
                collect(s, out)?;
            }
        }
        GenericSchedule::Repeat(_, _, s) | GenericSchedule::Saturate(_, s) => collect(s, out)?,
    }
    Ok(())
}
impl Runtime {
    pub fn new(eg: EGraph, commands: &[Command], worker: &Path, audit: bool) -> Result<Self> {
        Self::create(eg, commands, Some(worker), audit, Options::default())
    }
    pub fn new_cpu(eg: EGraph, commands: &[Command]) -> Result<Self> {
        Self::create(eg, commands, None, false, Options::default())
    }
    /// Optional analyze-backed projections. The complete query remains the
    /// fallback for every uncertified occurrence, including new union aliases.
    pub fn new_with_cells(
        eg: EGraph,
        commands: &[Command],
        worker: &Path,
        audit: bool,
        analysis: Option<&Path>,
        profile: bool,
    ) -> Result<Self> {
        Self::configured(
            eg,
            commands,
            worker,
            audit,
            Options {
                analysis,
                profile,
                plan_only: false,
            },
        )
    }
    pub fn configured(
        eg: EGraph,
        commands: &[Command],
        worker: &Path,
        audit: bool,
        options: Options<'_>,
    ) -> Result<Self> {
        Self::create(eg, commands, Some(worker), audit, options)
    }
    fn create(
        mut eg: EGraph,
        commands: &[Command],
        worker: Option<&Path>,
        audit: bool,
        options: Options<'_>,
    ) -> Result<Self> {
        let t = Instant::now();
        let cells = options
            .analysis
            .map(|dir| crate::cell_prefix::Library::load(&mut eg, dir))
            .transpose()?;
        let mut names = BTreeSet::new();
        for c in commands {
            let Command::RunSchedule(s) = c else {
                return Err("schedule-only packet required".into());
            };
            collect(s, &mut names)?;
        }
        let mut sinks = vec![];
        let mut sets = BTreeMap::new();
        let mut tables = BTreeSet::new();
        for set in names {
            let mut ids = vec![];
            for name in eg.cell_rule_names(&set)? {
                let sink = eg.prepare_cell_action(&set, &name)?;
                if !eg.cell_query_is_seminaive(&sink) {
                    return Err("shared runtime requires seminaive rules".into());
                }
                tables.extend(sink.atoms.iter().map(|a| a.function.clone()));
                ids.push(sinks.len());
                sinks.push(sink);
            }
            sets.insert(set, ids);
        }
        let mut reference = vec![];
        if audit || worker.is_none() {
            for s in &sinks {
                reference.push(eg.prepare_cell_query(s)?);
            }
        }
        let mut runtime = Self {
            eg,
            sinks,
            reference,
            sets,
            qstats: vec![],
            device: None,
            feed: None,
            sorts: vec![],
            table_ids: vec![],
            rows: vec![],
            keys: vec![],
            free: vec![],
            pending: BTreeSet::new(),
            max_stamp: vec![],
            columns: 0,
            slots: 0,
            epoch: 0,
            live: 0,
            audit,
            cpu: worker.is_none(),
            failed: false,
            last_device: Json::Null,
            cell_evidence: Json::Null,
            cell_work: BTreeMap::new(),
            stats: Stats::default(),
        };
        runtime.qstats = runtime
            .sinks
            .iter()
            .map(|_| QueryStats::default())
            .collect();
        if let Some(worker) = worker {
            let (feed, initial) = runtime
                .eg
                .subscribe_cell_commits(&tables.into_iter().collect::<Vec<_>>())?;
            runtime.columns = feed.tables.iter().map(|t| t.keys + 1).max().unwrap_or(1);
            runtime.slots = runtime
                .sinks
                .iter()
                .map(|s| s.slots().len())
                .max()
                .unwrap_or(1);
            runtime.sorts = feed
                .tables
                .iter()
                .map(|t| {
                    let f = runtime.eg.get_function(&t.name).unwrap();
                    f.schema()
                        .input
                        .iter()
                        .chain([&f.schema().output])
                        .cloned()
                        .collect()
                })
                .collect();
            runtime.table_ids = runtime
                .sinks
                .iter()
                .map(|s| {
                    s.atoms
                        .iter()
                        .map(|a| {
                            feed.tables
                                .iter()
                                .position(|t| t.name == a.function)
                                .unwrap()
                        })
                        .collect()
                })
                .collect();
            runtime.keys = feed.tables.iter().map(|_| BTreeMap::new()).collect();
            runtime.max_stamp = vec![0; feed.tables.len()];
            let config = family_program::compile(
                &runtime.eg,
                &runtime.sinks,
                &feed.tables,
                cells.as_ref(),
                options.profile,
                !options.plan_only,
            )?;
            runtime.cell_evidence = json!({"library":cells.as_ref().map(|c|&c.evidence),"plans":config["cell_evidence"],"profile":options.profile,"plan_only":options.plan_only});
            let mut device = Service::new(worker)?;
            device.family_init(config)?;
            runtime.device = Some(device);
            runtime.stats.workers = 1;
            runtime.stats.shader_compiles = 1;
            runtime.stats.bootstrap_events = initial.rows.len();
            runtime.feed = Some(feed);
            runtime.ingest(initial)?;
        }
        runtime.stats.setup_seconds = t.elapsed().as_secs_f64();
        Ok(runtime)
    }
    pub fn engine(&self) -> &EGraph {
        &self.eg
    }
    pub fn into_engine(self) -> EGraph {
        self.eg
    }
    fn retire(&mut self, table: usize, key: &[Value]) -> Result<()> {
        if let Some(slot) = self.keys[table].remove(key) {
            let row = &mut self.rows[slot];
            row.live = false;
            row.generation = row
                .generation
                .checked_add(1)
                .ok_or("row generation exhausted")?;
            row.values.clear();
            self.free.push(slot);
            self.pending.insert(slot);
            self.live -= 1;
        }
        Ok(())
    }
    fn ingest(&mut self, b: CellCommitBatch) -> Result<()> {
        self.stats.row_events += b.rows.len();
        self.stats.union_events += b.unions.len();
        for table in b.clears {
            let keys = self.keys[table].keys().cloned().collect::<Vec<_>>();
            for key in keys {
                self.retire(table, &key)?;
            }
        }
        // Delete first, so replacements in one atomic commit reuse slots immediately.
        for r in &b.rows {
            if !r.visible || r.row.is_none() {
                self.retire(r.table, &r.key)?;
            }
        }
        for r in b.rows {
            if !r.visible {
                continue;
            }
            let Some(mut values) = r.row else {
                continue;
            };
            values.truncate(self.sorts[r.table].len());
            let stamp = r.timestamp.ok_or("visible row has no native timestamp")?;
            for (v, sort) in values.iter_mut().zip(&self.sorts[r.table]) {
                if sort.is_eq_sort() {
                    *v = self.eg.get_canonical_value(*v, sort);
                }
            }
            self.max_stamp[r.table] = self.max_stamp[r.table].max(stamp);
            if let Some(&id) = self.keys[r.table].get(&r.key) {
                let old = &self.rows[id];
                if old.timestamp == stamp && old.values == values {
                    self.stats.unchanged_notifications += 1;
                    continue;
                }
            }
            let slot = if let Some(&id) = self.keys[r.table].get(&r.key) {
                id
            } else {
                self.live += 1;
                let id = if let Some(id) = self.free.pop() {
                    self.stats.reused_slots += 1;
                    id
                } else {
                    self.rows.push(Row {
                        table: r.table,
                        generation: 0,
                        timestamp: 0,
                        live: false,
                        values: vec![],
                    });
                    self.rows.len() - 1
                };
                self.keys[r.table].insert(r.key, id);
                id
            };
            let row = &mut self.rows[slot];
            row.generation = row
                .generation
                .checked_add(1)
                .ok_or("row generation exhausted")?;
            row.table = r.table;
            row.timestamp = stamp;
            row.live = true;
            row.values = values;
            self.pending.insert(slot);
        }
        self.stats.peak_live_rows = self.stats.peak_live_rows.max(self.live);
        Ok(())
    }
    pub fn input(&mut self, code: &str) -> Result<()> {
        let commands = self.eg.parse_program(None, code)?;
        if commands
            .iter()
            .any(|c| !matches!(c, Command::Action(_) | Command::Check(..)))
        {
            return Err(
                "live input accepts facts/actions/checks, not schema or schedule changes".into(),
            );
        }
        self.eg.run_program(commands)?;
        if let Some(feed) = &mut self.feed {
            let b = self.eg.drain_cell_commits(feed)?;
            self.ingest(b)?;
        }
        Ok(())
    }
    fn native_matches(&mut self, ids: &[usize]) -> Result<Vec<Vec<Vec<Value>>>> {
        let qs = ids.iter().map(|i| &self.reference[*i]).collect::<Vec<_>>();
        let mut result = self.eg.run_cell_queries(&qs)?;
        for rows in &mut result {
            rows.sort_unstable();
        }
        Ok(result)
    }
    fn gpu_matches(&mut self, ids: &[usize]) -> Result<Vec<Vec<Vec<Value>>>> {
        let t = Instant::now();
        let active = ids
            .iter()
            .copied()
            .filter(|i| {
                self.table_ids[*i]
                    .iter()
                    .any(|t| self.max_stamp[*t] >= self.eg.cell_query_watermark(&self.sinks[*i]))
            })
            .collect::<Vec<_>>();
        let mut matches = ids.iter().map(|_| vec![]).collect::<Vec<Vec<Vec<Value>>>>();
        for id in ids {
            if !active.contains(id) {
                self.qstats[*id].quiet_runs += 1;
            }
        }
        if active.is_empty() {
            return Ok(matches);
        }
        let mut words = vec![
            u32::try_from(self.rows.len())?,
            u32::try_from(self.pending.len())?,
            u32::try_from(active.len())?,
            u32::try_from(self.epoch + 1)?,
        ];
        for id in &active {
            words.extend([
                *id as u32,
                self.eg.cell_query_watermark(&self.sinks[*id]),
                0,
            ]);
            self.qstats[*id].dispatches += 1;
        }
        for slot in &self.pending {
            let row = &self.rows[*slot];
            words.extend([
                u32::try_from(*slot)?,
                row.table as u32,
                u32::from(row.live),
                row.generation,
                row.timestamp,
            ]);
            words.extend(row.values.iter().map(|v| v.rep()));
            words.extend(std::iter::repeat_n(0, self.columns - row.values.len()));
            for col in 0..self.columns {
                let n = if row.live
                    && col < row.values.len()
                    && self.feed.as_ref().unwrap().tables[row.table].i64_columns[col]
                {
                    self.eg.value_to_base::<i64>(row.values[col]) as u64
                } else {
                    0
                };
                words.extend([n as u32, (n >> 32) as u32]);
            }
        }
        self.stats.prepare_seconds += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let (r, body) = self
            .device
            .as_mut()
            .unwrap()
            .family_step(&words, self.epoch)?;
        self.stats.roundtrip_seconds += t.elapsed().as_secs_f64();
        self.stats.device_seconds += r["gpu_seconds"].as_f64().ok_or("missing device timer")?;
        if r["epoch"].as_u64() != Some((self.epoch + 1) as u64) || r["retained_matches"] != 0 {
            return Err("invalid family receipt".into());
        }
        self.epoch += 1;
        self.pending.clear();
        self.stats.dispatches += 1;
        self.stats.uploaded_bytes += 4 * words.len();
        self.stats.downloaded_bytes += 4 * body.len();
        if self.epoch == 1 {
            self.stats.first_upload_bytes = 4 * words.len();
        }
        self.last_device = r.clone();
        if let Some(work) = r["cell_work"].as_object() {
            for (k, v) in work {
                *self.cell_work.entry(k.clone()).or_default() +=
                    v.as_u64().ok_or("invalid Cell work counter")?;
            }
        }
        let t = Instant::now();
        let count = r["bindings"].as_u64().ok_or("missing binding count")? as usize;
        if count.checked_mul(1 + self.slots) != Some(body.len()) {
            return Err("family result shape".into());
        }
        self.stats.output_high_water = self.stats.output_high_water.max(count);
        for record in body.chunks_exact(1 + self.slots) {
            let q = record[0] as usize;
            let i = ids
                .iter()
                .position(|i| *i == q)
                .ok_or("result belongs to another ruleset")?;
            if !active.contains(&q) {
                return Err("result for quiet query".into());
            }
            let n = self.sinks[q].slots().len();
            matches[i].push(record[1..1 + n].iter().map(|v| Value::new(*v)).collect());
        }
        for bindings in &mut matches {
            bindings.sort_unstable();
            if bindings.windows(2).any(|p| p[0] == p[1]) {
                return Err("duplicate family binding".into());
            }
        }
        self.stats.decode_seconds += t.elapsed().as_secs_f64();
        Ok(matches)
    }
    fn run_set(&mut self, set: &str) -> Result<bool> {
        let ids = self.sets.get(set).ok_or("unregistered ruleset")?.clone();
        let matches = if self.cpu {
            let t = Instant::now();
            let r = self.native_matches(&ids)?;
            self.stats.native_query_seconds += t.elapsed().as_secs_f64();
            r
        } else {
            self.gpu_matches(&ids)?
        };
        if self.audit {
            let t = Instant::now();
            let expected = self.native_matches(&ids)?;
            for ((id, a), b) in ids.iter().zip(&matches).zip(&expected) {
                if a != b {
                    return Err(format!(
                        "independent native frontier differs for {}: {} GPU / {} native",
                        self.sinks[*id].name,
                        a.len(),
                        b.len()
                    )
                    .into());
                }
                self.qstats[*id].audit_queries += 1;
            }
            self.stats.audit_seconds += t.elapsed().as_secs_f64();
        }
        for (id, b) in ids.iter().zip(&matches) {
            self.qstats[*id].emitted_bindings += b.len();
        }
        let t = Instant::now();
        let batches = ids
            .iter()
            .zip(&matches)
            .map(|(id, b)| (&self.sinks[*id], b.as_slice()))
            .collect::<Vec<_>>();
        let report = self.eg.apply_completed_cell_ruleset(&batches)?;
        self.stats.native_rhs_seconds += t.elapsed().as_secs_f64();
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
        let t = Instant::now();
        if let Some(feed) = &mut self.feed {
            let b = self.eg.drain_cell_commits(feed)?;
            self.ingest(b)?;
        }
        self.stats.event_seconds += t.elapsed().as_secs_f64();
        self.stats.ruleset_runs += 1;
        if self.stats.ruleset_runs % 100 == 0 {
            eprintln!(
                "[shared {}] {} ruleset runs",
                if self.cpu { "CPU" } else { "GPU" },
                self.stats.ruleset_runs
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
            GenericSchedule::Repeat(_, n, s) => {
                let mut changed = false;
                for _ in 0..*n {
                    let c = self.schedule(s)?;
                    changed |= c;
                    if !c {
                        break;
                    }
                }
                Ok(changed)
            }
            GenericSchedule::Saturate(_, s) => {
                let mut changed = false;
                loop {
                    let c = self.schedule(s)?;
                    changed |= c;
                    if !c {
                        break;
                    }
                }
                Ok(changed)
            }
        }
    }
    pub fn run(&mut self, commands: &[Command]) -> Result<()> {
        if self.failed {
            return Err("failed shared runtime cannot resume".into());
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
        json!({"runtime":self.stats,"audit":self.audit,"backend":if self.cpu{"native-sorted-commit"}else{"shared-gpu"},"cells":self.cell_evidence,"cell_work":self.cell_work,"device":self.last_device,"shared_tables":self.keys.len(),"live_rows":self.live,"row_slots":self.rows.len(),"rules":self.sets.iter().flat_map(|(set,ids)|ids.iter().map(move |id|json!({"ruleset":set,"rule":self.sinks[*id].name,"stats":self.qstats[*id]}))).collect::<Vec<_>>(),"scope":"Shared native rows and sorted native RHS commit; optional analyzed Cell projections skip certified unique local prefixes. Every uncertified anchor uses the complete query. No virtual storage or arbitrary-Cell coverage claim."})
    }
}
