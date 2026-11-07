//! Checked positive postconditions. A port substitutes an exact constructor
//! key read, never an expansion or a negative result. Residual code keeps its
//! original order, ownership checks and source-snapshot semantics.
use crate::{
    graph::{Dependency, FactRef, Graph, Row},
    program::*,
    value::Value,
};
use rustc_hash::FxHashMap as Map;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Fact {
    pub op: usize,
    pub table: usize,
    pub fields: Vec<Arg>,
}
pub(crate) fn derive(p: &Program) -> Vec<Vec<Fact>> {
    p.templates
        .iter()
        .enumerate()
        .map(|(t, template)| {
            template
                .ops
                .iter()
                .enumerate()
                .filter_map(|(op, ins)| {
                    let (table, keys, value) = match ins {
                        Op::Let(dst, Call::Table(table), keys) => (*table, keys, Arg::Slot(*dst)),
                        Op::Set(table, keys, value) => (*table, keys, value.clone()),
                        _ => return None,
                    };
                    if !p.tables[table].constructor {
                        return None;
                    }
                    Some(Fact {
                        op,
                        table,
                        fields: keys
                            .iter()
                            .chain(std::iter::once(&value))
                            .map(|a| crate::graph::two_run::normalize(p, t, a.clone()))
                            .collect::<Option<Vec<_>>>()?,
                    })
                })
                .collect()
        })
        .collect()
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub(crate) enum Symbol {
    Slot(usize, usize),
    Literal(Value),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub(crate) enum ShapeKey {
    Root(usize),
    Chain(usize, usize),
}
#[derive(Serialize)]
pub(crate) struct Phase {
    pub template: usize,
    pub inputs: Vec<(usize, Symbol)>,
}
pub(crate) struct Shape {
    pub phases: Vec<Phase>,
}
impl Shape {
    fn arg(&self, p: &Program, phase: usize, a: &Arg) -> Option<Symbol> {
        let layer = &self.phases[phase];
        self.normalized_arg(
            phase,
            &crate::graph::two_run::normalize(p, layer.template, a.clone())?,
        )
    }
    fn normalized_arg(&self, phase: usize, a: &Arg) -> Option<Symbol> {
        let layer = &self.phases[phase];
        match a {
            Arg::Literal(v) => Some(Symbol::Literal(v.clone())),
            Arg::Slot(s) => Some(if phase == 0 {
                Symbol::Slot(0, *s)
            } else {
                layer
                    .inputs
                    .iter()
                    .find(|(i, _)| i == s)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(Symbol::Slot(phase, *s))
            }),
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
struct Port {
    phase: usize,
    op: usize,
}
struct PortShape {
    port: Port,
    table: usize,
    fields: Vec<Symbol>,
    result: Arg,
}
#[derive(Serialize)]
struct ReadPort {
    address: Port,
    result: Arg,
}
struct IndexedShape {
    shape: Shape,
    facts: Vec<PortShape>,
    by_port: Map<(usize, usize), usize>,
    by_key: Map<(usize, Vec<Symbol>), Vec<usize>>,
}
impl IndexedShape {
    fn new(p: &Program, shape: Shape) -> Self {
        let mut facts = vec![];
        let mut by_port = Map::default();
        let mut by_key = Map::<_, Vec<usize>>::default();
        for (phase, layer) in shape.phases.iter().enumerate() {
            for f in &p.block_postconditions[layer.template] {
                let Some(fields) = f
                    .fields
                    .iter()
                    .map(|a| shape.normalized_arg(phase, a))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                let i = facts.len();
                by_port.insert((phase, f.op), i);
                by_key
                    .entry((f.table, fields[..fields.len() - 1].to_vec()))
                    .or_default()
                    .push(i);
                facts.push(PortShape {
                    port: Port { phase, op: f.op },
                    table: f.table,
                    fields,
                    result: f.fields.last().unwrap().clone(),
                });
            }
        }
        Self {
            shape,
            facts,
            by_port,
            by_key,
        }
    }
}
pub(crate) struct Input {
    pub key: ShapeKey,
    pub ids: smallvec::SmallVec<[u32; 8]>,
    pub anchor_phase: usize,
}
#[derive(Serialize)]
struct Rhs {
    source: Port,
    plan: Rc<crate::graph::two_run::Plan>,
}
#[derive(Serialize)]
struct Plan {
    ports: Vec<Vec<ReadPort>>,
    residual: Vec<usize>,
    rhs: Option<Rhs>,
}
pub(crate) struct Context {
    input: Input,
    plan: Rc<Plan>,
}
#[derive(Clone)]
pub(crate) struct Lease {
    pub source: FactRef,
    pub plan: Rc<crate::graph::two_run::Plan>,
}
fn plan(
    p: &Program,
    indexed: &IndexedShape,
    anchor: Port,
    rid: usize,
    pivot: usize,
) -> Option<Plan> {
    let r = &p.rules[rid];
    let shape = &indexed.shape;
    let facts = &indexed.facts;
    let seed = &facts[*indexed.by_port.get(&(anchor.phase, anchor.op))?];
    if !matches!(r.query[pivot].call,Call::Table(t) if t==seed.table) {
        return None;
    }
    let mut symbols = BTreeMap::new();
    for (a, expr) in r.query[pivot].args.iter().zip(&seed.fields) {
        if let Arg::Slot(s) = a {
            symbols.entry(*s).or_insert(expr.clone());
        }
    }
    // Successful Unit guards return the unique Unit value. The guard itself
    // remains executable; this is conditional wiring, not a truth claim.
    for a in r.query.iter() {
        if let Call::Primitive(_, sort) = &a.call
            && sort == "Unit"
            && let Some(Arg::Slot(s)) = a.args.last()
        {
            symbols.entry(*s).or_insert(Symbol::Literal(Value::Int(0)));
        }
    }
    let mut ports = (0..r.query.len())
        .map(|_| vec![])
        .collect::<Vec<Vec<ReadPort>>>();
    loop {
        let mut changed = false;
        for (atom, a) in r.query.iter().enumerate() {
            if atom == pivot || !ports[atom].is_empty() {
                continue;
            }
            let Call::Table(table) = a.call else { continue };
            if !p.tables[table].constructor {
                continue;
            }
            let (out, keys) = a.args.split_last()?;
            let Some(keys) = keys
                .iter()
                .map(|a| match a {
                    Arg::Literal(v) => Some(Symbol::Literal(v.clone())),
                    Arg::Slot(s) => symbols.get(s).cloned(),
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let candidates = indexed
                .by_key
                .get(&(table, keys))
                .into_iter()
                .flatten()
                .take(4)
                .map(|&i| &facts[i])
                .collect::<Vec<_>>();
            if let Some(first) = candidates.first() {
                if let Arg::Slot(s) = out {
                    symbols.entry(*s).or_insert(first.fields.last()?.clone());
                }
                ports[atom] = candidates
                    .into_iter()
                    .map(|f| ReadPort {
                        address: f.port,
                        result: f.result.clone(),
                    })
                    .collect();
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    if ports.iter().all(Vec::is_empty) {
        return None;
    }
    let residual = (0..r.query.len())
        .filter(|&a| a != pivot && ports[a].is_empty())
        .collect();
    let rhs = (|| {
        let &[child_op] = p.port_frames.get(r.template)?.as_deref()? else {
            return None;
        };
        if !matches!(
            p.templates[r.template].ops[child_op as usize],
            Op::Let(_, Call::Table(_), _)
        ) {
            return None;
        }
        for phase in (0..shape.phases.len()).rev() {
            let template = shape.phases[phase].template;
            if p.port_frames.get(template)?.is_none() {
                continue;
            }
            let source = if phase == anchor.phase {
                anchor
            } else {
                let op = *p.port_frames[template].as_ref()?.first()? as usize;
                facts[*indexed.by_port.get(&(phase, op))?].port
            };
            let source_slots = (0..p.templates[template].sorts.len())
                .filter_map(|s| Some((s, shape.arg(p, phase, &Arg::Slot(s))?)))
                .collect::<Vec<_>>();
            let inputs = r
                .inputs
                .iter()
                .map(|s| {
                    let expr = symbols.get(s)?;
                    let a = match expr {
                        Symbol::Literal(v) => Arg::Literal(v.clone()),
                        _ => crate::graph::two_run::normalize(
                            p,
                            template,
                            Arg::Slot(source_slots.iter().find(|(_, v)| v == expr)?.0),
                        )?,
                    };
                    Some((*s, a))
                })
                .collect::<Option<Vec<_>>>();
            let Some(inputs) = inputs else { continue };
            let mut results = BTreeSet::new();
            let valid=inputs.iter().all(|(_,a)|{
                let Arg::Slot(s)=a else {return true};
                let Some(&op)=p.templates[template].defs.get(s) else {return true};
                if matches!(p.templates[template].ops[op],Op::Let(_,Call::Table(t),_) if p.tables[t].constructor) {results.insert(*s);true} else {false}
            });
            if valid {
                return Some(Rhs {
                    source,
                    plan: Rc::new(crate::graph::two_run::Plan {
                        producer_template: template,
                        producer_op: source.op,
                        consumer: rid,
                        pivot,
                        consumer_template: r.template,
                        child_op: child_op as usize,
                        inputs,
                        results: results.into_iter().collect(),
                    }),
                });
            }
        }
        None
    })();
    Some(Plan {
        ports,
        residual,
        rhs,
    })
}
#[derive(Default, Serialize)]
struct Stats {
    capture_plan_ns: u64,
    receipt_vectors: u64,
    receipt_vector_slots: u64,
    retained_receipt_frames: u64,
    captured_receipts: u64,
    alias_receipts: u64,
    missing_capture_receipts: u64,
    retired_receipts: u64,
    exact_receipt_attempts: u64,
    exact_receipt_hits: u64,
    shared_empty_key_hits: u64,
    exact_receipt_fallbacks: u64,
    capture_ns: u64,
    peak_receipts: usize,
    seeds: u64,
    cache_hits: u64,
    compiled_plans: u64,
    unsupported_shapes: u64,
    offered_atoms: u64,
    guard_reads: u64,
    provided_atoms: u64,
    misses: u64,
    invisible: u64,
    key_mismatch: u64,
    full_table_plans: u64,
    partial_plans: u64,
    rhs_leases: u64,
    lease_budget_fallbacks: u64,
    proof_ns: u64,
    compile_ns: u64,
    result_hints: u64,
    result_hint_hits: u64,
    imported_result_hints: u64,
    provided_by_phases: BTreeMap<usize, u64>,
    cross_phase_reads: u64,
}
pub(crate) struct State {
    pub enabled: bool,
    receipts_enabled: bool,
    capture_ops: Vec<Vec<usize>>,
    receipts: Map<u32, smallvec::SmallVec<[(u32, FactRef); 1]>>,
    live_receipts: usize,
    eligible: Vec<Vec<bool>>,
    shapes: Map<ShapeKey, Rc<IndexedShape>>,
    plans: Map<(ShapeKey, usize, usize, usize, usize), Option<Rc<Plan>>>,
    leases: Vec<Lease>,
    stats: Stats,
}
impl State {
    pub fn new(p: &Program) -> Self {
        let eligible=p.rules.iter().enumerate().map(|(rid,r)|(0..r.query.len()).map(|pivot|{
            let constructor=|i:usize|matches!(r.query[i].call,Call::Table(t) if p.tables[t].constructor);
            p.kernels.get(rid).and_then(|ks|ks.get(pivot)).and_then(Option::as_ref).is_some_and(|k|k.steps.iter().any(|s|matches!(s.access,crate::row_kernel::Access::Key) && constructor(s.atom)))
                || p.multi_stage.stage_entries.iter().any(|(rule,atom,order)|*rule==rid && *atom==pivot && order.iter().any(|&i|constructor(i)))
        }).collect()).collect();
        let mut tables = vec![false; p.tables.len()];
        for (rid, entries) in p.kernels.iter().enumerate() {
            for k in entries.iter().flatten() {
                for step in &k.steps {
                    if matches!(step.access, crate::row_kernel::Access::Key)
                        && let Call::Table(t) = p.rules[rid].query[step.atom].call
                    {
                        tables[t] = true;
                    }
                }
            }
        }
        for (rid, _, order) in &p.multi_stage.stage_entries {
            for &atom in order.iter() {
                if let Call::Table(t) = p.rules[*rid].query[atom].call {
                    tables[t] = true;
                }
            }
        }
        // A one-output frame cannot retain a reused output as its own anchor.
        // Multi-phase consumers read those outputs through their earlier owner.
        let capture_ops = p
            .block_postconditions
            .iter()
            .map(|facts| {
                if facts.len() < 2 {
                    vec![]
                } else {
                    facts
                        .iter()
                        // Nullary constructors already have an exact singleton
                        // receipt in the table directory. Do not copy it per Cell.
                        .filter(|f| tables[f.table] && f.fields.len() > 1)
                        .map(|f| f.op)
                        .collect()
                }
            })
            .collect();
        Self {
            enabled: false,
            receipts_enabled: false,
            capture_ops,
            receipts: Default::default(),
            live_receipts: 0,
            eligible,
            shapes: Default::default(),
            plans: Default::default(),
            leases: vec![],
            stats: Default::default(),
        }
    }
}
impl Graph {
    pub fn configure_block_receipts(&mut self, on: bool) {
        self.block_lhs.receipts_enabled = on;
        if on {
            self.block_lhs.enabled = true;
            let clock = std::time::Instant::now();
            // Capture only ports that a same-root consumer can actually use.
            // Shared-phase producers keep their conservative demand until the
            // concrete multi-phase layout is known; all other misses fall back.
            let p = &self.program;
            let mut readers = vec![vec![]; p.tables.len()];
            for (rid, r) in p.rules.iter().enumerate() {
                for pivot in 0..r.query.len() {
                    if !self.block_lhs.eligible[rid][pivot] || !r.query.iter().enumerate().any(|(i,a)|
                    i!=pivot && matches!(a.call,Call::Table(t) if p.tables[t].constructor && p.tables[t].schema.len()>1)) {continue}
                    if let Call::Table(t) = r.query[pivot].call {
                        readers[t].push((rid, pivot));
                    }
                }
            }
            let mut shared = vec![false; p.templates.len()];
            for edge in p.multi_run.iter().flatten() {
                shared[edge.producer_template] = true;
            }
            for t in 0..p.templates.len() {
                if self.block_lhs.capture_ops[t].is_empty() || shared[t] {
                    continue;
                }
                if !p.block_postconditions[t]
                    .iter()
                    .any(|f| !readers[f.table].is_empty())
                {
                    self.block_lhs.capture_ops[t].clear();
                    continue;
                }
                let shape = IndexedShape::new(
                    p,
                    Shape {
                        phases: vec![Phase {
                            template: t,
                            inputs: vec![],
                        }],
                    },
                );
                let mut needed = BTreeSet::new();
                for fact in &shape.facts {
                    for &(rid, pivot) in &readers[fact.table] {
                        if let Some(q) = plan(p, &shape, fact.port, rid, pivot) {
                            for port in q.ports.iter().flatten() {
                                if self.block_lhs.capture_ops[t]
                                    .binary_search(&port.address.op)
                                    .is_ok()
                                {
                                    needed.insert(port.address.op);
                                }
                            }
                        }
                    }
                }
                self.block_lhs.capture_ops[t] = needed.into_iter().collect();
            }
            self.block_lhs.stats.capture_plan_ns += clock.elapsed().as_nanos() as u64;
        }
    }
    pub(crate) fn block_capture_required(&self, template: usize) -> bool {
        self.block_lhs.receipts_enabled && !self.block_lhs.capture_ops[template].is_empty()
    }
    pub(crate) fn block_receipt_queries(&self, rid: usize) -> Vec<usize> {
        if !self.block_capture_required(self.program.rules[rid].template) {
            return vec![];
        }
        self.block_lhs.capture_ops[self.program.rules[rid].template]
            .iter()
            .filter_map(|&op| {
                match self
                    .program
                    .known_results
                    .static_sources
                    .get(rid)
                    .and_then(|s| s[op])
                {
                    Some(crate::known_results::Source::Query(i)) => Some(i),
                    _ => None,
                }
            })
            .collect()
    }
    pub(crate) fn note_block_receipt_vector(&mut self, slots: usize) {
        self.block_lhs.stats.receipt_vectors += 1;
        self.block_lhs.stats.receipt_vector_slots += slots as u64;
    }
    /// Receipts come from the original execution, including reuse and aliases.
    /// Commit metadata only if that execution left a physical block to query.
    pub(crate) fn seal_block_receipts(
        &mut self,
        cell: u32,
        rid: usize,
        prefix: usize,
        known: &crate::known_results::QueryFacts,
        effects: &[Option<Dependency>],
    ) {
        let template = self.program.rules[rid].template;
        if !self.block_capture_required(template) || self.frame_template(cell).is_none() {
            return;
        }
        let clock = std::time::Instant::now();
        let mut rows = smallvec::SmallVec::new();
        for &op in &self.block_lhs.capture_ops[template] {
            if op >= prefix {
                break;
            }
            let mut source = op;
            let mut witness = effects.get(source).copied().flatten();
            while witness.is_none() {
                match self
                    .program
                    .known_results
                    .static_sources
                    .get(rid)
                    .and_then(|s| s[source])
                {
                    Some(crate::known_results::Source::Query(q)) => {
                        witness = known.dependency(q);
                        break;
                    }
                    Some(crate::known_results::Source::Effect(prev)) if prev < source => {
                        source = prev;
                        witness = effects.get(source).copied().flatten();
                    }
                    _ => break,
                }
            }
            if let Some(Dependency::Fact(f)) = witness {
                if f != (FactRef {
                    cell,
                    op: op as u32,
                }) {
                    rows.push((op as u32, f));
                    self.block_lhs.stats.alias_receipts +=
                        u64::from(effects.get(op).copied().flatten().is_none());
                }
            } else {
                self.block_lhs.stats.missing_capture_receipts += 1;
            }
        }
        if !rows.is_empty() {
            self.block_lhs.stats.captured_receipts += rows.len() as u64;
            self.block_lhs.live_receipts += rows.len();
            self.block_lhs.stats.retained_receipt_frames += 1;
            if let Some(old) = self.block_lhs.receipts.insert(cell, rows) {
                self.block_lhs.live_receipts -= old.len();
            }
            self.block_lhs.stats.peak_receipts = self
                .block_lhs
                .stats
                .peak_receipts
                .max(self.block_lhs.live_receipts);
        }
        self.block_lhs.stats.capture_ns += clock.elapsed().as_nanos() as u64;
    }
    pub(crate) fn forget_block_receipts(&mut self, cell: u32) {
        if self.block_lhs.receipts_enabled
            && let Some(rows) = self.block_lhs.receipts.remove(&cell)
        {
            self.block_lhs.live_receipts -= rows.len();
            self.block_lhs.stats.retired_receipts += rows.len() as u64;
        }
    }
    pub fn configure_block_lhs(&mut self, on: bool) {
        self.block_lhs.enabled = on;
    }
    pub(crate) fn begin_block_match(&mut self) {
        self.block_lhs.leases.clear();
    }
    pub(crate) fn block_context(
        &mut self,
        rid: usize,
        pivot: usize,
        seed: &Row,
    ) -> Option<Context> {
        if !self.block_lhs.enabled || !self.block_lhs.eligible[rid][pivot] {
            return None;
        }
        let Dependency::Fact(f) = seed.witness else {
            return None;
        };
        self.block_lhs.stats.seeds += 1;
        let input = if let Some(input) = self.port_frames.query_input(f) {
            input
        } else {
            let c = self.rhs_frame(f.cell)?;
            Input {
                key: ShapeKey::Root(c.template),
                ids: smallvec::smallvec![f.cell],
                anchor_phase: 0,
            }
        };
        let key = (
            input.key.clone(),
            input.anchor_phase,
            f.op as usize,
            rid,
            pivot,
        );
        let planned = if let Some(cached) = self.block_lhs.plans.get(&key) {
            self.block_lhs.stats.cache_hits += 1;
            cached.clone()
        } else {
            let clock = std::time::Instant::now();
            let shape = if let Some(shape) = self.block_lhs.shapes.get(&input.key) {
                shape.clone()
            } else {
                let shape = match input.key {
                    ShapeKey::Root(template) => Shape {
                        phases: vec![Phase {
                            template,
                            inputs: vec![],
                        }],
                    },
                    ShapeKey::Chain(..) => self.port_frames.query_shape(&input)?,
                };
                let shape = Rc::new(IndexedShape::new(&self.program, shape));
                self.block_lhs
                    .shapes
                    .insert(input.key.clone(), shape.clone());
                shape
            };
            let derived = plan(
                &self.program,
                &shape,
                Port {
                    phase: input.anchor_phase,
                    op: f.op as usize,
                },
                rid,
                pivot,
            )
            .map(Rc::new);
            self.block_lhs.stats.compiled_plans += u64::from(derived.is_some());
            self.block_lhs.stats.unsupported_shapes += u64::from(derived.is_none());
            self.block_lhs.stats.compile_ns += clock.elapsed().as_nanos() as u64;
            self.block_lhs.plans.insert(key, derived.clone());
            derived
        }?;
        self.block_lhs.stats.offered_atoms +=
            planned.ports.iter().filter(|p| !p.is_empty()).count() as u64;
        let table_complete = planned
            .residual
            .iter()
            .all(|&a| !matches!(self.program.rules[rid].query[a].call, Call::Table(_)));
        if table_complete {
            self.block_lhs.stats.full_table_plans += 1;
        } else {
            self.block_lhs.stats.partial_plans += 1;
        }
        Some(Context {
            input,
            plan: planned,
        })
    }
    pub(crate) fn read_block_lhs(
        &mut self,
        ctx: &Context,
        atom: usize,
        table: usize,
        key: &[Value],
    ) -> Result<Option<Row>> {
        let ports = &ctx.plan.ports[atom];
        if ports.is_empty() {
            return Ok(None);
        }
        let clock = std::time::Instant::now();
        for port in ports {
            let f = FactRef {
                cell: ctx.input.ids[port.address.phase],
                op: port.address.op as u32,
            };
            if self.block_lhs.receipts_enabled
                && key.is_empty()
                && self.members[table].len() == 1
                && let Some(&word) = self.members[table].iter().next()
                && let Some(row) = self.fact(FactRef::from_ordered_id(word))?
                && !row.hidden
                && row.table == table
                && row.values.len() == 1
            {
                self.block_lhs.stats.shared_empty_key_hits += 1;
                self.block_lhs.stats.provided_atoms += 1;
                *self
                    .block_lhs
                    .stats
                    .provided_by_phases
                    .entry(ctx.input.ids.len())
                    .or_default() += 1;
                self.block_lhs.stats.cross_phase_reads +=
                    u64::from(port.address.phase != ctx.input.anchor_phase);
                self.block_lhs.stats.proof_ns += clock.elapsed().as_nanos() as u64;
                return Ok(Some(row));
            }
            if self.block_lhs.receipts_enabled
                && let Some(rows) = self.block_lhs.receipts.get(&f.cell)
                && let Ok(i) = rows.binary_search_by_key(&f.op, |x| x.0)
            {
                self.block_lhs.stats.exact_receipt_attempts += 1;
                let w = self.resolve_receipt(Dependency::Fact(rows[i].1));
                if let Some(row) = self.row(w)?
                    && !row.hidden
                    && row.table == table
                    && row.values[..row.values.len() - 1] == *key
                {
                    self.block_lhs.stats.exact_receipt_hits += 1;
                    self.block_lhs.stats.provided_atoms += 1;
                    *self
                        .block_lhs
                        .stats
                        .provided_by_phases
                        .entry(ctx.input.ids.len())
                        .or_default() += 1;
                    self.block_lhs.stats.cross_phase_reads +=
                        u64::from(port.address.phase != ctx.input.anchor_phase);
                    self.block_lhs.stats.proof_ns += clock.elapsed().as_nanos() as u64;
                    return Ok(Some(row));
                }
                self.block_lhs.stats.exact_receipt_fallbacks += 1;
            }
            self.block_lhs.stats.guard_reads += 1;
            let (actual, hint) = match self.fact_state(f) {
                Some(state) if !state.hidden => (f, false),
                Some(_) => {
                    self.block_lhs.stats.invisible += 1;
                    continue;
                }
                None => {
                    self.block_lhs.stats.invisible += 1;
                    self.block_lhs.stats.result_hints += 1;
                    // A constructor result often names its defining Cell/slot.
                    // Union can choose a different representative, so this is
                    // only a hint: table, liveness and the full key are checked.
                    let Ok(Value::Eq(id)) = self.argument(f.cell, &port.result) else {
                        continue;
                    };
                    let Some(slot) = (id as u32).checked_sub(1) else {
                        continue;
                    };
                    let cell = (id >> 32) as u32;
                    // Imported identities deliberately have no Cell address.
                    if cell == 0 {
                        self.block_lhs.stats.imported_result_hints += 1;
                        continue;
                    }
                    let Some(template) = self.frame_template(cell) else {
                        continue;
                    };
                    let Some(&op) = self.program.templates[template].defs.get(&(slot as usize))
                    else {
                        continue;
                    };
                    if !matches!(self.program.templates[template].ops[op],Op::Let(_,Call::Table(t),_) if t==table)
                    {
                        continue;
                    }
                    let target = FactRef {
                        cell,
                        op: op as u32,
                    };
                    self.block_lhs.stats.guard_reads += 1;
                    if self.fact_state(target).is_none_or(|s| s.hidden) {
                        continue;
                    }
                    (target, true)
                }
            };
            let Some(row) = self.fact(actual)? else {
                continue;
            };
            if row.hidden || row.table != table || row.values[..row.values.len() - 1] != *key {
                self.block_lhs.stats.key_mismatch += 1;
                continue;
            }
            self.block_lhs.stats.provided_atoms += 1;
            self.block_lhs.stats.result_hint_hits += u64::from(hint);
            *self
                .block_lhs
                .stats
                .provided_by_phases
                .entry(ctx.input.ids.len())
                .or_default() += 1;
            self.block_lhs.stats.cross_phase_reads +=
                u64::from(port.address.phase != ctx.input.anchor_phase);
            self.block_lhs.stats.proof_ns += clock.elapsed().as_nanos() as u64;
            return Ok(Some(row));
        }
        self.block_lhs.stats.misses += 1;
        self.block_lhs.stats.proof_ns += clock.elapsed().as_nanos() as u64;
        Ok(None)
    }
    pub(crate) fn lease_block_rhs(&mut self, ctx: Option<&Context>) -> u32 {
        let Some(ctx) = ctx else { return 0 };
        let Some(rhs) = &ctx.plan.rhs else { return 0 };
        let source = FactRef {
            cell: ctx.input.ids[rhs.source.phase],
            op: rhs.source.op as u32,
        };
        if self.port_frames.template(source.cell).is_none()
            || self.fact_state(source).is_none_or(|s| s.hidden)
        {
            return 0;
        }
        if self.block_lhs.leases.len() >= 65_536 {
            self.block_lhs.stats.lease_budget_fallbacks += 1;
            return 0;
        }
        self.block_lhs.leases.push(Lease {
            source,
            plan: rhs.plan.clone(),
        });
        self.block_lhs.stats.rhs_leases += 1;
        self.block_lhs.leases.len() as u32
    }
    pub(crate) fn block_rhs_lease(&self, id: u32) -> Option<Lease> {
        self.block_lhs
            .leases
            .get(id.checked_sub(1)? as usize)
            .cloned()
    }
    pub fn block_lhs_report(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.block_lhs.enabled,"stats":self.block_lhs.stats,
            "receipts_enabled":self.block_lhs.receipts_enabled,"live_receipt_frames":self.block_lhs.receipts.len(),
            "live_receipts":self.block_lhs.live_receipts,
            "receipt_payload_capacity_bytes":self.block_lhs.receipts.values().map(|v|v.capacity()*std::mem::size_of::<(u32,FactRef)>()).sum::<usize>(),
            "shape_count":self.block_lhs.shapes.len(),"cached_dispatches":self.block_lhs.plans.len(),
            "lease_capacity_bytes":self.block_lhs.leases.capacity()*std::mem::size_of::<Lease>(),
            "scope":"guarded constructor postconditions; only complete-key reads; expansions and residual guards unchanged"})
    }
    pub fn block_lhs_plans(&self) -> serde_json::Value {
        let mut entries=self.block_lhs.plans.iter().filter_map(|((shape,phase,op,rule,pivot),plan)|{
            Some(serde_json::json!({"shape":shape,"anchor_phase":phase,"anchor_op":op,"consumer":rule,"pivot":pivot,"plan":plan.as_ref()?}))
        }).collect::<Vec<_>>();
        entries.sort_by_key(|v| crate::checksum::canonical(v, false));
        serde_json::json!({"schema":"eggfuse-block-lhs-plans/v1","entries":entries})
    }
}
