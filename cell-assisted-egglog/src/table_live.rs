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
    pub saturation_boundaries: usize,
    pub skipped_saturations: usize,
    pub partial_saturations: usize,
    pub saturation_hook_seconds: f64,
    pub cell_run_calls: usize,
    pub cell_run_seconds: f64,
    pub native_only_runs: usize,
    pub native_only_seconds: f64,
}
#[derive(Default, Serialize)]
struct NativeStats {
    runs: usize,
    seconds: f64,
    search_apply_seconds: f64,
    insert_merge_seconds: f64,
    rebuild_seconds: f64,
    matches: usize,
}
/// Trusted saturation compiler, analogous to the complete-frontier provider.
/// `None` declines WITHOUT effects. Some may publish certified closure effects;
/// `complete=false` requires the ordinary backend to finish saturation.
/// Only invoked for explicit Saturate bodies, never for an ordinary Run.
pub trait SaturationHook {
    fn try_run(&mut self,_eg:&mut EGraph,_set:&str)->Result<Option<egglog_reports::RunReport>>{Ok(None)}
    fn handles(&self,_body:&Schedule)->bool{true}
    fn diagnostics(&self) -> Json {
        Json::Null
    }
    fn try_boundary(&mut self, eg: &mut EGraph, body: &Schedule) -> Result<Option<Json>>;
    /// Ordinary backend completed this exact Saturate body. A joint observer
    /// may renew its certificate after unsupported changes were handled there.
    fn after_boundary(&mut self, _eg: &mut EGraph, _body: &Schedule) -> Result<()> {
        Ok(())
    }
    /// Forward rows consumed from draining journals; table IDs refer to names.
    /// Union cursors remain independent, so the mirror uses its own union feed.
    fn take_commits(&mut self, eg: &mut EGraph) -> Result<(Vec<String>, Vec<CellCommitBatch>)>;
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
#[derive(Clone, Copy, Default)]
#[repr(u32)]
pub enum PortLocality {
    #[default]
    Independent = 0,
    SharedContext = 1,
    SharedJoins = 2,
}
#[derive(Clone, Copy, Default)]
#[repr(u32)]
pub enum QueryRouting {
    #[default]
    Direct = 0,
    Compact = 1,
    CellPriority = 2,
    /// Index every delta row directly into its consuming query anchors.
    TableAnchors = 3,
}
#[derive(Default)]
pub struct Options<'a> {
    pub analysis: Option<&'a Path>,
    /// Keep the same Cell-guided order but disable reuse for a fair ablation.
    pub plan_only: bool,
    pub cache_only: bool,
    pub profile: bool,
    pub port_locality: PortLocality,
    pub query_routing: QueryRouting,
}
pub struct Runtime {
    eg: EGraph,
    sinks: Vec<CellActionSink>,
    reference: Vec<CellQuery>,
    sets: BTreeMap<String, Vec<usize>>,
    native_sets: BTreeSet<String>,
    native_stats: BTreeMap<String, NativeStats>,
    qstats: Vec<QueryStats>,
    owners: Vec<String>,
    extended: bool,
    query_log: bool,
    experimental_cell_runs: bool,
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
    saturation_hook: Option<Box<dyn SaturationHook>>,
    hook_table_ids: BTreeMap<String, usize>,
    last_saturation: Json,
    pub stats: Stats,
}
fn collect(
    s: &Schedule,
    out: &mut BTreeSet<String>,
    selected: Option<&BTreeSet<String>>,
) -> Result<()> {
    match s {
        GenericSchedule::Run(_, r) => {
            if r.until.is_some() && selected.is_none_or(|sets| sets.contains(&r.ruleset)) {
                return Err("until schedule unsupported".into());
            }
            out.insert(r.ruleset.clone());
        }
        GenericSchedule::Sequence(_, xs) => {
            for s in xs {
                collect(s, out, selected)?;
            }
        }
        GenericSchedule::Repeat(_, _, s) | GenericSchedule::Saturate(_, s) => {
            collect(s, out, selected)?
        }
    }
    Ok(())
}
impl Runtime {
    /// Opt-in research lane: Pass 3 currently fails the full-graph gate after
    /// scalar Run elision (Smaller fact mismatch). Do not enable by default.
    pub fn enable_experimental_cell_runs(&mut self) {self.experimental_cell_runs=true;}
    pub fn set_saturation_hook(&mut self, hook: Box<dyn SaturationHook>) {
        self.hook_table_ids = self
            .feed
            .as_ref()
            .map(|f| {
                f.tables
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (t.name.clone(), i))
                    .collect()
            })
            .unwrap_or_default();
        self.saturation_hook = Some(hook);
    }
    pub fn new(eg: EGraph, commands: &[Command], worker: &Path, audit: bool) -> Result<Self> {
        Self::create(eg, commands, Some(worker), audit, Options::default(), None, false)
    }
    pub fn new_cpu(eg: EGraph, commands: &[Command]) -> Result<Self> {
        Self::create(eg, commands, None, false, Options::default(), None, false)
    }
    /// Exact explicit partition: selected rulesets use cached queries (CPU or
    /// GPU); all other scheduled rulesets execute natively, in their original
    /// position. Native commits feed the same persistent device tables.
    pub fn new_hybrid(
        eg: EGraph,
        commands: &[Command],
        worker: Option<&Path>,
        audit: bool,
        selected: &[String],
    ) -> Result<Self> {
        let sets = selected.iter().cloned().collect();
        Self::create(eg, commands, worker, audit, Options::default(), Some(&sets), false)
    }
    /// Full original Pass-3 LHS admission. Effects remain native; an unsupported
    /// selected rule is an error, never silently moved to the CPU.
    pub fn new_full(eg:EGraph,commands:&[Command],worker:Option<&Path>,audit:bool,selected:Option<&[String]>)->Result<Self>{
        let sets=selected.map(|s|s.iter().cloned().collect());
        Self::create(eg,commands,worker,audit,Options {query_routing:QueryRouting::TableAnchors,..Options::default()},sets.as_ref(),true)
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
                cache_only: false,
                port_locality: PortLocality::Independent,
                query_routing: QueryRouting::Direct,
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
        Self::create(eg, commands, Some(worker), audit, options, None, false)
    }
    fn create(
        mut eg: EGraph,
        commands: &[Command],
        worker: Option<&Path>,
        audit: bool,
        options: Options<'_>,
        selected: Option<&BTreeSet<String>>,
        extended: bool,
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
            collect(s, &mut names, selected)?;
        }
        if selected.is_some_and(|s| !s.is_subset(&names)) {
            return Err("selected ruleset absent from schedule".into());
        }
        if selected.is_some() {
            let available = eg
                .cell_rule_environment_identity()
                .into_iter()
                .map(|(n, _)| n)
                .collect::<BTreeSet<_>>();
            if !names.is_subset(&available) {
                return Err("scheduled native ruleset is not installed".into());
            }
        }
        let native_sets = selected
            .map(|s| names.difference(s).cloned().collect())
            .unwrap_or_default();
        let mut sinks = vec![];
        let mut sets = BTreeMap::new();
        let mut tables = BTreeSet::new();
        let mut compiled=BTreeMap::new();
        let mut owners=vec![];
        for set in names {
            if selected.is_some_and(|s| !s.contains(&set)) {
                continue;
            }
            let mut ids = vec![];
            let members=if extended{eg.cell_ruleset_members(&set)?}else{eg.cell_rule_names(&set)?.into_iter().map(|n|(set.clone(),n)).collect()};
            for (owner,name) in members {
                if let Some(id)=compiled.get(&(owner.clone(),name.clone())){
                    if ids.contains(id){return Err("repeated rule inside one combined Run".into())}
                    ids.push(*id);continue;
                }
                let sink = if extended{eg.prepare_gpu_cell_action(&owner,&name)?}else{eg.prepare_cell_action(&owner, &name)?};
                if !eg.cell_query_is_seminaive(&sink) {
                    return Err("shared runtime requires seminaive rules".into());
                }
                tables.extend(sink.atoms.iter().map(|a| a.function.clone()));
                owners.push(owner.clone());
                compiled.insert((owner,name),sinks.len());
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
            native_sets,
            native_stats: BTreeMap::new(),
            qstats: vec![],
            owners,
            extended,
            query_log: std::env::var_os("CELL_QUERY_LOG").is_some(),
            experimental_cell_runs:std::env::var("CELL_EXPERIMENTAL_RUN_PROGRAMS").as_deref()==Ok("1"),
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
            saturation_hook: None,
            hook_table_ids: BTreeMap::new(),
            last_saturation: Json::Null,
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
                .map(|s| s.device_slots())
                .max()
                .unwrap_or(1).max(1);
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
            let mut config = family_program::compile(
                &runtime.eg,
                &runtime.sinks,
                &feed.tables,
                cells.as_ref(),
                options.profile,
                !options.plan_only,
                !options.cache_only,
            )?;
            config["port_locality"] = json!(options.port_locality as u32);
            config["query_routing"] = json!(options.query_routing as u32);
            runtime.cell_evidence = json!({"library":cells.as_ref().map(|c|&c.evidence),"plans":config["cell_evidence"],"profile":options.profile,"plan_only":options.plan_only,"cache_only":options.cache_only,"port_locality":options.port_locality as u32,"query_routing":options.query_routing as u32,"port_join_groups":config["port_join_groups"]});
            let mut device = Service::new(worker)?;
            let installed = device.family_init(config)?;
            if installed["family_layout"] != json!(7) {
                return Err("GPU family layout changed; rebuild matrix.swift".into());
            }
            if options.port_locality as u32 > 0
                && installed["port_locality"] != json!(options.port_locality as u32)
            {
                return Err(
                    "GPU worker lacks requested Cell locality mode; rebuild matrix.swift".into(),
                );
            }
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
        self.collect_commits()?;
        Ok(())
    }
    fn collect_commits(&mut self) -> Result<()> {
        let forwarded = if let Some(hook) = &mut self.saturation_hook {
            Some(hook.take_commits(&mut self.eg)?)
        } else {
            None
        };
        if let Some(feed) = &mut self.feed {
            let batch = self.eg.drain_cell_commits(feed)?;
            if let Some((names, batches)) = forwarded {
                for b in batches {
                    let mut mapped = CellCommitBatch::default();
                    for mut row in b.rows {
                        if let Some(i) = self.hook_table_ids.get(&names[row.table]) {
                            row.table = *i;
                            mapped.rows.push(row);
                        }
                    }
                    for table in b.clears {
                        if let Some(i) = self.hook_table_ids.get(&names[table]) {
                            mapped.clears.push(*i);
                        }
                    }
                    self.ingest(mapped)?;
                }
            }
            self.ingest(batch)?;
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
                (self.table_ids[*i].is_empty() && self.eg.cell_query_watermark(&self.sinks[*i])==0) ||
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
        for (id, bindings) in ids.iter().zip(&mut matches) {
            bindings.sort_unstable();
            if let Some(pair) = bindings.windows(2).find(|p| p[0] == p[1]) {
                return Err(format!(
                    "duplicate family binding for {}: {:?} among {} rows; device counters {}",
                    self.sinks[*id].name,
                    pair[0],
                    bindings.len(),
                    self.last_device["cell_work"]
                )
                .into());
            }
        }
        self.stats.decode_seconds += t.elapsed().as_secs_f64();
        Ok(matches)
    }
    fn run_set(&mut self, set: &str) -> Result<bool> {
        let query_start=Instant::now();
        if self.query_log {eprintln!("[query start] {} {}",self.stats.ruleset_runs,set);}
        let ids = self.sets.get(set).ok_or("unregistered ruleset")?.clone();
        // Experimental single-Run Cell plans run before the ordinary GPU query.
        // The large Pass-3 graph gate currently fails, so opt-in is required.
        let started=Instant::now();
        let assisted=if self.experimental_cell_runs {if let Some(hook)=self.saturation_hook.as_mut(){hook.try_run(&mut self.eg,set)?}else{None}}else{None};
        if let Some(report)=assisted {
            self.stats.cell_run_calls+=1;
            self.stats.cell_run_seconds+=started.elapsed().as_secs_f64();
            if self.cpu || self.audit {
                for id in &ids {self.eg.sync_cell_query_after_native_run(&self.sinks[*id],&self.reference[*id]);}
            }
            let start=Instant::now();self.collect_commits()?;
            self.stats.event_seconds+=start.elapsed().as_secs_f64();
            self.stats.ruleset_runs+=1;
            if self.query_log {eprintln!("[cell Run] {set} {:.6}s",query_start.elapsed().as_secs_f64());}
            return Ok(report.updated);
        }
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
        self.collect_commits()?;
        self.stats.event_seconds += t.elapsed().as_secs_f64();
        self.stats.ruleset_runs += 1;
        if self.stats.ruleset_runs % 100 == 0 {
            eprintln!(
                "[shared {}] {} ruleset runs",
                if self.cpu { "CPU" } else { "GPU" },
                self.stats.ruleset_runs
            );
        }
        if self.query_log {eprintln!("[query done] {} {:.6}s",set,query_start.elapsed().as_secs_f64());}
        Ok(report.updated)
    }
    fn schedule(&mut self, s: &Schedule) -> Result<bool> {
        match s {
            GenericSchedule::Run(_, r) if self.native_sets.contains(&r.ruleset) => {
                let start = Instant::now();
                let outputs = if r.until.is_none() {
                    let assisted=if let Some(hook)=self.saturation_hook.as_mut(){hook.try_run(&mut self.eg,&r.ruleset)?}else{None};
                    vec![egglog::CommandOutput::RunSchedule(match assisted{Some(report)=>report,None=>self.eg.step_rules(&r.ruleset)?})]
                } else {
                    self.eg.run_program(vec![Command::RunSchedule(s.clone())])?
                };
                let seconds = start.elapsed().as_secs_f64();
                let totals = self.native_stats.entry(r.ruleset.clone()).or_default();
                totals.runs += 1;
                totals.seconds += seconds;
                let mut changed = false;
                for o in outputs {
                    if let egglog::CommandOutput::RunSchedule(report) = o {
                        changed |= report.updated;
                        totals.search_apply_seconds += report
                            .search_and_apply_time_per_ruleset
                            .values()
                            .map(|d| d.as_secs_f64())
                            .sum::<f64>();
                        totals.insert_merge_seconds += report
                            .merge_time_per_ruleset
                            .values()
                            .map(|d| d.as_secs_f64())
                            .sum::<f64>();
                        totals.rebuild_seconds += report
                            .rebuild_time_per_ruleset
                            .values()
                            .map(|d| d.as_secs_f64())
                            .sum::<f64>();
                        totals.matches += report.num_matches_per_rule.values().sum::<usize>();
                    }
                }
                self.stats.native_only_runs += 1;
                self.stats.native_only_seconds += seconds;
                self.stats.ruleset_runs += 1;
                let start = Instant::now();
                self.collect_commits()?;
                self.stats.event_seconds += start.elapsed().as_secs_f64();
                Ok(changed)
            }
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
                self.stats.saturation_boundaries += 1;
                let handles=self.saturation_hook.as_ref().is_some_and(|h|h.handles(s));
                if let Some(hook) = self.saturation_hook.as_mut().filter(|_|handles) {
                    let start = Instant::now();
                    let result = hook.try_boundary(&mut self.eg, s)?;
                    self.stats.saturation_hook_seconds += start.elapsed().as_secs_f64();
                    if let Some(r) = result {
                        let complete = r["complete"]
                            .as_bool()
                            .ok_or("missing saturation completion proof")?;
                        changed = r["updated"]
                            .as_bool()
                            .ok_or("missing saturation effect status")?;
                        self.last_saturation = r;
                        // Every macro effect must reach the ordinary shared
                        // table mirror before fallback or any later consumer.
                        let events_started = Instant::now();
                        self.collect_commits()?;
                        self.stats.event_seconds += events_started.elapsed().as_secs_f64();
                        if complete {
                            let sets = if let Some(xs) = self.last_saturation["rulesets"].as_array() {
                                xs.iter()
                                    .map(|s| s.as_str().ok_or("invalid certified ruleset"))
                                    .collect::<std::result::Result<Vec<_>, _>>()?
                            } else {
                                vec![self.last_saturation["ruleset"].as_str().ok_or("missing certified ruleset")?]
                            };
                            for set in sets {
                                let ids = self.sets.get(set).ok_or("unknown certified ruleset")?;
                                if self.cpu || self.audit {
                                    for id in ids {
                                        self.eg.sync_cell_query_after_saturation(
                                            &self.sinks[*id],
                                            &self.reference[*id],
                                        );
                                    }
                                }
                            }
                            self.stats.skipped_saturations += 1;
                            return Ok(changed);
                        }
                        self.stats.partial_saturations += 1;
                    }
                }
                loop {
                    let c = self.schedule(s)?;
                    changed |= c;
                    if !c {
                        break;
                    }
                }
                if let Some(hook) = self.saturation_hook.as_mut().filter(|_|handles) {
                    hook.after_boundary(&mut self.eg, s)?;
                    self.collect_commits()?;
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
    /// Execute one original schedule boundary and expose its public change flag
    /// for lockstep diagnostics. Keeps the same failure and timing semantics.
    pub fn run_schedule(&mut self, schedule: &Schedule) -> Result<bool> {
        if self.failed {
            return Err("failed shared runtime cannot resume".into());
        }
        let start = Instant::now();
        let result = self.schedule(schedule);
        self.stats.run_seconds += start.elapsed().as_secs_f64();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn report(&self) -> Json {
        json!({"runtime":self.stats,"native_only_rulesets":self.native_stats,"saturation_hook":self.last_saturation,"saturation_diagnostics":self.saturation_hook.as_ref().map(|h|h.diagnostics()),"audit":self.audit,"backend":if self.cpu{"native-sorted-commit"}else{"shared-gpu"},"cells":self.cell_evidence,"cell_work":self.cell_work,"device":self.last_device,"shared_tables":self.keys.len(),"live_rows":self.live,"row_slots":self.rows.len(),"extended_queries":self.extended,"experimental_cell_run_programs":self.experimental_cell_runs,"selected_rule_count":self.sinks.len(),"ruleset_memberships":self.sets,"rules":self.sinks.iter().enumerate().map(|(id,sink)|json!({"ruleset":self.owners[id],"rule":sink.name,"stats":self.qstats[id]})).collect::<Vec<_>>(),"scope":"Native commit deltas drive analyzed Cell interfaces. Optional certified whole-closure handler runs only at explicit saturation boundaries; unknown contexts retain complete queries. No virtual storage."})
    }
}
