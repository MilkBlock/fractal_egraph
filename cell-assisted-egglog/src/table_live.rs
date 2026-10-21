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
    pub gpu_maintenance_seconds: Option<f64>,
    pub gpu_matching_seconds: Option<f64>,
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
    pub cell_query_rules: usize,
    pub cell_query_bindings: usize,
    pub cell_query_seconds: f64,
    pub shared_query_requests: usize,
    pub shared_query_bindings: usize,
    pub native_only_runs: usize,
    pub native_only_seconds: f64,
}
#[derive(Default, Serialize)]
struct QueryCost {
    runs: usize,
    bindings: usize,
    cell_bindings: usize,
    shared_bindings: usize,
    query_seconds: f64,
    device_seconds: f64,
    maintenance_seconds: f64,
    matching_seconds: f64,
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
/// Trusted Cell provider with separate query and closure contracts.
/// `query_cells` is read-only and preserves an ordinary Run's commit boundary.
/// `try_boundary` may publish a certified closure only at an explicit Saturate;
/// `complete=false` requires the ordinary backend to finish that saturation.
pub trait SaturationHook {
    /// Complete incremental bindings for a subset of these exact rule sinks.
    /// Reads only: no RHS execution, frontier advancement, or saturation here.
    /// Returned indices refer to `sinks`; unclaimed rules retain their matcher.
    fn query_cells(&mut self,_eg:&mut EGraph,_set:&str,_sinks:&[&CellActionSink])->Result<Vec<(usize,Vec<Vec<Value>>) >> {Ok(vec![])}
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
    cell_queries: usize,
    cell_bindings: usize,
    shared_queries: usize,
    shared_bindings: usize,
    eligible_seeds:u64,
    refuted_seeds:u64,
    productive_seeds:u64,
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
    sharing:crate::query_sharing::Queries,
    share_queries:bool,
    reference: Vec<CellQuery>,
    sets: BTreeMap<String, Vec<usize>>,
    native_sets: BTreeSet<String>,
    native_stats: BTreeMap<String, NativeStats>,
    query_costs: BTreeMap<String, QueryCost>,
    qstats: Vec<QueryStats>,
    owners: Vec<String>,
    extended: bool,
    query_log: bool,
    query_timing: bool,
    cell_queries_enabled: bool,
    audit_cell_queries: bool,
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
    /// Reuse a certified complete query frontier without changing any effect.
    pub fn set_query_sharing(&mut self,enabled:bool){self.share_queries=enabled;}
    pub fn set_cell_query_reuse(&mut self, enabled: bool) {self.cell_queries_enabled=enabled;}
    /// Diagnose only Cell-supplied frontiers with cached native queries.
    /// This is an oracle run, never a timing result.
    pub fn enable_cell_query_audit(&mut self)->Result<()> {
        if self.reference.is_empty(){for s in &self.sinks {self.reference.push(self.eg.prepare_cell_query(s)?);}}
        self.audit_cell_queries=true;Ok(())
    }
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
        Self::create(eg,commands,worker,audit,Options {query_routing:QueryRouting::TableAnchors,profile:std::env::var("CELL_SEARCH_COUNTERS").as_deref()==Ok("1"),..Options::default()},sets.as_ref(),true)
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
        let sharing=crate::query_sharing::Queries::compile(&sinks);
        let mut runtime = Self {
            eg,
            sinks,
            sharing,
            share_queries:std::env::var("CELL_SHARE_IDENTICAL_QUERIES").as_deref()==Ok("1"),
            reference,
            sets,
            native_sets,
            native_stats: BTreeMap::new(),
            query_costs:BTreeMap::new(),
            qstats: vec![],
            owners,
            extended,
            query_log: std::env::var_os("CELL_QUERY_LOG").is_some(),
            query_timing:std::env::var("CELL_QUERY_TIMINGS").as_deref()==Ok("1"),
            cell_queries_enabled:extended && std::env::var("CELL_DISABLE_CELL_QUERIES").as_deref()!=Ok("1"),
            audit_cell_queries:false,
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
                matches!(options.query_routing,QueryRouting::TableAnchors),
            )?;
            config["phase_timing"]=json!(runtime.query_timing);
            config["port_locality"] = json!(options.port_locality as u32);
            config["query_routing"] = json!(options.query_routing as u32);
            runtime.cell_evidence = json!({"refutation_signatures":config["refutation_signatures"],"refutation_domains":config["refutation_domains"],"domain_checks":config["domain_checks"],"domain_summaries":config["domain_tables"].as_array().map(Vec::len),"refutation":config["refutation"],"refutation_checks":config["refutation_checks"],"staged_constraints":config["staged_constraints"],"compact_binding_workspace":config["compact_binding_workspace"]==true && matches!(options.query_routing,QueryRouting::TableAnchors),"library":cells.as_ref().map(|c|&c.evidence),"plans":config["cell_evidence"],"profile":options.profile,"plan_only":options.plan_only,"cache_only":options.cache_only,"port_locality":options.port_locality as u32,"query_routing":options.query_routing as u32,"port_join_groups":config["port_join_groups"]});
            let mut device = Service::new(worker)?;
            let installed = device.family_init(config)?;
            if runtime.query_timing && installed["phase_timing"]!=true {return Err("worker does not support phase timing; rebuild matrix.swift".into())}
            if installed["family_layout"] != json!(13) {
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
                // Resolve table names once for the batch, not for every row.
                let remap = names.iter().map(|n| self.hook_table_ids.get(n).copied()).collect::<Vec<_>>();
                for mut b in batches {
                    b.rows.retain_mut(|row| {
                        if let Some(i) = remap[row.table] { row.table = i; true } else { false }
                    });
                    b.clears.retain_mut(|t| {
                        if let Some(i) = remap[*t] { *t = i; true } else { false }
                    });
                    // This feed has its own union cursor; count those from the
                    // native batch below once, not again from the observer.
                    b.unions.clear();
                    self.ingest(b)?;
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
    fn gpu_matches(&mut self,ids:&[usize])->Result<Vec<Vec<Vec<Value>>>> {
        if !self.share_queries{return self.gpu_matches_unshared(ids)}
        let mut groups=BTreeMap::new();let mut leaders=vec![];let mut source=vec![];
        for id in ids {
            let key=(self.sharing.groups[*id],self.eg.cell_query_watermark(&self.sinks[*id]));
            let next=leaders.len();let slot=*groups.entry(key).or_insert_with(||{leaders.push(*id);next});source.push(slot);
        }
        let mut result=self.gpu_matches_unshared(&leaders)?.into_iter().map(Some).collect::<Vec<_>>();
        let mut outputs=vec![vec![];ids.len()];
        // Copy followers before moving the leader's buffer. No result survives
        // this native Run, and no consumer's action or cursor is omitted.
        for (i,id) in ids.iter().enumerate(){let slot=source[i];let leader=leaders[slot];
            if *id!=leader {
                outputs[i]=self.sharing.transport(leader,*id,result[slot].as_ref().unwrap());
                outputs[i].sort_unstable();
                self.qstats[*id].shared_queries+=1;self.qstats[*id].shared_bindings+=outputs[i].len();
                self.stats.shared_query_requests+=1;self.stats.shared_query_bindings+=outputs[i].len();
            }
        }
        for (i,id) in ids.iter().enumerate(){if *id==leaders[source[i]]{outputs[i]=result[source[i]].take().unwrap();}}
        Ok(outputs)
    }
    fn gpu_matches_unshared(&mut self, ids: &[usize]) -> Result<Vec<Vec<Vec<Value>>>> {
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
        let mut words = Vec::with_capacity(4 + 4*active.len() + self.pending.len()*(5 + 3*self.columns));
        words.extend([
            u32::try_from(self.rows.len())?,
            u32::try_from(self.pending.len())?,
            u32::try_from(active.len())?,
            u32::try_from(self.epoch + 1)?,
        ]);
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
        if self.query_timing {
            *self.stats.gpu_maintenance_seconds.get_or_insert(0.0)+=r["gpu_maintenance_seconds"].as_f64().ok_or("missing maintenance timer")?;
            *self.stats.gpu_matching_seconds.get_or_insert(0.0)+=r["gpu_matching_seconds"].as_f64().ok_or("missing matching timer")?;
        }
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
        if let Some(work) = r["cell_work"].as_object() {
            for (k, v) in work {
                *self.cell_work.entry(k.clone()).or_default() +=
                    v.as_u64().ok_or("invalid Cell work counter")?;
            }
        }
        if let Some(work)=r["query_seed_work"].as_array() {
            for row in work {
                let row=row.as_array().ok_or("invalid query seed profile")?;
                if row.len()!=4{return Err("invalid query seed profile width".into());}
                let id=row[0].as_u64().ok_or("missing seed query")? as usize;
                if !active.contains(&id){return Err("inactive seed query".into());}
                let a=row[1].as_u64().ok_or("missing seed total")?;
                let b=row[2].as_u64().ok_or("missing seed rejections")?;
                let c=row[3].as_u64().ok_or("missing productive seeds")?;
                if b+c>a{return Err("invalid seed outcomes".into());}
                self.qstats[id].eligible_seeds+=a;
                self.qstats[id].refuted_seeds+=b;
                self.qstats[id].productive_seeds+=c;
            }
        }
        let t = Instant::now();
        let count = r["bindings"].as_u64().ok_or("missing binding count")? as usize;
        let compact=r["compact_bindings"].as_bool().unwrap_or(false);
        if !compact && count.checked_mul(1 + self.slots) != Some(body.len()) {
            return Err("family result shape".into());
        }
        self.stats.output_high_water = self.stats.output_high_water.max(count);
        let mut positions = vec![usize::MAX; self.sinks.len()];
        for (i, id) in ids.iter().enumerate() {
            if active.contains(id) { positions[*id] = i; }
        }
        let (mut offset,mut decoded)=(0,0);
        while offset<body.len(){
            let q = body[offset] as usize;
            let i = *positions.get(q).filter(|i| **i != usize::MAX)
                .ok_or("result for inactive or foreign query")?;
            let n = self.sinks[q].slot_count();
            let width=if compact{1+n}else{1+self.slots};
            let record=body.get(offset..offset+width).ok_or("truncated compact binding")?;
            matches[i].push(record[1..1 + n].iter().map(|v| Value::new(*v)).collect());
            offset+=width;decoded+=1;
        }
        if decoded!=count{return Err("compact binding count differs".into())}
        for (id, bindings) in ids.iter().zip(&mut matches) {
            bindings.sort_unstable();
            if let Some(pair) = bindings.windows(2).find(|p| p[0] == p[1]) {
                return Err(format!(
                    "duplicate family binding for {}: {:?} among {} rows; device counters {}",
                    self.sinks[*id].name,
                    pair[0],
                    bindings.len(),
                    r["cell_work"]
                )
                .into());
            }
        }
        self.last_device = r;
        self.stats.decode_seconds += t.elapsed().as_secs_f64();
        Ok(matches)
    }
    fn run_set(&mut self, set: &str) -> Result<bool> {
        let query_start=Instant::now();
        if self.query_log {eprintln!("[query start] {} {}",self.stats.ruleset_runs,set);}
        let ids = self.sets.get(set).ok_or("unregistered ruleset")?.clone();
        // Cell plans supply bindings only. All rules still read this frozen
        // state and use the SAME sorted native commit, including residual rules.
        let started=Instant::now();
        let supplied=if self.cell_queries_enabled && self.saturation_hook.is_some() {
            let sinks=ids.iter().map(|i|&self.sinks[*i]).collect::<Vec<_>>();
            if let Some(hook)=self.saturation_hook.as_mut(){hook.query_cells(&mut self.eg,set,&sinks)?}else{vec![]}
        }else{vec![]};
        self.stats.cell_query_seconds+=started.elapsed().as_secs_f64();
        let mut matches=vec![vec![];ids.len()];let mut provided=BTreeSet::new();
        for (index,mut bindings) in supplied {
            if index>=ids.len() || !provided.insert(index){return Err("invalid or duplicate Cell query index".into())}
            let id=ids[index];bindings.sort_unstable();
            if bindings.windows(2).any(|p|p[0]==p[1]){return Err("duplicate Cell query binding".into())}
            self.qstats[id].cell_queries+=1;self.qstats[id].cell_bindings+=bindings.len();
            self.stats.cell_query_rules+=1;self.stats.cell_query_bindings+=bindings.len();
            matches[index]=bindings;
        }
        let remaining=ids.iter().enumerate().filter(|(i,_)|!provided.contains(i)).map(|(i,id)|(i,*id)).collect::<Vec<_>>();
        let pending=remaining.iter().map(|(_,id)|*id).collect::<Vec<_>>();
        let started=self.query_timing.then(Instant::now);let device_before=self.stats.device_seconds;
        let shared_before=self.stats.shared_query_bindings;
        let maintenance_before=self.stats.gpu_maintenance_seconds.unwrap_or(0.0);
        let matching_before=self.stats.gpu_matching_seconds.unwrap_or(0.0);
        let ordinary=if pending.is_empty(){vec![]}else if self.cpu {
            let t=Instant::now();let r=self.native_matches(&pending)?;
            self.stats.native_query_seconds+=t.elapsed().as_secs_f64();r
        }else{self.gpu_matches(&pending)?};
        for ((index,_),bindings) in remaining.into_iter().zip(ordinary){matches[index]=bindings;}
        if let Some(started)=started {
            let cost=self.query_costs.entry(set.to_string()).or_default();
            cost.runs+=1;cost.bindings+=matches.iter().map(Vec::len).sum::<usize>();
            cost.cell_bindings+=provided.iter().map(|i|matches[*i].len()).sum::<usize>();
            cost.shared_bindings+=self.stats.shared_query_bindings-shared_before;
            cost.query_seconds+=started.elapsed().as_secs_f64();cost.device_seconds+=self.stats.device_seconds-device_before;
            cost.maintenance_seconds+=self.stats.gpu_maintenance_seconds.unwrap_or(0.0)-maintenance_before;
            cost.matching_seconds+=self.stats.gpu_matching_seconds.unwrap_or(0.0)-matching_before;
        }
        if self.audit || self.audit_cell_queries {
            let t = Instant::now();
            let audited=ids.iter().enumerate().filter(|(i,_)|self.audit || provided.contains(i)).map(|(i,id)|(i,*id)).collect::<Vec<_>>();
            let expected = self.native_matches(&audited.iter().map(|(_,id)|*id).collect::<Vec<_>>())?;
            for ((i,id), b) in audited.iter().zip(&expected) {
                let a=&matches[*i];
                if a != b {
                    return Err(format!(
                        "independent native frontier differs for {}: {} assisted / {} native",
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
        if !self.audit && (self.cpu || self.audit_cell_queries) {
            for (i,id) in ids.iter().enumerate() {
                // Cell-only audit must also acknowledge GPU fallback runs,
                // otherwise re-enabling the Cell would audit an obsolete delta.
                if (self.cpu && !self.audit_cell_queries && provided.contains(&i)) || (self.audit_cell_queries && !provided.contains(&i)) {
                    self.eg.sync_cell_query_after_native_run(&self.sinks[*id],&self.reference[*id]);
                }
            }
        }
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
                                if self.cpu || self.audit || self.audit_cell_queries {
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
        json!({"runtime":self.stats,"native_only_rulesets":self.native_stats,"query_costs_by_ruleset":self.query_costs,"query_timing_profile":self.query_timing,"saturation_hook":self.last_saturation,"saturation_diagnostics":self.saturation_hook.as_ref().map(|h|h.diagnostics()),"audit":self.audit||self.audit_cell_queries,"audit_scope":if self.audit{"all queries"}else if self.audit_cell_queries{"Cell-supplied queries"}else{"none"},"backend":if self.cpu{"native-sorted-commit"}else{"shared-gpu"},"cells":self.cell_evidence,"cell_work":self.cell_work,"device":self.last_device,"shared_tables":self.keys.len(),"live_rows":self.live,"row_slots":self.rows.len(),"extended_queries":self.extended,"cell_query_reuse":self.cell_queries_enabled,"selected_rule_count":self.sinks.len(),"shared_query_groups":self.sharing.group_count,"query_sharing":self.share_queries,"ruleset_memberships":self.sets,"rules":self.sinks.iter().enumerate().map(|(id,sink)|json!({"ruleset":self.owners[id],"rule":sink.name,"stats":self.qstats[id]})).collect::<Vec<_>>(),"scope":"Native commit deltas drive analyzed Cell interfaces. Optional certified whole-closure handler runs only at explicit saturation boundaries; unknown contexts retain complete queries. No virtual storage."})
    }
}
