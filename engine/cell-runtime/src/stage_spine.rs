//! Source-scheduled Cell programs. A committed producer supplies the next
//! stage's complete context; the stage consumes it without a structural join.
//! Coverage is proved from functional table keys, not from observed frequency.
use crate::{
    entry_schedule::{Inputs, Seed},
    graph::{Dependency, Graph, Row},
    known_results::{QueryFacts, Source},
    program::*,
    value::{self, Value, canon},
};
use serde::Serialize;
use serde_json::{Value as J, json};
use std::{
    cell::{OnceCell, RefCell},
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
    time::Instant,
};

#[cfg(feature = "spine-shapes")]
#[path = "spine_shapes.rs"]
mod shapes;
#[derive(Clone)]
pub struct Cursor {
    #[cfg(feature = "spine-blocks")]
    via_block: bool,
    #[cfg(feature = "spine-shapes")]
    trace: shapes::Trace,
    root: Rc<Root>,
    pub depth: u32,
    pub producer: usize,
}
/// Shared by every published anchor of one logical root. The cell stores a
/// pointer so an unused root never reserves space for the full Context.
struct Root {
    id: u64,
    epoch: u64,
    set: Rc<str>,
    context: OnceCell<Box<RefCell<Context>>>,
}
impl Root {
    fn materialize(&self, stats: &mut Stats) -> &RefCell<Context> {
        self.context.get_or_init(|| {
            stats.contexts_created += 1;
            Box::new(RefCell::new(Context {
                sets: BTreeSet::from([self.set.to_string()]),
                last_epoch: self.epoch,
                visits: 1,
                unions: 0,
                facts: BTreeMap::new(),
            }))
        })
    }
}
/// One RHS application. New roots carry identity and source position until a
/// visible anchor wins publication; resumed applications already own a context.
pub(crate) enum Started {
    Root {
        id: u64,
        producer: usize,
        epoch: u64,
        set: Rc<str>,
    },
    Live(Cursor),
}
impl Started {
    fn cursor(&mut self, stats: &mut Stats) -> &Cursor {
        if let Self::Root {
            id,
            producer,
            epoch,
            set,
        } = self
        {
            *self = Self::Live(Cursor {
                root: Rc::new(Root {
                    id: *id,
                    epoch: *epoch,
                    set: set.clone(),
                    context: OnceCell::new(),
                }),
                #[cfg(feature = "spine-shapes")]
                trace: Default::default(),
                #[cfg(feature = "spine-blocks")]
                via_block: false,
                depth: 1,
                producer: *producer,
            });
            stats.root_handles_created += 1;
        }
        let Self::Live(cursor) = self else {
            unreachable!()
        };
        cursor
    }
}
struct Context {
    sets: BTreeSet<String>,
    last_epoch: u64,
    visits: u64,
    unions: u64,
    facts: BTreeMap<(usize, Vec<Value>), Dependency>,
}
pub struct Answer {
    pub(crate) parameter_parent: Option<crate::graph::FactRef>,
    pub values: Vec<Value>,
    pub witnesses: Vec<Option<Dependency>>,
    pub epoch: u64,
}
/// Exact entry coverage, independently usable from residual query completion.
pub struct Coverage {
    pub answers: Vec<Answer>,
    pub covered: BTreeSet<(usize, Dependency)>,
    pub complete: bool,
}
#[derive(Default, Serialize)]
pub struct Stats {
    root_attempts: u64,
    root_handles_created: u64,
    contexts_created: u64,
    captured_contexts: u64,
    capture_reads: u64,
    receipt_attempts: u64,
    receipt_hits: u64,
    receipt_redirects: u64,
    receipt_rejections: u64,
    receipt_missing: u64,
    receipt_decodes: u64,
    receipt_hits_by_source: BTreeMap<String, u64>,
    anchor_publications: u64,
    missing_capture_rows: u64,
    anchors_retired: u64,
    query_attempts: u64,
    routed_spine_seeds: u64,
    routed_matcher_seeds: u64,
    mixed_routed_queries: u64,
    query_takeovers: u64,
    partial_queries: u64,
    pivot_candidates: u64,
    shared_entry_hits: u64,
    entry_parameters_copied: u64,
    shared_entry_cross_rule_hits: u64,
    shared_row_hits: u64,
    peak_cached_entries: usize,
    entry_requests_by_rule: BTreeMap<usize, u64>,
    entry_checks_by_rule: BTreeMap<usize, u64>,
    fallback_pivots: u64,
    missing_context_pivots: u64,
    unsupported_key_pivots: u64,
    uncovered_queries: u64,
    pivots_skipped: u64,
    delta_rows_examined: u64,
    dispatch_headers: u64,
    context_reads: u64,
    captured_anchors: u64,
    alias_query_forwards: u64,
    alias_effect_forwards: u64,
    distinct_anchor_keys: u64,
    context_fact_publications: u64,
    completed_steps: u64,
    port_refreshes: u64,
    pre_resume_port_lookups: u64,
    port_cache_fills: u64,
    guards: u64,
    guard_rejections: u64,
    bindings: u64,
    steps_executed: u64,
    resumed_steps: u64,
    #[cfg(feature = "spine-blocks")]
    block_entries: u64,
    #[cfg(feature = "spine-blocks")]
    block_resumptions: u64,
    cross_stage_steps: u64,
    marker_roots: u64,
    constructor_requests: u64,
    function_updates: u64,
    union_requests: u64,
    max_chain: u32,
    max_sets_per_spine: usize,
    max_stage_visits: u64,
    capture_seconds: f64,
    publication_seconds: f64,
    query_seconds: f64,
    by_set: BTreeMap<String, u64>,
    resumed_by_set: BTreeMap<String, u64>,
    bindings_by_rule: BTreeMap<usize, u64>,
    covered_by_rule: BTreeMap<usize, u64>,
    fallback_by_rule: BTreeMap<usize, u64>,
}
struct Publication {
    slot: usize,
    receipt: Option<Dependency>,
    #[cfg(feature = "spine-shapes")]
    source: crate::graph::FactRef,
}
pub struct Programs {
    #[cfg(feature = "spine-shapes")]
    shapes: shapes::Profile,
    pub enabled: bool,
    pub partial: bool,
    pub receipts: bool,
    pub audit_receipts: bool,
    shared_entries: bool,
    cache_epoch: u64,
    anchor_shapes: BTreeMap<(usize, usize), (usize, Vec<(usize, usize)>)>,
    entry_cache: HashMap<(usize, Dependency), (bool, usize)>,
    row_cache: HashMap<Dependency, Option<Rc<Row>>>,
    consumer_tables: BTreeMap<usize, BTreeSet<usize>>,
    orders: BTreeMap<(usize, usize), Vec<usize>>,
    entries: HashMap<Dependency, Cursor>,
    active_tables: BTreeMap<usize, usize>,
    // Anchor-producing operations and their optional proven receipt sources.
    anchor_ops: Vec<Vec<(usize, Option<Source>)>>,
    alias_query_facts: Vec<QueryFacts>,
    // All anchors of one application share this slot, including after key coalescence.
    pending_steps: Vec<Started>,
    pending_anchors: Vec<(usize, Vec<Value>, Publication)>,
    root_set: Rc<str>,
    capture_unions: Option<u64>,
    origins: BTreeMap<(usize, Vec<Value>), Cursor>,
    next_context: u64,

