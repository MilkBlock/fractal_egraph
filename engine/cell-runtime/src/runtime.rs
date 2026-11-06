#[cfg(feature = "family-frontiers")]
#[path = "family_frontier.rs"]
mod family_frontier;
#[cfg(feature = "family-replay")]
#[path = "family_replay.rs"]
mod family_replay;
#[cfg(feature = "family-scan")]
#[path = "family_scan.rs"]
mod family_scan;
use crate::query::QueryIndexes;
#[path = "free_join.rs"]
mod free_join;
#[cfg(feature = "view-consumers")]
#[path = "view_exec.rs"]
mod view_exec;
use free_join::{Domains, FreeJoin, Subtrie};
#[path = "execute_kernel.rs"]
mod execute_kernel;
#[path = "ready_sets.rs"]
mod ready_sets;
#[cfg(feature="sparse-cells")]
#[path = "point_ports.rs"]
mod point_ports;
use crate::known_results::QueryFacts;
use crate::schedule::{self, Task};
use crate::{
    graph::*,
    program::*,
    value::{self, Value, canon},
};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Instant;

#[derive(Debug)]
struct Binding {
    #[cfg(feature = "view-consumers")]
    view_source: Option<usize>,
    #[cfg(feature = "iterative-bake")]
    direct: Option<crate::stage_spine::DirectOrigin>,
    known: QueryFacts,

    values: crate::slots::Slots,
    epoch: u64,
    parent: Option<FactRef>,
    continuation: Option<(usize, u32, bool)>,
    compiled: bool,
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct ContinuationStats {
    merge_context_entries: u64,
    merge_port_reads: u64,
    merge_candidate_rows: u64,
    merge_guard_checks: u64,
    merge_bindings: u64,
    merge_bindings_by_rule: BTreeMap<usize, u64>,
    point_reads: u64,
    inverse_reads: u64,
    inverse_ambiguous: u64,
    point_primitives: u64,
    compiled_bindings: u64,
    entered: u64,
    projected_atoms: u64,
    local_effect_reads: u64,
    keyed_reads: u64,
    ambiguous_reads: u64,
    unavailable_sources: u64,
    complete_bindings: u64,
    residual_bindings: u64,
    published_facts: u64,
    function_updates: u64,
    max_depth: u32,
}
/// All bindings belong to this exact source Run occurrence. The patch is sealed
/// before publication, so no sibling rule can observe its future effects.
struct StagePatch {
    groups: Vec<(usize, Vec<Binding>)>,
    len: usize,
}
impl StagePatch {
    fn iter(&self) -> impl Iterator<Item = (&usize, &Binding)> {
        self.groups.iter().flat_map(|(rid, rows)| rows.iter().map(move |row| (rid, row)))
    }
}
pub struct Executor {
    #[cfg(feature = "sparse-cells")]
    work_scratch: Vec<(usize, Vec<Binding>)>,
    #[cfg(feature = "rhs-locals")]
    local_width: usize,
    #[cfg(feature = "rhs-locals")]
    local_selection: Option<Vec<bool>>,
    #[cfg(feature = "rhs-locals")]
    local_counts: crate::local_rhs::Counts,
    #[cfg(feature = "rhs-work-audit")]
    ledger: crate::rhs_work::Ledger,
    #[cfg(feature = "family-scan")]
    raw_scan: family_scan::Scan,
    #[cfg(feature = "family-frontiers")]
    fronts: family_frontier::Trace,
    #[cfg(feature = "family-frontiers")]
    prior_union: bool,
    #[cfg(feature = "family-survey")]
    survey: crate::family_survey::Survey,
    #[cfg(feature = "family-replay")]
    replay: family_replay::Replay,
    #[cfg(feature = "family-overlap")]
    overlap: crate::family_overlap::Overlap,
    #[cfg(feature = "view-consumers")]
    blocks: view_exec::Blocks,
    publication: crate::change_batch::Publication,
    #[cfg(feature="scheduled-functions")]
    scheduled_functions:crate::scheduled_functions::State,
    range_index: Option<crate::range_index::RangeIndex>,

    dedup_rows: u64,
    dedup_full_elements: u64,
    dedup_elements: u64,
    dedup_duplicates: u64,
    stage_spine: crate::stage_spine::Programs,
    entry_schedule: crate::entry_schedule::Scheduler,
    current_stage: String,
    neighbor: crate::neighbor::Context,
    local_context: crate::local_context::Contexts,
    pub graph: Graph,
    pub steps: Vec<schedule::RunStep>,
    last_change:usize,
    watermarks: Vec<u64>,
    dirty: Vec<bool>,
    pending: Vec<BTreeSet<Dependency>>,
    readers: Vec<Vec<usize>>,
    required_tables: Vec<Vec<usize>>,
    ready_sets: Option<ready_sets::ReadySets>,
    pub audit: bool,
    pub snapshots: Vec<J>,
    pub binding_audit: Vec<J>,
    pub initial_audit: Option<J>,
    pub tasks: Vec<Task>,

    #[cfg(feature = "sparse-cells")]
    sum_domains: rustc_hash::FxHashMap<(usize, usize), execute_kernel::SumDomain>,
    #[cfg(feature="sparse-cells")]
    point_ports: point_ports::PointPorts,
    query_indexes: QueryIndexes,
    free_join: FreeJoin,
    sieve: crate::sieve::Sieve,
    profile_queries: bool,
    query_profile: Vec<(u64, f64, u64, u64)>,
    progress: bool,

    continuation_stats: ContinuationStats,
    handoffs: BTreeSet<(usize, usize)>,
}
// Keep the ordinary lane's rows owned: optional shared entry caches must not
// introduce reference-counted heap allocations when the feature is disabled.
enum SeedRow {
    Owned(Row),
    Shared(Rc<Row>),
}
impl std::ops::Deref for SeedRow {
    type Target = Row;
    fn deref(&self) -> &Row {
        match self {
            Self::Owned(r) => r,
            Self::Shared(r) => r,
        }
    }
}
/// Run-local binding workspace. Only successful matches become stored bindings;
/// failed join branches undo slot assignments instead of cloning tree maps.
struct QueryEnv {
    known: QueryFacts,
    values: crate::entry_schedule::Inputs,
    trail: Vec<usize>,
    domains: Domains,
}
impl QueryEnv {
    fn new(slots: usize, known: QueryFacts) -> Self {
        Self {
            known,

            values: crate::entry_schedule::Inputs::owned(slots),
            trail: vec![],
            domains: Domains::new(),
        }
    }
    fn shared(seed: crate::entry_schedule::Seed, known: QueryFacts) -> Self {
        Self {
            known,
            values: crate::entry_schedule::Inputs::shared(seed),
            trail: vec![],
            domains: Domains::new(),
        }
    }
    fn domain(&self, atom: usize) -> Option<&Rc<Vec<Dependency>>> {
        self.domains
            .iter()
            .rev()
            .find(|d| d.atom == atom)
            .map(|d| &d.rows)
    }
    fn record(&mut self, atom: usize, witness: Option<Dependency>) {
        self.known.record(atom, witness);
    }
    fn rollback(&mut self, mark: usize) {
        while self.trail.len() > mark {
            self.values.set(self.trail.pop().unwrap(), None);
        }
    }
    fn binding(&self) -> crate::slots::Slots {
        self.values
            .refs()
            .enumerate()
            .filter_map(|(i, v)| v.cloned().map(|v| (i, v)))
            .collect()
    }
}
fn operand(a: &Arg, env: &QueryEnv, g: &Graph) -> Option<Value> {
    match a {
        Arg::Literal(v) => Some(canon(v, &g.parents)),
        Arg::Slot(i) => env.values.get(*i).cloned(),
    }
}
fn bind(a: &Arg, v: &Value, env: &mut QueryEnv, g: &Graph) -> bool {
    // The graph is immutable throughout matching, so assigned values stay canonical.
    // All callers provide a canonical row, local slot, or primitive output.
    // No writes are visible during this immutable matching snapshot.
    match a {
        Arg::Literal(l) => canon(l, &g.parents) == *v,
        Arg::Slot(i) => {
            if let Some(old) = env.values.get(*i) {
                old == v
            } else {
                env.values.set(*i, Some(v.clone()));
                env.trail.push(*i);
                true
            }
        }
    }
}
impl Executor {
    #[cfg(feature="scheduled-functions")]
    pub fn configure_scheduled_functions(&mut self,on:bool)->Result<()> {
        if !self.steps.is_empty() {return Err("configure scheduled functions before execution".into())}
        if on && cfg!(any(feature="projection-semijoin",feature="family-replay")) {
            return Err("scheduled functions do not yet cover auxiliary publication observers".into())
        }
        self.scheduled_functions.enabled=on;
        Ok(())
    }
    #[cfg(feature="scheduled-functions")]
    pub fn scheduled_functions_report(&self)->J {self.scheduled_functions.report(&self.graph.program)}
    #[cfg(feature="scheduled-functions")]
    fn activate_function_readers(&mut self, rules: &[usize], probe: bool) -> Result<()> {
        if !self.scheduled_functions.has_work() { return Ok(()); }
        let mut start = None;
        for &rid in rules {
            if !self.scheduled_functions.reads(rid).iter().any(|&t|self.scheduled_functions.has_pending(t)) {
                continue;
            }
            if (!probe && self.graph.program.rules[rid].seminaive && !self.dirty[rid])
                || self.required_tables[rid].iter().any(|&t|self.graph.witness_count(t)==0) {
                continue;
            }
            for at in 0..self.scheduled_functions.reads(rid).len() {
                let t = self.scheduled_functions.reads(rid)[at];
                if !self.scheduled_functions.has_pending(t) { continue; }
                start.get_or_insert_with(Instant::now);
                let ids = self.scheduled_functions.take(t);
                #[cfg(feature="view-consumers")]
                let dedicated = self.blocks.dedicated(t);
                #[cfg(not(feature="view-consumers"))]
                let dedicated = false;
                if !dedicated {
                    self.entry_schedule.publish_witnesses(t,ids.iter().copied().map(Dependency::Function));
                }
                let retired = self.publication.activate_columns(
                    &self.graph,t,&ids,&mut self.query_indexes,
                    self.range_index.as_mut(),&mut self.neighbor,
                )?;
                self.scheduled_functions.retired(retired);
                self.scheduled_functions.recycle(t,ids);
            }
        }
        if let Some(start)=start {
            // Apply every changed dependency before exposing any necessary
            // condition. No intermediate, unobserved value is required here.
            self.sieve.publish(&mut self.query_indexes);
            self.scheduled_functions.activated(start.elapsed().as_secs_f64());
        }
        Ok(())
    }
    #[cfg(feature = "iterative-bake")]
    pub fn collect_iterative_pairs(&mut self) {
        self.stage_spine.training.collect_pairs = true;
    }
    #[cfg(feature = "iterative-bake")]
    pub fn iterative_training(&self) -> &crate::iterative::Training {
        &self.stage_spine.training
    }

    #[cfg(feature = "spine-shapes")]
    pub fn spine_shape_report(&self) -> J {
        let mut report = self.stage_spine.shape_report();
        report["program_sha256"] = self.graph.program.artifact["sha256"].clone();
        report
    }

