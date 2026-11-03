//! Source-scheduled Cell programs. A committed producer supplies the next
//! stage's complete context; the stage consumes it without a structural join.
//! Coverage is proved from functional table keys, not from observed frequency.
use crate::{
    graph::{Dependency, Graph},
    program::*,
    value::{self, Value, canon},
};
use serde::Serialize;
use serde_json::{Value as J, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
    time::Instant,
};

#[derive(Clone)]
pub struct Cursor {
    context: Rc<RefCell<Context>>,
    pub depth: u32,
    pub producer: usize,
}
struct Context {
    id: u64,
    sets: BTreeSet<String>,
    last_epoch: u64,
    visits: u64,
    unions: u64,
    facts: BTreeMap<(usize, Vec<Value>), Dependency>,
}
pub struct Answer {
    pub values: Vec<Value>,
    pub witnesses: Vec<Option<Dependency>>,
    pub epoch: u64,
}
#[derive(Default, Serialize)]
pub struct Stats {
    contexts_created: u64,
    captured_contexts: u64,
    capture_reads: u64,
    anchor_publications: u64,
    missing_capture_rows: u64,
    anchors_retired: u64,
    query_attempts: u64,
    query_takeovers: u64,
    uncovered_queries: u64,
    pivots_skipped: u64,
    delta_rows_examined: u64,
    context_reads: u64,
    context_fact_publications: u64,
    completed_steps: u64,
    port_refreshes: u64,
    guards: u64,
    guard_rejections: u64,
    bindings: u64,
    steps_executed: u64,
    resumed_steps: u64,
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
}
pub struct Programs {
    pub enabled: bool,
    producers: BTreeSet<usize>,
    anchor_tables: BTreeSet<usize>,
    consumer_tables: BTreeMap<usize, BTreeSet<usize>>,
    orders: BTreeMap<(usize, usize), Vec<usize>>,
    entries: HashMap<Dependency, Cursor>,
    active_tables: BTreeMap<usize, usize>,
    contexts_pending: Vec<(Cursor, Vec<(usize, Vec<Value>, bool)>)>,
    origins: BTreeMap<(usize, Vec<Value>), Cursor>,
    next_context: u64,

