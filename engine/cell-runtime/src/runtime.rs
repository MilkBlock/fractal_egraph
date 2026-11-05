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
use free_join::{FreeJoin, Subtrie};
#[path = "execute_kernel.rs"]
mod execute_kernel;
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

#[derive(Clone, Debug)]
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
    bindings: Vec<(usize, Binding)>,
}
pub struct Executor {
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
    pub steps: Vec<J>,
    watermarks: Vec<u64>,
    dirty: Vec<bool>,
    pending: Vec<BTreeSet<Dependency>>,
    readers: Vec<Vec<usize>>,
    pub audit: bool,
    pub snapshots: Vec<J>,
    pub binding_audit: Vec<J>,
    pub initial_audit: Option<J>,
    pub tasks: Vec<Task>,

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
    domains: Vec<Subtrie>,
}
impl QueryEnv {
    fn new(slots: usize, known: QueryFacts) -> Self {
        Self {
            known,

            values: crate::entry_schedule::Inputs::owned(slots),
            trail: vec![],
            domains: vec![],
        }
    }
    fn shared(seed: crate::entry_schedule::Seed, known: QueryFacts) -> Self {
        Self {
            known,
            values: crate::entry_schedule::Inputs::shared(seed),
            trail: vec![],
            domains: vec![],
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
            self.values[self.trail.pop().unwrap()] = None;
        }
    }
    fn binding(&self) -> crate::slots::Slots {
        self.values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.clone().map(|v| (i, v)))
            .collect()
    }
}
fn operand(a: &Arg, env: &QueryEnv, g: &Graph) -> Option<Value> {
    match a {
        Arg::Literal(v) => Some(canon(v, &g.parents)),
        Arg::Slot(i) => env.values[*i].clone(),
    }
}
fn bind(a: &Arg, v: &Value, env: &mut QueryEnv, g: &Graph) -> bool {
    // The graph is immutable throughout matching, so assigned values stay canonical.
    // All callers provide a canonical row, local slot, or primitive output.
    // No writes are visible during this immutable matching snapshot.
    match a {
        Arg::Literal(l) => canon(l, &g.parents) == *v,
        Arg::Slot(i) => {
            if let Some(old) = &env.values[*i] {
                old == v
            } else {
                env.values[*i] = Some(v.clone());
                env.trail.push(*i);
                true
            }
        }
    }
}
impl Executor {
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
        for (rid, r) in graph.program.rules.iter().enumerate() {
            for a in r.query.iter() {
                if let Call::Table(t) = a.call {
                    if !readers[t].contains(&rid) {
                        readers[t].push(rid)
                    }
                }
            }
        }
        let n = graph.program.rules.len();
        let tasks = schedule::initial(&graph.program.control);
        let initial_audit = if audit { graph.export().ok() } else { None };
        let sieve = crate::sieve::Sieve::new(&graph.program);
        let mut query_indexes = QueryIndexes::default();
        query_indexes.track_changes = sieve.enabled;
        let mut stage_spine = crate::stage_spine::Programs::new(&graph.program);
        stage_spine.audit_receipts = audit;
        let free_join = FreeJoin::new(&graph.program);
        let entry_schedule = crate::entry_schedule::Scheduler::with_epoch_counts(
            &graph.program,
            true,
            free_join.planned,
        );
        graph.receipt_tracking =
            (stage_spine.enabled && stage_spine.receipts) || entry_schedule.enabled;
        let mut e = Self {
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
            graph,
            steps: vec![],
            watermarks: vec![0; n],
            dirty: vec![true; n],
            pending: vec![BTreeSet::new(); n],
            readers,
            audit,
            snapshots: vec![],
            binding_audit: vec![],
            initial_audit,
            tasks,

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
                self.dirty[rid] = true;
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
        let mut best = None;
        for (i, atom) in query.iter().enumerate().filter(|(i, _)| remaining[*i]) {
            let bound = atom
                .args
                .iter()
                .map(|a| operand(a, env, &self.graph))
                .collect::<Vec<_>>();
            let key_bound = bound[..bound.len() - 1].iter().all(Option::is_some);
            let cost = match atom.call {
                Call::Primitive(..) if key_bound => 0,
                Call::Primitive(..) => continue,
                Call::Table(_) if key_bound => 1,
                Call::Table(t) => {
                    2 + {
                        let n = self.query_indexes.count(&self.graph, t, &bound)?;
                        env.domain(i).map_or(n, |d| n.min(d.len()))
                    }
                }
            };
            if best.is_none_or(|b| (cost, i) < b) {
                best = Some((cost, i));
            }
        }
        let chosen = best.ok_or("unbound primitive query cycle")?.1;
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
                if let Some(v) = value::primitive(
                    name,
                    &vals,
                    output,
                    &self.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                )? {
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
                    Rc::new(
                        self.graph
                            .lookup(*t, &key)?
                            .into_iter()
                            .map(|r| r.witness)
                            .collect(),
                    )
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
        let atom = &query[chosen];
        for w in witnesses.iter().copied() {
            if !self.sieve.allow(rid, chosen, table, w, &self.query_indexes) {
                continue;
            }
            self.graph.stats.residual_candidates += 1;
            self.graph.stats.decoded_rows += 1;
            let Some(r) = self.graph.row(w)? else {
                continue;
            };
            // Disjoint partition by latest committed witness, then atom.
            // Within one RHS, later recipe positions (outer constructors)
            // own the match instead of their earlier constituent leaves.
            if r.hidden {
                continue;
            }
            if delta.is_some_and(|(pivot, epoch, witness)| {
                (r.epoch, r.witness) > (epoch, witness)
                    || ((r.epoch, r.witness) == (epoch, witness) && chosen < pivot)
            }) {
                continue;
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
        }
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
                    let Some(value) = value::primitive(
                        name,
                        &args,
                        output,
                        &self.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                    )?
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
                if let Some(v) = value::primitive(
                    name,
                    &args,
                    sort,
                    &self.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                )? {
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
        let empty = self.sieve.empty_join(rid);
        if empty {
            self.pending[rid].clear();
        }
        Ok(empty)
    }
    fn matches(&mut self, rid: usize, probe: bool) -> Result<Vec<Binding>> {
        let rule = self.graph.program.rules[rid].clone();

        if !probe && rule.seminaive && !std::mem::replace(&mut self.dirty[rid], false) {
            return Ok(vec![]);
        }
        // No join can succeed if a required relation is empty. This is a query
        // property, not a Cell proof or a prediction about future stages.
        if rule.query.iter().any(|a| match a.call {
            Call::Table(t) => self.graph.witness_count(t) == 0,
            _ => false,
        }) {
            if !probe {
                self.pending[rid].clear();
            }
            return Ok(vec![]);
        }
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
                        #[cfg(feature = "family-frontiers")]
                        let gate_clock = Instant::now();
                        let atom = batch.atom();
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
                                spine_routes.push((out.len(), cursor.clone()));
                                // Keep domain slots aligned with the shared seed vector.
                                if early_join {
                                    entry_domains.push(Vec::new());
                                }
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
    fn apply(&mut self, rid: usize, b: Binding) -> Result<()> {
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
            let (cell, index, key, changed) = self
                .graph
                .apply_write(template, b.values, b.parent, &write)?;
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
        #[cfg(feature = "iterative-bake")]
        if !available.is_empty()
            && stage_cursor
                .as_ref()
                .is_some_and(|s| s.closed(&self.graph.program.iterative))
        {
            if let Some((cell, mut keys, receipts, unions)) =
                self.graph.finish_handoff(template, &b.values, &available)?
            {
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
        let static_skip = {
            self.graph
                .program
                .known_results
                .static_skip
                .get(rid)
                .cloned()
        };
        let aliases = static_skip.as_deref().map(Vec::as_slice).filter(|_| true);
        let (cell, first_op) = if self.graph.program.rules[rid].constructor_only
            && stage_cursor.is_none()
            && origin.is_none()
        {
            let Some(start) = self
                .graph
                .start_constructor_rhs(template, b.values, b.parent, aliases)?
            else {
                return Ok(());
            };
            start
        } else {
            (self.graph.linked_slots(template, b.values, b.parent)?, 0)
        };

        if let Some((producer, depth, residual)) = origin {
            self.graph.cells.get_mut(&cell).unwrap().recipe_depth = depth;
            self.handoffs.insert((producer, rid));
            if residual {
                self.continuation_stats.residual_bindings += 1;
            } else {
                self.continuation_stats.complete_bindings += 1;
            }
            self.continuation_stats.max_depth = self.continuation_stats.max_depth.max(depth);
        }
        let ops = self.graph.program.templates[template].ops.clone();
        let mut effect_receipts = self
            .stage_spine
            .capture_receipts(rid)
            .then(|| vec![None; ops.len()]);
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
                                    .cells
                                    .get_mut(&cell)
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
                        let key = self.graph.arguments(cell, &aa)?;
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
                            .cells
                            .get_mut(&cell)
                            .unwrap()
                            .values
                            .insert(*dst, row.values.last().unwrap().clone());
                    }
                }
                Op::Let(dst, Call::Primitive(name, output), aa) => {
                    let args = self.graph.arguments(cell, &aa)?;
                    if name.starts_with("cell-key-") {
                        self.graph.stats.key_operations += 1;
                    }
                    let v = value::primitive(
                        &name,
                        &args,
                        &output,
                        &self.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                    )?
                    .ok_or_else(|| format!("RHS primitive failed: {name}"))?;
                    self.graph
                        .cells
                        .get_mut(&cell)
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
                    let key = self.graph.arguments(cell, &aa)?;
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
        let state = self.graph.cells.get_mut(&cell).unwrap();
        state.completed = completed;
        state.completed_prefix = completed_prefix;
        if let Some((c, f, u)) = stage_counts {
            self.stage_spine.effects(
                self.graph.stats.constructor_requests - c,
                self.graph.stats.function_updates - f,
                self.graph.stats.union_requests - u,
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
            if !self.matches(r, true)?.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn run(&mut self, set: &str, path: &[usize], iterations: &[u64]) -> Result<bool> {
        self.current_stage = set.into();
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
        let rules = self
            .graph
            .program
            .sets
            .get(set)
            .ok_or_else(|| format!("unknown ruleset {set}"))?
            .clone();
        let mut work = vec![];
        // Every LHS is read before any effect is made visible. Prepared successor
        // recipes therefore cannot leak into this finite Run's matching snapshot.
        #[cfg(feature = "family-replay")]
        let run_start = self.survey.samples.len();
        for rid in rules {
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
            work.extend(bindings.into_iter().map(|b| (rid, b)));
        }
        let patch = StagePatch { bindings: work };
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::record_run(patch.bindings.iter().map(|(rid, b)| (rid, &b.values)))?;
        let matches = patch.bindings.len();
        let search_seconds = start.elapsed().as_secs_f64();
        #[cfg(feature = "family-replay")]
        self.replay
            .run(&self.graph, set, &patch, &self.survey.samples[run_start..])?;
        if self.audit {
            self.binding_audit.push(json!({"ruleset":set,"bindings":patch.bindings.iter().map(|(rid,b)|{let template=&self.graph.program.templates[self.graph.program.rules[*rid].template];json!({"rule":rid,"values":b.values.iter().map(|(i,v)|json!({"slot":i,"value":crate::value::exported(v,&template.sorts[*i])})).collect::<Vec<_>>()})}).collect::<Vec<_>>()}));
        }
        let phase = Instant::now();
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::begin_writes();
        let executed = patch.bindings.len();
        #[cfg(feature = "family-survey")]
        let mut rhs_clock: Option<(usize, Instant)> = None;
        for (rid, binding) in patch.bindings {
            #[cfg(feature = "family-survey")]
            let before = {
                if rhs_clock.as_ref().is_none_or(|(r, _)| *r != rid) {
                    if let Some((prev, clock)) = rhs_clock.replace((rid, Instant::now())) {
                        self.survey.rhs(prev, clock.elapsed().as_nanos() as u64);
                    }
                }
                self.stage_spine.survey_resumed()
            };
            self.apply(rid, binding)?;
            #[cfg(feature = "family-survey")]
            self.survey
                .resumed(rid, self.stage_spine.survey_resumed() - before);
        }
        #[cfg(feature = "family-survey")]
        if let Some((r, clock)) = rhs_clock {
            self.survey.rhs(r, clock.elapsed().as_nanos() as u64);
        }

        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::repaired();
        let rhs_seconds = phase.elapsed().as_secs_f64();
        self.graph.stats.bindings += executed as u64;
        let phase = Instant::now();
        #[cfg(feature = "family-survey")]
        let unions_before = self.graph.stats.unions;
        self.graph.repair()?;
        let repair_seconds = phase.elapsed().as_secs_f64();
        #[cfg(feature = "family-frontiers")]
        {
            self.prior_union = unions_before != self.graph.stats.unions;
        }
        let phase = Instant::now();
        self.stage_spine.publish(&mut self.graph)?;
        #[cfg(feature = "view-consumers")]
        self.entry_schedule
            .publish_except(&self.graph, |t| self.blocks.dedicated(t));
        #[cfg(not(feature = "view-consumers"))]
        self.entry_schedule.publish(&self.graph);
        self.graph.receipt_forwards.clear();
        self.publication.publish(
            #[cfg(feature = "family-survey")]
            (&mut self.survey, unions_before != self.graph.stats.unions),
            &self.graph,
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
        let query_update_seconds = phase.elapsed().as_secs_f64();
        let phase = Instant::now();
        self.deliver();
        #[cfg(feature = "family-replay")]
        self.replay
            .publish(&self.graph, unions_before != self.graph.stats.unions)?;
        self.graph.changes.clear();
        self.graph.collect_garbage();
        let delivery_seconds = phase.elapsed().as_secs_f64();
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::end_writes();
        let changed = self.graph.changed;
        self.steps.push(json!({"ordinal":self.steps.len()+1,"ruleset":set,"path":path,"iterations":iterations,"epoch":self.graph.epoch,"logical_matches":matches,"local_bindings":0,"executed_bindings":executed,"changed":changed,"live_cells":self.graph.cells.len(),"elapsed_seconds":start.elapsed().as_secs_f64(),"phases":{"search":search_seconds,"rhs":rhs_seconds,"repair":repair_seconds,"delivery_gc":delivery_seconds,"query_update":query_update_seconds}}));
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
                    if let Some(set) = node["run"].as_str() {
                        if let Some(probe) = node["until_run"].as_str() {
                            if self.probe(probe)? {
                                continue;
                            }
                        }
                        if node["assert"].as_bool() == Some(true) {
                            if !self.probe(set)? {
                                return Err(format!("source check failed: {set}"));
                            }
                            continue;
                        }
                        if !node["until"].is_null() || !node["assert_table"].is_null() {
                            return Err("unlowered source probe".into());
                        }
                        self.run(set, &path, &iterations)?;
                    } else if let Some(seq) = node["sequence"].as_array() {
                        for (i, child) in seq.iter().enumerate().rev() {
                            let mut p = path.clone();
                            p.push(i);
                            self.tasks.push(Task::Eval {
                                node: child.clone(),
                                path: p,
                                iterations: iterations.clone(),
                            });
                        }
                    } else if let Some(count) = node["repeat"].as_u64() {
                        if count > 0 {
                            self.tasks.push(Task::Repeat {
                                body: node["body"].clone(),
                                count,
                                next: 0,
                                start: self.steps.len(),
                                path,
                                iterations,
                            });
                        }
                    } else if !node["saturate"].is_null() {
                        let body = node["saturate"].clone();
                        let mut it = iterations.clone();
                        it.push(0);
                        self.tasks.push(Task::Saturate {
                            body: body.clone(),
                            iteration: 0,
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
                        return Err("unknown schedule instruction".into());
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
                    if next > 0 && !self.steps[start..].iter().any(|s| s["changed"] == true) {
                        continue;
                    }
                    if next < count {
                        let mut it = iterations.clone();
                        it.push(next);
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
                    if self.steps[start..].iter().any(|s| s["changed"] == true) {
                        let iteration = iteration
                            .checked_add(1)
                            .ok_or("saturation occurrence exhausted")?;
                        let mut it = iterations.clone();
                        it.push(iteration);
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
    pub fn report(&self) -> J {
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
            "stats":self.graph.stats,"live_cells":self.graph.cells.len(),"steps":self.steps,"program_sha256":self.graph.program.artifact["sha256"],
            "view_rule_profile":view_rule_profile,"view_consumers":views,"view_storage":self.graph.unit_stats(),"publication":self.publication,
            "semijoin":self.free_join.semijoin_report(),"projection_survey":survey,"entry_schedule":self.entry_schedule.report(),"free_join":self.free_join.stats,"sieve":self.sieve.report(),"neighbor_context":self.neighbor.report(&self.graph.program),
            "local_context":self.local_context.report(&self.graph.program),"stage_spine":self.stage_spine.report(),"key_lookup_calls":self.graph.key_lookup_calls.get(),
            "multi_stage":{"stats":self.continuation_stats},"physical":{"range":self.range_index.as_ref().map(|r|&r.stats)}})
    }
}
