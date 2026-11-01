use crate::query::QueryIndexes;
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
    composition: Option<(Rc<crate::compositions::Edge>, crate::views::Context)>,
    witnesses: MatchedFacts,
    values: BTreeMap<usize, Value>,
    epoch: u64,
    parent: Option<FactRef>,
    continuation: Option<(usize, u32, bool)>,
    compiled: bool,
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct CompositionStats {
    match_seconds: f64,
    entry_seconds: f64,
    attempts: u64,
    bindings: u64,
    rejected: u64,
    unavailable: u64,
    known_atoms: u64,
    port_reads: u64,
    guard_checks: u64,
    constructor_reuses: u64,
    reuse_fallbacks: u64,
    root_steps: u64,
    continued_steps: u64,
    marker_continued_steps: u64,
    max_steps: u32,
    source_rules: BTreeSet<usize>,
}
enum ComposedMatch {
    Fallback,
    Rejected,
    Matched(Binding),
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct ContinuationStats {
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
    pub compositions_enabled: bool,
    composition_stats: CompositionStats,
    pub graph: Graph,
    pub steps: Vec<J>,
    watermarks: Vec<u64>,
    dirty: BTreeSet<usize>,
    pending: Vec<BTreeSet<Dependency>>,
    readers: Vec<Vec<usize>>,
    pub audit: bool,
    pub snapshots: Vec<J>,
    pub binding_audit: Vec<J>,
    pub initial_audit: Option<J>,
    pub tasks: Vec<Task>,
    indexed_queries: bool,
    delta_queries: bool,
    column_queries: bool,
    query_indexes: QueryIndexes,
    sieve: crate::sieve::Sieve,
    profile_queries: bool,
    query_profile: Vec<(u64, f64, u64)>,
    progress: bool,
    multistage_enabled: bool,
    point_programs: bool,
    local_projections: bool,
    continuation_stats: ContinuationStats,
    handoffs: BTreeSet<(usize, usize)>,
}
/// Run-local binding workspace. Only successful matches become stored bindings;
/// failed join branches undo slot assignments instead of cloning tree maps.
struct QueryEnv {
    witnesses: MatchedFacts,
    values: Vec<Option<Value>>,
    trail: Vec<usize>,
}
impl QueryEnv {
    fn new(slots: usize, witness_slots: usize, full_witnesses: bool) -> Self {
        Self {
            witnesses: MatchedFacts::new(witness_slots, full_witnesses),
            values: vec![None; slots],
            trail: vec![],
        }
    }
    fn record(&mut self, atom: usize, witness: Option<Dependency>) {
        self.witnesses.record(atom, witness);
    }
    fn rollback(&mut self, mark: usize) {
        while self.trail.len() > mark {
            self.values[self.trail.pop().unwrap()] = None;
        }
    }
    fn binding(&self) -> BTreeMap<usize, Value> {
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
    let v = v.clone();
    match a {
        Arg::Literal(l) => canon(l, &g.parents) == v,
        Arg::Slot(i) => {
            if let Some(old) = &env.values[*i] {
                *old == v
            } else {
                env.values[*i] = Some(v);
                env.trail.push(*i);
                true
            }
        }
    }
}
impl Executor {
    pub fn new(graph: Graph, audit: bool) -> Self {
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
        let mut e = Self {
            compositions_enabled: std::env::var("CELL_CPU_COMPOSITIONS").as_deref() == Ok("1"),
            composition_stats: CompositionStats::default(),
            graph,
            steps: vec![],
            watermarks: vec![0; n],
            dirty: (0..n).collect(),
            pending: vec![BTreeSet::new(); n],
            readers,
            audit,
            snapshots: vec![],
            binding_audit: vec![],
            initial_audit,
            tasks,
            column_queries: std::env::var("CELL_CPU_QUERY_MODE").as_deref() != Ok("reverse"),
            query_indexes,
            sieve,
            profile_queries: std::env::var_os("CELL_CPU_PROFILE").is_some(),
            query_profile: vec![(0, 0.0, 0); n],
            progress: std::env::var_os("CELL_CPU_PROGRESS").is_some(),
            multistage_enabled: std::env::var("CELL_CPU_MULTISTAGE").as_deref() != Ok("0"),
            point_programs: std::env::var("CELL_CPU_ENTRY_PROGRAMS").as_deref() != Ok("0"),
            local_projections: std::env::var("CELL_CPU_LOCAL_PROJECTIONS").as_deref() != Ok("0"),
            continuation_stats: ContinuationStats::default(),
            handoffs: BTreeSet::new(),
            indexed_queries: std::env::var("CELL_CPU_QUERY_MODE").as_deref() != Ok("scan"),
            delta_queries: !matches!(
                std::env::var("CELL_CPU_QUERY_MODE").as_deref(),
                Ok("scan" | "indexed")
            ),
        };
        e.deliver();
        e.graph.query_changes.clear();
        e
    }
    fn deliver(&mut self) {
        // All events are delivered at one published Run boundary. Repeated
        // repairs of one fact have the same final state and subscriber set.
        let mut events = std::mem::take(&mut self.graph.events);
        self.graph.stats.delivery_events += events.len() as u64;
        events.sort_unstable();
        events.dedup();
        self.graph.stats.distinct_delivery_events += events.len() as u64;
        for event in events {
            if let Some(table) = self.graph.live_table(event) {
                for &rid in &self.readers[table] {
                    self.dirty.insert(rid);
                    if self.graph.program.rules[rid].direct.is_some() || self.delta_queries {
                        self.pending[rid].insert(event);
                    }
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
                composition: None,
                witnesses: env.witnesses.clone(),
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
                    2 + if !self.indexed_queries {
                        self.graph.witness_count(t)
                    } else if self.column_queries {
                        self.query_indexes.count(&self.graph, t, &bound)?
                    } else {
                        self.graph.candidate_count(t, &bound)
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
                let witnesses = if let Some(key) = key {
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
                    if !self.indexed_queries {
                        Rc::new(self.graph.witnesses(*t))
                    } else if self.column_queries {
                        self.query_indexes.candidates(&self.graph, *t, &bound)?
                    } else {
                        Rc::new(self.graph.candidates(*t, &bound))
                    }
                };
                for w in witnesses.iter().copied() {
                    if !self.sieve.allow(rid, chosen, *t, w, &self.query_indexes) {
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
                    if r.hidden
                        || delta.is_some_and(|(pivot, epoch, witness)| {
                            (r.epoch, r.witness) > (epoch, witness)
                                || ((r.epoch, r.witness) == (epoch, witness) && chosen < pivot)
                        })
                    {
                        continue;
                    }
                    let mark = env.trail.len();
                    if atom
                        .args
                        .iter()
                        .zip(&r.values)
                        .all(|(a, v)| bind(a, v, env, &self.graph))
                    {
                        env.record(chosen, Some(r.witness));
                        self.solve(query, rid, remaining, env, epoch.max(r.epoch), out, delta)?;
                        env.record(chosen, None);
                    }
                    env.rollback(mark);
                }
            }
        }
        remaining[chosen] = true;
        Ok(())
    }
    fn points(
        &mut self,
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
                        self.graph.lookup(*t, &args)?
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
                        .zip(&row.values)
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
    fn composed_match(
        &mut self,
        rid: usize,
        anchor: usize,
        seed: &Row,
        env: &mut QueryEnv,
    ) -> Result<ComposedMatch> {
        if !self.compositions_enabled {
            return Ok(ComposedMatch::Fallback);
        }
        let start = Instant::now();
        let result = self.composed_match_inner(rid, anchor, seed, env);
        self.composition_stats.match_seconds += start.elapsed().as_secs_f64();
        result
    }
    fn composed_match_inner(
        &mut self,
        rid: usize,
        anchor: usize,
        seed: &Row,
        env: &mut QueryEnv,
    ) -> Result<ComposedMatch> {
        if !self.compositions_enabled || matches!(env.witnesses, MatchedFacts::Rows(_)) {
            return Ok(ComposedMatch::Fallback);
        }
        let Some(context) = self.graph.continuation_context(seed.witness) else {
            return Ok(ComposedMatch::Fallback);
        };
        if !context.completed {
            return Ok(ComposedMatch::Fallback);
        }
        let Some(edge) = self
            .graph
            .program
            .compositions
            .edges
            .get(&(context.template, context.op, rid, anchor))
            .cloned()
        else {
            return Ok(ComposedMatch::Fallback);
        };
        self.composition_stats.attempts += 1;
        let query = self.graph.program.rules[rid].query.clone();
        let mark = env.trail.len();
        let mut epoch = seed.epoch;
        for instruction in &edge.instructions {
            let atom = &query[instruction.atom];
            if let Some(columns) = &instruction.columns {
                let values = match self.graph.context_arguments(context, columns) {
                    Ok(v) => v,
                    Err(_) => {
                        env.rollback(mark);
                        for i in &edge.instructions {
                            env.record(i.atom, None);
                        }
                        self.composition_stats.unavailable += 1;
                        return Ok(ComposedMatch::Fallback);
                    }
                };
                self.composition_stats.known_atoms += 1;
                if !atom
                    .args
                    .iter()
                    .zip(&values)
                    .all(|(a, v)| bind(a, v, env, &self.graph))
                {
                    self.composition_stats.rejected += 1;
                    return Ok(ComposedMatch::Rejected);
                }
            } else {
                let key = atom.args[..atom.args.len() - 1]
                    .iter()
                    .map(|a| {
                        operand(a, env, &self.graph)
                            .ok_or_else(|| "unbound compiled port".to_owned())
                    })
                    .collect::<Result<Vec<_>>>()?;
                let value = match &atom.call {
                    Call::Table(t) => {
                        self.composition_stats.port_reads += 1;
                        let Some(row) = self.graph.lookup(*t, &key)? else {
                            self.composition_stats.rejected += 1;
                            return Ok(ComposedMatch::Rejected);
                        };
                        if row.hidden {
                            self.composition_stats.rejected += 1;
                            return Ok(ComposedMatch::Rejected);
                        }
                        epoch = epoch.max(row.epoch);
                        env.record(instruction.atom, Some(row.witness));
                        row.values.last().unwrap().clone()
                    }
                    Call::Primitive(name, output) => {
                        self.composition_stats.guard_checks += 1;
                        let Some(v) = value::primitive(
                            name,
                            &key,
                            output,
                            &self.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                        )?
                        else {
                            self.composition_stats.rejected += 1;
                            return Ok(ComposedMatch::Rejected);
                        };
                        v
                    }
                };
                if !bind(atom.args.last().unwrap(), &value, env, &self.graph) {
                    self.composition_stats.rejected += 1;
                    return Ok(ComposedMatch::Rejected);
                }
            }
        }
        // Every remaining key is a function of this complete anchor binding.
        // Other delta pivots still run and the existing binding map deduplicates
        // overlaps; no inverse alternative or late input is excluded.
        self.composition_stats.bindings += 1;
        let parent = if self.graph.program.rules[rid].direct.is_some() {
            match seed.witness {
                Dependency::Fact(f) => Some(f),
                _ => None,
            }
        } else {
            None
        };
        Ok(ComposedMatch::Matched(Binding {
            composition: Some((edge.clone(), context)),
            witnesses: env.witnesses.clone(),
            values: env.binding(),
            epoch,
            parent,
            continuation: Some((
                edge.producer,
                context
                    .depth
                    .checked_add(1)
                    .ok_or("recipe depth exhausted")?,
                false,
            )),
            compiled: true,
        }))
    }
    fn continue_local(
        &mut self,
        rid: usize,
        pivot: usize,
        seed: &Row,
        env: &mut QueryEnv,
        remaining: &mut [bool],
        mut epoch: u64,
        _watermark: u64,
    ) -> Result<(bool, u64, Option<(usize, u32)>)> {
        if !self.multistage_enabled || !self.local_projections {
            return Ok((true, epoch, None));
        }
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
        if !self.graph.cells.contains_key(&context.cell) {
            self.graph.view_stats.recovered_continuations += 1;
        }
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
                .zip(&values)
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
    fn matches(&mut self, rid: usize, probe: bool) -> Result<Vec<Binding>> {
        let rule = self.graph.program.rules[rid].clone();
        let full_witnesses = !self.graph.marker_fields_copied
            && self
                .graph
                .program
                .views
                .layouts
                .get(&rule.template)
                .is_some_and(|l| l.reference_only);
        // Witnesses are storage provenance, not part of logical LHS matching.
        // Copied markers without takeover do not consume them at all.
        let witness_slots = if self.graph.views_enabled
            && !probe
            && self
                .graph
                .program
                .views
                .layouts
                .get(&rule.template)
                .is_some_and(|l| {
                    !l.carriers.is_empty()
                        && (self.graph.absorption_enabled
                            || (l.reference_only && !self.graph.marker_fields_copied))
                }) {
            rule.query.len()
        } else {
            0
        };
        if !probe && rule.seminaive && !self.dirty.remove(&rid) {
            return Ok(vec![]);
        }
        // No join can succeed if a required relation is empty. This is a query
        // property, not a Cell proof or a prediction about future stages.
        if self.indexed_queries
            && rule.query.iter().any(|a| match a.call {
                Call::Table(t) => self.graph.witness_count(t) == 0,
                _ => false,
            })
        {
            if !probe {
                self.pending[rid].clear();
            }
            return Ok(vec![]);
        }
        self.sieve
            .prepare(rid, &self.graph, &mut self.query_indexes)?;
        let mut found = vec![];
        if let Some(anchor) = rule.direct {
            let atom = &rule.query[anchor];
            let Call::Table(t) = atom.call else {
                unreachable!()
            };
            let candidates = if probe || !rule.seminaive {
                self.graph.witnesses(t)
            } else {
                std::mem::take(&mut self.pending[rid]).into_iter().collect()
            };
            for w in candidates {
                self.graph.stats.decoded_rows += 1;
                let Some(row) = self.graph.row(w)? else {
                    continue;
                };
                if row.hidden || (!probe && rule.seminaive && row.epoch < self.watermarks[rid]) {
                    continue;
                }
                let mut env = QueryEnv::new(
                    self.graph.program.templates[rule.template].sorts.len(),
                    witness_slots,
                    full_witnesses,
                );
                if atom
                    .args
                    .iter()
                    .zip(&row.values)
                    .all(|(a, v)| bind(a, v, &mut env, &self.graph))
                {
                    env.record(anchor, Some(row.witness));
                    if !probe {
                        match self.composed_match(rid, anchor, &row, &mut env)? {
                            ComposedMatch::Matched(b) => {
                                self.graph.stats.direct_bindings += 1;
                                found.push(b);
                                continue;
                            }
                            ComposedMatch::Rejected => continue,
                            ComposedMatch::Fallback => {}
                        }
                    }
                    let n = found.len();
                    let mut remaining = (0..rule.query.len())
                        .map(|i| i != anchor)
                        .collect::<Vec<_>>();
                    let mut epoch = row.epoch;
                    if self.multistage_enabled && self.point_programs {
                        if let Some(order) = self
                            .graph
                            .program
                            .multi_stage
                            .entries
                            .get(&(rid, anchor))
                            .cloned()
                        {
                            if !self.points(
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
                    if self.multistage_enabled && !remaining.iter().any(|v| *v) {
                        found.push(Binding {
                            composition: None,
                            witnesses: env.witnesses.clone(),
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
                            if self.multistage_enabled {
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
        } else if self.delta_queries
            && rule.seminaive
            && !probe
            && rule.query.iter().any(|a| matches!(a.call, Call::Table(_)))
        {
            let watermark = self.watermarks[rid];
            let changed = std::mem::take(&mut self.pending[rid])
                .into_iter()
                .map(|w| self.graph.row(w))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .filter(|r| !r.hidden && r.epoch >= watermark)
                .collect::<Vec<_>>();
            for (pivot, atom) in rule.query.iter().enumerate() {
                let Call::Table(table) = atom.call else {
                    continue;
                };
                for row in changed.iter().filter(|r| r.table == table) {
                    if !self
                        .sieve
                        .allow(rid, pivot, table, row.witness, &self.query_indexes)
                    {
                        continue;
                    }
                    self.graph.stats.residual_candidates += 1;
                    self.graph.stats.decoded_rows += 1;
                    let mut env = QueryEnv::new(
                        self.graph.program.templates[rule.template].sorts.len(),
                        witness_slots,
                        full_witnesses,
                    );
                    if atom
                        .args
                        .iter()
                        .zip(&row.values)
                        .all(|(a, v)| bind(a, v, &mut env, &self.graph))
                    {
                        let mut remaining = (0..rule.query.len())
                            .map(|i| i != pivot)
                            .collect::<Vec<_>>();
                        let mut epoch = row.epoch;
                        env.record(pivot, Some(row.witness));
                        match self.composed_match(rid, pivot, row, &mut env)? {
                            ComposedMatch::Matched(b) => {
                                found.push(b);
                                continue;
                            }
                            ComposedMatch::Rejected => continue,
                            ComposedMatch::Fallback => {}
                        }
                        let delta = Some((pivot, row.epoch, row.witness));
                        if self.multistage_enabled && self.point_programs {
                            if let Some(order) = self
                                .graph
                                .program
                                .multi_stage
                                .entries
                                .get(&(rid, pivot))
                                .cloned()
                            {
                                if !self.points(
                                    &rule.query,
                                    &order,
                                    &mut env,
                                    &mut remaining,
                                    &mut epoch,
                                    delta,
                                    false,
                                )? {
                                    continue;
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
                            watermark,
                        )?;
                        if !live {
                            continue;
                        }
                        // Prefer certified producer-local slots. Only unresolved
                        // reads use conditional singleton projections of the current
                        // public interface (including data-loader-origin nodes).
                        if self.multistage_enabled && self.point_programs {
                            if let Some(order) = self
                                .graph
                                .program
                                .multi_stage
                                .entries
                                .get(&(rid, pivot))
                                .cloned()
                            {
                                if !self.points(
                                    &rule.query,
                                    &order,
                                    &mut env,
                                    &mut remaining,
                                    &mut epoch,
                                    delta,
                                    true,
                                )? {
                                    continue;
                                }
                            }
                        }
                        let residual = remaining
                            .iter()
                            .enumerate()
                            .any(|(i, p)| *p && matches!(rule.query[i].call, Call::Table(_)));
                        let before = found.len();
                        if self.multistage_enabled && self.point_programs && !residual {
                            if let Some(order) =
                                self.graph.program.multi_stage.finishes.get(rid).cloned()
                            {
                                if !self.points(
                                    &rule.query,
                                    &order,
                                    &mut env,
                                    &mut remaining,
                                    &mut epoch,
                                    delta,
                                    false,
                                )? {
                                    continue;
                                }
                            }
                        }
                        if self.multistage_enabled && !remaining.iter().any(|v| *v) {
                            found.push(Binding {
                                composition: None,
                                witnesses: env.witnesses.clone(),
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
                                delta,
                            )?;
                        }
                        if origin.is_some() {
                            for binding in &mut found[before..] {
                                binding.continuation = origin.map(|(p, d)| (p, d, residual));
                            }
                        }
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
                    witness_slots,
                    full_witnesses,
                ),
                0,
                &mut found,
                None,
            )?;
        }
        let mut unique = BTreeMap::new();
        for b in found {
            if probe || !rule.seminaive || b.epoch >= self.watermarks[rid] {
                let key = rule
                    .inputs
                    .iter()
                    .map(|i| b.values.get(i).cloned().ok_or("incomplete binding"))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                match unique.entry(key) {
                    std::collections::btree_map::Entry::Vacant(e) => {
                        e.insert(b);
                    }
                    std::collections::btree_map::Entry::Occupied(mut e) => {
                        if e.get().composition.is_none() && b.composition.is_some() {
                            e.insert(b);
                        }
                    }
                }
            }
        }
        Ok(unique.into_values().collect())
    }
    fn apply(&mut self, rid: usize, b: Binding) -> Result<()> {
        let template = self.graph.program.rules[rid].template;
        let origin = b.continuation;
        if b.compiled {
            self.continuation_stats.compiled_bindings += 1;
        }
        let facts = self.graph.stats.logical_facts_created;
        let updates = self.graph.stats.function_updates;
        let cell = self
            .graph
            .reference_frame(template, b.values, b.parent, &b.witnesses)?;
        if self.compositions_enabled {
            let started = Instant::now();
            if let Some((edge, context)) = &b.composition {
                let old = context.composition.filter(|c| c.program == edge.program);
                let cursor = crate::compositions::Cursor {
                    program: edge.program,
                    entry: old.map_or(context.cell, |c| c.entry),
                    steps: old
                        .map_or(1, |c| c.steps)
                        .checked_add(1)
                        .ok_or("composition step overflow")?,
                    marker_entry: old.is_some_and(|c| c.marker_entry),
                };
                self.graph.cells.get_mut(&cell).unwrap().composition = Some(cursor);
                self.composition_stats.continued_steps += 1;
                self.composition_stats.marker_continued_steps += u64::from(cursor.marker_entry);
                self.composition_stats.max_steps =
                    self.composition_stats.max_steps.max(cursor.steps);
                self.composition_stats.source_rules.insert(rid);
            } else if let Some(&program) = self.graph.program.compositions.rule_program.get(&rid) {
                self.graph.cells.get_mut(&cell).unwrap().composition =
                    Some(crate::compositions::Cursor {
                        program,
                        entry: cell,
                        steps: 1,
                        marker_entry: self
                            .graph
                            .program
                            .compositions
                            .marker_entries
                            .contains(&rid),
                    });
                self.composition_stats.root_steps += 1;
            }
            self.composition_stats.entry_seconds += started.elapsed().as_secs_f64();
        }
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
        let mut completed = true;
        for (i, op) in ops.iter().enumerate() {
            match op {
                Op::Copy(..) => {}
                Op::Let(dst, Call::Table(t), aa) => {
                    if self.graph.program.tables[*t].virtual_fact {
                        if let Some((edge, context)) = &b.composition
                            && let Some(columns) = edge.reuse.get(&i)
                        {
                            if let Ok(values) = self.graph.context_arguments(*context, columns) {
                                let key = self.graph.arguments(cell, aa)?;
                                if values[..values.len() - 1] == key {
                                    self.graph
                                        .cells
                                        .get_mut(&cell)
                                        .unwrap()
                                        .values
                                        .insert(*dst, values.last().unwrap().clone());
                                    self.composition_stats.constructor_reuses += 1;
                                    continue;
                                }
                            }
                            self.composition_stats.reuse_fallbacks += 1;
                        }
                        self.graph.construct(cell, i, *t, aa, *dst)?;
                    } else {
                        let key = self.graph.arguments(cell, &aa)?;
                        let row = self.graph.lookup(*t, &key)?.ok_or_else(|| {
                            format!(
                                "missing function read {}",
                                self.graph.program.tables[*t].name
                            )
                        })?;
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
                Op::Set(t, aa, v) => self.graph.set(cell, i, *t, aa, v)?,
                Op::Union(a, b) => {
                    let a = self.graph.argument(cell, &a)?;
                    let b = self.graph.argument(cell, &b)?;
                    self.graph.enqueue_union(a, b)?;
                }
                Op::Hide(t, aa, delete) => {
                    let key = self.graph.arguments(cell, &aa)?;
                    if !self.graph.hide(*t, &key, *delete)? {
                        completed = false;
                        break;
                    }
                }
                Op::Panic => return Err("source panic".into()),
            }
        }
        if origin.is_some() {
            self.continuation_stats.published_facts +=
                self.graph.stats.logical_facts_created - facts;
            self.continuation_stats.function_updates += self.graph.stats.function_updates - updates;
        }
        self.graph.cells.get_mut(&cell).unwrap().completed = completed;
        self.graph.queue_absorption(cell, &b.witnesses);
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
        self.graph.epoch = self.graph.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.graph.changed = false;
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
        for rid in rules {
            let clock = self.profile_queries.then(Instant::now);
            let candidates = self.graph.stats.residual_candidates;
            if self.progress && self.profile_queries {
                eprintln!("[cell-cpu] query {rid}");
            }
            let bindings = self.matches(rid, false)?;
            if let Some(clock) = clock {
                self.query_profile[rid].0 += 1;
                self.query_profile[rid].1 += clock.elapsed().as_secs_f64();
                self.query_profile[rid].2 += self.graph.stats.residual_candidates - candidates;
            }
            self.watermarks[rid] = self.graph.epoch;
            work.extend(bindings.into_iter().map(|b| (rid, b)));
        }
        if std::env::var("CELL_CPU_QUERY_INDEX_SCOPE").as_deref() == Ok("run") {
            self.query_indexes.clear();
            self.sieve.invalidate();
        }
        let patch = StagePatch { bindings: work };
        let matches = patch.bindings.len();
        let search_seconds = start.elapsed().as_secs_f64();
        if self.audit {
            self.binding_audit.push(json!({"ruleset":set,"bindings":patch.bindings.iter().map(|(rid,b)|{let template=&self.graph.program.templates[self.graph.program.rules[*rid].template];json!({"rule":rid,"values":b.values.iter().map(|(i,v)|json!({"slot":i,"value":crate::value::exported(v,&template.sorts[*i])})).collect::<Vec<_>>()})}).collect::<Vec<_>>()}));
        }
        let phase = Instant::now();
        for (rid, b) in patch.bindings {
            self.apply(rid, b)?;
        }
        let rhs_seconds = phase.elapsed().as_secs_f64();
        self.graph.stats.bindings += matches as u64;
        let phase = Instant::now();
        self.graph.repair()?;
        let moved = self.graph.absorb_ready()?;
        if !moved.is_empty() {
            // Only readers of the relocated fact's table can have that handle
            // pending. Keep all unrelated rules and pending sets untouched.
            for &old in moved.keys() {
                let new = self.graph.resolve_reference(old);
                let table = self
                    .graph
                    .dependency_table(Dependency::Fact(new))
                    .ok_or("relocated fact has no table")?;
                for &rid in &self.readers[table] {
                    if self.pending[rid].remove(&Dependency::Fact(old)) {
                        self.pending[rid].insert(Dependency::Fact(new));
                    }
                }
            }
            self.graph.events = self
                .graph
                .events
                .iter()
                .map(|d| match *d {
                    Dependency::Fact(f) => Dependency::Fact(self.graph.resolve_reference(f)),
                    _ => *d,
                })
                .collect();
        }
        let repair_seconds = phase.elapsed().as_secs_f64();
        let phase = Instant::now();
        if std::env::var("CELL_CPU_QUERY_INDEX_SCOPE").as_deref() == Ok("table") {
            self.query_indexes.invalidate(&self.graph);
            self.sieve.invalidate();
            self.graph.query_changes.clear();
        } else {
            self.query_indexes.update(&mut self.graph)?;
            self.sieve.publish(&mut self.query_indexes);
        }
        let query_update_seconds = phase.elapsed().as_secs_f64();
        let phase = Instant::now();
        self.deliver();
        self.graph.collect_garbage();
        let delivery_seconds = phase.elapsed().as_secs_f64();
        let changed = self.graph.changed;
        self.steps.push(json!({"ordinal":self.steps.len()+1,"ruleset":set,"path":path,"iterations":iterations,"epoch":self.graph.epoch,"logical_matches":matches,"changed":changed,"live_cells":self.graph.cells.len(),"elapsed_seconds":start.elapsed().as_secs_f64(),"phases":{"search":search_seconds,"rhs":rhs_seconds,"repair":repair_seconds,"delivery_gc":delivery_seconds,"query_update":query_update_seconds}}));
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
                    }
                }
            }
        }
        Ok(())
    }
    pub fn checkpoint(&self) -> J {
        let mut c = json!({"schema":"eggfuse-cell-checkpoint/v1","program":self.graph.program.artifact,"graph":self.graph.image(),"tasks":self.tasks,"watermarks":self.watermarks,"steps":self.steps,"continuation_stats":self.continuation_stats,"handoffs":self.handoffs});
        c["compositions_enabled"] = self.compositions_enabled.into();
        c["composition_stats"] = json!(self.composition_stats);
        c["sha256"] = crate::checksum::digest(&c).into();
        c
    }
    pub fn resume(program: Program, mut checkpoint: J, audit: bool) -> Result<Self> {
        let checksum = checkpoint
            .as_object_mut()
            .ok_or("checkpoint object")?
            .remove("sha256")
            .ok_or("checkpoint checksum missing")?;
        if checksum != crate::checksum::digest(&checkpoint) {
            return Err("checkpoint checksum differs".into());
        }
        if checkpoint["schema"] != "eggfuse-cell-checkpoint/v1"
            || checkpoint["program"] != program.artifact
        {
            return Err("checkpoint program differs".into());
        }
        let graph = Graph::restore(program, &checkpoint["graph"])?;
        let mut e = Self::new(graph, audit);
        if checkpoint["compositions_enabled"]
            .as_bool()
            .unwrap_or(false)
            != e.compositions_enabled
        {
            return Err("checkpoint composition mode differs".into());
        }
        if !checkpoint["composition_stats"].is_null() {
            e.composition_stats = serde_json::from_value(checkpoint["composition_stats"].clone())
                .map_err(|x| x.to_string())?;
        }
        e.tasks = serde_json::from_value(checkpoint["tasks"].clone()).map_err(|x| x.to_string())?;
        e.watermarks =
            serde_json::from_value(checkpoint["watermarks"].clone()).map_err(|x| x.to_string())?;
        e.steps = serde_json::from_value(checkpoint["steps"].clone()).map_err(|x| x.to_string())?;
        if !checkpoint["continuation_stats"].is_null() {
            e.continuation_stats = serde_json::from_value(checkpoint["continuation_stats"].clone())
                .map_err(|x| x.to_string())?;
        }
        if !checkpoint["handoffs"].is_null() {
            e.handoffs = serde_json::from_value(checkpoint["handoffs"].clone())
                .map_err(|x| x.to_string())?;
        }
        if e.watermarks.len() != e.graph.program.rules.len()
            || e.watermarks.iter().any(|v| *v > e.graph.epoch)
            || e.steps.len() as u64 != e.graph.epoch
        {
            return Err("checkpoint stage metadata invalid".into());
        }
        for task in &e.tasks {
            let start = match task {
                Task::Repeat { start, .. } | Task::Saturate { start, .. } => *start,
                _ => 0,
            };
            if start > e.steps.len() {
                return Err("checkpoint continuation invalid".into());
            }
        }
        Ok(e)
    }
    pub fn report(&self) -> J {
        json!({"key_lookup_calls":self.graph.key_lookup_calls.get(),"compositions":{"enabled":self.compositions_enabled,"programs":self.graph.program.compositions.programs,"edges":self.graph.program.compositions.edges.len(),"stats":self.composition_stats,"live_cursors":self.graph.cells.values().filter(|c|c.composition.is_some()).count()},"reference_views":self.graph.reference_report(),"local_congruence":self.graph.congruence_report(),"sieve":self.sieve.report(),"join_refinements":{"stats":self.query_indexes.refinements,"live_entries_and_references":self.query_indexes.refinement_storage()},"multi_stage":{"enabled":self.multistage_enabled,"entry_programs_enabled":self.point_programs,"local_projections_enabled":self.local_projections,"entry_programs":self.graph.program.multi_stage.entries.len(),"compiled_routes":self.graph.program.multi_stage.routes.len(),"stats":self.continuation_stats,"executed_rule_pairs":self.handoffs,"stage_patches":self.steps.len()},"query_profile": if self.profile_queries {Some(self.query_profile.iter().enumerate().filter(|(_,v)|v.0>0).map(|(rid,v)|json!({"rule":rid,"name":self.graph.program.templates[self.graph.program.rules[rid].template].name,"calls":v.0,"seconds":v.1,"candidates":v.2})).collect::<Vec<_>>())}else{None},"query_indexes":{"builds":self.query_indexes.builds,"decoded_facts":self.query_indexes.decoded_facts,"updated_facts":self.query_indexes.updated_facts,"peak_fact_references":self.query_indexes.peak_references,"scope":"versioned virtual-fact indexes; references only"},"query_mode":std::env::var("CELL_CPU_QUERY_MODE").unwrap_or_else(|_|"delta".into()),"backend":"cell-cpu","scope":"standalone Cell recipe executor; native frontend only; no egglog runtime linked","stats":self.graph.stats,"live_cells":self.graph.cells.len(),"stored_binding_values":self.graph.cells.values().map(|c|c.values.len()).sum::<usize>(),"virtual_constructor_facts":self.graph.cells.values().map(|c|c.facts.len()).sum::<usize>(),"function_ports":self.graph.functions.iter().filter(|f|f.live).count(),"promoted_aliases":self.graph.parents.len(),"steps":self.steps,"status":if self.tasks.is_empty(){"complete"}else{"checkpointed"}})
    }
}