    pub stats: Stats,
}
fn bind(a: &Arg, v: &Value, input: &mut [Option<Value>], g: &Graph) -> bool {
    match a {
        Arg::Literal(x) => canon(x, &g.parents) == *v,
        Arg::Slot(i) => match &input[*i] {
            Some(x) => x == v,
            None => {
                input[*i] = Some(v.clone());
                true
            }
        },
    }
}
fn operand(a: &Arg, input: &[Option<Value>], g: &Graph) -> Option<Value> {
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
            producers: BTreeSet::new(),
            anchor_tables: BTreeSet::new(),
            consumer_tables: BTreeMap::new(),
            orders: BTreeMap::new(),
            entries: HashMap::new(),
            active_tables: BTreeMap::new(),
            contexts_pending: vec![],
            origins: BTreeMap::new(),
            next_context: 0,

            stats: Stats::default(),
        };
        if !enabled {
            return s;
        }
        for (rid, pivot, order) in &p.multi_stage.stage_entries {
            s.orders.insert((*rid, *pivot), order.as_ref().clone());
            let Call::Table(t) = p.rules[*rid].query[*pivot].call else {
                unreachable!()
            };
            s.anchor_tables.insert(t);
            s.consumer_tables.entry(*rid).or_default().insert(t);
        }
        // Store one cursor per committed anchor. Consumer programs attach only
        // on their own Run, rather than preparing the entire fanout at publish.
        for (rid, r) in p.rules.iter().enumerate() {
            if p.templates[r.template]
                .ops
                .iter()
                .enumerate()
                .any(|(op, _)| {
                    crate::multistage::source(p, rid, "effect", op)
                        .is_ok_and(|(t, _)| s.anchor_tables.contains(&t))
                })
            {
                s.producers.insert(rid);
            }
        }
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
        if previous.is_none() && !self.producers.contains(&rid) {
            return None;
        }
        let cursor = if let Some(mut cursor) = previous {
            self.stats.resumed_steps += 1;
            *self.stats.resumed_by_set.entry(set.into()).or_default() += 1;
            self.stats.cross_stage_steps += u64::from(!cursor.context.borrow().sets.contains(set));
            cursor.depth = cursor
                .depth
                .checked_add(1)
                .expect("stage recipe depth exhausted");
            cursor.producer = rid;
            cursor
        } else {
            let id = self.next_context;
            self.next_context += 1;
            self.stats.contexts_created += 1;
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
            Cursor {
                context: Rc::new(RefCell::new(Context {
                    id,
                    sets: BTreeSet::new(),
                    last_epoch: epoch,
                    visits: 1,
                    unions: 0,
                    facts: BTreeMap::new(),
                })),
                depth: 1,
                producer: rid,
            }
        };
        let mut c = cursor.context.borrow_mut();
        c.sets.insert(set.into());
        if c.last_epoch != epoch {
            c.visits += 1;
            c.last_epoch = epoch;
        }
        self.stats.max_chain = self.stats.max_chain.max(cursor.depth);
        self.stats.max_sets_per_spine = self.stats.max_sets_per_spine.max(c.sets.len());
        self.stats.max_stage_visits = self.stats.max_stage_visits.max(c.visits);
        self.stats.steps_executed += 1;
        *self.stats.by_set.entry(set.into()).or_default() += 1;
        drop(c);
        Some(cursor)
    }
    pub fn capture(&mut self, g: &Graph, rid: usize, cell: u32, cursor: Cursor) -> Result<()> {
        self.stats.completed_steps += 1;
        if !self.producers.contains(&rid) {
            return Ok(());
        }
        let clock = Instant::now();
        let rule = &g.program.rules[rid];
        let template = &g.program.templates[rule.template];
        let mut facts = vec![];
        for atom in rule.query.iter() {
            if let Call::Table(t) = atom.call {
                facts.push((t, g.arguments(cell, &atom.args)?, false));
            }
        }
        for op in template.ops.iter() {
            match op {
                Op::Let(dst, Call::Table(t), args) => {
                    let mut row = g.arguments(cell, args)?;
                    row.push(g.argument(cell, &Arg::Slot(*dst))?);
                    facts.push((*t, row, self.anchor_tables.contains(t)));
                }
                Op::Set(t, args, v) => {
                    let mut row = g.arguments(cell, args)?;
                    row.push(g.argument(cell, v)?);
                    facts.push((*t, row, self.anchor_tables.contains(t)));
                }
                _ => (),
            }
        }
        self.contexts_pending.push((cursor.clone(), facts));
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
        let mut resolved: BTreeMap<(usize, Vec<Value>), Option<Dependency>> = BTreeMap::new();
        for (cursor, rows) in std::mem::take(&mut self.contexts_pending) {
            let mut context = cursor.context.borrow_mut();
            context.canonicalize(g);
            for (t, values, anchor) in rows {
                let key = values[..values.len() - 1]
                    .iter()
                    .map(|v| canon(v, &g.parents))
                    .collect::<Vec<_>>();
                let cache_key = (t, key);
                let w = if let Some(w) = resolved.get(&cache_key) {
                    *w
                } else {
                    self.stats.capture_reads += 1;
                    let w = g
                        .lookup(t, &cache_key.1)?
                        .filter(|r| !r.hidden)
                        .map(|r| r.witness);
                    resolved.insert(cache_key.clone(), w);
                    w
                };
                if let Some(w) = w {
                    if anchor {
                        if self.entries.insert(w, cursor.clone()).is_none() {
                            *self.active_tables.entry(t).or_default() += 1;
                        }
                        self.stats.anchor_publications += 1;
                    }
                    context.facts.insert(cache_key, w);
                    self.stats.context_fact_publications += 1;
                } else {
                    if anchor {
                        self.stats.missing_capture_rows += 1;
                    }
                    context.facts.remove(&cache_key);
                }
            }
        }
        self.origins.clear();
        self.stats.publication_seconds += clock.elapsed().as_secs_f64();
        Ok(())
    }
    pub fn answers(
        &mut self,
        g: &mut Graph,
        rid: usize,
        pending: &BTreeSet<Dependency>,
        watermark: u64,
    ) -> Result<Option<Vec<Answer>>> {
        if !self.enabled
            || !self
                .consumer_tables
                .get(&rid)
                .is_some_and(|tables| tables.iter().any(|t| self.active_tables.contains_key(t)))
        {
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
    ) -> Result<Option<Vec<Answer>>> {
        self.stats.query_attempts += 1;
        let rule = g.program.rules[rid].clone();
        let mut ready = vec![];
        let mut skips = 0;
        for &w in pending {
            self.stats.delta_rows_examined += 1;
            g.stats.decoded_rows += 1;
            let Some(row) = g.row(w)? else { continue };
            if row.hidden || row.epoch < watermark {
                continue;
            }
            for (pivot, a) in rule.query.iter().enumerate() {
                if !matches!(a.call,Call::Table(t) if t==row.table) {
                    continue;
                }
                let mut input = vec![None; g.program.templates[rule.template].sorts.len()];
                if !a
                    .args
                    .iter()
                    .zip(&row.values)
                    .all(|(a, v)| bind(a, v, &mut input, g))
                {
                    continue;
                }
                let Some(cursor) = self
                    .entries
                    .get(&w)
                    .filter(|_| self.orders.contains_key(&(rid, pivot)))
                    .cloned()
                else {
                    self.stats.uncovered_queries += 1;
                    return Ok(None);
                };
                ready.push((pivot, row.clone(), input, cursor));
                skips += 1;
            }
        }
        let mut results: BTreeMap<Vec<Value>, (Answer, Cursor)> = BTreeMap::new();
        for (pivot, row, mut input, cursor) in ready {
            let mut witnesses = vec![None; rule.query.len()];
            witnesses[pivot] = Some(row.witness);
            let mut epoch = row.epoch;
            let mut valid = true;
            for &i in &self.orders[&(rid, pivot)] {
                let a = &rule.query[i];
                let args = a.args[..a.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, &input, g).ok_or("unbound stage recipe key".into()))
                    .collect::<Result<Vec<_>>>()?;
                match &a.call {
                    Call::Table(t) => {
                        let good = |r: &crate::graph::Row| {
                            r.table == *t && !r.hidden && r.values[..args.len()] == args
                        };
                        let witness = {
                            let mut context = cursor.context.borrow_mut();
                            context.canonicalize(g);
                            context.facts.get(&(*t, args.clone())).copied()
                        };
                        let mut row = if let Some(w) = witness {
                            self.stats.context_reads += 1;
                            g.stats.decoded_rows += 1;
                            g.row(w)?.filter(good)
                        } else {
                            None
                        };
                        if row.is_none() {
                            self.stats.port_refreshes += 1;
                            row = g.lookup(*t, &args)?;
                        }
                        if let Some(r) = row.as_ref().filter(|r| !r.hidden) {
                            cursor
                                .context
                                .borrow_mut()
                                .facts
                                .insert((*t, args.clone()), r.witness);
                        } else {
                            cursor
                                .context
                                .borrow_mut()
                                .facts
                                .remove(&(*t, args.clone()));
                        }
                        let Some(row) = row.filter(|r| !r.hidden) else {
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
                        input[*i]
                            .clone()
                            .ok_or("incomplete stage recipe input".into())
                    })
                    .collect::<Result<Vec<_>>>()?;
                let candidate = (
                    Answer {
                        values: values.clone(),
                        witnesses,
                        epoch,
                    },
                    cursor.clone(),
                );
                match results.entry(values) {
                    std::collections::btree_map::Entry::Vacant(e) => {
                        e.insert(candidate);
                    }
                    std::collections::btree_map::Entry::Occupied(mut e) => {
                        if cursor.depth > e.get().1.depth {
                            e.insert(candidate);
                        }
                    }
                }
            }
        }
        self.stats.query_takeovers += 1;
        self.stats.pivots_skipped += skips;
        self.stats.bindings += results.len() as u64;
        *self.stats.bindings_by_rule.entry(rid).or_default() += results.len() as u64;
        Ok(Some(
            results
                .into_iter()
                .map(|(key, (answer, cursor))| {
                    self.origins.insert((rid, key), cursor);
                    answer
                })
                .collect(),
        ))
    }
    pub fn effects(&mut self, constructors: u64, updates: u64, unions: u64) {
        self.stats.constructor_requests += constructors;
        self.stats.function_updates += updates;
        self.stats.union_requests += unions;
    }
    pub fn report(&self) -> J {
        let contexts = self
            .entries
            .values()
            .map(|c| (c.context.borrow().id, c.context.clone()))
            .collect::<BTreeMap<_, _>>();
        let facts: usize = contexts.values().map(|c| c.borrow().facts.len()).sum();
        json!({"enabled":self.enabled,"stats":self.stats,"producer_rules":self.producers.len(),
            "keyed_entries":self.orders.len(),"live_spines":contexts.len(),
            "entry_references":self.entries.len(),"context_fact_references":facts,
            "scope":"source-scheduled RHS continuation from committed producer contexts; exhaustive functional-key entries; unknown delta entries use ordinary matching; no early effect visibility"})
    }
}
