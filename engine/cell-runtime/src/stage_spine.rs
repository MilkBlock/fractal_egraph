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
use rustc_hash::FxHashMap as HashMap;
use serde::Serialize;
use serde_json::{Value as J, json};
use std::{
    cell::{OnceCell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::Instant,
};

#[cfg(feature = "spine-shapes")]
#[path = "spine_shapes.rs"]
mod shapes;
#[derive(Clone)]
pub struct Cursor {
    #[cfg(feature = "iterative-bake")]
    state: crate::iterative::State,
    #[cfg(feature = "spine-blocks")]
    via_block: bool,
    #[cfg(feature = "spine-shapes")]
    trace: shapes::Trace,
    root: Rc<Root>,
    pub depth: u32,
    pub producer: usize,
}
#[cfg(feature = "sparse-cells")]
#[derive(Clone)]
enum PublishedBase {
    Root(Rc<Root>, usize),
    Active(Rc<Cursor>),
    #[cfg(feature = "sparse-cells")]
    Solo {
        id: u64,
        epoch: u64,
        stage: usize,
        producer: usize,
        root: OnceCell<Rc<Root>>,
    },
}
#[cfg(feature = "sparse-cells")]
pub(crate) struct Published {
    base: PublishedBase,
    output: usize,
    #[cfg(feature = "spine-shapes")]
    source: crate::graph::FactRef,
}
#[cfg(feature = "sparse-cells")]
impl Published {
    fn root_info(&self) -> (u64, Option<&Root>) {
        match &self.base {
            PublishedBase::Root(r, _) => (r.id, Some(r)),
            PublishedBase::Active(c) => (c.root.id, Some(&c.root)),
            #[cfg(feature = "sparse-cells")]
            PublishedBase::Solo { id, root, .. } => (*id, root.get().map(Rc::as_ref)),
        }
    }
    pub(crate) fn expand(&self) -> Cursor {
        let mut c = match &self.base {
            PublishedBase::Root(r, p) => Cursor::fresh(r.clone(), *p),
            PublishedBase::Active(c) => (**c).clone(),
            #[cfg(feature = "sparse-cells")]
            PublishedBase::Solo {
                id,
                epoch,
                stage,
                producer,
                root,
            } => Cursor::fresh(
                root.get_or_init(|| {
                    Rc::new(Root {
                        id: *id,
                        epoch: *epoch,
                        set: *stage,
                        context: OnceCell::new(),
                    })
                })
                .clone(),
                *producer,
            ),
        };
        c.state.output = self.output;
        #[cfg(feature = "spine-shapes")]
        {
            c.trace.source = Some(self.source);
        }
        c
    }
}
#[cfg(not(feature = "sparse-cells"))]
pub(crate) type Published = Cursor;
#[cfg(not(feature = "sparse-cells"))]
impl Cursor {
    pub(crate) fn expand(&self) -> Self {
        self.clone()
    }
    fn root_info(&self) -> (u64, Option<&Root>) {
        (self.root.id, Some(&self.root))
    }
}
#[cfg(feature = "sparse-cells")]
impl Cursor {
    fn fresh(root: Rc<Root>, producer: usize) -> Self {
        Self {
            root,
            producer,
            depth: 1,
            state: Default::default(),
            via_block: false,
            #[cfg(feature = "spine-shapes")]
            trace: Default::default(),
        }
    }
}
#[cfg(feature = "sparse-cells")]
type StageTag = usize;
#[cfg(not(feature = "sparse-cells"))]
type StageTag = Rc<str>;
/// Shared by every published anchor of one logical root. The cell stores a
/// pointer so an unused root never reserves space for the full Context.
struct Root {
    id: u64,
    epoch: u64,
    set: StageTag,
    context: OnceCell<Box<RefCell<Context>>>,
}
impl Root {
    fn materialize(&self, stats: &mut Stats) -> &RefCell<Context> {
        self.context.get_or_init(|| {
            stats.contexts_created += 1;
            Box::new(RefCell::new(Context {
                #[cfg(not(feature = "sparse-cells"))]
                sets: BTreeSet::from([self.set.to_string()]),
                #[cfg(feature = "sparse-cells")]
                sets: smallvec::smallvec![self.set],
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
        set: StageTag,
    },
    Live(Cursor),
}
impl Started {
    #[cfg(feature = "iterative-bake")]
    pub(crate) fn closed(&self, catalog: &crate::iterative::Catalog) -> bool {
        matches!(self,Self::Live(c) if catalog.closed(&c.state))
    }
    #[cfg(feature = "iterative-bake")]
    pub(crate) fn result_inputs(
        &self,
        catalog: &crate::iterative::Catalog,
    ) -> smallvec::SmallVec<[crate::result_handoff::Input; 4]> {
        let Self::Live(c) = self else {
            return smallvec::SmallVec::new();
        };
        catalog
            .program(&c.state)
            .iter()
            .map(|r| crate::result_handoff::Input {
                op: r.op,
                back: r.back,
                fact: c.state.history.get(r),
                equal_output: r.equal_output,
            })
            .collect()
    }
    #[cfg(feature = "iterative-bake")]
    fn remember(
        &mut self,
        cat: &crate::iterative::Catalog,
        rid: usize,
        read: impl Fn(usize) -> Option<Dependency>,
        effects: Option<&[Option<Dependency>]>,
    ) {
        if cat.history_depth() == 0 {
            return;
        }
        if let Self::Live(c) = self {
            c.state.history.capture(
                cat.history_depth(),
                rid,
                cat.capture_sources(&c.state),
                read,
                effects,
            );
        }
    }
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
                #[cfg(feature = "iterative-bake")]
                state: Default::default(),
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
#[cfg(feature = "sparse-cells")]
type PortKey = smallvec::SmallVec<[Value; 1]>;
#[cfg(not(feature = "sparse-cells"))]
type PortKey = Vec<Value>;
struct Context {
    #[cfg(not(feature = "sparse-cells"))]
    sets: BTreeSet<String>,
    #[cfg(feature = "sparse-cells")]
    sets: smallvec::SmallVec<[usize; 2]>,
    last_epoch: u64,
    visits: u64,
    unions: u64,
    facts: BTreeMap<(usize, PortKey), Dependency>,
}
#[cfg(all(feature = "iterative-bake", feature = "sparse-cells"))]
#[derive(Debug)]
pub(crate) struct DirectOrigin(usize);

#[cfg(all(feature = "iterative-bake", not(feature = "sparse-cells")))]
pub(crate) struct DirectOrigin(pub Box<Cursor>);
#[cfg(all(feature = "iterative-bake", not(feature = "sparse-cells")))]
impl std::fmt::Debug for DirectOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DirectOrigin")
            .field(&self.0.producer)
            .finish()
    }
}
pub struct Answer {
    #[cfg(feature = "iterative-bake")]
    pub(crate) direct: Option<DirectOrigin>,

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
/// Keep the current source set's counter directly accessible between RHS
/// applications. Serialization includes both the current and settled counts.
#[derive(Default)]
struct SetCounts {
    settled: BTreeMap<String, u64>,
    recent: Option<(String, u64)>,
}
impl SetCounts {
    fn add(&mut self, name: &str) {
        if let Some((current, count)) = &mut self.recent
            && current == name
        {
            *count += 1;
            return;
        }
        if let Some((old, count)) = self.recent.take() {
            self.settled.insert(old, count);
        }
        let (key, count) = self.settled.remove_entry(name).unwrap_or_else(|| (name.to_owned(), 0));
        self.recent = Some((key, count + 1));
    }
}
impl Serialize for SetCounts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_map(
            self.settled.iter().map(|(name, count)| (name.as_str(), count))
                .chain(self.recent.iter().map(|(name, count)| (name.as_str(), count))),
        )
    }
}
#[cfg(test)]
mod set_counts_test {
    use super::*;
    #[test]
    fn serialization_includes_current_set_and_prior_occurrences() {
        let mut counts=SetCounts::default();
        for set in ["b","b","a","b"] { counts.add(set); }
        assert_eq!(json!(counts),json!({"a":1,"b":3}));
    }
}
#[derive(Default, Serialize)]
pub struct Stats {
    root_attempts: u64,
    root_handles_created: u64,
    #[cfg(feature = "sparse-cells")]
    deferred_single_roots: u64,
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
    sealed_partial_origins: u64,
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
    #[cfg(feature = "sparse-cells")]
    unmaterialized_anchor_keys: u64,
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
    by_set: SetCounts,
    resumed_by_set: SetCounts,
    bindings_by_rule: BTreeMap<usize, u64>,
    covered_by_rule: BTreeMap<usize, u64>,
    fallback_by_rule: BTreeMap<usize, u64>,
}
struct Publication {
    #[cfg(feature = "sparse-cells")]
    ordinal: u64,
    #[cfg(feature = "iterative-bake")]
    output: usize,
    slot: usize,
    receipt: Option<Dependency>,
    #[cfg(feature = "spine-shapes")]
    source: crate::graph::FactRef,
}
pub struct Programs {
    #[cfg(feature = "sparse-cells")]
    pub(crate) direct_sealed_answers: bool,
    #[cfg(feature = "structural-depth")]
    pub(crate) structural: crate::depth_runtime::Depth,
    #[cfg(feature = "structural-depth")]
    pub(crate) track_ports: Vec<usize>,
    #[cfg(feature = "iterative-bake")]
    pub(crate) training: crate::iterative::Training,
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
    table_readers: Vec<Vec<usize>>,
    // Number of distinct planned input tables with at least one live anchor.
    live_inputs: Vec<usize>,
    orders: BTreeMap<(usize, usize), Vec<usize>>,
    entries: HashMap<Dependency, Published>,
    #[cfg(feature = "sparse-cells")]
    published_bases: Vec<Option<PublishedBase>>,
    active_tables: Vec<usize>,
    // Anchor-producing operations and their optional proven receipt sources.
    anchor_ops: Vec<Vec<(usize, Option<Source>)>>,
    alias_query_facts: Vec<QueryFacts>,
    // All anchors of one application share this slot, including after key coalescence.
    pending_steps: Vec<Started>,
    #[cfg(feature = "sparse-cells")]
    pub(crate) compiled_commits: bool,
    #[cfg(feature = "sparse-cells")]
    permanent: Vec<bool>,
    #[cfg(feature = "sparse-cells")]
    direct_publications: HashMap<Dependency, (usize, Publication)>,
    pending_anchors: Vec<(usize, Vec<Value>, Publication)>,
    marker_root: Vec<bool>,
    root_set: Rc<str>,
    #[cfg(feature = "sparse-cells")]
    stage_names: HashMap<String, usize>,
    #[cfg(feature = "sparse-cells")]
    stage_number: usize,
    capture_unions: Option<u64>,
    origins: Vec<HashMap<Vec<Value>, Cursor>>,
    origin_rules: Vec<usize>,
    // Sealed bindings own move-only indices into this Run's winning cursors.
    #[cfg(all(feature = "iterative-bake", feature = "sparse-cells"))]
    sealed_cursors: Vec<Option<Cursor>>,
    next_context: u64,

    #[cfg(feature = "sparse-cells")]
    answer_scratch: Winners,
    pub stats: Stats,
}
fn bind(a: &Arg, v: &Value, input: &mut Inputs, g: &Graph) -> bool {
    match a {
        Arg::Literal(x) => canon(x, &g.parents) == *v,
        Arg::Slot(i) => match input.get(*i) {
            Some(x) => x == v,
            None => {
                input.set(*i, Some(v.clone()));
                true
            }
        },
    }
}
#[cfg(feature = "sparse-cells")]
type Winners = Vec<(Vec<Value>, (Answer, Cursor))>;
#[cfg(not(feature = "sparse-cells"))]
type Winners = BTreeMap<Vec<Value>, (Answer, Cursor)>;
fn record_answer(
    results: &mut Winners,
    values: Vec<Value>,
    witnesses: Vec<Option<Dependency>>,
    epoch: u64,

    cursor: Cursor,
) {
    #[cfg(feature = "iterative-bake")]
    let direct = cursor.state.direct;
    #[cfg(not(feature = "iterative-bake"))]
    let direct = false;
    let candidate = (
        Answer {
            #[cfg(feature = "iterative-bake")]
            direct: None,

            values: if direct || cfg!(feature = "sparse-cells") {
                Vec::new()
            } else {
                values.clone()
            },
            witnesses,
            epoch,
        },
        cursor,
    );
    #[cfg(feature = "sparse-cells")]
    results.push((values, candidate));
    #[cfg(not(feature = "sparse-cells"))]
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
    args: PortKey,
    stats: &mut Stats,
    #[cfg(feature = "structural-depth")] structural: (&mut crate::depth_runtime::Depth, Dependency),
) -> Result<Option<Row>> {
    let key = (table, args);
    let good = |r: &Row| {
        r.table == table && !r.hidden && r.values[..key.1.len()] == key.1[..]
    };
    // A candidate may fail a later port or guard, or lose
    // binding deduplication. Only an applied origin promotes the root.
    let cache = cursor.root.context.get();
    let witness = cache.and_then(|context| {
        let mut context = context.borrow_mut();
        context.canonicalize(g);
        context.facts.get(&key).copied()
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
        #[cfg(feature = "structural-depth")]
        let forwarded = structural.0.spine_read(g, structural.1, table, &key.1)?;
        #[cfg(not(feature = "structural-depth"))]
        let forwarded: Option<Row> = None;
        let probed = forwarded.is_none();
        row = match forwarded {
            Some(row) => Some(row),
            None => g.lookup_canonical(table, &key.1)?,
        };
        if let Some(context) = cache {
            let mut context = context.borrow_mut();
            if let Some(r) = row.as_ref().filter(|r| !r.hidden) {
                context.facts.insert(key, r.witness);
                stats.port_cache_fills += 1;
            } else {
                context.facts.remove(&key);
            }
        } else if probed {
            stats.pre_resume_port_lookups += 1;
        }
    }
    Ok(row.filter(|r| !r.hidden))
}
fn operand(a: &Arg, input: &Inputs, g: &Graph) -> Option<Value> {
    match a {
        Arg::Literal(v) => Some(canon(v, &g.parents)),
        Arg::Slot(i) => input.get(*i).cloned(),
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
        #[cfg(feature = "allocation-census")]
        let _allocation_scope = crate::allocation_census::scope(crate::allocation_census::Tag::Spine);
        let enabled = true;
        #[cfg(feature = "sparse-cells")]
        let permanent = {
            let mut v = vec![true; p.tables.len()];
            for t in &p.templates {
                for op in t.ops.iter() {
                    if let Op::Hide(table, ..) = op {
                        v[*table] = false;
                    }
                }
            }
            for init in &p.initial {
                if init["subsumed"].as_bool() == Some(true)
                    && let Some(t) = p.tables.iter().position(|t| init["table"] == t.name)
                {
                    v[t] = false;
                }
            }
            v
        };
        let mut s = Self {
            #[cfg(feature = "sparse-cells")]
            direct_sealed_answers: false,
            enabled,
            receipts: true,
            audit_receipts: false,
            partial: false,
            shared_entries: false,
            cache_epoch: u64::MAX,
            anchor_shapes: BTreeMap::new(),
            entry_cache: HashMap::default(),
            row_cache: HashMap::default(),
            table_readers: vec![Vec::new(); p.tables.len()],
            live_inputs: vec![0; p.rules.len()],
            orders: BTreeMap::new(),
            entries: HashMap::default(),
            #[cfg(feature = "sparse-cells")]
            published_bases: vec![],
            active_tables: vec![0; p.tables.len()],
            #[cfg(feature = "spine-shapes")]
            shapes: Default::default(),
            #[cfg(feature = "iterative-bake")]
            training: Default::default(),
            anchor_ops: vec![],
            alias_query_facts: vec![],
            pending_steps: vec![],
            pending_anchors: vec![],
            #[cfg(feature = "sparse-cells")]
            compiled_commits: false,
            #[cfg(feature = "sparse-cells")]
            permanent,
            #[cfg(feature = "sparse-cells")]
            direct_publications: Default::default(),
            marker_root: p.rules.iter().map(|r| p.templates[r.template].ops.iter().any(|op| match op {
                Op::Set(t, _, _) | Op::Let(_, Call::Table(t), _) => p.marker_tables.contains(t),
                _ => false,
            })).collect(),
            root_set: Rc::from(""),
            #[cfg(feature = "sparse-cells")]
            stage_names: Default::default(),
            #[cfg(feature = "sparse-cells")]
            stage_number: usize::MAX,
            capture_unions: None,
            #[cfg(feature = "structural-depth")]
            structural: crate::depth_runtime::Depth::new(p),
            #[cfg(feature = "structural-depth")]
            track_ports: vec![0; p.rules.len()],
            origins: vec![HashMap::default(); p.rules.len()],
            origin_rules: Vec::new(),
            #[cfg(all(feature = "iterative-bake", feature = "sparse-cells"))]
            sealed_cursors: Vec::new(),
            next_context: 0,

            #[cfg(feature = "sparse-cells")]
            answer_scratch: vec![],
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
            if !s.table_readers[t].contains(rid) {
                s.table_readers[t].push(*rid);
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
            #[cfg(feature = "iterative-bake")]
            for src in p.iterative.exports(rid) {
                if let crate::result_handoff::Donor::Query(i) = *src {
                    match query_facts {
                        QueryFacts::None => query_facts = QueryFacts::One(i, None),
                        QueryFacts::One(j, _) if i != j => {
                            query_facts = QueryFacts::All(vec![None; r.query.len()])
                        }
                        _ => {}
                    }
                }
            }
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
        values: &crate::slots::Slots,
        set: &str,
        epoch: u64,
    ) -> Option<Cursor> {
        let mut step = self.begin_step(
            p,
            rid,
            values,
            set,
            epoch,
            #[cfg(feature = "iterative-bake")]
            None,
        )?;
        let cursor = step.cursor(&mut self.stats).clone();
        cursor.root.materialize(&mut self.stats);
        Some(cursor)
    }
    pub(crate) fn begin_step(
        &mut self,
        p: &Program,
        rid: usize,
        values: &crate::slots::Slots,
        set: &str,
        epoch: u64,
        #[cfg(feature = "iterative-bake")] direct: Option<DirectOrigin>,
    ) -> Option<Started> {
        #[cfg(feature = "allocation-census")]
        let _allocation_scope = crate::allocation_census::scope(crate::allocation_census::Tag::Spine);
        if !self.enabled {
            return None;
        }
        #[cfg(all(feature = "iterative-bake", feature = "sparse-cells"))]
        let supplied = direct.map(|token| self.sealed_cursors[token.0].take().expect("sealed Spine cursor"));
        #[cfg(all(feature = "iterative-bake", not(feature = "sparse-cells")))]
        let supplied = direct.map(|d| *d.0);
        #[cfg(not(feature = "iterative-bake"))]
        let supplied: Option<Cursor> = None;
        let previous = supplied.or_else(|| {
            let bucket = &mut self.origins[rid];
            if bucket.is_empty() {
                return None;
            }
            let key: smallvec::SmallVec<[Value; 6]> = p.rules[rid]
                .inputs
                .iter()
                .map(|i| values[i].clone())
                .collect();
            bucket.remove(key.as_slice())
        });
        if previous.is_none() && self.anchor_ops.get(rid).is_none_or(Vec::is_empty) {
            return None;
        }
        #[cfg(feature = "sparse-cells")]
        if self.root_set.as_ref() != set || self.stage_number == usize::MAX {
            self.root_set = Rc::from(set);
            self.stage_number = if let Some(&n) = self.stage_names.get(set) {
                n
            } else {
                let n = self.stage_names.len();
                self.stage_names.insert(set.into(), n);
                n
            };
        }
        self.stats.steps_executed += 1;
        self.stats.by_set.add(set);
        if let Some(mut cursor) = previous {
            self.stats.resumed_steps += 1;
            #[cfg(feature = "slot-transfer-audit")]
            crate::transfer_audit::resume(cursor.root.id, cursor.producer, rid, cursor.depth);
            #[cfg(feature = "iterative-bake")]
            self.training
                .applied(&p.iterative, &mut cursor.state, p.rules[rid].inputs.len());
            #[cfg(feature = "spine-blocks")]
            {
                self.stats.block_resumptions += u64::from(cursor.via_block);
            }
            self.stats.resumed_by_set.add(set);
            cursor.depth = cursor
                .depth
                .checked_add(1)
                .expect("stage recipe depth exhausted");
            #[cfg(feature = "spine-shapes")]
            self.shapes.resume(&mut cursor.trace, cursor.producer, rid);
            cursor.producer = rid;
            let mut c = cursor.root.materialize(&mut self.stats).borrow_mut();
            #[cfg(not(feature = "sparse-cells"))]
            if !c.sets.contains(set) {
                self.stats.cross_stage_steps += 1;
                c.sets.insert(set.into());
            }
            #[cfg(feature = "sparse-cells")]
            if !c.sets.contains(&self.stage_number) {
                self.stats.cross_stage_steps += 1;
                c.sets.push(self.stage_number);
            }
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
            self.stats.marker_roots += u64::from(self.marker_root[rid]);
            self.stats.max_chain = self.stats.max_chain.max(1);
            self.stats.max_sets_per_spine = self.stats.max_sets_per_spine.max(1);
            self.stats.max_stage_visits = self.stats.max_stage_visits.max(1);
            #[cfg(not(feature = "sparse-cells"))]
            if self.root_set.as_ref() != set {
                self.root_set = Rc::from(set);
            }
            Some(Started::Root {
                id,
                producer: rid,
                epoch,
                #[cfg(not(feature = "sparse-cells"))]
                set: self.root_set.clone(),
                #[cfg(feature = "sparse-cells")]
                set: self.stage_number,
            })
        }
    }
    #[cfg(all(feature = "rhs-work-audit", feature = "rhs-locals"))]
    pub(crate) fn anchor_at(&self, r: usize, op: usize) -> bool {
        self.anchor_ops[r].iter().any(|&(i, _)| i == op)
    }
    pub fn capture_receipts(&self, rid: usize) -> bool {
        self.enabled && self.receipts && self.anchor_ops.get(rid).is_some_and(|ops| !ops.is_empty())
    }
    pub(crate) fn alias_query_facts(&self, rid: usize) -> QueryFacts {
        let base = if self.receipts {
            self.alias_query_facts.get(rid).cloned().unwrap_or_default()
        } else {
            QueryFacts::None
        };
        #[cfg(feature = "structural-depth")]
        if self.track_ports[rid] > 0 {
            return QueryFacts::Tracked(Box::new(base), vec![None; self.track_ports[rid]], 0);
        }
        base
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
        self.capture_keys(g, rid, cell, step, query_fact, effects, |_, args| {
            g.arguments(cell, args)
        })
    }
    pub(crate) fn capture_keys(
        &mut self,
        g: &Graph,
        rid: usize,
        _cell: u32,
        step: Started,
        query_fact: impl Fn(usize) -> Option<Dependency>,
        effects: Option<&[Option<Dependency>]>,
        mut key: impl FnMut(usize, &[Arg]) -> Result<Vec<Value>>,
    ) -> Result<()> {
        #[cfg(feature = "allocation-census")]
        let _allocation_scope = crate::allocation_census::scope(crate::allocation_census::Tag::Spine);
        self.stats.completed_steps += 1;
        if self.anchor_ops.get(rid).is_none_or(Vec::is_empty) {
            return Ok(());
        }
        let clock = Instant::now();
        let rule = &g.program.rules[rid];
        let template = &g.program.templates[rule.template];
        self.capture_unions.get_or_insert(g.stats.unions);
        let slot = self.pending_steps.len();
        #[cfg(feature = "iterative-bake")]
        let step = {
            let mut s = step;
            s.remember(&g.program.iterative, rid, &query_fact, effects);
            s
        };
        self.pending_steps.push(step);
        #[cfg(feature = "sparse-cells")]
        self.published_bases.push(None);
        for &(i, source) in &self.anchor_ops[rid] {
            let (table, args) = match &template.ops[i] {
                Op::Let(_, Call::Table(t), args) | Op::Set(t, args, _) => (*t, args),
                _ => unreachable!("validated anchor operation"),
            };
            let receipt = effects.and_then(|rows| rows[i]).or_else(|| {
                if !self.receipts {
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
            let publication = Publication {
                #[cfg(feature = "sparse-cells")]
                ordinal: self.stats.captured_anchors,
                slot,
                receipt,
                #[cfg(feature = "iterative-bake")]
                output: i,
                #[cfg(feature = "spine-shapes")]
                source: crate::graph::FactRef {
                    cell: _cell,
                    op: i as u32,
                },
            };
            #[cfg(feature = "sparse-cells")]
            if self.compiled_commits
                && self.permanent[table]
                && !g.has_visibility_changes(table)
                && self.receipts
                && !self.audit_receipts
                && let Some(w) = receipt
            {
                self.direct_publications.insert(w, (table, publication));
                self.stats.unmaterialized_anchor_keys += 1;
                self.stats.captured_anchors += 1;
                continue;
            }
            self.pending_anchors
                .push((table, key(i, args)?, publication));
            self.stats.captured_anchors += 1;
        }
        self.stats.captured_contexts += 1;
        self.stats.capture_seconds += clock.elapsed().as_secs_f64();
        Ok(())
    }
    pub(crate) fn write_needs_key(
        &self,
        _table: usize,
        #[cfg(feature = "sparse-cells")] visibility_changed: bool,
    ) -> bool {
        #[cfg(feature = "sparse-cells")]
        { !(self.compiled_commits && self.permanent[_table] && !visibility_changed
            && self.receipts && !self.audit_receipts) }
        #[cfg(not(feature = "sparse-cells"))]
        { true }
    }
    pub(crate) fn capture_write(
        &mut self,
        step: Started,
        rid: usize,
        _cell: u32,
        op: usize,
        table: usize,
        key: Option<Vec<Value>>,
        w: Dependency,
        unions: u64,
        #[cfg(feature = "iterative-bake")] catalog: &crate::iterative::Catalog,
        #[cfg(feature = "iterative-bake")] query: &QueryFacts,
    ) {
        #[cfg(feature = "allocation-census")]
        let _allocation_scope = crate::allocation_census::scope(crate::allocation_census::Tag::Spine);
        self.stats.completed_steps += 1;
        if self.anchor_ops[rid].is_empty() {
            return;
        }
        debug_assert_eq!(self.anchor_ops[rid].len(), 1);
        debug_assert_eq!(self.anchor_ops[rid][0].0, op);
        self.capture_unions.get_or_insert(unions);
        let slot = self.pending_steps.len();
        #[cfg(feature = "iterative-bake")]
        let step = {
            let mut s = step;
            s.remember(catalog, rid, |i| query.get(i).map(Dependency::Fact), None);
            s
        };
        self.pending_steps.push(step);
        #[cfg(feature = "sparse-cells")]
        self.published_bases.push(None);
        let publication = Publication {
            #[cfg(feature = "sparse-cells")]
            ordinal: self.stats.captured_anchors,
            slot,
            receipt: self.receipts.then_some(w),
            #[cfg(feature = "iterative-bake")]
            output: op,
            #[cfg(feature = "spine-shapes")]
            source: crate::graph::FactRef {
                cell: _cell,
                op: op as u32,
            },
        };
        if let Some(key) = key {
            self.pending_anchors.push((table, key, publication));
        } else {
            #[cfg(feature = "sparse-cells")]
            self.direct_publications.insert(w, (table, publication));
            #[cfg(not(feature = "sparse-cells"))]
            unreachable!("ordinary publication needs a key");
        }
        self.stats.captured_anchors += 1;
        self.stats.captured_contexts += 1;
    }
    fn install_anchor(&mut self, w: Dependency, table: usize, publication: Publication) {
        let slot = publication.slot;
        #[cfg(feature = "sparse-cells")]
        let cursor = {
            let base =
                self.published_bases[slot].get_or_insert_with(|| match &self.pending_steps[slot] {
                    #[cfg(feature = "sparse-cells")]
                    Started::Root {
                        id,
                        producer,
                        epoch,
                        set,
                    } if self.anchor_ops[*producer].len() == 1 => {
                        self.stats.deferred_single_roots += 1;
                        PublishedBase::Solo {
                            id: *id,
                            producer: *producer,
                            epoch: *epoch,
                            stage: *set,
                            root: OnceCell::new(),
                        }
                    }
                    Started::Root {
                        id,
                        producer,
                        epoch,
                        set,
                    } => {
                        self.stats.root_handles_created += 1;
                        PublishedBase::Root(
                            Rc::new(Root {
                                id: *id,
                                epoch: *epoch,
                                set: set.clone(),
                                context: OnceCell::new(),
                            }),
                            *producer,
                        )
                    }
                    Started::Live(c) => PublishedBase::Active(Rc::new(c.clone())),
                });
            Published {
                base: base.clone(),
                output: publication.output,
                #[cfg(feature = "spine-shapes")]
                source: publication.source,
            }
        };
        #[cfg(not(feature = "sparse-cells"))]
        let cursor = {
            let cursor = self.pending_steps[slot].cursor(&mut self.stats).clone();
            #[cfg(feature = "spine-shapes")]
            let cursor = {
                let mut cursor = cursor;
                cursor.trace.source = Some(publication.source);
                cursor
            };
            #[cfg(feature = "iterative-bake")]
            let cursor = {
                let mut c = cursor;
                c.state.output = publication.output;
                c
            };
            cursor
        };
        if self.entries.insert(w, cursor).is_none() {
            if self.active_tables[table] == 0 {
                for &rid in &self.table_readers[table] {
                    self.live_inputs[rid] += 1;
                }
            }
            self.active_tables[table] += 1;
        }
        self.stats.anchor_publications += 1;
    }
    #[cfg(feature = "sparse-cells")]
    pub(crate) fn settled(&self)->bool {
        self.pending_steps.is_empty() && self.pending_anchors.is_empty()
            && self.published_bases.is_empty() && self.direct_publications.is_empty()
            && self.sealed_cursors.is_empty() && self.origin_rules.is_empty()
            && self.capture_unions.is_none() && self.entry_cache.is_empty() && self.row_cache.is_empty()
    }
    pub fn publish(&mut self, g: &mut Graph) -> Result<()> {
        #[cfg(feature = "allocation-census")]
        let _allocation_scope = crate::allocation_census::scope(crate::allocation_census::Tag::Spine);
        if !self.enabled {
            return Ok(());
        };
        let clock = Instant::now();
        // Removed/coalesced anchors are not reusable. Other stale ports are
        // refreshed by exact keys when their legal consumer stage arrives.
        for (t, changes) in g.changes.tables() {
            if self.active_tables[t] == 0 { continue; }
            for w in changes.keys() {
                let live = match *w {
                    Dependency::Fact(f) => g.fact_state(f).is_some_and(|f| !f.hidden),
                    Dependency::Function(i) => g.functions.get(i).is_some_and(|f| f.live),
                };
                if live {
                    continue;
                }
                if self.entries.remove(w).is_some() {
                    self.stats.anchors_retired += 1;
                    let n = &mut self.active_tables[t];
                    *n -= 1;
                    if *n == 0 {
                        for &rid in &self.table_readers[t] {
                            self.live_inputs[rid] -= 1;
                        }
                        break;
                    }
                }
            }
        }
        // Unions can merge different keys after capture. Preserve the last
        // producer in original RHS order at the post-repair canonical key.
        let mut anchors = HashMap::default();
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
        #[cfg(feature = "sparse-cells")]
        for &old in g.receipt_forwards.keys() {
            if let Some(incoming) = self.direct_publications.remove(&old) {
                let target = g.resolve_receipt(old);
                match self.direct_publications.entry(target) {
                    std::collections::hash_map::Entry::Vacant(e) => {
                        e.insert(incoming);
                    }
                    std::collections::hash_map::Entry::Occupied(mut e) => {
                        if incoming.1.ordinal > e.get().1.ordinal {
                            e.insert(incoming);
                        }
                    }
                }
            }
        }
        #[cfg(feature = "sparse-cells")]
        {
            self.stats.distinct_anchor_keys += self.direct_publications.len() as u64;
        }
        let hits_before = self.stats.receipt_hits;
        for ((table, key), publication) in anchors {
            let receipt = publication.receipt;
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
                #[cfg(feature = "sparse-cells")]
                if let Some((_, direct)) = self.direct_publications.get(&w) {
                    self.stats.distinct_anchor_keys -= 1;
                    if direct.ordinal > publication.ordinal {
                        continue;
                    }
                    self.direct_publications.remove(&w);
                }
                self.install_anchor(w, table, publication);
            } else {
                self.stats.missing_capture_rows += 1;
            }
        }
        #[cfg(feature = "sparse-cells")]
        {
            let mut resolved = std::mem::take(&mut self.direct_publications);
            for (w, (table, p)) in resolved.drain() {
                // All subsume/delete producers and initially hidden rows were
                // excluded. Only same-key repair forwarding can move a receipt.
                if !g.receipt_visible(w, table) {
                    return Err("permanent producer receipt lost visibility".into());
                }
                self.stats.receipt_attempts += 1;
                self.stats.receipt_hits += 1;
                self.stats.receipt_redirects += u64::from(p.receipt != Some(w));
                self.install_anchor(w, table, p);
            }
            self.direct_publications = resolved;
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
        #[cfg(all(feature = "iterative-bake", feature = "sparse-cells"))]
        self.sealed_cursors.clear();
        #[cfg(feature = "sparse-cells")]
        self.published_bases.clear();
        for rid in self.origin_rules.drain(..) {
            self.origins[rid].clear();
        }
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
                        .zip(row.values.iter())
                        .all(|(a, v)| bind(a, v, &mut input, g))
                };
                if !live {
                    continue;
                }
                let Some(cursor) = self
                    .entries
                    .get(&w)
                    .filter(|_| self.orders.contains_key(&(rid, pivot)))
                    .map(Published::expand)
                else {
                    self.record_fallbacks(rid, pivot, 1);
                    self.stats.uncovered_queries += 1;
                    return Ok(None);
                };
                ready.push((pivot, row.clone(), input, cursor));
                covered.insert((pivot, w));
            }
        }
        self.finish_answers(
            g,
            rid,
            ready.into_iter().map(|(a, r, i, c)| (a, r, move || i, c)),
            covered,
            complete,
            watermark,
        )
    }
    #[cfg(feature = "family-survey")]
    pub(crate) fn survey_resumed(&self) -> u64 {
        self.stats.resumed_steps
    }
    pub(crate) fn active_for_rule(&self, rid: usize) -> bool {
        self.enabled && self.live_inputs[rid] != 0
    }

    /// A live cursor only owns this pivot if the compiler supplied an exhaustive
    /// functional-key order for it. Other witnesses of the same table can fall back.
    pub(crate) fn scheduled_cursors(
        &self,
        rid: usize,
        pivot: usize,
        table: usize,
    ) -> Option<&HashMap<Dependency, Published>> {
        (self.enabled
            && self.active_tables[table] != 0
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
            return self.finish_answers(
                g,
                rid,
                std::iter::empty::<(usize, Rc<Row>, fn() -> Inputs, Cursor)>(),
                BTreeSet::new(),
                true,
                watermark,
            );
        }
        let mut routes = Vec::new();
        for (i, seed) in seeds.iter().enumerate() {
            let row = &seed.packet.row;
            if let Some(cursor) = self
                .scheduled_cursors(rid, seed.atom, row.table)
                .and_then(|c| c.get(&row.witness))
            {
                routes.push((i, cursor.expand()));
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
        // A complete takeover has no residual consumer for the coverage set.
        // Stream the producer packet into its block; only the uncompiled route
        // needs a mutable slot environment.
        if !complete {
            covered.extend(
                routes
                    .iter()
                    .map(|(i, _)| (seeds[*i].atom, seeds[*i].packet.row.witness)),
            );
        }
        let ready = routes.into_iter().map(|(i, cursor)| {
            let seed = &seeds[i];
            (
                seed.atom,
                seed.packet.row.clone(),
                move || Inputs::shared(seed.clone()),
                cursor,
            )
        });
        let result = self.finish_answers(g, rid, ready, covered, complete, watermark);
        self.stats.query_seconds += clock.elapsed().as_secs_f64();
        result
    }
    fn finish_answers<F: FnOnce() -> Inputs>(
        &mut self,
        g: &mut Graph,
        rid: usize,
        ready: impl ExactSizeIterator<Item = (usize, Rc<Row>, F, Cursor)>,
        covered: BTreeSet<(usize, Dependency)>,
        complete: bool,
        watermark: u64,
    ) -> Result<Option<Coverage>> {
        #[cfg(feature = "allocation-census")]
        let _allocation_scope = crate::allocation_census::scope(crate::allocation_census::Tag::Spine);
        if !complete && covered.is_empty() {
            return Ok(None);
        }
        let rule = g.program.rules[rid].clone();
        #[cfg(feature = "sparse-cells")]
        let mut results = std::mem::take(&mut self.answer_scratch);
        #[cfg(not(feature = "sparse-cells"))]
        let mut results = Winners::new();
        let covered_count = if complete { ready.len() } else { covered.len() };
        for (pivot, row, make_input, cursor) in ready {
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
                #[cfg(feature = "iterative-bake")]
                let selected = if g.program.iterative.enabled() {
                    cursor.state.collect_pairs = self.training.collect_pairs;
                    g.program.iterative.dispatch(
                        &mut cursor.state,
                        rid,
                        pivot,
                        complete,
                        row.witness,
                    )
                } else {
                    g.program.spine_blocks.get(rid, pivot)
                };
                #[cfg(not(feature = "iterative-bake"))]
                let selected = { g.program.spine_blocks.get(rid, pivot) };
                if let Some(block) = selected {
                    self.stats.block_entries += 1;
                    let port = if let Some(p) = &block.port {
                        let args = p.key.iter().map(|v| v.get(g, &row, None)).collect();
                        let Some(found) = read_port(
                            g,
                            &cursor,
                            p.table,
                            args,
                            &mut self.stats,
                            #[cfg(feature = "structural-depth")]
                            (&mut self.structural, row.witness),
                        )?
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
                        Some(found)
                    } else {
                        None
                    };
                    if epoch >= watermark {
                        let mut witnesses = vec![None; rule.query.len()];
                        witnesses[pivot] = Some(row.witness);
                        if let Some((p, r)) = block.port.as_ref().zip(port.as_ref()) {
                            witnesses[p.atom] = Some(r.witness);
                        }
                        let values = block
                            .inputs
                            .iter()
                            .map(|v| v.get(g, &row, port.as_ref()))
                            .collect();
                        cursor.via_block = true;
                        #[cfg(feature = "iterative-bake")]
                        if cursor.state.direct {
                            self.training.seal_moves += block.inputs.len() as u64;
                        }
                        record_answer(&mut results, values, witnesses, epoch, cursor);
                    }
                    continue;
                }
            }
            let mut input = make_input();
            let mut witnesses = vec![None; rule.query.len()];
            witnesses[pivot] = Some(row.witness);
            let mut valid = true;
            let order = &self.orders[&(rid, pivot)];
            for &i in order {
                let a = &rule.query[i];
                let args = a.args[..a.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, &input, g).ok_or_else(|| String::from("unbound stage recipe key")))
                    .collect::<Result<PortKey>>()?;
                match &a.call {
                    Call::Table(t) => {
                        let Some(row) = read_port(
                            g,
                            &cursor,
                            *t,
                            args,
                            &mut self.stats,
                            #[cfg(feature = "structural-depth")]
                            (&mut self.structural, row.witness),
                        )?
                        else {
                            valid = false;
                            break;
                        };
                        if !a
                            .args
                            .iter()
                            .zip(row.values.iter())
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
                        let value = value::primitive(name, &args, output, g.program.key_nodes())?;
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
                        input.get(*i).cloned().ok_or_else(|| String::from("incomplete stage recipe input"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                record_answer(&mut results, values, witnesses, epoch, cursor);
            }
        }
        #[cfg(feature = "sparse-cells")]
        {
            // Stable ordering retains the first producer when duplicate
            // bindings have the same depth, as in the ordered-map route.
            results.sort_by(|(ka, (_, ca)), (kb, (_, cb))| {
                ka.cmp(kb).then_with(|| cb.depth.cmp(&ca.depth))
            });
            results.dedup_by(|a, b| a.0 == b.0);
        }
        if complete {
            self.stats.query_takeovers += 1;
        } else {
            self.stats.partial_queries += 1;
        }
        self.stats.pivots_skipped += covered_count as u64;
        *self.stats.covered_by_rule.entry(rid).or_default() += covered_count as u64;
        self.stats.bindings += results.len() as u64;
        *self.stats.bindings_by_rule.entry(rid).or_default() += results.len() as u64;
        #[cfg(feature = "sparse-cells")]
        let rows = results.drain(..);
        #[cfg(not(feature = "sparse-cells"))]
        let rows = results.into_iter();
        let answers = rows
            .map(|(key, (answer, cursor))| {
                #[cfg(feature = "iterative-bake")]
                if cursor.state.direct || {
                    #[cfg(feature = "sparse-cells")]
                    { self.direct_sealed_answers }
                    #[cfg(not(feature = "sparse-cells"))]
                    { false }
                } {
                    // The executor appends residual matches after these winners.
                    // Stable sealing keeps the Spine binding on an equal RHS key.
                    // The public standalone Programs route still resolves by key.
                    self.stats.sealed_partial_origins += u64::from(!complete);
                    let mut answer = answer;
                    answer.values = key;
                    #[cfg(feature = "sparse-cells")]
                    {
                        answer.direct = Some(DirectOrigin(self.sealed_cursors.len()));
                        self.sealed_cursors.push(Some(cursor));
                    }
                    #[cfg(not(feature = "sparse-cells"))]
                    { answer.direct = Some(DirectOrigin(Box::new(cursor))); }
                    return answer;
                }
                if self.origins[rid].is_empty() {
                    self.origin_rules.push(rid);
                }
                #[cfg(feature = "sparse-cells")]
                let answer = {
                    let mut a = answer;
                    a.values = key.clone();
                    a
                };
                self.origins[rid].insert(key, cursor);
                answer
            })
            .collect();
        #[cfg(feature = "sparse-cells")]
        {
            self.answer_scratch = results;
        }
        Ok(Some(Coverage {
            complete,
            covered,
            answers,
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
                .zip(row.values.iter())
                .all(|(a, v)| match a {
                    Arg::Literal(l) => canon(l, &g.parents) == *v,
                    Arg::Slot(i) => match &input[*i] {
                        Some(old) => old == v,
                        None => {
                            self.stats.entry_parameters_copied += 1;

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
            .map(Published::root_info)
            .collect::<BTreeMap<_, _>>();
        let materialized = contexts
            .values()
            .filter_map(|root| root.and_then(|r| r.context.get()));
        let live_materialized = materialized.clone().count();
        let facts: usize = materialized.map(|c| c.borrow().facts.len()).sum();
        let report = json!({"enabled":self.enabled,"receipts_enabled":self.receipts,"partial_enabled":self.partial,"shared_entries_enabled":self.shared_entries,"stats":self.stats,"producer_rules":self.anchor_ops.iter().filter(|ops| !ops.is_empty()).count(),
            "keyed_entries":self.orders.len(),"live_spines":contexts.len(),"live_materialized_contexts":live_materialized,
            "entry_references":self.entries.len(),"context_fact_references":facts,
            "scope":"source-scheduled RHS continuation from committed producer contexts; exhaustive functional-key entries; unknown delta entries use ordinary matching; no early effect visibility"});
        #[cfg(feature = "iterative-bake")]
        let report = {
            let mut r = report;
            r["iterative_bake"] = self.training.report();
            r
        };
        report
    }
}
