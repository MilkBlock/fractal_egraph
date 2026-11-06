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
        Self {
            enabled: false,
            eligible,
            shapes: Default::default(),
            plans: Default::default(),
            leases: vec![],
            stats: Default::default(),
        }
    }
}
impl Graph {
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