    pub fn new(mut graph: Graph, audit: bool) -> Self {
        let mut readers = vec![vec![]; graph.program.tables.len()];
        let mut required_tables = Vec::with_capacity(graph.program.rules.len());
        for (rid, rule) in graph.program.rules.iter().enumerate() {
            let mut required = Vec::new();
            for atom in rule.query.iter() {
                if let Call::Table(table) = atom.call
                    && readers[table].last() != Some(&rid)
                {
                    readers[table].push(rid);
                    required.push(table);
                }
            }
            required.sort_by_key(|&table| graph.witness_count(table));
            required_tables.push(required);
        }
        let n = graph.program.rules.len();
        let tasks = schedule::initial(&graph.program.control);
        let initial_audit = if audit { graph.export().ok() } else { None };
        let sieve = crate::sieve::Sieve::new(&graph.program);
        let mut query_indexes = QueryIndexes::default();
        query_indexes.track_changes = sieve.enabled;
        let mut stage_spine = crate::stage_spine::Programs::new(&graph.program);
        stage_spine.audit_receipts = audit;
        #[cfg(feature = "sparse-cells")]
        {
            stage_spine.compiled_commits = true;
        }
        #[cfg(feature = "sparse-cells")]
        {
            stage_spine.direct_sealed_answers = true;
        }
        let free_join = FreeJoin::new(&graph.program);
        let entry_schedule = crate::entry_schedule::Scheduler::with_epoch_counts(
            &graph.program,
            true,
            free_join.planned,
        );
        graph.receipt_tracking =
            (stage_spine.enabled && stage_spine.receipts) || entry_schedule.enabled;
        let mut e = Self {
            #[cfg(feature = "sparse-cells")]
            work_scratch: vec![],
            #[cfg(feature = "rhs-locals")]
            local_width: 0,
            #[cfg(feature = "rhs-locals")]
            local_selection: None,
            #[cfg(feature = "rhs-locals")]
            local_counts: Default::default(),
            #[cfg(feature = "rhs-work-audit")]
            ledger: Default::default(),
            #[cfg(feature = "family-scan")]
            raw_scan: family_scan::Scan::new(&graph.program),
            #[cfg(feature = "family-frontiers")]
            fronts: family_frontier::Trace::new(&graph.program, &sieve),
            #[cfg(feature = "family-frontiers")]
            prior_union: false,
            #[cfg(feature = "family-survey")]
            survey: crate::family_survey::Survey::new(&graph.program),
            #[cfg(feature = "family-replay")]
            replay: family_replay::Replay::new(&graph.program),
            #[cfg(feature = "family-overlap")]
            overlap: Default::default(),
            #[cfg(feature = "view-consumers")]
            blocks: view_exec::Blocks::new(&graph),
            publication: Default::default(),
            #[cfg(feature="scheduled-functions")]
            scheduled_functions:crate::scheduled_functions::State::new(&graph.program),
            range_index: Some(Default::default()),

            dedup_rows: 0,
            dedup_full_elements: 0,
            dedup_elements: 0,
            dedup_duplicates: 0,
            free_join,
            stage_spine,
            entry_schedule,
            current_stage: String::new(),
            neighbor: crate::neighbor::Context::new(
                &graph.program,
                std::env::var_os("CELL_CPU_PROFILE").is_some(),
            ),
            local_context: crate::local_context::Contexts::new(&graph.program),
            #[cfg(feature="sparse-cells")]
            point_ports: point_ports::PointPorts::new(&graph.program),
            ready_sets: (cfg!(feature="sparse-cells") && !cfg!(any(feature="family-survey", feature="family-overlap", feature="structural-depth"))
                && std::env::var_os("CELL_CPU_PROFILE").is_none()).then(|| ready_sets::ReadySets::new(&graph.program.sets, graph.program.rules.len())),
            graph,
            steps: vec![],
            last_change:0,
            watermarks: vec![0; n],
            dirty: vec![true; n],
            pending: vec![BTreeSet::new(); n],
            readers,
            required_tables,
            audit,
            snapshots: vec![],
            binding_audit: vec![],
            initial_audit,
            tasks,

            #[cfg(feature = "sparse-cells")]
            sum_domains: Default::default(),
            query_indexes,
            sieve,
            profile_queries: std::env::var_os("CELL_CPU_PROFILE").is_some(),
            query_profile: vec![(0, 0.0, 0, 0); n],
            progress: std::env::var_os("CELL_CPU_PROGRESS").is_some(),

            continuation_stats: ContinuationStats::default(),
            handoffs: BTreeSet::new(),
        };
        e.entry_schedule.publish(&e.graph);
        e.deliver();
        e.graph.changes.clear();
        e
    }
    fn deliver(&mut self) {
        use crate::change_batch::Change;
        let batch = &self.graph.changes;
        self.graph.stats.delivery_events += batch.events;
        self.graph.stats.distinct_delivery_events += batch.distinct_events;
        for (table, changes) in batch.tables() {
            if !changes.values().any(|c| *c == Change::Wake) {
                continue;
            }
            for &rid in &self.readers[table] {
                #[cfg(feature = "rhs-work-audit")]
                self.ledger.wake();
                if !self.dirty[rid] {
                    self.dirty[rid] = true;
                    if let Some(sets)=&mut self.ready_sets { sets.wake(rid); }
                }
                if !self.entry_schedule.accepts(rid) {
                    self.pending[rid].extend(
                        changes
                            .iter()
                            .filter_map(|(w, c)| (*c == Change::Wake).then_some(*w)),
                    );
                }
            }
        }
    }
    fn solve(
        &mut self,
        query: &[Atom],
        rid: usize,
        remaining: &mut [bool],
        env: &mut QueryEnv,
        epoch: u64,
        out: &mut Vec<Binding>,
        delta: Option<(usize, u64, Dependency)>,
    ) -> Result<()> {
        if !remaining.iter().any(|pending| *pending) {
            out.push(Binding {
                #[cfg(feature = "view-consumers")]
                view_source: None,
                #[cfg(feature = "iterative-bake")]
                direct: None,
                known: env.known.clone(),
                values: env.binding(),
                epoch,
                parent: None,
                continuation: None,
                compiled: false,
            });
            return Ok(());
        }
        // Primitive and complete-key atoms outrank every partial domain.
        // Decide those two ranks from slot availability before touching any
        // index or allocating values for a losing partial atom.
        let mut exact = None;
        for (i, atom) in query.iter().enumerate().filter(|(i, _)| remaining[*i]) {
            let bound = atom.args[..atom.args.len() - 1]
                .iter()
                .all(|arg| match arg {
                    Arg::Literal(_) => true,
                    Arg::Slot(slot) => env.values.get(*slot).is_some(),
                });
            if bound {
                if matches!(atom.call, Call::Primitive(..)) {
                    exact = Some(i);
                    break;
                }
                if exact.is_none() {
                    exact = Some(i);
                }
            }
        }
        let chosen = if let Some(i) = exact {
            i
        } else {
            let mut best = None;
            for (i, atom) in query.iter().enumerate().filter(|(i, _)| remaining[*i]) {
                let Call::Table(table) = atom.call else {
                    continue;
                };
                let bound: smallvec::SmallVec<[Option<Value>; 6]> = atom
                    .args
                    .iter()
                    .map(|a| operand(a, env, &self.graph))
                    .collect();
                let size = self.query_indexes.count(&self.graph, table, &bound)?;
                let cost = env.domain(i).map_or(size, |rows| size.min(rows.len()));
                if best.is_none_or(|previous| (cost, i) < previous) {
                    best = Some((cost, i));
                }
                if cost == 0 {
                    break;
                }
            }
            best.ok_or("unbound primitive query cycle")?.1
        };
        remaining[chosen] = false;
        let atom = &query[chosen];
        match &atom.call {
            Call::Primitive(name, output) => {
                let vals = atom.args[..atom.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, &env, &self.graph).ok_or("unbound primitive input"))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                if name.starts_with("cell-key-") {
                    self.graph.stats.key_operations += 1;
                }
                if let Some(v) =
                    value::primitive(name, &vals, output, self.graph.program.key_nodes())?
                {
                    let mark = env.trail.len();
                    if bind(atom.args.last().unwrap(), &v, env, &self.graph) {
                        self.solve(query, rid, remaining, env, epoch, out, delta)?;
                    }
                    env.rollback(mark);
                }
            }
            Call::Table(t) => {
                let key = atom.args[..atom.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, &env, &self.graph))
                    .collect::<Option<Vec<_>>>();
                let complete_key = key.is_some();
                let witnesses = if let Some(child) = env.domain(chosen).filter(|d| d.len() <= 1) {
                    self.free_join.stats.subtrie_uses += 1;
                    child.clone()
                } else if let Some(key) = key {
                    // The keyed lookup already returns the current row. Run the
                    // ordinary candidate checks against it without a second decode.
                    if let Some(row) = self.graph.lookup_canonical(*t, &key)?
                        && self
                            .sieve
                            .allow(rid, chosen, *t, row.witness, &self.query_indexes)
                    {
                        self.graph.stats.residual_candidates += 1;
                        self.graph.stats.decoded_rows += 1;
                        self.scan_row(query, rid, chosen, remaining, env, epoch, out, delta, &row)?;
                    }
                    remaining[chosen] = true;
                    return Ok(());
                } else {
                    let bound = atom
                        .args
                        .iter()
                        .map(|a| operand(a, env, &self.graph))
                        .collect::<Vec<_>>();
                    {
                        let num_bound = bound.iter().filter(|v| v.is_some()).count();
                        if let Some(domain) = env
                            .domains
                            .iter()
                            .rev()
                            .find(|d| d.atom == chosen)
                            .filter(|d| {
                                !self.free_join.entry_mode() || num_bound <= d.projected_columns
                            })
                        {
                            self.free_join.stats.subtrie_uses += 1;
                            domain.rows.clone()
                        } else {
                            self.query_indexes.candidates(&self.graph, *t, &bound)?
                        }
                    }
                };
                let mut witnesses = witnesses;
                if !complete_key
                    && witnesses.len() >= crate::range_index::MIN_DOMAIN
                    && let Some(index) = &mut self.range_index
                {
                    for guard in &self.graph.program.physical.ranges[rid][chosen] {
                        if !remaining[guard.atom]
                            || operand(&atom.args[guard.column], env, &self.graph).is_some()
                        {
                            continue;
                        }
                        let Some(Value::Int(limit)) = operand(&guard.threshold, env, &self.graph)
                        else {
                            continue;
                        };
                        let prefix = atom.args[..atom.args.len() - 1]
                            .iter()
                            .enumerate()
                            .find_map(|(i, a)| operand(a, env, &self.graph).map(|v| (i, v)));
                        if let Some(prefix) = prefix {
                            witnesses = index.select(
                                &self.graph,
                                *t,
                                prefix,
                                guard.column,
                                guard.bound,
                                limit,
                                &witnesses,
                            )?;
                            break;
                        }
                    }
                }
                let groups = if self.free_join.enabled && !complete_key {
                    self.free_join.partition(
                        &self.graph,
                        &mut self.query_indexes,
                        rid,
                        query,
                        chosen,
                        remaining,
                        env,
                        &witnesses,
                    )?
                } else {
                    None
                };
                if let Some(groups) = groups {
                    for group in groups {
                        let base = env.domains.len();
                        env.domains.extend(group.probes);
                        self.scan_domain(
                            query,
                            rid,
                            chosen,
                            *t,
                            &group.cover,
                            remaining,
                            env,
                            epoch,
                            out,
                            delta,
                        )?;
                        env.domains.truncate(base);
                    }
                } else {
                    self.scan_domain(
                        query, rid, chosen, *t, &witnesses, remaining, env, epoch, out, delta,
                    )?;
                }
            }
        }
        remaining[chosen] = true;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn scan_domain(
        &mut self,
        query: &[Atom],
        rid: usize,
        chosen: usize,
        table: usize,
        witnesses: &[Dependency],
        remaining: &mut [bool],
        env: &mut QueryEnv,
        epoch: u64,
        out: &mut Vec<Binding>,
        delta: Option<(usize, u64, Dependency)>,
    ) -> Result<()> {
        for w in witnesses.iter().copied() {
            if !self.sieve.allow(rid, chosen, table, w, &self.query_indexes) {
                continue;
            }
            self.graph.stats.residual_candidates += 1;
            self.graph.stats.decoded_rows += 1;
            let Some(r) = self.graph.row(w)? else {
                continue;
            };
            self.scan_row(query,rid,chosen,remaining,env,epoch,out,delta,&r)?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn scan_row(
        &mut self,
        query: &[Atom],
        rid: usize,
        chosen: usize,
        remaining: &mut [bool],
        env: &mut QueryEnv,
        epoch: u64,
        out: &mut Vec<Binding>,
        delta: Option<(usize, u64, Dependency)>,
        r: &Row,
    ) -> Result<()> {
        let atom = &query[chosen];
        // Disjoint partition by latest committed witness, then atom.
        // Within one RHS, later recipe positions (outer constructors)
        // own the match instead of their earlier constituent leaves.
        if r.hidden {
            return Ok(());
        }
        if delta.is_some_and(|(pivot, epoch, witness)| {
            (r.epoch, r.witness) > (epoch, witness)
                || ((r.epoch, r.witness) == (epoch, witness) && chosen < pivot)
        }) {
            return Ok(());
        }
        let mark = env.trail.len();
        if atom
            .args
            .iter()
            .zip(r.values.iter())
            .all(|(a, v)| bind(a, v, env, &self.graph))
        {
            env.record(chosen, Some(r.witness));
            {
                self.solve(query, rid, remaining, env, epoch.max(r.epoch), out, delta)?;
            }
            env.record(chosen, None);
        }
        env.rollback(mark);
        Ok(())
    }
    fn points(
        &mut self,
        _rid: usize,
        query: &[Atom],
        order: &[usize],
        env: &mut QueryEnv,
        remaining: &mut [bool],
        epoch: &mut u64,
        delta: Option<(usize, u64, Dependency)>,
        allow_inverse: bool,
    ) -> Result<bool> {
        for &i in order {
            if !remaining[i] {
                continue;
            }
            // A completed effect is not a negative match proof. Do not publish
            // this short circuit into the separate refutation/query caches.

            let atom = &query[i];
            let args = atom.args[..atom.args.len() - 1]
                .iter()
                .map(|a| operand(a, env, &self.graph))
                .collect::<Option<Vec<_>>>();
            match &atom.call {
                Call::Primitive(name, output) => {
                    let Some(args) = args else { continue };
                    self.continuation_stats.point_primitives += 1;
                    if name.starts_with("cell-key-") {
                        self.graph.stats.key_operations += 1;
                    }
                    let Some(value) =
                        value::primitive(name, &args, output, self.graph.program.key_nodes())?
                    else {
                        return Ok(false);
                    };
                    if !bind(atom.args.last().unwrap(), &value, env, &self.graph) {
                        return Ok(false);
                    }
                }
                Call::Table(t) => {
                    let row = if let Some(args) = args {
                        self.continuation_stats.point_reads += 1;
                        if let Some(domain) = env.domain(i).filter(|d| d.len() == 1) {
                            self.free_join.stats.entry_point_uses += 1;
                            self.graph.row(domain[0])?
                        } else {
                            self.graph.lookup_canonical(*t, &args)?
                        }
                    } else if allow_inverse
                        && matches!(
                            operand(atom.args.last().unwrap(), env, &self.graph),
                            Some(Value::Eq(_))
                        )
                    {
                        let bound = atom
                            .args
                            .iter()
                            .map(|a| operand(a, env, &self.graph))
                            .collect::<Vec<_>>();
                        match self.query_indexes.domain(&self.graph, *t, &bound)? {
                            crate::query::Domain::Empty => return Ok(false),
                            crate::query::Domain::Many => {
                                self.continuation_stats.inverse_ambiguous += 1;
                                continue;
                            }
                            crate::query::Domain::One(w) => {
                                self.continuation_stats.inverse_reads += 1;
                                self.graph.row(w)?
                            }
                        }
                    } else {
                        continue;
                    };
                    let Some(row) = row else { return Ok(false) };
                    self.graph.stats.decoded_rows += 1;
                    if row.hidden
                        || delta.is_some_and(|(pivot, stamp, w)| {
                            (row.epoch, row.witness) > (stamp, w)
                                || ((row.epoch, row.witness) == (stamp, w) && i < pivot)
                        })
                    {
                        return Ok(false);
                    }
                    if !atom
                        .args
                        .iter()
                        .zip(row.values.iter())
                        .all(|(a, v)| bind(a, v, env, &self.graph))
                    {
                        return Ok(false);
                    }
                    env.record(i, Some(row.witness));
                    *epoch = (*epoch).max(row.epoch);
                }
            }
            remaining[i] = false;
        }
        Ok(true)
    }
    fn continue_local(
        &mut self,
        rid: usize,
        pivot: usize,
        seed: &Row,
        env: &mut QueryEnv,
        remaining: &mut [bool],
        mut epoch: u64,
    ) -> Result<(bool, u64, Option<(usize, u32)>)> {
        let Some(context) = self.graph.continuation_context(seed.witness) else {
            return Ok((true, epoch, None));
        };
        let Some(route) = self
            .graph
            .program
            .multi_stage
            .routes
            .get(&(context.template, context.op, rid, pivot))
            .cloned()
        else {
            return Ok((true, epoch, None));
        };
        let depth = context
            .depth
            .checked_add(1)
            .ok_or("recipe depth exhausted")?;
        self.continuation_stats.entered += 1;
        if !self.graph.cells.contains_key(&context.cell) {}
        let query = self.graph.program.rules[rid].query.clone();
        for read in &route.reads {
            if !remaining[read.atom] {
                continue;
            }

            let atom = &query[read.atom];
            let bound = atom
                .args
                .iter()
                .map(|a| operand(a, env, &self.graph))
                .collect::<Vec<_>>();
            let full_key = bound[..bound.len() - 1]
                .iter()
                .cloned()
                .collect::<Option<Vec<_>>>();
            let expected = if full_key.is_none() {
                match self.query_indexes.domain(&self.graph, read.table, &bound)? {
                    crate::query::Domain::Empty => return Ok((false, epoch, None)),
                    crate::query::Domain::Many => {
                        self.continuation_stats.ambiguous_reads += 1;
                        continue;
                    }
                    crate::query::Domain::One(w) => Some(w),
                }
            } else {
                None
            };
            // A live recipe slot can supply the fields directly, without creating
            // an intermediate Row or rediscovering its constructor in a table.
            let mut local = None;
            if let Some(op) = read.effect {
                let f = self.graph.context_fact(context, op);
                if expected.is_none_or(|w| w == Dependency::Fact(f)) {
                    if let Some(row) = self.graph.fact(f)? {
                        if !row.hidden
                            && row.table == read.table
                            && full_key
                                .as_ref()
                                .is_none_or(|key| row.values[..key.len()] == *key)
                        {
                            self.continuation_stats.local_effect_reads += 1;
                            local = Some((row.values, row.epoch, row.witness));
                        }
                    }
                }
            }
            if local.is_none() {
                let key = if let Some(key) = full_key {
                    key
                } else {
                    let Ok(key) = self
                        .graph
                        .context_arguments(context, &read.columns[..read.columns.len() - 1])
                    else {
                        self.continuation_stats.unavailable_sources += 1;
                        continue;
                    };
                    key
                };
                self.continuation_stats.keyed_reads += 1;
                let row = self.graph.lookup(read.table, &key)?;
                let Some(row) = row else {
                    if expected.is_none() {
                        return Ok((false, epoch, None));
                    }
                    self.continuation_stats.unavailable_sources += 1;
                    continue;
                };
                if expected.is_some_and(|w| w != row.witness) {
                    self.continuation_stats.unavailable_sources += 1;
                    continue;
                }
                if row.hidden {
                    return Ok((false, epoch, None));
                }
                local = Some((row.values, row.epoch, row.witness));
            }
            let (values, stamp, witness) = local.unwrap();
            if (stamp, witness) > (seed.epoch, seed.witness)
                || (read.atom < pivot && (stamp, witness) == (seed.epoch, seed.witness))
            {
                return Ok((false, epoch, None));
            }
            let mark = env.trail.len();
            if !atom
                .args
                .iter()
                .zip(values.iter())
                .all(|(a, v)| bind(a, v, env, &self.graph))
            {
                env.rollback(mark);
                return Ok((false, epoch, None));
            }
            env.record(read.atom, Some(witness));
            remaining[read.atom] = false;
            epoch = epoch.max(stamp);
            self.continuation_stats.projected_atoms += 1;
        }
        Ok((true, epoch, Some((route.producer, depth))))
    }
    fn merge_points(
        &mut self,
        rid: usize,
        query: &[Atom],
        order: &[usize],
        env: &mut QueryEnv,
        epoch: u64,
        delta: (usize, u64, Dependency),
        out: &mut Vec<Binding>,
    ) -> Result<()> {
        let Some((&i, tail)) = order.split_first() else {
            out.push(Binding {
                #[cfg(feature = "view-consumers")]
                view_source: None,
                #[cfg(feature = "iterative-bake")]
                direct: None,
                known: env.known.clone(),
                values: env.binding(),
                epoch,
                parent: None,
                continuation: None,
                compiled: true,
            });
            return Ok(());
        };
        let a = &query[i];
        let mark = env.trail.len();
        match &a.call {
            Call::Primitive(name, sort) => {
                self.continuation_stats.merge_guard_checks += 1;
                let args = a.args[..a.args.len() - 1]
                    .iter()
                    .map(|a| {
                        operand(a, env, &self.graph).ok_or_else(|| "merge guard input".to_string())
                    })
                    .collect::<Result<Vec<_>>>()?;
                if name.starts_with("cell-key-") {
                    self.graph.stats.key_operations += 1;
                }
                if let Some(v) =
                    value::primitive(name, &args, sort, self.graph.program.key_nodes())?
                {
                    if bind(a.args.last().unwrap(), &v, env, &self.graph) {
                        self.merge_points(rid, query, tail, env, epoch, delta, out)?;
                    }
                }
            }
            Call::Table(t) => {
                self.continuation_stats.merge_port_reads += 1;
                let key = a.args[..a.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, env, &self.graph))
                    .collect::<Option<Vec<_>>>();
                let rows = if let Some(key) = key {
                    self.graph.lookup(*t, &key)?.into_iter().collect::<Vec<_>>()
                } else {
                    let Some(Value::Eq(id)) = operand(a.args.last().unwrap(), env, &self.graph)
                    else {
                        return Err("merge port has no fixed key or output".into());
                    };
                    // Reuse the exact versioned constructor result-column index.
                    // Do not traverse all incoming uses of this eclass for each
                    // comparison, nor build a second untracked context cache.
                    let mut bound = vec![None; a.args.len()];
                    *bound.last_mut().unwrap() = Some(Value::Eq(id));
                    let refs = self.query_indexes.candidates(&self.graph, *t, &bound)?;
                    refs.iter()
                        .map(|w| self.graph.row(*w))
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .flatten()
                        .collect()
                };
                for row in rows {
                    self.continuation_stats.merge_candidate_rows += 1;
                    self.graph.stats.decoded_rows += 1;
                    if row.hidden {
                        continue;
                    }
                    if (row.epoch, row.witness) > (delta.1, delta.2)
                        || ((row.epoch, row.witness) == (delta.1, delta.2) && i < delta.0)
                    {
                        continue;
                    }
                    if a.args
                        .iter()
                        .zip(row.values.iter())
                        .all(|(a, v)| bind(a, v, env, &self.graph))
                    {
                        env.record(i, Some(row.witness));
                        self.merge_points(rid, query, tail, env, epoch.max(row.epoch), delta, out)?;
                        env.record(i, None);
                    }
                    env.rollback(mark);
                }
            }
        }
        env.rollback(mark);
        Ok(())
    }
    fn match_merge_program(
        &mut self,
        rid: usize,
        pivot: usize,
        row: &Row,
        env: &mut QueryEnv,
        out: &mut Vec<Binding>,
    ) -> Result<bool> {
        let Some(order) = self
            .graph
            .program
            .merge_programs
            .entries
            .get(&(rid, pivot))
            .cloned()
        else {
            return Ok(false);
        };
        self.continuation_stats.merge_context_entries += 1;
        let before = out.len();
        let query = self.graph.program.rules[rid].query.clone();
        self.merge_points(
            rid,
            &query,
            &order,
            env,
            row.epoch,
            (pivot, row.epoch, row.witness),
            out,
        )?;
        let n = (out.len() - before) as u64;
        self.continuation_stats.merge_bindings += n;
        *self
            .continuation_stats
            .merge_bindings_by_rule
            .entry(rid)
            .or_default() += n;
        Ok(true)
    }
    fn context_bindings(
        rule: &Rule,
        answers: Vec<crate::local_context::Answer>,
        facts: &QueryFacts,
    ) -> Vec<Binding> {
        answers
            .into_iter()
            .map(|answer| {
                let mut known = facts.clone();
                for (i, w) in answer.witnesses.into_iter().enumerate() {
                    known.record(i, w);
                }
                Binding {
                    #[cfg(feature = "view-consumers")]
                    view_source: None,
                    #[cfg(feature = "iterative-bake")]
                    direct: None,
                    values: rule.inputs.iter().copied().zip(answer.values).collect(),
                    known,
                    epoch: answer.epoch,
                    parent: None,
                    continuation: None,
                    compiled: true,
                }
            })
            .collect()
    }
    fn empty_by_sieve(&mut self, rid: usize) -> Result<bool> {
        self.sieve
            .prepare(rid, &self.graph, &mut self.query_indexes)?;
        let empty = self.sieve.empty_join(rid) || self.free_join.disjoint(rid, &self.graph, &self.query_indexes);
        if empty {
            self.pending[rid].clear();
        }
        Ok(empty)
    }
    fn matches(&mut self, rid: usize, probe: bool) -> Result<Vec<Binding>> {
        #[cfg(feature = "allocation-census")]
        let _allocation_scope = crate::allocation_census::scope(crate::allocation_census::Tag::Search);
        if !probe
            && self.graph.program.rules[rid].seminaive
            && !std::mem::replace(&mut self.dirty[rid], false)
        {
            return Ok(vec![]);
        }
        let tables = &mut self.required_tables[rid];
        if let Some(pos) = tables.iter().position(|&t| self.graph.witness_count(t) == 0) {
            // Retest the last empty input first, using current storage. No
            // maintained negative result can hide the appearance of a row.
            tables.swap(0,pos);
            if !probe { self.pending[rid].clear(); }
            return Ok(Vec::new());
        }
        let rule = self.graph.program.rules[rid].clone();
        #[cfg(feature = "view-consumers")]
        if !probe
            && let Some(rows) = self.blocks.matches(
                rid,
                self.watermarks[rid],
                &mut self.graph,
                &mut self.query_indexes,
                &mut self.entry_schedule,
                &self.stage_spine,
            )?
        {
            self.pending[rid].clear();
            return Ok(rows);
        }
        let query_facts = if probe {
            QueryFacts::None
        } else {
            self.stage_spine.alias_query_facts(rid)
        };
        let scheduled_entry = !probe && self.entry_schedule.accepts(rid);
        // A necessary join summary can reject an empty domain before either route.
        let early_sieve = scheduled_entry && self.sieve.enabled && rule.direct.is_none();
        let context = self.local_context.accepts(&self.graph.program, rid);
        let sieve_first = self.local_context.general() || !context;
        if early_sieve && sieve_first && self.empty_by_sieve(rid)? {
            return Ok(vec![]);
        }
        if context
            && scheduled_entry
            && let Some(answers) = self.local_context.matches(
                rid,
                self.watermarks[rid],
                !matches!(query_facts, QueryFacts::None),
                self.stage_spine.active_for_rule(rid),
                &mut self.graph,
                &mut self.query_indexes,
                &mut self.entry_schedule,
            )?
        {
            self.pending[rid].clear();
            return Ok(Self::context_bindings(&rule, answers, &query_facts));
        }
        if early_sieve && !sieve_first && self.empty_by_sieve(rid)? {
            return Ok(vec![]);
        }
        let early_join = scheduled_entry && rule.direct.is_none() && self.free_join.has_entry(rid);
        let route_spine = scheduled_entry && self.stage_spine.enabled;
        let active_spine = route_spine && self.stage_spine.active_for_rule(rid);
        let mut spine_routes = Vec::new();
        let mut entry_domains = Vec::new();
        let scheduled = if scheduled_entry {
            let watermark = self.watermarks[rid];
            Some(if early_join || route_spine {
                self.entry_schedule
                    .batches(&self.graph, rid, watermark, |batch, out| {
                        #[cfg(not(any(feature = "family-frontiers", feature = "family-scan", feature = "family-overlap")))]
                        if batch.empty_delta() {
                            self.free_join.stats.empty_entry_batches += 1;
                            return Ok(());
                        }
                        #[cfg(feature = "family-frontiers")]
                        let gate_clock = Instant::now();
                        let atom = batch.atom();
                        #[cfg(feature = "sparse-cells")]
                        let point_only = rule.direct.is_none()
                            && self.graph.program.kernels[rid][atom]
                                .as_ref()
                                .is_some_and(|k| {
                                    k.steps.iter().all(|s| {
                                        !matches!(s.access, crate::row_kernel::Access::Expand)
                                    })
                                });
                        #[cfg(not(feature = "sparse-cells"))]
                        let point_only = false;
                        let start = out.len();
                        let claimed = spine_routes.len();
                        let cursors = if active_spine {
                            self.stage_spine.scheduled_cursors(rid, atom, batch.table)
                        } else {
                            None
                        };
                        let inverted = if early_join && self.free_join.planned {
                            self.free_join.reverse(
                                &self.graph,
                                &mut self.query_indexes,
                                rid,
                                batch,
                            )?
                        } else {
                            None
                        };
                        #[cfg(feature = "projection-semijoin")]
                        let inverted = if inverted.is_none() && early_join && cursors.is_none() {
                            self.free_join.gates.restrict(
                                &self.graph,
                                &mut self.query_indexes,
                                rid,
                                batch,
                            )?
                        } else {
                            inverted
                        };
                        #[cfg(feature = "projection-shared")]
                        let shared = if inverted.is_none() && early_join && cursors.is_none() {
                            self.free_join.shared_frontier(
                                &self.graph,
                                &mut self.query_indexes,
                                rid,
                                batch,
                            )?
                        } else {
                            None
                        };
                        #[cfg(feature = "family-frontiers")]
                        let mut delivered = 0;
                        let mut deliver = |w| {
                            #[cfg(feature = "family-frontiers")]
                            {
                                delivered += 1;
                            }
                            #[cfg(feature = "family-overlap")]
                            self.overlap
                                .seed(watermark, rid, batch.node(), batch.table, w);
                            if early_sieve
                                && !self
                                    .sieve
                                    .allow(rid, atom, batch.table, w, &self.query_indexes)
                            {
                                return Ok(());
                            }
                            let packet = batch.packet(w).expect("current delta entry");
                            if let Some(cursor) = cursors.and_then(|entries| entries.get(&w)) {
                                spine_routes.push((out.len(), cursor.expand()));
                                // Keep domain slots aligned with the shared seed vector.
                                if early_join {
                                    entry_domains.push(Domains::new());
                                }
                            } else if early_join && point_only {
                                // The residual register block checks every full key and
                                // its owner. It consumes no expansion domain.
                                entry_domains.push(Domains::new());
                            } else if early_join {
                                let Some(domains) = self.free_join.admit(
                                    &self.graph,
                                    &mut self.query_indexes,
                                    rid,
                                    atom,
                                    &packet.row,
                                    #[cfg(feature = "family-overlap")]
                                    (&mut self.overlap, watermark),
                                )?
                                else {
                                    return Ok(());
                                };
                                entry_domains.push(domains);
                            }
                            out.push(batch.seed(packet));
                            Ok::<(), String>(())
                        };
                        #[cfg(feature = "projection-shared")]
                        let shared_rows = shared.as_deref().map(Vec::as_slice);
                        #[cfg(not(feature = "projection-shared"))]
                        let shared_rows: Option<&[Dependency]> = None;
                        #[cfg(feature = "family-scan")]
                        if shared_rows.is_none() && inverted.is_none() {
                            self.raw_scan
                                .record(&self.current_stage, self.graph.epoch, rid, batch);
                        }
                        if let Some(rows) = shared_rows {
                            for &w in rows {
                                deliver(w)?;
                            }
                        } else if let Some(rows) = inverted {
                            for w in rows {
                                deliver(w)?
                            }
                        } else {
                            for &(_, w) in batch.delta() {
                                deliver(w)?
                            }
                        }
                        #[cfg(feature = "family-frontiers")]
                        {
                            let ns = gate_clock.elapsed().as_nanos() as u64;
                            self.fronts.batches.push([
                                self.graph.epoch,
                                watermark,
                                rid as u64,
                                atom as u64,
                                batch.node() as u64,
                                delivered,
                                (out.len() - start) as u64,
                                (spine_routes.len() - claimed) as u64,
                                ns,
                            ]);
                            if cursors.is_none() {
                                self.fronts.observe(
                                    &self.current_stage,
                                    self.graph.epoch,
                                    self.prior_union,
                                    rid,
                                    batch,
                                    early_join,
                                    &out[start..],
                                    if early_join {
                                        &entry_domains[start..]
                                    } else {
                                        &[]
                                    },
                                    ns,
                                );
                            }
                        }
                        if active_spine {
                            let ordinary = (out.len() - start) - (spine_routes.len() - claimed);
                            self.stage_spine
                                .record_fallbacks(rid, atom, ordinary as u64);
                        }
                        Ok(())
                    })?
            } else if early_sieve {
                self.entry_schedule
                    .seeds_where(&self.graph, rid, watermark, |atom, table, w| {
                        self.sieve.allow(rid, atom, table, w, &self.query_indexes)
                    })?
            } else {
                self.entry_schedule.seeds(&self.graph, rid, watermark)?
            })
        } else {
            None
        };
        let mut stage_covered = BTreeSet::new();
        let mut found = vec![];
        if !probe && rule.seminaive {
            let watermark = self.watermarks[rid];
            let coverage = match &scheduled {
                Some(seeds) if route_spine => self.stage_spine.answers_routed(
                    &mut self.graph,
                    rid,
                    seeds,
                    spine_routes,
                    watermark,
                )?,
                Some(_) => None,
                None => {
                    self.stage_spine
                        .answers(&mut self.graph, rid, &self.pending[rid], watermark)?
                }
            };
            if self.graph.program.neighbor_plans[rid].is_some()
                && let Some(seeds) = &scheduled
            {
                let claimed = coverage.as_ref().map_or(0, |coverage| {
                    if coverage.complete {
                        seeds.len()
                    } else {
                        coverage.covered.len()
                    }
                });
                let stats = self.neighbor.stats(rid);
                stats.spine_entries += claimed as u64;
                stats.residual_entries += (seeds.len() - claimed) as u64;
            }
            if let Some(coverage) = coverage {
                found = coverage
                    .answers
                    .into_iter()
                    .map(|a| {
                        let mut known = query_facts.clone();
                        for (i, w) in a.witnesses.into_iter().enumerate() {
                            known.record(i, w);
                        }

                        Binding {
                            #[cfg(feature = "view-consumers")]
                            view_source: None,
                            #[cfg(feature = "iterative-bake")]
                            direct: a.direct,
                            values: rule.inputs.iter().copied().zip(a.values).collect(),
                            known,
                            parent: None,
                            continuation: None,
                            epoch: a.epoch,
                            compiled: true,
                        }
                    })
                    .collect();
                if coverage.complete {
                    self.pending[rid].clear();
                    return Ok(found);
                }
                stage_covered = coverage.covered;
            }
        }
        if !early_sieve {
            self.sieve
                .prepare(rid, &self.graph, &mut self.query_indexes)?;
        }
        let shared_seeds = !probe && rule.seminaive && self.stage_spine.shared();
        if let Some(anchor) = rule.direct {
            let atom = &rule.query[anchor];
            let Call::Table(t) = atom.call else {
                unreachable!()
            };
            let candidates = if let Some(seeds) = &scheduled {
                seeds.iter().map(|s| s.packet.row.witness).collect()
            } else if probe || !rule.seminaive {
                self.graph.witnesses(t)
            } else {
                std::mem::take(&mut self.pending[rid]).into_iter().collect()
            };
            for (seed_index, w) in candidates.into_iter().enumerate() {
                if stage_covered.contains(&(anchor, w)) {
                    continue;
                }
                let supplied = scheduled.as_ref().map(|seeds| seeds[seed_index].clone());
                let row = if let Some(seed) = &supplied {
                    SeedRow::Shared(seed.packet.row.clone())
                } else {
                    let row = if shared_seeds {
                        self.stage_spine
                            .seed_row(&mut self.graph, w)?
                            .map(SeedRow::Shared)
                    } else {
                        self.graph.stats.decoded_rows += 1;
                        self.graph.row(w)?.map(SeedRow::Owned)
                    };
                    let Some(row) = row else { continue };
                    row
                };
                if row.hidden || (!probe && rule.seminaive && row.epoch < self.watermarks[rid]) {
                    continue;
                }
                let mut env = if let Some(seed) = supplied {
                    QueryEnv::shared(seed, query_facts.clone())
                } else {
                    QueryEnv::new(
                        self.graph.program.templates[rule.template].sorts.len(),
                        query_facts.clone(),
                    )
                };
                let live = if scheduled.is_some() {
                    true
                } else if shared_seeds {
                    self.stage_spine.bind_seed(
                        &self.graph,
                        rid,
                        anchor,
                        &row,
                        env.values.owned_mut(),
                    )
                } else {
                    self.graph.stats.direct_candidates += 1;
                    atom.args
                        .iter()
                        .zip(row.values.iter())
                        .all(|(a, v)| bind(a, v, &mut env, &self.graph))
                };
                if live {
                    env.record(anchor, Some(row.witness));
                    let n = found.len();
                    let mut remaining = (0..rule.query.len())
                        .map(|i| i != anchor)
                        .collect::<Vec<_>>();
                    let mut epoch = row.epoch;
                    {
                        if let Some(order) = self
                            .graph
                            .program
                            .multi_stage
                            .entries
                            .get(&(rid, anchor))
                            .cloned()
                        {
                            if !self.points(
                                rid,
                                &rule.query,
                                &order,
                                &mut env,
                                &mut remaining,
                                &mut epoch,
                                None,
                                false,
                            )? {
                                continue;
                            }
                        }
                    }
                    if !remaining.iter().any(|v| *v) {
                        found.push(Binding {
                            #[cfg(feature = "view-consumers")]
                            view_source: None,
                            #[cfg(feature = "iterative-bake")]
                            direct: None,
                            known: env.known.clone(),
                            values: env.binding(),
                            epoch,
                            parent: None,
                            continuation: None,
                            compiled: true,
                        });
                    } else {
                        self.solve(
                            &rule.query,
                            rid,
                            &mut remaining,
                            &mut env,
                            epoch,
                            &mut found,
                            None,
                        )?;
                    }
                    if let Some(f) = self.graph.origin(row.witness) {
                        for b in &mut found[n..] {
                            if matches!(row.witness, Dependency::Fact(_)) {
                                b.parent = Some(f);
                            }
                            let source = self
                                .graph
                                .continuation_context(row.witness)
                                .ok_or("missing continuation context")?;
                            {
                                if let Some(route) = self.graph.program.multi_stage.routes.get(&(
                                    source.template,
                                    source.op,
                                    rid,
                                    anchor,
                                )) {
                                    b.continuation = Some((
                                        route.producer,
                                        source
                                            .depth
                                            .checked_add(1)
                                            .ok_or("recipe depth exhausted")?,
                                        false,
                                    ));
                                }
                            }
                        }
                    }
                    self.graph.stats.direct_bindings += (found.len() - n) as u64;
                }
            }
        } else if rule.seminaive
            && !probe
            && rule.query.iter().any(|a| matches!(a.call, Call::Table(_)))
        {
            let watermark = self.watermarks[rid];
            let changed = if scheduled.is_some() {
                vec![]
            } else {
                std::mem::take(&mut self.pending[rid])
                    .into_iter()
                    .map(|w| {
                        if shared_seeds {
                            self.stage_spine
                                .seed_row(&mut self.graph, w)
                                .map(|r| r.map(SeedRow::Shared))
                        } else {
                            self.graph.row(w).map(|r| r.map(SeedRow::Owned))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .filter(|r| !r.hidden && r.epoch >= watermark)
                    .collect::<Vec<_>>()
            };
            let use_kernel = self.free_join.planned && scheduled.is_some();
            let mut regs = None;
            for (pivot, atom) in rule.query.iter().enumerate() {
                let Call::Table(table) = atom.call else {
                    continue;
                };
                let kernel = use_kernel
                    .then(|| self.graph.program.kernels[rid][pivot].clone())
                    .flatten()
                    .filter(|k| {
                        self.range_index.is_none()
                            || !k.steps.iter().any(|s| {
                                matches!(s.access, crate::row_kernel::Access::Expand)
                                    && !self.graph.program.physical.ranges[rid][s.atom].is_empty()
                            })
                    });
                let use_neighbors = kernel.is_some()
                    && self.graph.program.neighbor_plans[rid]
                        .as_ref()
                        .is_some_and(|plan| plan.pivots.contains(&pivot));
                let candidates = scheduled
                    .as_ref()
                    .into_iter()
                    .flat_map(|seeds| {
                        seeds
                            .iter()
                            .enumerate()
                            .filter(move |(_, s)| s.atom == pivot)
                            .map(|(i, s)| (Some((i, s)), s.packet.row.as_ref()))
                    })
                    .chain(
                        changed
                            .iter()
                            .filter(|r| r.table == table)
                            .map(|r| (None, &**r)),
                    );
                for (supplied, row) in candidates {
                    if stage_covered.contains(&(pivot, row.witness)) {
                        continue;
                    }
                    if !early_sieve
                        && !self
                            .sieve
                            .allow(rid, pivot, table, row.witness, &self.query_indexes)
                    {
                        continue;
                    }
                    if !shared_seeds && scheduled.is_none() {
                        self.graph.stats.residual_candidates += 1;
                        self.graph.stats.decoded_rows += 1;
                    }
                    if let (Some(kernel), Some((i, _))) = (&kernel, supplied) {
                        #[cfg(feature = "structural-depth")]
                        if let Some(sol) =
                            self.stage_spine
                                .structural
                                .try_entry(&self.graph, rid, pivot, row)?
                        {
                            let mut known = query_facts.clone();
                            for (i, w) in sol.witnesses.into_iter().enumerate() {
                                known.record(i, w);
                            }
                            known.mark(sol.depth);
                            found.push(Binding {
                                #[cfg(feature = "view-consumers")]
                                view_source: None,
                                direct: None,
                                known,
                                values: rule
                                    .inputs
                                    .iter()
                                    .map(|i| (*i, sol.values[i].clone()))
                                    .collect(),
                                epoch: sol.epoch,
                                parent: None,
                                continuation: None,
                                compiled: true,
                            });
                            continue;
                        }
                        let regs =
                            regs.get_or_insert_with(|| execute_kernel::Registers::new(&rule));
                        let domains = if early_join {
                            entry_domains[i].as_slice()
                        } else {
                            &[]
                        };
                        if use_neighbors {
                            let before = found.len();
                            regs.start::<true>(
                                self,
                                rid,
                                &rule,
                                kernel,
                                row,
                                domains,
                                &query_facts,
                                &mut found,
                            )?;
                            let stats = self.neighbor.stats(rid);
                            stats.seeds += 1;
                            stats.bindings += (found.len() - before) as u64;
                        } else {
                            regs.start::<false>(
                                self,
                                rid,
                                &rule,
                                kernel,
                                row,
                                domains,
                                &query_facts,
                                &mut found,
                            )?;
                        }
                        continue;
                    }
                    let mut env = if let Some((i, seed)) = supplied {
                        let mut env = QueryEnv::shared(seed.clone(), query_facts.clone());
                        if early_join {
                            env.domains = std::mem::take(&mut entry_domains[i]);
                        }
                        env
                    } else {
                        QueryEnv::new(
                            self.graph.program.templates[rule.template].sorts.len(),
                            query_facts.clone(),
                        )
                    };
                    let live = if scheduled.is_some() {
                        true
                    } else if shared_seeds {
                        self.stage_spine.bind_seed(
                            &self.graph,
                            rid,
                            pivot,
                            row,
                            env.values.owned_mut(),
                        )
                    } else {
                        atom.args
                            .iter()
                            .zip(row.values.iter())
                            .all(|(a, v)| bind(a, v, &mut env, &self.graph))
                    };
                    if live {
                        (|| -> Result<()> {
                            let mut remaining = (0..rule.query.len())
                                .map(|i| i != pivot)
                                .collect::<Vec<_>>();
                            let mut epoch = row.epoch;
                            env.record(pivot, Some(row.witness));
                            if self.match_merge_program(rid, pivot, row, &mut env, &mut found)? {
                                return Ok(());
                            }
                            let delta = Some((pivot, row.epoch, row.witness));
                            {
                                if let Some(order) = self
                                    .graph
                                    .program
                                    .multi_stage
                                    .entries
                                    .get(&(rid, pivot))
                                    .cloned()
                                {
                                    if !self.points(
                                        rid,
                                        &rule.query,
                                        &order,
                                        &mut env,
                                        &mut remaining,
                                        &mut epoch,
                                        delta,
                                        false,
                                    )? {
                                        return Ok(());
                                    }
                                }
                            }

                            let (live, mut epoch, origin) = self.continue_local(
                                rid,
                                pivot,
                                row,
                                &mut env,
                                &mut remaining,
                                epoch,
                            )?;
                            if !live {
                                return Ok(());
                            }
                            // Prefer certified producer-local slots. Only unresolved
                            // reads use conditional singleton projections of the current
                            // public interface (including data-loader-origin nodes).
                            {
                                if let Some(order) = self
                                    .graph
                                    .program
                                    .multi_stage
                                    .entries
                                    .get(&(rid, pivot))
                                    .cloned()
                                {
                                    if !self.points(
                                        rid,
                                        &rule.query,
                                        &order,
                                        &mut env,
                                        &mut remaining,
                                        &mut epoch,
                                        delta,
                                        true,
                                    )? {
                                        return Ok(());
                                    }
                                }
                            }
                            let residual = remaining
                                .iter()
                                .enumerate()
                                .any(|(i, p)| *p && matches!(rule.query[i].call, Call::Table(_)));
                            let before = found.len();
                            if !residual {
                                if let Some(order) =
                                    self.graph.program.multi_stage.finishes.get(rid).cloned()
                                {
                                    if !self.points(
                                        rid,
                                        &rule.query,
                                        &order,
                                        &mut env,
                                        &mut remaining,
                                        &mut epoch,
                                        delta,
                                        false,
                                    )? {
                                        return Ok(());
                                    }
                                }
                            }
                            if !remaining.iter().any(|v| *v) {
                                found.push(Binding {
                                    #[cfg(feature = "view-consumers")]
                                    view_source: None,
                                    #[cfg(feature = "iterative-bake")]
                                    direct: None,
                                    known: env.known.clone(),
                                    values: { env.binding() },
                                    epoch,
                                    parent: None,
                                    continuation: None,
                                    compiled: true,
                                });
                            } else {
                                self.solve(
                                    &rule.query,
                                    rid,
                                    &mut remaining,
                                    &mut env,
                                    epoch,
                                    &mut found,
                                    delta,
                                )?;
                            }
                            if origin.is_some() {
                                for binding in &mut found[before..] {
                                    binding.continuation = origin.map(|(p, d)| (p, d, residual));
                                }
                            }
                            Ok(())
                        })()?;
                    }
                }
            }
        } else {
            self.solve(
                &rule.query,
                rid,
                &mut vec![true; rule.query.len()],
                &mut QueryEnv::new(
                    self.graph.program.templates[rule.template].sorts.len(),
                    query_facts.clone(),
                ),
                0,
                &mut found,
                None,
            )?;
        }
        let key_slots = { rule.inputs.as_slice() };
        found.retain(|b| probe || !rule.seminaive || b.epoch >= self.watermarks[rid]);
        let n = found.len() as u64;
        self.dedup_rows += n;
        self.dedup_full_elements += n * rule.inputs.len() as u64;
        self.dedup_elements += n * key_slots.len() as u64;
        #[cfg(feature = "sparse-cells")]
        if found
            .iter()
            .all(|b| b.values.len() == key_slots.len() && b.values.keys().eq(key_slots))
        {
            found.sort_by(|a, b| a.values.cmp(&b.values));
            let before = found.len();
            found.dedup_by(|a, b| a.values == b.values);
            self.dedup_duplicates += (before - found.len()) as u64;
            return Ok(found);
        }
        if found
            .iter()
            .any(|b| key_slots.iter().any(|s| !b.values.contains_key(s)))
        {
            return Err("incomplete binding".into());
        }
        let compare = |a: &Binding, b: &Binding| {
            key_slots
                .iter()
                .map(|s| &a.values[s])
                .cmp(key_slots.iter().map(|s| &b.values[s]))
        };
        found.sort_by(compare);
        found.dedup_by(|later, first| {
            if compare(first, later).is_ne() {
                return false;
            }
            true
        });
        self.dedup_duplicates += n - found.len() as u64;
        Ok(found)
    }
    fn apply(
        &mut self,
        rid: usize,
        b: Binding,
        #[cfg(feature = "structural-depth")] evidence: Option<crate::depth_runtime::Evidence>,
    ) -> Result<()> {
        #[cfg(feature = "allocation-census")]
        let _allocation_scope = crate::allocation_census::scope(crate::allocation_census::Tag::Rhs);
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::rhs(rid);
        let stage_cursor = self.stage_spine.begin_step(
            &self.graph.program,
            rid,
            &b.values,
            &self.current_stage,
            self.graph.epoch,
            #[cfg(feature = "iterative-bake")]
            b.direct,
        );
        #[cfg(feature="two-run-survey")]
        self.graph.prepare_two_run(rid,stage_cursor.as_ref().and_then(|c|c.join_anchor()));
        #[cfg(feature="two-run-blocks")]
        self.graph.prepare_two_run_execution(rid,&b.known,matches!(&stage_cursor,Some(crate::stage_spine::Started::Live(_))));
        #[cfg(feature = "rhs-work-audit")]
        self.ledger.resumed(matches!(
            &stage_cursor,
            Some(crate::stage_spine::Started::Live(_))
        ))?;
        let stage_counts = stage_cursor.as_ref().map(|_| {
            (
                self.graph.stats.constructor_requests,
                self.graph.stats.function_updates,
                self.graph.stats.union_requests,
            )
        });
        let template = self.graph.program.rules[rid].template;
        let origin = b.continuation;
        if b.compiled {
            self.continuation_stats.compiled_bindings += 1;
        }
        let facts = self.graph.stats.logical_facts_created;
        let updates = self.graph.stats.function_updates;

        if origin.is_none()
            && let Some(write) = self.graph.program.writes[rid].clone()
        {
            #[cfg(feature = "structural-depth")]
            if let Some(ev) = &evidence {
                self.stage_spine.structural.retained(b.known.depth());
                self.stage_spine.structural.evaluate(
                    &self.graph,
                    rid,
                    &b.values,
                    &b.known,
                    ev,
                    matches!(&stage_cursor, Some(crate::stage_spine::Started::Live(_))),
                    &[],
                )?;
            }
            let keep_key = stage_cursor.is_some() && self.stage_spine.write_needs_key(
                write.table,
                #[cfg(feature = "sparse-cells")]
                self.graph.has_visibility_changes(write.table),
            );
            let (cell, index, key, changed) = self
                .graph
                .apply_write(template, b.values, b.parent, &write, keep_key)?;
            #[cfg(feature = "structural-depth")]
            if self.stage_spine.structural.capture_rule(rid) {
                let mut rs = vec![None; write.op + 1];
                rs[write.op] = Some(Dependency::Function(index));
                self.stage_spine
                    .structural
                    .capture(&self.graph, rid, &b.known, &rs);
            }
            if let Some(step) = stage_cursor {
                self.stage_spine.effects(0, u64::from(changed), 0);
                self.stage_spine.capture_write(
                    step,
                    rid,
                    cell,
                    write.op,
                    write.table,
                    key,
                    Dependency::Function(index),
                    self.graph.stats.unions,
                    #[cfg(feature = "iterative-bake")]
                    &self.graph.program.iterative,
                    #[cfg(feature = "iterative-bake")]
                    &b.known,
                );
            }
            return Ok(());
        }
        #[cfg(feature = "iterative-bake")]
        let available = stage_cursor
            .as_ref()
            .filter(|_| true)
            .map(|s| s.result_inputs(&self.graph.program.iterative))
            .unwrap_or_default();
        #[cfg(feature = "structural-depth")]
        let (ports, proven, closed) = if let Some(ev) = &evidence {
            self.stage_spine.structural.retained(b.known.depth());
            self.stage_spine.structural.evaluate(
                &self.graph,
                rid,
                &b.values,
                &b.known,
                ev,
                matches!(&stage_cursor, Some(crate::stage_spine::Started::Live(_))),
                &available,
            )?
        } else {
            Default::default()
        };
        #[cfg(feature = "iterative-bake")]
        if !available.is_empty()
            && stage_cursor
                .as_ref()
                .is_some_and(|s| s.closed(&self.graph.program.iterative))
        {
            if let Some((cell, mut keys, receipts, unions)) =
                self.graph.finish_handoff(template, &b.values, &available)?
            {
                #[cfg(feature = "structural-depth")]
                self.stage_spine
                    .structural
                    .capture(&self.graph, rid, &b.known, &receipts);
                self.stage_spine.effects(0, 0, unions);
                self.stage_spine.capture_keys(
                    &self.graph,
                    rid,
                    cell,
                    stage_cursor.unwrap(),
                    |i| b.known.get(i).map(Dependency::Fact),
                    Some(&receipts),
                    |i, _| {
                        keys[i]
                            .take()
                            .ok_or("missing completed publication key".into())
                    },
                )?;
                return Ok(());
            }
            self.graph.stats.handoff_closed_fallback += 1;
        }
        #[cfg(feature = "structural-depth")]
        if self.stage_spine.structural.execute
            && origin.is_none()
            && let Some(d) = closed
            && let Some((cell, mut keys, receipts, unions)) =
                self.graph.finish_handoff(template, &b.values, &ports)?
        {
            #[cfg(feature = "rhs-work-audit")]
            if !matches!(&stage_cursor, Some(crate::stage_spine::Started::Live(_))) {
                self.ledger.closed(&self.graph, rid, &b.values);
            }
            self.stage_spine.structural.closed(
                d,
                matches!(&stage_cursor, Some(crate::stage_spine::Started::Live(_))),
            );
            for &(_, d) in &proven {
                self.stage_spine.structural.accepted(d as usize);
            }
            self.stage_spine
                .structural
                .capture(&self.graph, rid, &b.known, &receipts);
            if let Some(cursor) = stage_cursor {
                self.stage_spine.effects(0, 0, unions);
                self.stage_spine.capture_keys(
                    &self.graph,
                    rid,
                    cell,
                    cursor,
                    |i| b.known.get(i).map(Dependency::Fact),
                    Some(&receipts),
                    |i, _| {
                        keys[i]
                            .take()
                            .ok_or("missing structural publication key".into())
                    },
                )?;
            }
            return Ok(());
        }
        #[cfg(feature = "structural-depth")]
        let available = {
            let mut merged = available;
            if self.stage_spine.structural.execute && origin.is_none() {
                for input in ports
                    .into_iter()
                    .filter(|x| proven.iter().any(|(op, _)| *op == x.op))
                {
                    if let Some(old) = merged.iter_mut().find(|x| x.op == input.op) {
                        *old = input
                    } else {
                        merged.push(input);
                    }
                }
                if !proven.is_empty() {
                    merged.sort_by_key(|x| x.op);
                }
            }
            merged
        };
        let static_skip = {
            self.graph
                .program
                .known_results
                .static_skip
                .get(rid)
                .cloned()
        };
        let aliases = static_skip.as_deref().map(Vec::as_slice).filter(|_| true);
        #[cfg(feature = "structural-depth")]
        let capture = self
            .stage_spine
            .structural
            .capture_rule(rid)
            .then(|| vec![None; self.graph.program.templates[template].ops.len()]);
        #[cfg(not(feature = "structural-depth"))]
        let capture: Option<Vec<Option<Dependency>>> = None;
        let mut effect_receipts = capture;
        #[cfg(feature = "rhs-locals")]
        let local = if self.local_width > 0
            && self.local_selection.as_ref().is_none_or(|s| s[rid])
            && origin.is_none()
            && !matches!(&stage_cursor, Some(crate::stage_spine::Started::Live(_)))
        {
            self.graph.program.local_rhs[rid]
                .as_ref()
                .filter(|p| p.steps.len() <= self.local_width)
                .cloned()
        } else {
            None
        };
        #[cfg(all(feature = "rhs-work-audit", feature = "rhs-locals"))]
        let inspect_births = (local.clone(), stage_cursor.is_some());
        // Move the binding exactly once. All hits leave only a temporary
        // frame; on the first miss the original suffix resumes after it.
        let ordinary = |me: &mut Self,
                        values: crate::slots::Slots,
                        _receipts: &mut Option<Vec<Option<Dependency>>>|
         -> Result<Option<(u32, usize)>> {
            if me.graph.program.rules[rid].constructor_only
                && stage_cursor.is_none()
                && origin.is_none()
            {
                me.graph.start_constructor_rhs(
                    template,
                    values,
                    b.parent,
                    aliases,
                    #[cfg(feature = "structural-depth")]
                    _receipts.as_deref_mut(),
                )
            } else {
                Ok(Some((
                    {
                        #[cfg(feature = "sparse-cells")]
                        let id = me.graph.begin_transient(template, values, b.parent)?;
                        #[cfg(not(feature = "sparse-cells"))]
                        let id = me.graph.linked_slots(template, values, b.parent)?;
                        id
                    },
                    0,
                )))
            }
        };
        #[cfg(feature = "rhs-locals")]
        let start = if let Some(block) = local {
            if effect_receipts.is_none() && self.stage_spine.capture_receipts(rid) {
                effect_receipts =
                    Some(vec![None; self.graph.program.templates[template].ops.len()]);
            }
            match self.graph.local_rhs(
                template,
                b.values,
                b.parent,
                &block,
                effect_receipts.as_deref_mut(),
                &mut self.local_counts,
            )? {
                crate::graph::local_rhs_exec::Start::Promoted { cell, next_op } => {
                    Some((cell, next_op))
                }
                crate::graph::local_rhs_exec::Start::Complete { cell, frame } => {
                    #[cfg(feature = "structural-depth")]
                    self.stage_spine.structural.capture(
                        &self.graph,
                        rid,
                        &b.known,
                        effect_receipts.as_deref().unwrap_or(&[]),
                    );
                    if let Some(cursor) = stage_cursor {
                        let (c, f, u) = stage_counts.unwrap();
                        self.stage_spine.effects(
                            self.graph.stats.constructor_requests - c,
                            self.graph.stats.function_updates - f,
                            self.graph.stats.union_requests - u,
                        );
                        let g = &self.graph;
                        self.stage_spine.capture_keys(
                            g,
                            rid,
                            cell,
                            cursor,
                            |i| b.known.get(i).map(Dependency::Fact),
                            effect_receipts.as_deref(),
                            |_, args| g.local_key(cell, &frame, args),
                        )?;
                    }
                    return Ok(());
                }
            }
        } else {
            ordinary(self, b.values, &mut effect_receipts)?
        };
        #[cfg(not(feature = "rhs-locals"))]
        let start = ordinary(self, b.values, &mut effect_receipts)?;
        let Some((cell, first_op)) = start else {
            #[cfg(feature = "structural-depth")]
            self.stage_spine.structural.capture(
                &self.graph,
                rid,
                &b.known,
                effect_receipts.as_deref().unwrap_or(&[]),
            );
            return Ok(());
        };

        if let Some((producer, depth, residual)) = origin {
            self.graph.rhs_frame_mut(cell).unwrap().recipe_depth = depth;
            self.handoffs.insert((producer, rid));
            if residual {
                self.continuation_stats.residual_bindings += 1;
            } else {
                self.continuation_stats.complete_bindings += 1;
            }
            self.continuation_stats.max_depth = self.continuation_stats.max_depth.max(depth);
        }
        let ops = self.graph.program.templates[template].ops.clone();
        let mut effect_receipts = effect_receipts.or_else(|| {
            self.stage_spine
                .capture_receipts(rid)
                .then(|| vec![None; ops.len()])
        });
        let mut completed = true;
        let mut completed_prefix = first_op;
        #[cfg(feature = "iterative-bake")]
        let mut transfers = available.as_slice();
        for (i, op) in ops.iter().enumerate().skip(first_op) {
            if self.graph.known_stats.elide_static(aliases, i) {
                completed_prefix = i + 1;
                continue;
            }

            #[cfg(feature = "iterative-bake")]
            if let Some((input, tail)) = transfers.split_first().filter(|(r, _)| r.op == i) {
                transfers = tail;
                self.graph.stats.handoff_attempts += 1;
                if let Some(f) = input.fact {
                    let (t, args) = match op {
                        Op::Let(_, Call::Table(t), a) | Op::Set(t, a, _) => (*t, a),
                        _ => unreachable!("validated constructor handoff"),
                    };
                    if let Some(row) = self.graph.handoff_row(f, cell, t, args)? {
                        let value = row.values.last().unwrap().clone();
                        match op {
                            Op::Let(dst, ..) => {
                                self.graph
                                    .rhs_frame_mut(cell)
                                    .unwrap()
                                    .values
                                    .insert(*dst, value);
                                self.graph.stats.handoff_lets += 1;
                            }
                            Op::Set(_, _, v) => {
                                let rhs = self.graph.argument(cell, v)?;
                                if input.equal_output && value == rhs {
                                    self.graph.stats.handoff_noop_unions += 1;
                                } else {
                                    self.graph.enqueue_union(value, rhs)?;
                                }
                                self.graph.stats.handoff_sets += 1;
                            }
                            _ => unreachable!(),
                        }
                        if let Some(e) = &mut effect_receipts {
                            e[i] = Some(row.witness);
                        }
                        #[cfg(feature = "structural-depth")]
                        if self.stage_spine.structural.execute
                            && origin.is_none()
                            && let Some((_, d)) = proven.iter().find(|(op, _)| *op == i)
                        {
                            self.stage_spine.structural.accepted(*d as usize);
                        }
                        self.graph.stats.handoff_deep += u64::from(input.back > 1);
                        completed_prefix = i + 1;
                        continue;
                    }
                    self.graph.stats.handoff_stale += 1;
                } else {
                    self.graph.stats.handoff_missing += 1;
                }
            }
            match op {
                Op::Copy(..) => {}
                Op::Let(dst, Call::Table(t), aa) => {
                    if self.graph.program.tables[*t].virtual_fact {
                        if let Some(receipts) = &mut effect_receipts {
                            let mut visible = false;
                            self.graph.construct_recorded(
                                cell,
                                i,
                                *t,
                                aa,
                                *dst,
                                &mut receipts[i],
                                &mut visible,
                            )?;
                        } else {
                            self.graph.construct(cell, i, *t, aa, *dst)?;
                        };
                    } else {
                        let key = self.graph.inline_arguments(cell, aa)?;
                        let row = self.graph.lookup_canonical(*t, &key)?.ok_or_else(|| {
                            format!(
                                "missing function read {}",
                                self.graph.program.tables[*t].name
                            )
                        })?;
                        if let Some(receipts) = &mut effect_receipts {
                            receipts[i] = Some(row.witness);
                        }
                        self.graph
                            .rhs_frame_mut(cell)
                            .unwrap()
                            .values
                            .insert(*dst, row.values.last().unwrap().clone());
                    }
                }
                Op::Let(dst, Call::Primitive(name, output), aa) => {
                    let args = self.graph.inline_arguments(cell, aa)?;
                    if name.starts_with("cell-key-") {
                        self.graph.stats.key_operations += 1;
                    }
                    let v =
                        value::primitive(&name, &args, &output, self.graph.program.key_nodes())?
                            .ok_or_else(|| format!("RHS primitive failed: {name}"))?;
                    self.graph
                        .rhs_frame_mut(cell)
                        .unwrap()
                        .values
                        .insert(*dst, v);
                }
                Op::Set(t, aa, v) => {
                    #[cfg(feature = "view-consumers")]
                    let source = b.view_source;
                    #[cfg(not(feature = "view-consumers"))]
                    let source = None;
                    self.graph.set_inner(
                        cell,
                        i,
                        *t,
                        aa,
                        v,
                        None,
                        effect_receipts.as_mut().map(|rs| &mut rs[i]),
                        source,
                    )?;
                }
                Op::Union(a, b) => {
                    let a = self.graph.argument(cell, &a)?;
                    let b = self.graph.argument(cell, &b)?;
                    self.graph.enqueue_union(a, b)?;
                }
                Op::Hide(t, aa, delete) => {
                    let key = self.graph.inline_arguments(cell, aa)?;
                    if !self.graph.hide(*t, &key, *delete)? {
                        completed_prefix = i + 1;
                        completed = false;
                        break;
                    }
                }
                Op::Panic => return Err("source panic".into()),
            }
            completed_prefix = i + 1;
        }
        if origin.is_some() {
            self.continuation_stats.published_facts +=
                self.graph.stats.logical_facts_created - facts;
            self.continuation_stats.function_updates += self.graph.stats.function_updates - updates;
        }
        let state = self.graph.rhs_frame_mut(cell).unwrap();
        state.completed = completed;
        state.completed_prefix = completed_prefix;
        if let Some((c, f, u)) = stage_counts {
            self.stage_spine.effects(
                self.graph.stats.constructor_requests - c,
                self.graph.stats.function_updates - f,
                self.graph.stats.union_requests - u,
            );
        }
        #[cfg(feature = "structural-depth")]
        if completed {
            self.stage_spine.structural.capture(
                &self.graph,
                rid,
                &b.known,
                effect_receipts.as_deref().unwrap_or(&[]),
            );
        }
        if completed && let Some(cursor) = stage_cursor {
            self.stage_spine.capture_step(
                &self.graph,
                rid,
                cell,
                cursor,
                |i| b.known.get(i).map(Dependency::Fact),
                effect_receipts.as_deref(),
            )?;
        }
        #[cfg(all(feature = "rhs-work-audit", feature = "rhs-locals"))]
        if let (Some(block), root) = inspect_births {
            for &op in &block.intermediate_ops {
                if self
                    .graph
                    .rhs_frame(cell)
                    .unwrap()
                    .facts
                    .contains_key(&(op as u32))
                {
                    let f = FactRef {
                        cell,
                        op: op as u32,
                    };
                    let table = self.graph.live_table(Dependency::Fact(f)).unwrap();
                    self.graph.rhs_births.insert(
                        f,
                        rid,
                        table,
                        root,
                        root && self.stage_spine.anchor_at(rid, op),
                    );
                }
            }
        }
        self.graph.release_empty(cell);
        Ok(())
    }
    fn probe(&mut self, set: &str) -> Result<bool> {
        let rules = self
            .graph
            .program
            .sets
            .get(set)
            .ok_or("unknown probe ruleset")?
            .clone();
        for r in rules {
            #[cfg(feature="scheduled-functions")]
            self.activate_function_readers(std::slice::from_ref(&r),true)?;
            if !self.matches(r, true)?.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn run(&mut self, set: &str, path: Rc<[usize]>, iterations: Rc<[u64]>) -> Result<bool> {
        if self.current_stage != set {
            self.current_stage.clear();
            self.current_stage.push_str(set);
        }
        #[cfg(feature = "iterative-bake")]
        {
            self.stage_spine.training.run = self.steps.len() + 1;
        }
        self.graph.receipt_forwards.clear();
        self.graph.epoch = self.graph.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.graph.changed = false;
        #[cfg(feature = "family-survey")]
        self.survey.start(self.graph.epoch, set);
        #[cfg(feature = "family-overlap")]
        self.overlap.start(self.graph.epoch, set);
        let start = Instant::now();
        if self.progress {
            eprintln!("[cell-cpu] Run {}: {set}", self.steps.len() + 1);
        }
        let (set_id, rules) = if let Some(sets) = &self.ready_sets {
            let (id,rules) = sets.select(set)?;
            (Some(id),rules)
        } else {
            (None,self.graph.program.sets.get(set).ok_or_else(||format!("unknown ruleset {set}"))?.clone())
        };
        #[cfg(feature = "sparse-cells")]
        if rules.is_empty() && !self.audit && !cfg!(feature="projection-semijoin")
            && let Some(id) = set_id
            && self.graph.settled() && self.stage_spine.settled()
            && self.entry_schedule.settled(&self.graph) && self.query_indexes.changes.is_empty() {
            // No dirty reader and no producer, repair or index work at this
            // source occurrence. Commit the empty audit and its real watermark.
            self.ready_sets.as_mut().unwrap().finish(id,self.graph.epoch);
            #[cfg(feature="entry-retirement")] self.entry_schedule.complete_consumers(&self.graph.program.sets[set],self.graph.epoch);
            #[cfg(feature = "slot-transfer-audit")]
            {
                crate::transfer_audit::record_run(std::iter::empty::<(&usize,&crate::slots::Slots)>())?;
                crate::transfer_audit::begin_writes();
                crate::transfer_audit::repaired();
                crate::transfer_audit::end_writes();
            }
            self.graph.stats.idle_scheduled_runs += 1;
            let elapsed = start.elapsed().as_secs_f64();
            self.steps.push(schedule::RunStep {
                ordinal:self.steps.len()+1, ruleset:set.into(), path, iterations,
                epoch:self.graph.epoch, logical_matches:0,local_bindings:0,executed_bindings:0,
                changed:false, live_cells:self.graph.cells.len(),elapsed_seconds:elapsed,
                phases:schedule::Phases{search:elapsed,rhs:0.0,repair:0.0,delivery_gc:0.0,query_update:0.0},
                json:Default::default(),
            });
            return Ok(false);
        }
        #[cfg(feature = "sparse-cells")]
        let mut work = std::mem::take(&mut self.work_scratch);
        #[cfg(not(feature = "sparse-cells"))]
        let mut work = vec![];
        // Every LHS is read before any effect is made visible. Prepared successor
        // recipes therefore cannot leak into this finite Run's matching snapshot.
        #[cfg(feature = "family-replay")]
        let run_start = self.survey.samples.len();
        #[cfg(feature = "structural-depth")]
        let marks = self
            .stage_spine
            .structural
            .measure_bindings()
            .then(|| self.watermarks.clone());
        #[cfg(feature="scheduled-functions")]
        self.activate_function_readers(&rules,false)?;
        let mut count = 0;
        for rid in rules {
            // Only semi-naive rules clear their dirty flag. An untriggered
            // source occurrence still advances its own watermark.
            #[cfg(not(any(feature = "family-survey", feature = "family-overlap")))]
            if !self.dirty[rid] && !self.profile_queries {
                if let Some(sets) = &mut self.ready_sets { sets.clean(rid); }
                self.watermarks[rid] = self.graph.epoch;
                continue;
            }
            let clock = self.profile_queries.then(Instant::now);
            let candidates = self.graph.stats.residual_candidates;
            if self.progress && self.profile_queries {
                eprintln!("[cell-cpu] query {rid}");
            }
            #[cfg(feature = "family-overlap")]
            crate::family_overlap::begin_reads(rid, self.watermarks[rid]);
            #[cfg(feature = "family-survey")]
            let sample = (
                self.dirty[rid] || !self.graph.program.rules[rid].seminaive,
                Instant::now(),
            );
            if let Some(sets) = &self.ready_sets {
                self.watermarks[rid] = self.watermarks[rid].max(sets.watermark(rid));
            }
            let bindings = self.matches(rid, false)?;
            #[cfg(feature = "family-survey")]
            self.survey.rule(
                self.graph.epoch,
                self.watermarks[rid],
                rid,
                sample.0,
                sample.1.elapsed().as_nanos() as u64,
                bindings.len(),
                self.graph.stats.residual_candidates - candidates,
                self.stage_spine.active_for_rule(rid),
                bindings
                    .iter()
                    .any(|b| b.parent.is_some() || b.continuation.is_some()),
            );
            #[cfg(feature = "family-overlap")]
            crate::family_overlap::end_reads();
            if let Some(clock) = clock {
                self.query_profile[rid].0 += 1;
                self.query_profile[rid].3 += bindings.len() as u64;
                self.query_profile[rid].1 += clock.elapsed().as_secs_f64();
                self.query_profile[rid].2 += self.graph.stats.residual_candidates - candidates;
            }
            self.watermarks[rid] = self.graph.epoch;
            if !self.dirty[rid] && let Some(sets) = &mut self.ready_sets { sets.clean(rid); }
            if !bindings.is_empty() {
                count += bindings.len();
                work.push((rid, bindings));
            }
        }
        if let (Some(id),Some(sets)) = (set_id,&mut self.ready_sets) {
            sets.finish(id,self.graph.epoch);
        }
        #[cfg(feature="entry-retirement")] self.entry_schedule.complete_consumers(&self.graph.program.sets[set],self.graph.epoch);
        let patch = StagePatch { groups: work, len: count };
        #[cfg(feature = "structural-depth")]
        let mut evidence = if let Some(marks) = marks {
            Some(
                patch.iter()
                    .map(|(r, b)| {
                        self.stage_spine.structural.prepare(
                            &self.graph,
                            *r,
                            &b.known,
                            &b.values,
                            marks[*r],
                        )
                    })
                    .collect::<Result<Vec<_>>>()?
                    .into_iter(),
            )
        } else {
            None
        };
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::record_run(patch.iter().map(|(rid, b)| (rid, &b.values)))?;
        let matches = patch.len;
        let search_end = Instant::now();
        let search_seconds = search_end.duration_since(start).as_secs_f64();
        #[cfg(feature = "family-replay")]
        self.replay
            .run(&self.graph, set, &patch, &self.survey.samples[run_start..])?;
        if self.audit {
            self.binding_audit.push(json!({"ruleset":set,"bindings":patch.iter().map(|(rid,b)|{let template=&self.graph.program.templates[self.graph.program.rules[*rid].template];json!({"rule":rid,"values":b.values.iter().map(|(i,v)|json!({"slot":i,"value":crate::value::exported(v,&template.sorts[*i])})).collect::<Vec<_>>()})}).collect::<Vec<_>>()}));
        }
        let phase = if self.audit || cfg!(feature = "family-replay") {
            Instant::now()
        } else { search_end };
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::begin_writes();
        let executed = patch.len;
        #[cfg(feature = "sparse-cells")]
        let binding_generation = self.graph.stats.unions;
        #[cfg(feature = "family-survey")]
        let mut rhs_clock: Option<(usize, Instant)> = None;
        #[cfg(feature = "rhs-work-audit")]
        {
            self.graph.rhs_births.reading = true;
        }
        #[cfg(feature = "sparse-cells")]
        let mut buffer = patch.groups;
        #[cfg(feature = "sparse-cells")]
        let groups = buffer.drain(..);
        #[cfg(not(feature = "sparse-cells"))]
        let groups = patch.groups.into_iter();
        for (rid, binding) in groups.flat_map(|(rid, rows)| rows.into_iter().map(move |b| (rid, b))) {
            #[cfg(feature = "family-survey")]
            let before = {
                if rhs_clock.as_ref().is_none_or(|(r, _)| *r != rid) {
                    if let Some((prev, clock)) = rhs_clock.replace((rid, Instant::now())) {
                        self.survey.rhs(prev, clock.elapsed().as_nanos() as u64);
                    }
                }
                self.stage_spine.survey_resumed()
            };
            #[cfg(feature = "rhs-work-audit")]
            let sample = self.ledger.begin(&self.graph, rid, &binding.values)?;
            #[cfg(all(feature = "rhs-work-audit", feature = "rhs-locals"))]
            let local_sample = (
                self.local_counts.block_local_intermediates,
                self.local_counts.local_complete,
            );
            #[cfg(feature = "sparse-cells")]
            { self.graph.sealed_generation = Some(binding_generation); }
            let applied = self.apply(
                rid,
                binding,
                #[cfg(feature = "structural-depth")]
                evidence.as_mut().and_then(Iterator::next),
            );
            #[cfg(feature = "sparse-cells")]
            {
                self.graph.sealed_input_cell = None;
                self.graph.sealed_generation = None;
            }
            #[cfg(feature = "sparse-cells")]
            if applied.is_err() {
                self.graph.promote_active();
            }
            applied?;
            #[cfg(feature = "rhs-work-audit")]
            {
                #[cfg(feature = "rhs-locals")]
                self.ledger.locals(local_sample, &self.local_counts);
                self.ledger.end(&self.graph, sample)?;
            }
            #[cfg(feature = "family-survey")]
            self.survey
                .resumed(rid, self.stage_spine.survey_resumed() - before);
        }
        #[cfg(feature = "sparse-cells")]
        {
            self.work_scratch = buffer;
        }
        #[cfg(feature = "family-survey")]
        if let Some((r, clock)) = rhs_clock {
            self.survey.rhs(r, clock.elapsed().as_nanos() as u64);
        }

        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::repaired();
        let rhs_end = Instant::now();
        let rhs_seconds = rhs_end.duration_since(phase).as_secs_f64();
        self.graph.stats.bindings += executed as u64;
        let phase = rhs_end;
        #[cfg(feature = "family-survey")]
        let unions_before = self.graph.stats.unions;
        #[cfg(feature = "rhs-work-audit")]
        {
            self.graph.rhs_births.reading = false;
        }
        #[cfg(feature = "keeper-proof")]
        self.graph
            .proof
            .before_repair(&self.graph.parents, &self.graph.pending_unions);
        self.graph.repair()?;
        #[cfg(feature = "rhs-work-audit")]
        {
            let mut births = std::mem::take(&mut self.graph.rhs_births);
            self.ledger.observe_births(&mut self.graph, &mut births)?;
            self.graph.rhs_births = births;
        }
        let repair_end = Instant::now();
        let repair_seconds = repair_end.duration_since(phase).as_secs_f64();
        #[cfg(feature = "family-frontiers")]
        {
            self.prior_union = unions_before != self.graph.stats.unions;
        }
        let phase = repair_end;
        self.stage_spine.publish(&mut self.graph)?;
        #[cfg(feature="scheduled-functions")]
        self.scheduled_functions.capture(&self.graph,|t|
            self.query_indexes.has_columns(t) || self.entry_schedule.observes_table(t)
            || self.neighbor.observes(t) || self.range_index.as_ref().is_some_and(|r|r.observes(t)));
        #[cfg(feature = "structural-depth")]
        self.stage_spine.structural.publish(&mut self.graph)?;
        #[cfg(all(feature = "view-consumers",not(feature="scheduled-functions")))]
        self.entry_schedule
            .publish_except(&self.graph, |t| self.blocks.dedicated(t));
        #[cfg(all(not(feature = "view-consumers"),not(feature="scheduled-functions")))]
        self.entry_schedule.publish(&self.graph);
        #[cfg(feature="scheduled-functions")]
        self.entry_schedule.publish_except(&self.graph,|t|{
            #[cfg(feature="view-consumers")]
            if self.blocks.dedicated(t) {return true}
            self.scheduled_functions.defers(t)
        });
        self.graph.receipt_forwards.clear();
        self.publication.publish(
            #[cfg(feature = "family-survey")]
            (&mut self.survey, unions_before != self.graph.stats.unions),
            &self.graph,
            #[cfg(feature="scheduled-functions")]
            &self.scheduled_functions,
            &mut self.query_indexes,
            self.range_index.as_mut(),
            &mut self.neighbor,
            |_table, _changes, _indexed| {
                #[cfg(feature = "view-consumers")]
                self.blocks
                    .publish_table(&self.graph, _table, _changes, _indexed);
            },
        )?;
        #[cfg(feature = "projection-semijoin")]
        self.free_join
            .gates
            .publish(&self.graph, &self.query_indexes);
        self.sieve.publish(&mut self.query_indexes);
        let publication_end = Instant::now();
        let query_update_seconds = publication_end.duration_since(phase).as_secs_f64();
        let phase = publication_end;
        self.deliver();
        #[cfg(feature = "rhs-work-audit")]
        self.ledger.barrier(
            &self.graph,
            self.publication.index_writes(),
            self.entry_schedule.seed_deliveries(),
        );
        #[cfg(feature = "family-replay")]
        self.replay
            .publish(&self.graph, unions_before != self.graph.stats.unions)?;
        self.graph.changes.clear();
        self.graph.collect_garbage();
        let end = Instant::now();
        let delivery_seconds = end.duration_since(phase).as_secs_f64();
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::end_writes();
        let changed = self.graph.changed;
        let ordinal=self.steps.len()+1;
        if changed {self.last_change=ordinal;}
        self.steps.push(schedule::RunStep {ordinal,ruleset:set.into(),path,iterations,
            epoch:self.graph.epoch, logical_matches:matches,local_bindings:0,executed_bindings:executed,
            changed,live_cells:self.graph.cells.len(),elapsed_seconds:if cfg!(feature="slot-transfer-audit") { start.elapsed() } else { end.duration_since(start) }.as_secs_f64(),
            phases:schedule::Phases{search:search_seconds,rhs:rhs_seconds,repair:repair_seconds,delivery_gc:delivery_seconds,query_update:query_update_seconds},json:Default::default()});
        if self.audit {
            self.snapshots.push(self.graph.export()?);
        }
        Ok(changed)
    }
    pub fn execute(&mut self) -> Result<()> {
        self.execute_until(None)
    }
    pub fn execute_until(&mut self, stop: Option<usize>) -> Result<()> {
        while !self.tasks.is_empty() {
            if stop.is_some_and(|n| self.steps.len() >= n) {
                break;
            }
            match self.tasks.pop().unwrap() {
                Task::Eval {
                    node,
                    path,
                    iterations,
                } => {
                    use schedule::Instruction;
                    match node.instruction() {
                        Instruction::Run {
                            set,
                            until_run,
                            check,
                            unlowered,
                        } => {
                            if let Some(probe) = until_run {
                                if self.probe(probe)? {
                                    continue;
                                }
                            }
                            if *check {
                                if !self.probe(set)? {
                                    return Err(format!("source check failed: {set}"));
                                }
                                continue;
                            }
                            if *unlowered {
                                return Err("unlowered source probe".into());
                            }
                            self.run(set, path, iterations)?;
                        }
                        Instruction::Sequence(seq) => {
                            for (i, child) in seq.iter().enumerate().rev() {
                                let p = if node.static_path.as_ref().is_some_and(|base| Rc::ptr_eq(base,&path)) {
                                    child.static_path.clone().expect("compiled child path")
                                } else {
                                    path.iter().copied().chain(std::iter::once(i)).collect()
                                };
                                self.tasks.push(Task::Eval {
                                    node: child.clone(),
                                    path: p,
                                    iterations: iterations.clone(),
                                });
                            }
                        }
                        Instruction::Repeat(count, body) => {
                            if *count > 0 {
                                self.tasks.push(Task::Repeat {
                                    body: body.clone(),
                                    count: *count,
                                    next: 0,
                                    start: self.steps.len(),
                                    path,
                                    iterations,
                                });
                            }
                        }
                        Instruction::Saturate(body) => {
                            let it: Rc<[u64]> = iterations.iter().copied().chain(std::iter::once(0)).collect();
                            self.tasks.push(Task::Saturate {
                                body: body.clone(),
                                iteration: 0,
                                start: self.steps.len(),
                                path: path.clone(),
                                iterations,
                            });
                            self.tasks.push(Task::Eval {
                                node: body.clone(),
                                path,
                                iterations: it,
                            });
                        }
                        Instruction::Unknown => return Err("unknown schedule instruction".into()),
                    }
                }
                Task::Repeat {
                    body,
                    count,
                    next,
                    start,
                    path,
                    iterations,
                } => {
                    if next > 0 && self.last_change <= start {
                        continue;
                    }
                    if next < count {
                        let it: Rc<[u64]> = iterations.iter().copied().chain(std::iter::once(next)).collect();
                        self.tasks.push(Task::Repeat {
                            body: body.clone(),
                            count,
                            next: next + 1,
                            start: self.steps.len(),
                            path: path.clone(),
                            iterations,
                        });
                        self.tasks.push(Task::Eval {
                            node: body,
                            path,
                            iterations: it,
                        });
                    }
                }
                Task::Saturate {
                    body,
                    iteration,
                    start,
                    path,
                    iterations,
                } => {
                    if self.last_change > start {
                        let iteration = iteration
                            .checked_add(1)
                            .ok_or("saturation occurrence exhausted")?;
                        let it: Rc<[u64]> = iterations.iter().copied().chain(std::iter::once(iteration)).collect();
                        self.tasks.push(Task::Saturate {
                            body: body.clone(),
                            iteration,
                            start: self.steps.len(),
                            path: path.clone(),
                            iterations,
                        });
                        self.tasks.push(Task::Eval {
                            node: body,
                            path,
                            iterations: it,
                        });
                    } else {
                    }
                }
            }
        }
        Ok(())
    }
    #[cfg(feature = "family-survey")]
    pub fn family_survey(&self) -> J {
        #[allow(unused_mut)]
        let mut report = self.survey.report();
        report["program_sha256"] = self.graph.program.artifact["sha256"].clone();
        report["schema"] = "eggfuse-family-survey/v1".into();
        #[cfg(feature = "family-scan")]
        {
            report["raw_scan"] = self.raw_scan.report();
        }
        report["view_ports"] = cfg!(feature = "view-ports").into();
        #[cfg(feature = "family-frontiers")]
        {
            report["entry_frontiers"] = self.fronts.report();
            report["entry_batches"] = json!(self.fronts.batches);
        }

        #[cfg(feature = "family-overlap")]
        {
            report["overlap"] = self.overlap.report();
        }
        #[cfg(feature = "family-replay")]
        {
            report["replay"] = self.replay.report();
        }
        report
    }
    #[cfg(feature = "structural-depth")]
    pub fn configure_depth(&mut self, d: u8, survey: bool) -> Result<()> {
        if !self.steps.is_empty() {
            return Err("configure structural ports before execution".into());
        }
        self.stage_spine.structural.configure(d, survey)?;
        self.stage_spine.track_ports.fill(0);
        if d > 0 {
            self.stage_spine.track_ports = self
                .graph
                .program
                .rules
                .iter()
                .map(|r| r.query.len())
                .collect();
        }
        Ok(())
    }
    #[cfg(feature = "structural-depth")]
    pub fn use_depth_catalog(&mut self, catalog: J) -> Result<()> {
        if !self.steps.is_empty() {
            return Err("install structural catalog before execution".into());
        }
        self.stage_spine
            .structural
            .specialize(&self.graph.program, catalog)?;
        for (r, slots) in self.stage_spine.track_ports.iter_mut().enumerate() {
            *slots = if self.stage_spine.structural.capture_rule(r) {
                self.graph.program.rules[r].query.len()
            } else {
                0
            };
        }
        Ok(())
    }
    #[cfg(feature = "structural-depth")]
    pub fn depth_report(&self) -> J {
        self.stage_spine.structural.report(&self.graph.program)
    }
    #[cfg(feature = "rhs-work-audit")]
    pub fn rhs_cohort(&mut self, path: &std::path::Path) -> Result<()> {
        let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        self.ledger.load(
            &self.graph.program,
            serde_json::from_reader(std::io::BufReader::new(f)).map_err(|e| e.to_string())?,
        )
    }
    #[cfg(feature = "rhs-work-audit")]
    pub fn write_rhs_work(&self, out: &std::path::Path) -> Result<()> {
        self.ledger.write(&self.graph, out)
    }
    #[cfg(feature = "rhs-locals")]
    pub fn configure_rhs_locals(&mut self, width: usize) -> Result<()> {
        if !self.steps.is_empty() {
            return Err("configure RHS blocks before execution".into());
        }
        self.local_width = width;
        Ok(())
    }
    #[cfg(feature = "rhs-locals")]
    pub fn use_rhs_local_profile(&mut self, profile: &J) -> Result<()> {
        if !self.steps.is_empty() || self.local_width == 0 {
            return Err("a local RHS profile requires positive width before execution".into());
        }
        if profile["schema"] != "eggfuse/rhs-work-admission/v1" {
            return Err("unknown local RHS profile".into());
        }
        let shapes: BTreeSet<Vec<[String; 2]>> =
            serde_json::from_value(profile["shapes"].clone()).map_err(|e| e.to_string())?;
        self.local_selection = Some(
            (0..self.graph.program.rules.len())
                .map(|r| {
                    self.graph
                        .program
                        .rhs_operator_shape(r)
                        .is_some_and(|s| shapes.contains(&s))
                })
                .collect(),
        );
        Ok(())
    }
    #[cfg(feature = "rhs-locals")]
    pub fn rhs_local_report(&self) -> J {
        serde_json::json!({"width":self.local_width,"admitted_rules":self.local_selection.as_ref().map(|v|v.iter().filter(|x|**x).count()),"stats":self.local_counts})
    }
    #[cfg(feature = "keeper-proof")]
    pub fn read_keepers(&mut self, path: &std::path::Path) -> Result<()> {
        let mut proof = crate::prefix_proof::Proof::default();
        proof.load(&self.graph, path)?;
        self.graph.proof = proof;
        Ok(())
    }
    #[cfg(feature = "keeper-proof")]
    pub fn write_keepers(&self, path: &std::path::Path) -> Result<()> {
        self.graph.proof.write(&self.graph, path)
    }
    #[cfg(feature = "entry-retirement")]
    pub fn configure_entry_retirement(&mut self, interval: u64) -> Result<()> {
        if interval == 0 || !self.steps.is_empty() {
            return Err("configure positive retirement interval before execution".into());
        }
        self.entry_schedule.retirement_interval = interval;
        Ok(())
    }
    /// Discard source/validation IR after all runtime plans are constructed.
    /// The remaining artifact is provenance metadata, and is intentionally
    /// rejected by Program::parse. Offline Bake requires reloading the source.
    pub fn release_validation_artifact(&mut self) -> Result<()> {
        if !self.steps.is_empty() {
            return Err("release validation IR before execution".into());
        }
        let p = &mut self.graph.program;
        if p.artifact["schema"] == "eggfuse-execution-provenance/v1" {
            return Ok(());
        }
        let sha = p.artifact["sha256"].take();
        let recipe = p.artifact["recipe"]["sha256"].take();
        let nodes = p.artifact["source_core"]["structural_key"]["nodes"].take();
        p.artifact = serde_json::json!({"schema":"eggfuse-execution-provenance/v1","sha256":sha,"recipe":{"sha256":recipe},"source_core":{"structural_key":{"nodes":nodes}}});
        Ok(())
    }
    #[cfg(feature="two-run-blocks")]
    pub fn configure_two_run(&mut self,on:bool){
        if on {self.graph.configure_port_frames(true);}
        self.graph.configure_two_run(on);
        self.stage_spine.configure_two_run(&self.graph.program,on);
    }
    pub fn report(&self) -> J {
        #[cfg(feature = "deferred-incidence")]
        let deferred = json!({"enabled": self.graph.deferred_incidence.enabled,
            "admitted_positions": self.graph.program.deferred_incidence.iter().flatten().filter(|x|**x).count(),
            "payloads": self.graph.deferred_incidence.payloads,
            "reads_before_repair": self.graph.deferred_incidence.reads_before_repair.get(),
            "stats": self.graph.deferred_incidence.stats,
            "pending": self.graph.deferred_incidence.live.len()});
        #[cfg(not(feature = "deferred-incidence"))]
        let deferred = J::Null;
        let view_rule_profile=self.profile_queries.then(||{
            self.graph.program.view_blocks.iter().flat_map(|b|b.producer.members.iter().chain(&b.consumers.members))
                .map(|m|{let (calls,seconds,visits,answers)=self.query_profile[m.rule];
                    json!({"rule":m.rule,"calls":calls,"search_s":seconds,"residual_visits":visits,"bindings":answers})
                }).collect::<Vec<_>>()
        });
        #[cfg(feature = "view-consumers")]
        let views = self.blocks.report(&self.graph, &self.query_indexes);
        #[cfg(not(feature = "view-consumers"))]
        let views = J::Null;
        #[cfg(feature = "projection-survey")]
        let survey = self.free_join.survey.report();
        #[cfg(not(feature = "projection-survey"))]
        let survey = J::Null;
        json!({"backend":"cell-cpu","scope":"standalone Cell recipe executor","status":if self.tasks.is_empty(){"complete"}else{"checkpointed"},
            "deferred_incidence":deferred,"indexed_columns":self.query_indexes.column_storage(),"stats":self.graph.stats,"live_cells":self.graph.cells.len(),"steps":self.steps,"program_sha256":self.graph.program.artifact["sha256"],
            "view_rule_profile":view_rule_profile,"view_consumers":views,"view_storage":self.graph.unit_stats(),"publication":self.publication,
            "semijoin":self.free_join.semijoin_report(),"projection_survey":survey,"entry_schedule":self.entry_schedule.report(),"free_join":self.free_join.stats,"sieve":self.sieve.report(),"neighbor_context":self.neighbor.report(&self.graph.program),
            "local_context":self.local_context.report(&self.graph.program),"stage_spine":self.stage_spine.report(),"key_lookup_calls":self.graph.key_lookup_calls.get(),
            "multi_stage":{"stats":self.continuation_stats},"physical":{"range":self.range_index.as_ref().map(|r|&r.stats)}})
    }
}