    pub stats: Stats,
}
fn bind(a: &Arg, v: &Value, input: &mut Inputs, g: &Graph) -> bool {
    match a {
        Arg::Literal(x) => canon(x, &g.parents) == *v,
        Arg::Slot(i) => match &input[*i] {
            Some(x) => x == v,
            None => {
                #[cfg(feature = "slot-transfer-audit")]
                crate::transfer_audit::count(|c| c.fresh_bind_copies += 1);
                input[*i] = Some(v.clone());
                true
            }
        },
    }
}
fn record_answer(
    results: &mut BTreeMap<Vec<Value>, (Answer, Cursor)>,
    values: Vec<Value>,
    witnesses: Vec<Option<Dependency>>,
    epoch: u64,
    parameter_parent: Option<crate::graph::FactRef>,
    cursor: Cursor,
) {
    let candidate = (
        Answer {
            parameter_parent,
            values: values.clone(),
            witnesses,
            epoch,
        },
        cursor,
    );
    match results.entry(values) {
        std::collections::btree_map::Entry::Vacant(e) => {
            e.insert(candidate);
        }
        std::collections::btree_map::Entry::Occupied(mut e) => {
            if candidate.1.depth > e.get().1.depth {
                e.insert(candidate);
            }
        }
    }
}
fn read_port(
    g: &mut Graph,
    cursor: &Cursor,
    table: usize,
    args: Vec<Value>,
    stats: &mut Stats,
) -> Result<Option<Row>> {
    let good =
        |r: &crate::graph::Row| r.table == table && !r.hidden && r.values[..args.len()] == args;
    // A candidate may fail a later port or guard, or lose
    // binding deduplication. Only an applied origin promotes the root.
    let cache = cursor.root.context.get();
    let witness = cache.and_then(|context| {
        let mut context = context.borrow_mut();
        context.canonicalize(g);
        context.facts.get(&(table, args.clone())).copied()
    });
    let mut row = if let Some(w) = witness {
        stats.context_reads += 1;
        g.stats.decoded_rows += 1;
        g.row(w)?.filter(good)
    } else {
        None
    };
    if row.is_none() {
        stats.port_refreshes += 1;
        row = g.lookup(table, &args)?;
        if let Some(context) = cache {
            let mut context = context.borrow_mut();
            if let Some(r) = row.as_ref().filter(|r| !r.hidden) {
                context.facts.insert((table, args), r.witness);
                stats.port_cache_fills += 1;
            } else {
                context.facts.remove(&(table, args));
            }
        } else {
            stats.pre_resume_port_lookups += 1;
        }
    }
    Ok(row.filter(|r| !r.hidden))
}
fn operand(a: &Arg, input: &Inputs, g: &Graph) -> Option<Value> {
    match a {
        Arg::Literal(v) => Some(canon(v, &g.parents)),
        Arg::Slot(i) => input[*i].clone(),
    }
}
fn order(rule: &Rule, pivot: usize) -> Option<Vec<usize>> {
    let mut known = rule.query[pivot]
        .args
        .iter()
        .filter_map(|a| if let Arg::Slot(i) = a { Some(*i) } else { None })
        .collect::<BTreeSet<_>>();
    let mut pending = (0..rule.query.len())
        .filter(|i| *i != pivot)
        .collect::<BTreeSet<_>>();
    let mut out = vec![];
    while !pending.is_empty() {
        let i = pending
            .iter()
            .copied()
            .filter(|i| {
                rule.query[*i].args[..rule.query[*i].args.len() - 1]
                    .iter()
                    .all(|a| {
                        matches!(a, Arg::Literal(_))
                            || matches!(a,Arg::Slot(s) if known.contains(s))
                    })
            })
            .min_by_key(|i| {
                (
                    usize::from(matches!(rule.query[*i].call, Call::Table(_))),
                    *i,
                )
            })?;
        pending.remove(&i);
        out.push(i);
        known.extend(
            rule.query[i]
                .args
                .iter()
                .filter_map(|a| if let Arg::Slot(s) = a { Some(*s) } else { None }),
        );
    }
    rule.inputs.iter().all(|i| known.contains(i)).then_some(out)
}
pub fn compile_entries(p: &Program) -> J {
    let mut entries = vec![];
    for (rid, r) in p.rules.iter().enumerate() {
        if r.seminaive {
            for (pivot, a) in r.query.iter().enumerate() {
                if let Call::Table(t) = a.call {
                    if let Some(atoms) = order(r, pivot) {
                        entries.push(json!({"rule":rid,"anchor":pivot,"table":p.tables[t].name,"atoms":atoms}));
                    }
                }
            }
        }
    }
    J::Array(entries)
}
impl Context {
    fn canonicalize(&mut self, g: &Graph) {
        if self.unions == g.stats.unions {
            return;
        }
        self.facts = std::mem::take(&mut self.facts)
            .into_iter()
            .map(|((t, key), w)| ((t, key.iter().map(|v| canon(v, &g.parents)).collect()), w))
            .collect();
        self.unions = g.stats.unions;
    }
}
impl Programs {
    pub fn new(p: &Program) -> Self {
        let enabled = std::env::var("CELL_CPU_STAGE_SPINE").as_deref() == Ok("1");
        let mut s = Self {
            enabled,
            receipts: enabled
                && std::env::var("CELL_CPU_STAGE_SPINE_RECEIPTS")
                    .map_or(true, |value| value == "1"),
            audit_receipts: false,
            partial: std::env::var("CELL_CPU_STAGE_SPINE_PARTIAL").as_deref() == Ok("1"),
            shared_entries: std::env::var("CELL_CPU_STAGE_SPINE_SHARED_ENTRIES").as_deref()
                == Ok("1"),
            cache_epoch: u64::MAX,
            anchor_shapes: BTreeMap::new(),
            entry_cache: HashMap::new(),
            row_cache: HashMap::new(),
            consumer_tables: BTreeMap::new(),
            orders: BTreeMap::new(),
            entries: HashMap::new(),
            active_tables: BTreeMap::new(),
            #[cfg(feature = "spine-shapes")]
            shapes: Default::default(),
            anchor_ops: vec![],
            alias_query_facts: vec![],
            pending_steps: vec![],
            pending_anchors: vec![],
            root_set: Rc::from(""),
            capture_unions: None,
            origins: BTreeMap::new(),
            next_context: 0,

            stats: Stats::default(),
        };
        if !enabled {
            return s;
        }
        let mut anchor_tables = BTreeSet::new();
        for (rid, pivot, order) in &p.multi_stage.stage_entries {
            s.orders.insert((*rid, *pivot), order.as_ref().clone());
            let Call::Table(t) = p.rules[*rid].query[*pivot].call else {
                unreachable!()
            };
            anchor_tables.insert(t);
            s.consumer_tables.entry(*rid).or_default().insert(t);
        }
        if std::env::var("CELL_CPU_SHARED_NODE_SCHEDULE").as_deref() != Ok("1") {
            let mut shapes = BTreeMap::new();
            for (rid, pivot) in p.rules.iter().enumerate().flat_map(|(rid, r)| {
                r.query
                    .iter()
                    .enumerate()
                    .filter_map(move |(i, a)| matches!(a.call, Call::Table(_)).then_some((rid, i)))
            }) {
                let a = &p.rules[rid].query[pivot];
                let Call::Table(t) = a.call else {
                    unreachable!()
                };
                let mut slots = BTreeMap::new();
                let mut projection = vec![];
                let pattern = a
                    .args
                    .iter()
                    .enumerate()
                    .map(|(col, arg)| match arg {
                        Arg::Literal(v) => Arg::Literal(v.clone()),
                        Arg::Slot(i) => {
                            let n = slots.len();
                            let normalized = *slots.entry(*i).or_insert_with(|| {
                                projection.push((*i, col));
                                n
                            });
                            Arg::Slot(normalized)
                        }
                    })
                    .collect::<Vec<_>>();
                let n = shapes.len();
                let id = *shapes.entry((t, pattern)).or_insert(n);
                s.anchor_shapes.insert((rid, pivot), (id, projection));
            }
        }
        // Store one cursor per committed anchor. Consumer programs attach only
        // on their own Run, rather than preparing the entire fanout at publish.
        for (rid, r) in p.rules.iter().enumerate() {
            let ops = p.templates[r.template]
                .ops
                .iter()
                .enumerate()
                .filter_map(|(i, op)| {
                    // A participating RHS can also read a function whose current
                    // row is an anchor. Preserve that publication, as with a write.
                    match op {
                        Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _)
                            if anchor_tables.contains(t) =>
                        {
                            Some(i)
                        }
                        _ => None,
                    }
                })
                .collect::<Vec<_>>();
            let is_producer = ops
                .iter()
                .any(|&i| crate::multistage::source(p, rid, "effect", i).is_ok());
            let mut query_facts = QueryFacts::None;
            let mut anchors = Vec::new();
            if is_producer {
                for op in ops {
                    let source = p
                        .known_results
                        .static_sources
                        .get(rid)
                        .and_then(|rows| rows[op]);
                    if let Some(Source::Query(atom)) = source {
                        match query_facts {
                            QueryFacts::None => query_facts = QueryFacts::One(atom, None),
                            QueryFacts::One(first, _) if first != atom => {
                                query_facts = QueryFacts::All(vec![None; r.query.len()])
                            }
                            _ => {}
                        }
                    }
                    anchors.push((op, source));
                }
            }
            s.anchor_ops.push(anchors);
            s.alias_query_facts.push(query_facts);
        }
        #[cfg(feature = "spine-shapes")]
        s.shapes.catalog(p, &s.anchor_ops);
        s
    }
    pub fn begin(
        &mut self,
        p: &Program,
        rid: usize,
        values: &BTreeMap<usize, Value>,
        set: &str,
        epoch: u64,
    ) -> Option<Cursor> {
        let mut step = self.begin_step(p, rid, values, set, epoch)?;
        let cursor = step.cursor(&mut self.stats).clone();
        cursor.root.materialize(&mut self.stats);
        Some(cursor)
    }
    pub(crate) fn begin_step(
        &mut self,
        p: &Program,
        rid: usize,
        values: &BTreeMap<usize, Value>,
        set: &str,
        epoch: u64,
    ) -> Option<Started> {
        if !self.enabled {
            return None;
        }
        let key = (!self.origins.is_empty()).then(|| {
            (
                rid,
                p.rules[rid]
                    .inputs
                    .iter()
                    .map(|i| values[i].clone())
                    .collect(),
            )
        });
        let previous = key.and_then(|k| self.origins.remove(&k));
        if previous.is_none() && self.anchor_ops.get(rid).is_none_or(Vec::is_empty) {
            return None;
        }
        self.stats.steps_executed += 1;
        *self.stats.by_set.entry(set.into()).or_default() += 1;
        if let Some(mut cursor) = previous {
            self.stats.resumed_steps += 1;
            #[cfg(feature = "spine-blocks")]
            {
                self.stats.block_resumptions += u64::from(cursor.via_block);
            }
            *self.stats.resumed_by_set.entry(set.into()).or_default() += 1;
            cursor.depth = cursor
                .depth
                .checked_add(1)
                .expect("stage recipe depth exhausted");
            #[cfg(feature = "spine-shapes")]
            self.shapes.resume(&mut cursor.trace, cursor.producer, rid);
            cursor.producer = rid;
            let mut c = cursor.root.materialize(&mut self.stats).borrow_mut();
            self.stats.cross_stage_steps += u64::from(!c.sets.contains(set));
            c.sets.insert(set.into());
            if c.last_epoch != epoch {
                c.visits += 1;
                c.last_epoch = epoch;
            }
            self.stats.max_chain = self.stats.max_chain.max(cursor.depth);
            self.stats.max_sets_per_spine = self.stats.max_sets_per_spine.max(c.sets.len());
            self.stats.max_stage_visits = self.stats.max_stage_visits.max(c.visits);
            drop(c);
            Some(Started::Live(cursor))
        } else {
            let id = self.next_context;
            self.next_context += 1;
            self.stats.root_attempts += 1;
            self.stats.marker_roots += u64::from(
                p.templates[p.rules[rid].template]
                    .ops
                    .iter()
                    .any(|o| match o {
                        Op::Set(t, _, _) | Op::Let(_, Call::Table(t), _) => {
                            p.views.markers.contains(t)
                        }
                        _ => false,
                    }),
            );
            self.stats.max_chain = self.stats.max_chain.max(1);
            self.stats.max_sets_per_spine = self.stats.max_sets_per_spine.max(1);
            self.stats.max_stage_visits = self.stats.max_stage_visits.max(1);
            if self.root_set.as_ref() != set {
                self.root_set = Rc::from(set);
            }
            Some(Started::Root {
                id,
                producer: rid,
                epoch,
                set: self.root_set.clone(),
            })
        }
    }
    pub fn capture_receipts(&self, rid: usize) -> bool {
        self.enabled && self.receipts && self.anchor_ops.get(rid).is_some_and(|ops| !ops.is_empty())
    }
    pub(crate) fn alias_query_facts(&self, rid: usize) -> QueryFacts {
        if self.receipts {
            self.alias_query_facts.get(rid).cloned().unwrap_or_default()
        } else {
            QueryFacts::None
        }
    }
    /// Query witnesses are used only as receipts for proved RHS aliases.
    pub fn capture(
        &mut self,
        g: &Graph,
        rid: usize,
        cell: u32,
        cursor: Cursor,
        query: Option<&[Option<Dependency>]>,
        effects: Option<&[Option<Dependency>]>,
    ) -> Result<()> {
        self.capture_step(
            g,
            rid,
            cell,
            Started::Live(cursor),
            |atom| query.and_then(|rows| rows.get(atom).copied().flatten()),
            effects,
        )
    }
    pub(crate) fn capture_step(
        &mut self,
        g: &Graph,
        rid: usize,
        cell: u32,
        step: Started,
        query_fact: impl Fn(usize) -> Option<Dependency>,
        effects: Option<&[Option<Dependency>]>,
    ) -> Result<()> {
        self.stats.completed_steps += 1;
        if self.anchor_ops.get(rid).is_none_or(Vec::is_empty) {
            return Ok(());
        }
        let clock = Instant::now();
        let rule = &g.program.rules[rid];
        let template = &g.program.templates[rule.template];
        self.capture_unions.get_or_insert(g.stats.unions);
        let slot = self.pending_steps.len();
        self.pending_steps.push(step);
        for &(i, source) in &self.anchor_ops[rid] {
            let (table, args) = match &template.ops[i] {
                Op::Let(_, Call::Table(t), args) | Op::Set(t, args, _) => (*t, args),
                _ => unreachable!("validated anchor operation"),
            };
            let receipt = effects.and_then(|rows| rows[i]).or_else(|| {
                if !self.receipts || g.known_mode != "static" {
                    return None;
                }
                match source? {
                    Source::Query(atom) => {
                        let w = query_fact(atom)?;
                        self.stats.alias_query_forwards += 1;
                        Some(w)
                    }
                    Source::Effect(op) => {
                        let w = effects?[op]?;
                        self.stats.alias_effect_forwards += 1;
                        Some(w)
                    }
                }
            });
            self.pending_anchors.push((
                table,
                g.arguments(cell, args)?,
                Publication {
                    slot,
                    receipt,
                    #[cfg(feature = "spine-shapes")]
                    source: crate::graph::FactRef { cell, op: i as u32 },
                },
            ));
            self.stats.captured_anchors += 1;
        }
        self.stats.captured_contexts += 1;
        self.stats.capture_seconds += clock.elapsed().as_secs_f64();
        Ok(())
    }
    pub fn publish(&mut self, g: &mut Graph) -> Result<()> {
        if !self.enabled {
            return Ok(());
        };
        let clock = Instant::now();
        // Removed/coalesced anchors are not reusable. Other stale ports are
        // refreshed by exact keys when their legal consumer stage arrives.
        for w in g.query_changes.keys() {
            let live = match *w {
                Dependency::Fact(f) => g
                    .cells
                    .get(&f.cell)
                    .and_then(|c| c.facts.get(&f.op))
                    .is_some_and(|f| !f.hidden),
                Dependency::Function(i) => g.functions.get(i).is_some_and(|f| f.live),
            };
            if live {
                continue;
            }
            if self.entries.remove(w).is_some() {
                self.stats.anchors_retired += 1;
                let t = g.query_changes[w];
                let n = self.active_tables.get_mut(&t).unwrap();
                *n -= 1;
                if *n == 0 {
                    self.active_tables.remove(&t);
                }
            }
        }
        // Unions can merge different keys after capture. Preserve the last
        // producer in original RHS order at the post-repair canonical key.
        let mut anchors = HashMap::new();
        let recanonicalize = self
            .capture_unions
            .take()
            .is_some_and(|before| before != g.stats.unions);
        for (table, mut key, publication) in self.pending_anchors.drain(..) {
            if recanonicalize {
                for value in &mut key {
                    *value = canon(value, &g.parents);
                }
            }
            anchors.insert((table, key), publication);
        }
        self.stats.distinct_anchor_keys += anchors.len() as u64;
        let hits_before = self.stats.receipt_hits;
        for ((table, key), publication) in anchors {
            let Publication { slot, receipt, .. } = publication;
            let mut witness = None;
            if self.receipts {
                if let Some(old) = receipt {
                    self.stats.receipt_attempts += 1;
                    let w = g.resolve_receipt(old);
                    self.stats.receipt_redirects += u64::from(w != old);
                    if g.receipt_visible(w, table) {
                        if self.audit_receipts {
                            self.stats.receipt_decodes += 1;
                            if g.row(w)?.is_none_or(|r| {
                                r.hidden || r.table != table || r.values[..key.len()] != key
                            }) {
                                return Err(
                                    "commit receipt does not preserve its source key".into()
                                );
                            }
                        }
                        self.stats.receipt_hits += 1;
                        witness = Some(w);
                    } else {
                        self.stats.receipt_rejections += 1;
                    }
                } else {
                    self.stats.receipt_missing += 1;
                }
            }
            if witness.is_none() {
                self.stats.capture_reads += 1;
                witness = g
                    .lookup(table, &key)?
                    .filter(|r| !r.hidden)
                    .map(|r| r.witness);
            }
            if let Some(w) = witness {
                let cursor = self.pending_steps[slot].cursor(&mut self.stats).clone();
                #[cfg(feature = "spine-shapes")]
                let cursor = {
                    let mut cursor = cursor;
                    cursor.trace.source = Some(publication.source);
                    cursor
                };
                if self.entries.insert(w, cursor).is_none() {
                    *self.active_tables.entry(table).or_default() += 1;
                }
                self.stats.anchor_publications += 1;
            } else {
                self.stats.missing_capture_rows += 1;
            }
        }
        let hits = self.stats.receipt_hits - hits_before;
        if hits > 0 {
            *self
                .stats
                .receipt_hits_by_source
                .entry("effect".into())
                .or_default() += hits;
        }
        self.pending_steps.clear();
        self.origins.clear();
        self.entry_cache.clear();
        self.row_cache.clear();
        self.stats.publication_seconds += clock.elapsed().as_secs_f64();
        Ok(())
    }
    pub fn answers(
        &mut self,
        g: &mut Graph,
        rid: usize,
        pending: &BTreeSet<Dependency>,
        watermark: u64,
    ) -> Result<Option<Coverage>> {
        if !self.active_for_rule(rid) {
            return Ok(None);
        }
        let clock = Instant::now();
        let result = self.answers_inner(g, rid, pending, watermark);
        self.stats.query_seconds += clock.elapsed().as_secs_f64();
        result
    }
    fn answers_inner(
        &mut self,
        g: &mut Graph,
        rid: usize,
        pending: &BTreeSet<Dependency>,
        watermark: u64,
    ) -> Result<Option<Coverage>> {
        self.stats.query_attempts += 1;
        self.prepare_epoch(g.epoch);
        let shared = self.shared();
        let rule = g.program.rules[rid].clone();
        let mut ready = vec![];
        let mut covered = BTreeSet::new();
        let mut complete = true;
        for &w in pending {
            // Partial dispatch uses witness identity/table headers, not a second
            // candidate decode for every entry that ordinary matching must read.
            let mut eligible = BTreeSet::new();
            if self.partial {
                self.stats.dispatch_headers += 1;
                let Some(t) = g.live_table(w) else { continue };
                for (pivot, a) in rule.query.iter().enumerate() {
                    if !matches!(a.call, Call::Table(table) if table == t) {
                        continue;
                    }
                    if self.entries.contains_key(&w) && self.orders.contains_key(&(rid, pivot)) {
                        eligible.insert(pivot);
                    } else {
                        self.record_fallbacks(rid, pivot, 1);
                        if complete {
                            self.stats.uncovered_queries += 1;
                        }
                        complete = false;
                    }
                }
                if eligible.is_empty() {
                    continue;
                }
            }
            let row = if shared {
                self.seed_row(g, w)?
            } else {
                self.stats.delta_rows_examined += 1;
                g.stats.decoded_rows += 1;
                g.row(w)?.map(Rc::new)
            };
            let Some(row) = row else { continue };
            if row.hidden || row.epoch < watermark {
                continue;
            }
            for (pivot, a) in rule.query.iter().enumerate() {
                if !matches!(a.call,Call::Table(t) if t==row.table)
                    || (self.partial && !eligible.contains(&pivot))
                {
                    continue;
                }
                let mut input = Inputs::owned(g.program.templates[rule.template].sorts.len());
                // A failed literal/alias bind proves this exact entry empty too.
                // Only partial mode needs to mark it for residual dispatch.
                if self.partial {
                    covered.insert((pivot, w));
                }
                let live = if shared {
                    self.bind_seed(g, rid, pivot, &row, input.owned_mut())
                } else {
                    self.stats.pivot_candidates += 1;
                    a.args
                        .iter()
                        .zip(&row.values)
                        .all(|(a, v)| bind(a, v, &mut input, g))
                };
                if !live {
                    continue;
                }
                let Some(cursor) = self
                    .entries
                    .get(&w)
                    .filter(|_| self.orders.contains_key(&(rid, pivot)))
                    .cloned()
                else {
                    self.record_fallbacks(rid, pivot, 1);
                    self.stats.uncovered_queries += 1;
                    return Ok(None);
                };
                ready.push((pivot, row.clone(), input, cursor));
                covered.insert((pivot, w));
            }
        }
        self.finish_answers(g, rid, ready, covered, complete, watermark)
    }
    pub(crate) fn active_for_rule(&self, rid: usize) -> bool {
        self.enabled
            && self
                .consumer_tables
                .get(&rid)
                .is_some_and(|tables| tables.iter().any(|t| self.active_tables.contains_key(t)))
    }

    /// A live cursor only owns this pivot if the compiler supplied an exhaustive
    /// functional-key order for it. Other witnesses of the same table can fall back.
    pub(crate) fn scheduled_cursors(
        &self,
        rid: usize,
        pivot: usize,
        table: usize,
    ) -> Option<&HashMap<Dependency, Cursor>> {
        (self.enabled
            && self.active_tables.contains_key(&table)
            && self.orders.contains_key(&(rid, pivot)))
        .then_some(&self.entries)
    }

    pub fn answers_scheduled(
        &mut self,
        g: &mut Graph,
        rid: usize,
        seeds: &[Seed],
        watermark: u64,
    ) -> Result<Option<Coverage>> {
        if !self.active_for_rule(rid) {
            return Ok(None);
        }
        // This API also certifies an externally supplied empty domain. The
        // scheduler's routed API only reports takeovers for delivered seeds.
        if seeds.is_empty() {
            self.stats.query_attempts += 1;
            return self.finish_answers(g, rid, Vec::new(), BTreeSet::new(), true, watermark);
        }
        let mut routes = Vec::new();
        for (i, seed) in seeds.iter().enumerate() {
            let row = &seed.packet.row;
            if let Some(cursor) = self
                .scheduled_cursors(rid, seed.atom, row.table)
                .and_then(|c| c.get(&row.witness))
            {
                routes.push((i, cursor.clone()));
            } else {
                self.record_fallbacks(rid, seed.atom, 1);
            }
        }
        self.answers_routed(g, rid, seeds, routes, watermark)
    }

    pub(crate) fn answers_routed(
        &mut self,
        g: &mut Graph,
        rid: usize,
        seeds: &[Seed],
        mut routes: Vec<(usize, Cursor)>,
        watermark: u64,
    ) -> Result<Option<Coverage>> {
        self.stats.routed_spine_seeds += routes.len() as u64;
        self.stats.routed_matcher_seeds += (seeds.len() - routes.len()) as u64;
        if !self.active_for_rule(rid) {
            return Ok(None);
        }
        self.stats.query_attempts += 1;
        if routes.is_empty() {
            self.stats.uncovered_queries += u64::from(!seeds.is_empty());
            return Ok(None);
        }
        let clock = Instant::now();
        let complete = routes.len() == seeds.len();
        if !complete {
            self.stats.mixed_routed_queries += 1;
            self.stats.uncovered_queries += 1;
        }
        // Equal-depth continuations retain the original witness/atom tie break.
        routes.sort_unstable_by_key(|(i, _)| (seeds[*i].packet.row.witness, seeds[*i].atom));
        let mut covered = BTreeSet::new();
        let mut ready = Vec::with_capacity(routes.len());
        for (i, cursor) in routes {
            let seed = &seeds[i];
            covered.insert((seed.atom, seed.packet.row.witness));
            ready.push((
                seed.atom,
                seed.packet.row.clone(),
                Inputs::shared(seed.clone()),
                cursor,
            ));
        }
        let result = self.finish_answers(g, rid, ready, covered, complete, watermark);
        self.stats.query_seconds += clock.elapsed().as_secs_f64();
        result
    }
    fn finish_answers(
        &mut self,
        g: &mut Graph,
        rid: usize,
        ready: Vec<(usize, Rc<Row>, Inputs, Cursor)>,
        covered: BTreeSet<(usize, Dependency)>,
        complete: bool,
        watermark: u64,
    ) -> Result<Option<Coverage>> {
        if !complete && covered.is_empty() {
            return Ok(None);
        }
        let rule = g.program.rules[rid].clone();
        let mut results: BTreeMap<Vec<Value>, (Answer, Cursor)> = BTreeMap::new();
        for (pivot, row, mut input, cursor) in ready {
            let mut witnesses = vec![None; rule.query.len()];
            witnesses[pivot] = Some(row.witness);
            let mut epoch = row.epoch;
            #[cfg(feature = "spine-shapes")]
            let cursor = {
                let mut c = cursor;
                c.trace.pivot = pivot;
                c.trace.same_fact = c.trace.source.map(Dependency::Fact) == Some(row.witness);
                c
            };
            #[cfg(feature = "spine-blocks")]
            let mut cursor = cursor;
            #[cfg(feature = "spine-blocks")]
            {
                cursor.via_block = false;
                if g.slot_transfer == crate::graph::SlotTransfer::Off
                    && let Some(block) = g.program.spine_blocks.get(rid, pivot)
                {
                    self.stats.block_entries += 1;
                    let port = if let Some(p) = &block.port {
                        let args = p.key.iter().map(|v| v.get(g, &row, None)).collect();
                        let Some(found) = read_port(g, &cursor, p.table, args, &mut self.stats)?
                        else {
                            continue;
                        };
                        if p.check_result
                            .as_ref()
                            .is_some_and(|v| v.get(g, &row, None) != *found.values.last().unwrap())
                        {
                            continue;
                        }
                        epoch = epoch.max(found.epoch);
                        witnesses[p.atom] = Some(found.witness);
                        Some(found)
                    } else {
                        None
                    };
                    if epoch >= watermark {
                        let values = block
                            .inputs
                            .iter()
                            .map(|v| v.get(g, &row, port.as_ref()))
                            .collect();
                        cursor.via_block = true;
                        record_answer(&mut results, values, witnesses, epoch, None, cursor);
                    }
                    continue;
                }
            }
            let mut valid = true;
            let order = &self.orders[&(rid, pivot)];
            // Only the owner of the actual committed pivot can back RHS slots.
            // A different cell may have proposed the same canonical anchor.
            let parent = g.slot_transfer.parent_routes() && rule.direct == Some(pivot);
            let siblings = g.slot_transfer.sibling_routes() && !order.is_empty();
            #[cfg(feature = "slot-transfer-audit")]
            let proof_sample = if (parent || siblings) && !g.views_enabled {
                crate::transfer_audit::Sample::begin(crate::transfer_audit::Region::SpineProof)
            } else {
                None
            };
            let source = if (parent || siblings) && !g.views_enabled {
                #[cfg(feature = "slot-transfer-audit")]
                crate::transfer_audit::count(|c| c.spine_owner_checks += 1);
                match row.witness {
                    Dependency::Fact(f)
                        if g.cells.get(&f.cell).is_some_and(|c| {
                            c.completed
                                && c.template == g.program.rules[cursor.producer].template
                                && c.facts
                                    .get(&f.op)
                                    .is_some_and(|s| s.entry.is_none() && s.marker_row.is_none())
                        }) =>
                    {
                        Some(f)
                    }
                    _ => None,
                }
            } else {
                None
            };
            let route = source
                .and_then(|f| {
                    #[cfg(feature = "slot-transfer-audit")]
                    crate::transfer_audit::count(|c| c.spine_route_lookups += 1);
                    g.program.multi_stage.routes.get(&(
                        g.program.rules[cursor.producer].template,
                        f.op as usize,
                        rid,
                        pivot,
                    ))
                })
                .cloned();
            #[cfg(feature = "slot-transfer-audit")]
            crate::transfer_audit::count(|c| c.spine_route_hits += u64::from(route.is_some()));
            let parameter_parent = source.filter(|_| parent && route.is_some());
            let local_route = if !siblings {
                None
            } else {
                route.and_then(|r| g.continuation_context(row.witness).map(|c| (c, r)))
            };
            #[cfg(feature = "slot-transfer-audit")]
            drop(proof_sample);
            let mut wired = false;
            let mut trail = Vec::new();
            for &i in order {
                let a = &rule.query[i];
                if let Some((ctx, route)) = &local_route
                    && let Some(read) = route
                        .reads
                        .iter()
                        .find(|r| r.atom == i && r.effect.is_some())
                    && let Some((w, stamp)) =
                        read.wire(g, *ctx, row.witness, &mut input, &mut trail, None)?
                {
                    g.stats.slot_transfer.spine_atoms += 1;
                    witnesses[i] = Some(w);
                    epoch = epoch.max(stamp);
                    wired = true;
                    continue;
                }
                let args = a.args[..a.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, &input, g).ok_or("unbound stage recipe key".into()))
                    .collect::<Result<Vec<_>>>()?;
                match &a.call {
                    Call::Table(t) => {
                        let Some(row) = read_port(g, &cursor, *t, args, &mut self.stats)? else {
                            valid = false;
                            break;
                        };
                        if !a
                            .args
                            .iter()
                            .zip(&row.values)
                            .all(|(a, v)| bind(a, v, &mut input, g))
                        {
                            valid = false;
                            break;
                        }
                        epoch = epoch.max(row.epoch);
                        witnesses[i] = Some(row.witness);
                    }
                    Call::Primitive(name, output) => {
                        self.stats.guards += 1;
                        if name.starts_with("cell-key-") {
                            g.stats.key_operations += 1;
                        }
                        let value = value::primitive(
                            name,
                            &args,
                            output,
                            &g.program.artifact["source_core"]["structural_key"]["nodes"],
                        )?;
                        if value
                            .as_ref()
                            .is_none_or(|v| !bind(a.args.last().unwrap(), v, &mut input, g))
                        {
                            self.stats.guard_rejections += 1;
                            valid = false;
                            break;
                        }
                    }
                }
            }
            if valid && epoch >= watermark {
                let values = rule
                    .inputs
                    .iter()
                    .map(|i| {
                        (if wired {
                            input.take_slot(*i, &mut g.stats.slot_transfer.sealed_moves)
                        } else {
                            #[cfg(feature = "slot-transfer-audit")]
                            crate::transfer_audit::count(|c| {
                                c.binding_seal_copies += u64::from(input[*i].is_some())
                            });
                            input[*i].clone()
                        })
                        .ok_or("incomplete stage recipe input".into())
                    })
                    .collect::<Result<Vec<_>>>()?;
                record_answer(
                    &mut results,
                    values,
                    witnesses,
                    epoch,
                    parameter_parent,
                    cursor,
                );
            }
        }
        if complete {
            self.stats.query_takeovers += 1;
        } else {
            self.stats.partial_queries += 1;
        }
        self.stats.pivots_skipped += covered.len() as u64;
        *self.stats.covered_by_rule.entry(rid).or_default() += covered.len() as u64;
        self.stats.bindings += results.len() as u64;
        *self.stats.bindings_by_rule.entry(rid).or_default() += results.len() as u64;
        Ok(Some(Coverage {
            complete,
            covered,
            answers: results
                .into_iter()
                .map(|(key, (answer, cursor))| {
                    self.origins.insert((rid, key), cursor);
                    answer
                })
                .collect(),
        }))
    }
    pub fn shared(&self) -> bool {
        self.enabled && self.partial && self.shared_entries
    }
    fn prepare_epoch(&mut self, epoch: u64) {
        if self.cache_epoch != epoch {
            self.entry_cache.clear();
            self.row_cache.clear();
            self.cache_epoch = epoch;
        }
    }
    /// All source queries share one immutable LHS snapshot within this Run.
    pub fn seed_row(&mut self, g: &mut Graph, w: Dependency) -> Result<Option<Rc<Row>>> {
        self.prepare_epoch(g.epoch);
        if let Some(row) = self.row_cache.get(&w) {
            self.stats.shared_row_hits += 1;
            return Ok(row.clone());
        }
        self.stats.delta_rows_examined += 1;
        g.stats.decoded_rows += 1;
        let row = g.row(w)?.map(Rc::new);
        self.row_cache.insert(w, row.clone());
        Ok(row)
    }
    /// Exact alpha-equivalent anchor predicates can feed different residual
    /// recipes too. Reuse their checked fields, never an observed full answer.
    pub fn bind_seed(
        &mut self,
        g: &Graph,
        rid: usize,
        pivot: usize,
        row: &Row,
        input: &mut [Option<Value>],
    ) -> bool {
        self.prepare_epoch(g.epoch);
        *self.stats.entry_requests_by_rule.entry(rid).or_default() += 1;
        let (shape, projection) = &self.anchor_shapes[&(rid, pivot)];
        let key = (*shape, row.witness);
        if let Some((valid, last_rule)) = self.entry_cache.get_mut(&key) {
            self.stats.shared_entry_hits += 1;
            self.stats.shared_entry_cross_rule_hits += u64::from(*last_rule != rid);
            *last_rule = rid;
            if *valid {
                self.stats.entry_parameters_copied += projection.len() as u64;
                for &(slot, col) in projection {
                    input[slot] = Some(row.values[col].clone());
                }
            }
            *valid
        } else {
            self.stats.pivot_candidates += 1;
            *self.stats.entry_checks_by_rule.entry(rid).or_default() += 1;
            let valid = g.program.rules[rid].query[pivot]
                .args
                .iter()
                .zip(&row.values)
                .all(|(a, v)| match a {
                    Arg::Literal(l) => canon(l, &g.parents) == *v,
                    Arg::Slot(i) => match &input[*i] {
                        Some(old) => old == v,
                        None => {
                            self.stats.entry_parameters_copied += 1;
                            #[cfg(feature = "slot-transfer-audit")]
                            crate::transfer_audit::count(|c| c.fresh_bind_copies += 1);
                            input[*i] = Some(v.clone());
                            true
                        }
                    },
                });
            self.entry_cache.insert(key, (valid, rid));
            self.stats.peak_cached_entries =
                self.stats.peak_cached_entries.max(self.entry_cache.len());
            valid
        }
    }
    pub(crate) fn record_fallbacks(&mut self, rid: usize, pivot: usize, count: u64) {
        if count == 0 {
            return;
        }
        self.stats.fallback_pivots += count;
        *self.stats.fallback_by_rule.entry(rid).or_default() += count;
        if self.orders.contains_key(&(rid, pivot)) {
            self.stats.missing_context_pivots += count;
        } else {
            self.stats.unsupported_key_pivots += count;
        }
    }
    pub fn effects(&mut self, constructors: u64, updates: u64, unions: u64) {
        self.stats.constructor_requests += constructors;
        self.stats.function_updates += updates;
        self.stats.union_requests += unions;
    }
    #[cfg(feature = "spine-shapes")]
    pub fn shape_report(&self) -> J {
        self.shapes.report()
    }
    pub fn report(&self) -> J {
        let contexts = self
            .entries
            .values()
            .map(|c| (c.root.id, &c.root))
            .collect::<BTreeMap<_, _>>();
        let materialized = contexts.values().filter_map(|root| root.context.get());
        let live_materialized = materialized.clone().count();
        let facts: usize = materialized.map(|c| c.borrow().facts.len()).sum();
        json!({"enabled":self.enabled,"receipts_enabled":self.receipts,"partial_enabled":self.partial,"shared_entries_enabled":self.shared_entries,"stats":self.stats,"producer_rules":self.anchor_ops.iter().filter(|ops| !ops.is_empty()).count(),
            "keyed_entries":self.orders.len(),"live_spines":contexts.len(),"live_materialized_contexts":live_materialized,
            "entry_references":self.entries.len(),"context_fact_references":facts,
            "scope":"source-scheduled RHS continuation from committed producer contexts; exhaustive functional-key entries; unknown delta entries use ordinary matching; no early effect visibility"})
    }
}
