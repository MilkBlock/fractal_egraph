//! Bounded port completion on the original single-thread Run schedule.
use crate::{
    depth_plan::{BUDGET, MAX_DEPTH, Path, Plan, Source},
    graph::{Dependency, FactRef, Graph, Row},
    known_results::{QueryFacts, Source as Alias},
    program::*,
    result_handoff::Input,
    slots::Slots,
    value::{self, Value, canon},
};
use rustc_hash::FxHashMap as Map;
#[path = "depth_focus.rs"]
mod focus;
use serde::Serialize;
use smallvec::SmallVec;
use std::{collections::BTreeMap, rc::Rc, time::Instant};

#[derive(Clone, Copy, Debug)]
struct Port {
    w: Dependency,
    t: usize,
    path: Path,
}
#[derive(Default)]
struct Packet {
    ports: SmallVec<[Port; 4]>,
}
#[derive(Default, Serialize)]
pub(crate) struct Stats {
    bindings: [u64; 2],
    owner_known: [u64; 2],
    eligible_lhs: [u64; 2],
    eligible_rhs: [u64; 2],
    lhs_ports: [[u64; 4]; 2],
    lhs_full: [[u64; 4]; 2],
    rhs_ports: [[u64; 4]; 2],
    rhs_full: [[u64; 4]; 2],
    candidates: u64,
    full_lhs_emitted: [u64; 4],
    retained_lhs: [u64; 4],
    spine_reads: [u64; 4],
    rhs_accepted: [u64; 4],
    closed_rhs: [u64; 4],
    closed_by_lane: [[u64; 4]; 2],
    published: u64,
    captured: u64,
    stale_exports: u64,
    peak_anchors: usize,
    peak_refs: usize,
    first_exports: [u64; 4],
    saturated_packets: u64,
    missing_query_receipts: u64,
    oracle_owner_reads: u64,
    capture_ns: u64,
    publish_ns: u64,
}
#[derive(Default, Clone)]
pub(crate) struct Evidence {
    pub owner: Option<(usize, Dependency)>,
    lhs: SmallVec<[(usize, Path); 4]>,
    full: u8,
    eligible: bool,
    known: bool,
}
pub(crate) struct Solution {
    pub values: Slots,
    pub witnesses: Vec<Option<Dependency>>,
    pub epoch: u64,
    pub depth: u8,
}
#[derive(Default)]
struct RunRhs {
    ports: Vec<(usize, Path)>,
    inputs: Vec<Input>,
    complete: Option<u8>,
}
pub(crate) struct Depth {
    pub depth: u8,
    pub survey: bool,
    pub execute: bool,
    profile: bool,
    focus: Option<focus::Focus>,
    plan: Rc<Plan>,
    live: Map<Dependency, Packet>,
    pending: Map<Dependency, (usize, u64, Packet)>,
    serial: u64,
    live_refs: usize,
    stats: Stats,
    shapes: BTreeMap<(u8, u8, usize, usize, Path), u64>,
    by_rule: BTreeMap<(usize, u8, usize, u8), [u64; 4]>,
}
impl Depth {
    pub fn new(p: &Program) -> Self {
        Self {
            depth: 0,
            survey: false,
            execute: false,
            profile: false,
            focus: None,
            plan: p.depth_plan.clone(),
            live: Default::default(),
            pending: Default::default(),
            serial: 0,
            live_refs: 0,
            stats: Default::default(),
            shapes: Default::default(),
            by_rule: Default::default(),
        }
    }
    pub fn configure(&mut self, depth: u8, survey: bool) -> Result<()> {
        if depth > MAX_DEPTH as u8 {
            return Err("structural depth must be 0..3".into());
        }
        self.focus = None;
        self.depth = depth;
        self.survey = survey;
        self.execute = !survey && depth > 0;
        self.profile = std::env::var_os("CELL_CPU_PROFILE").is_some();
        Ok(())
    }
    pub fn specialize(&mut self, p: &Program, c: serde_json::Value) -> Result<()> {
        if !self.execute {
            return Err("a structural catalog needs positive depth in execution mode".into());
        }
        let catalog = serde_json::from_value(c).map_err(|e| e.to_string())?;
        self.focus = Some(focus::Focus::compile(p, catalog, self.depth)?);
        Ok(())
    }
    pub fn measure_bindings(&self) -> bool {
        self.depth > 0 && self.focus.is_none()
    }
    pub fn capture_rule(&self, r: usize) -> bool {
        if let Some(focus) = &self.focus {
            return focus.has_rule(r);
        }
        self.depth > 0 && !self.plan.recipes[r].outputs.is_empty()
    }
    fn source(
        g: &Graph,
        r: usize,
        s: Source,
        known: &QueryFacts,
        effect: &[Option<Dependency>],
    ) -> Option<Dependency> {
        match s {
            Source::Query(i) => known.tracked(i),
            Source::Effect(i) => effect.get(i).copied().flatten().or_else(|| {
                match g
                    .program
                    .known_results
                    .static_sources
                    .get(r)?
                    .get(i)
                    .copied()
                    .flatten()?
                {
                    Alias::Query(a) => known.tracked(a),
                    Alias::Effect(a) => effect.get(a).copied().flatten(),
                }
            }),
        }
    }
    pub fn capture(
        &mut self,
        g: &Graph,
        r: usize,
        known: &QueryFacts,
        effect: &[Option<Dependency>],
    ) {
        if !self.capture_rule(r) {
            return;
        }
        let clock = self.profile.then(Instant::now);
        if let Some(focus) = &self.focus {
            focus.capture(g, r, known, effect, &self.live, |anchor, table, packet| {
                self.serial += 1;
                self.stats.captured += 1;
                self.stats.saturated_packets += u64::from(packet.ports.len() == BUDGET);
                self.pending.insert(anchor, (table, self.serial, packet));
            });
            if let Some(t) = clock {
                self.stats.capture_ns += t.elapsed().as_nanos() as u64;
            }
            return;
        }
        for o in &self.plan.recipes[r].outputs {
            let Some(anchor) = Self::source(g, r, Source::Effect(o.op), known, effect) else {
                continue;
            };
            let mut packet = Packet::default();
            'depth: for depth in 1..=self.depth {
                for d in &o.donors {
                    if d.path.len > depth {
                        break;
                    }
                    let Some(w) = Self::source(g, r, d.src, known, effect) else {
                        continue;
                    };
                    if !d
                        .via
                        .iter()
                        .all(|s| Self::source(g, r, *s, known, effect).is_some())
                    {
                        continue;
                    }
                    let mut offer = |p: Port| {
                        if p.w != anchor && !packet.ports.iter().any(|x| x.w == p.w) {
                            packet.ports.push(p);
                        }
                        packet.ports.len() == BUDGET
                    };
                    if d.path.len == depth {
                        if offer(Port {
                            w,
                            t: d.table,
                            path: d.path,
                        }) {
                            break 'depth;
                        }
                    } else if let Some(prev) = self.live.get(&w) {
                        for port in &prev.ports {
                            if d.path.len + port.path.len != depth {
                                continue;
                            }
                            if let Some(path) = d.path.then(port.path)
                                && offer(Port {
                                    w: port.w,
                                    t: port.t,
                                    path,
                                })
                            {
                                break 'depth;
                            }
                        }
                    }
                }
            }
            self.serial += 1;
            self.stats.captured += 1;
            self.stats.saturated_packets += u64::from(packet.ports.len() == BUDGET);
            self.pending.insert(anchor, (o.table, self.serial, packet));
        }
        if let Some(t) = clock {
            self.stats.capture_ns += t.elapsed().as_nanos() as u64;
        }
    }
    pub fn publish(&mut self, g: &Graph) {
        if self.depth == 0 {
            return;
        }
        let clock = self.profile.then(Instant::now);
        for (t, changes) in g.changes.tables() {
            for w in changes.keys() {
                if !g.receipt_visible(*w, t)
                    && let Some(old) = self.live.remove(w)
                {
                    self.live_refs -= old.ports.len();
                }
            }
        }
        // Same-key forwarding can bring previously distinct publications together.
        // Choose the last actual RHS publication at the repaired witness.
        let mut winners = Map::default();
        for (raw, (t, ordinal, mut p)) in self.pending.drain() {
            let w = g.resolve_receipt(raw);
            if !g.receipt_visible(w, t) {
                self.stats.stale_exports += 1;
                continue;
            }
            for port in &mut p.ports {
                port.w = g.resolve_receipt(port.w);
            }
            p.ports.retain(|a| a.w != w && g.receipt_visible(a.w, a.t));
            let choice = winners.entry(w).or_insert((0, Packet::default()));
            if ordinal > choice.0 {
                *choice = (ordinal, p);
            }
        }
        for (w, (_, p)) in winners {
            if let Some(old) = self.live.remove(&w) {
                self.live_refs -= old.ports.len();
            }
            if !p.ports.is_empty() {
                self.live_refs += p.ports.len();
                for x in &p.ports {
                    self.stats.first_exports[x.path.len as usize] += 1;
                }
                self.live.insert(w, p);
                self.stats.published += 1;
            }
        }
        self.stats.peak_refs = self.stats.peak_refs.max(self.live_refs);
        self.stats.peak_anchors = self.stats.peak_anchors.max(self.live.len());
        if let Some(t) = clock {
            self.stats.publish_ns += t.elapsed().as_nanos() as u64;
        }
    }
    fn supplied(
        &self,
        g: &Graph,
        anchor: Dependency,
        t: usize,
        key: &[Value],
    ) -> Result<Option<(Row, Path)>> {
        let allowed = if let Some(f) = &self.focus {
            let Some(paths) = g.live_table(anchor).and_then(|from| f.ports(from, t)) else {
                return Ok(None);
            };
            Some(paths)
        } else {
            None
        };
        if let Some(p) = self.live.get(&anchor) {
            for donor in p
                .ports
                .iter()
                .filter(|p| p.t == t && allowed.is_none_or(|set| set.contains(&p.path)))
            {
                if let Some(row) = g.row(donor.w)?
                    && !row.hidden
                    && row.table == t
                    && row.values.split_last().is_some_and(|(_, k)| k == key)
                {
                    return Ok(Some((row, donor.path)));
                }
            }
        }
        Ok(None)
    }
    pub fn spine_read(
        &mut self,
        g: &Graph,
        anchor: Dependency,
        t: usize,
        key: &[Value],
    ) -> Result<Option<Row>> {
        if !self.execute {
            return Ok(None);
        }
        let row = self.supplied(g, anchor, t, key)?;
        if let Some((_, path)) = &row {
            self.stats.spine_reads[path.len as usize] += 1;
        }
        Ok(row.map(|x| x.0))
    }
    fn lhs(
        &self,
        g: &Graph,
        rid: usize,
        pivot: usize,
        anchor: &Row,
        ordinary: bool,
    ) -> Result<(Evidence, Option<Solution>)> {
        let mut evidence = Evidence::default();
        if !self.live.contains_key(&anchor.witness) {
            return Ok((evidence, None));
        }
        let r = &g.program.rules[rid];
        let order = &self.plan.recipes[rid].orders[pivot];
        if !order
            .iter()
            .any(|&i| matches!(r.query[i].call, Call::Table(_)))
        {
            return Ok((evidence, None));
        }
        let mut slots = Slots::default();
        let mut witnesses = vec![None; r.query.len()];
        let mut depths = vec![0; g.program.templates[r.template].sorts.len()];
        fn bind(
            a: &Arg,
            v: &Value,
            depth: u8,
            slots: &mut Slots,
            depths: &mut [u8],
            g: &Graph,
        ) -> bool {
            match a {
                Arg::Literal(k) => canon(k, &g.parents) == *v,
                Arg::Slot(i) => {
                    if let Some(old) = slots.get(i) {
                        old == v
                    } else {
                        slots.insert(*i, v.clone());
                        depths[*i] = depth;
                        true
                    }
                }
            }
        }
        if !r.query[pivot]
            .args
            .iter()
            .zip(anchor.values.iter())
            .all(|(a, v)| bind(a, v, 0, &mut slots, &mut depths, g))
        {
            return Ok((evidence, None));
        }
        witnesses[pivot] = Some(anchor.witness);
        let mut complete = true;
        let mut depth = 0;
        let mut epoch = anchor.epoch;
        for &i in order {
            let atom = &r.query[i];
            let (last, aa) = atom.args.split_last().unwrap();
            let Some(key) = aa
                .iter()
                .map(|a| operand(a, &slots, g))
                .collect::<Option<SmallVec<[Value; 4]>>>()
            else {
                complete = false;
                continue;
            };
            let input_depth = aa
                .iter()
                .filter_map(|a| {
                    if let Arg::Slot(s) = a {
                        Some(depths[*s])
                    } else {
                        None
                    }
                })
                .max()
                .unwrap_or(0);
            let (value, h) = match &atom.call {
                Call::Table(t) => {
                    let Some((row, path)) = self.supplied(g, anchor.witness, *t, &key)? else {
                        complete = false;
                        continue;
                    };
                    if ordinary
                        && ((row.epoch, row.witness) > (anchor.epoch, anchor.witness)
                            || ((row.epoch, row.witness) == (anchor.epoch, anchor.witness)
                                && i < pivot))
                    {
                        complete = false;
                        continue;
                    }
                    let value = row.values.last().unwrap().clone();
                    if let Some(v) = operand(last, &slots, g)
                        && v != value
                    {
                        complete = false;
                        continue;
                    }
                    witnesses[i] = Some(row.witness);
                    epoch = epoch.max(row.epoch);
                    evidence.lhs.push((i, path));
                    (value, input_depth.max(path.len))
                }
                Call::Primitive(name, ty) => {
                    // An optional route can visit guards in a different order.
                    // Defer a partial primitive to the original plan.
                    let Ok(Some(v)) = value::primitive(
                        name,
                        &key,
                        ty,
                        &g.program.artifact["source_core"]["structural_key"]["nodes"],
                    ) else {
                        complete = false;
                        continue;
                    };
                    (v, input_depth)
                }
            };
            if !bind(last, &value, h, &mut slots, &mut depths, g) {
                complete = false;
            }
            depth = depth.max(h);
        }
        if !complete
            || order.len() + 1 != r.query.len()
            || evidence.lhs.is_empty()
            || r.inputs.iter().any(|s| !slots.contains_key(s))
        {
            return Ok((evidence, None));
        }
        evidence.full = depth;
        Ok((
            evidence,
            Some(Solution {
                values: slots,
                witnesses,
                epoch,
                depth,
            }),
        ))
    }
    pub fn try_entry(
        &mut self,
        g: &Graph,
        r: usize,
        pivot: usize,
        row: &Row,
    ) -> Result<Option<Solution>> {
        if !self.execute
            || self.focus.is_some()
            || self.plan.recipes[r].orders[pivot].len() + 1 != g.program.rules[r].query.len()
        {
            return Ok(None);
        }
        self.stats.candidates += 1;
        let sol = self.lhs(g, r, pivot, row, true)?.1;
        if let Some(s) = &sol {
            self.stats.full_lhs_emitted[s.depth as usize] += 1
        }
        Ok(sol)
    }
    pub fn prepare(
        &mut self,
        g: &Graph,
        rid: usize,
        known: &QueryFacts,
        values: &Slots,
        wm: u64,
    ) -> Result<Evidence> {
        if self.depth == 0 {
            return Ok(Evidence::default());
        }
        let r = &g.program.rules[rid];
        let mut owner = None;
        let mut exact = true;
        for (i, a) in r.query.iter().enumerate() {
            let Call::Table(t) = a.call else { continue };
            let mut w = known.tracked(i);
            if w.is_none() && self.survey {
                self.stats.missing_query_receipts += 1;
                if let Some(key) = a.args[..a.args.len() - 1]
                    .iter()
                    .map(|x| operand(x, values, g))
                    .collect::<Option<SmallVec<[Value; 4]>>>()
                {
                    self.stats.oracle_owner_reads += 1;
                    w = g
                        .lookup_canonical(t, &key)?
                        .filter(|x| {
                            !x.hidden
                                && a.args
                                    .iter()
                                    .zip(x.values.iter())
                                    .all(|(a, v)| operand(a, values, g).as_ref() == Some(v))
                        })
                        .map(|x| x.witness);
                }
            }
            let Some((w, e)) = w.and_then(|w| g.visible_epoch(w).map(|e| (w, e))) else {
                exact = false;
                continue;
            };
            let stamp = (e, w, std::cmp::Reverse(i));
            if owner.is_none_or(|old| stamp > old) {
                owner = Some(stamp)
            }
        }
        let Some((e, w, std::cmp::Reverse(pivot))) = owner else {
            return Ok(Evidence::default());
        };
        if !exact || e < wm {
            return Ok(Evidence::default());
        }
        let mut ev = Evidence::default();
        let eligible = self.plan.recipes[rid].orders[pivot]
            .iter()
            .any(|&i| matches!(r.query[i].call, Call::Table(_)));
        if self.survey
            && let Some(row) = g.row(w)?
        {
            let (candidate, solution) = self.lhs(g, rid, pivot, &row, false)?;
            ev = candidate;
            if let Some(s) = solution
                && !r.inputs.iter().all(|i| s.values.get(i) == values.get(i))
            {
                return Err("structural replay input disagreement".into());
            }
        }
        ev.owner = Some((pivot, w));
        ev.known = true;
        ev.eligible = eligible;
        Ok(ev)
    }
    fn rhs(
        &self,
        g: &Graph,
        rid: usize,
        values: &Slots,
        known: &QueryFacts,
        anchor: Dependency,
        old: &[Input],
    ) -> Result<RunRhs> {
        let recipe = &self.plan.recipes[rid];
        let mut result = RunRhs::default();
        if recipe.ctor_ops == 0 || !self.live.contains_key(&anchor) {
            return Ok(result);
        }
        let r = &g.program.rules[rid];
        let t = &g.program.templates[r.template];
        let mut fields = values.clone();
        let mut complete = recipe.closed_shape;
        let mut deepest = 0;
        let mut receipts = Map::<usize, FactRef>::default();
        for (op, ins) in t.ops.iter().enumerate() {
            let (tab, aa) = match ins {
                Op::Copy(dst, a) => {
                    if let Some(v) = operand(a, &fields, g) {
                        fields.insert(*dst, v);
                    } else {
                        complete = false
                    }
                    continue;
                }
                Op::Union(a, b) => {
                    complete &=
                        operand(a, &fields, g).is_some() && operand(b, &fields, g).is_some();
                    continue;
                }
                Op::Hide(..) | Op::Panic => break,
                Op::Let(_, Call::Table(t), aa) | Op::Set(t, aa, _)
                    if g.program.tables[*t].constructor =>
                {
                    (*t, aa)
                }
                _ => {
                    complete = false;
                    continue;
                }
            };
            let Some(key) = aa
                .iter()
                .map(|a| operand(a, &fields, g))
                .collect::<Option<SmallVec<[Value; 4]>>>()
            else {
                complete = false;
                continue;
            };
            let static_alias = g.program.known_results.static_skip[rid][op];
            let original = if static_alias {
                match g.program.known_results.static_sources[rid][op] {
                    Some(Alias::Query(a)) => known.tracked(a).and_then(|w| {
                        if let Dependency::Fact(f) = w {
                            Some(f)
                        } else {
                            None
                        }
                    }),
                    Some(Alias::Effect(o)) => receipts.get(&o).copied(),
                    None => None,
                }
            } else {
                old.iter().find(|i| i.op == op).and_then(|i| i.fact)
            };
            let existing = original
                .map(|f| g.checked_constructor(f, tab, &key))
                .transpose()?
                .flatten();
            let found = if let Some(row) = existing {
                Some((row, None))
            } else {
                self.supplied(g, anchor, tab, &key)?
                    .and_then(|(row, path)| {
                        if let Dependency::Fact(f) = row.witness {
                            Some((f, path))
                        } else {
                            None
                        }
                    })
                    .map(|(f, path)| {
                        g.checked_constructor(f, tab, &key)
                            .map(|v| v.map(|row| (row, Some(path))))
                    })
                    .transpose()?
                    .flatten()
            };
            let Some((row, path)) = found else {
                if static_alias
                    && let Op::Let(dst, ..) = ins
                    && let Some(a) = &g.program.known_results.static_slots[rid][*dst]
                    && let Some(v) = operand(a, &fields, g)
                {
                    fields.insert(*dst, v);
                }
                complete = false;
                continue;
            };
            let Dependency::Fact(f) = row.witness else {
                continue;
            };
            receipts.insert(op, f);
            let v = row.values.last().unwrap();
            let mut equal_output = false;
            match ins {
                Op::Let(dst, ..) => {
                    fields.insert(*dst, v.clone());
                }
                Op::Set(_, _, a) => {
                    let rhs = operand(a, &fields, g);
                    complete &= rhs.is_some();
                    equal_output = rhs.as_ref() == Some(v);
                }
                _ => unreachable!(),
            }
            if let Some(path) = path
                && !static_alias
            {
                result.ports.push((op, path));
                deepest = deepest.max(path.len);
            }
            result.inputs.push(Input {
                op,
                back: path.map_or(1, |p| p.len as usize),
                fact: Some(f),
                equal_output,
            });
        }
        if complete && deepest > 0 {
            result.complete = Some(deepest);
        }
        Ok(result)
    }
    pub fn evaluate(
        &mut self,
        g: &Graph,
        r: usize,
        values: &Slots,
        known: &QueryFacts,
        ev: &Evidence,
        spine: bool,
        old: &[Input],
    ) -> Result<(Vec<Input>, Vec<(usize, u8)>, Option<u8>)> {
        let lane = spine as usize;
        self.stats.bindings[lane] += 1;
        self.stats.owner_known[lane] += u64::from(ev.known);
        self.stats.eligible_lhs[lane] += u64::from(ev.eligible);
        self.stats.eligible_rhs[lane] += u64::from(self.plan.recipes[r].ctor_ops > 0);
        let Some((pivot, anchor)) = ev.owner else {
            return Ok((vec![], vec![], None));
        };
        let rhs = self.rhs(g, r, values, known, anchor, old)?;
        if self.survey {
            if ev.full != 0 {
                self.stats.lhs_full[lane][ev.full as usize] += 1
            }
            if let Some(d) = rhs.complete {
                self.stats.rhs_full[lane][d as usize] += 1
            }
            for (kind, ports) in [(0u8, ev.lhs.as_slice()), (1u8, rhs.ports.as_slice())] {
                for &(op, path) in ports {
                    let bin = path.len as usize;
                    if kind == 0 {
                        self.stats.lhs_ports[lane][bin] += 1
                    } else {
                        self.stats.rhs_ports[lane][bin] += 1
                    }
                    *self
                        .shapes
                        .entry((kind, lane as u8, r, op, path))
                        .or_default() += 1;
                    self.by_rule
                        .entry((r, kind, pivot, lane as u8))
                        .or_default()[bin] += 1;
                }
            }
        }
        Ok((
            if rhs.complete.is_some() {
                rhs.inputs
            } else {
                rhs.inputs
                    .into_iter()
                    .filter(|i| rhs.ports.iter().any(|(o, _)| *o == i.op))
                    .collect()
            },
            rhs.ports.iter().map(|(o, p)| (*o, p.len)).collect(),
            rhs.complete,
        ))
    }
    pub fn accepted(&mut self, d: usize) {
        self.stats.rhs_accepted[d] += 1
    }
    pub fn closed(&mut self, d: u8, spine: bool) {
        self.stats.closed_rhs[d as usize] += 1;
        self.stats.closed_by_lane[spine as usize][d as usize] += 1
    }
    pub fn retained(&mut self, d: u8) {
        if d > 0 {
            self.stats.retained_lhs[d as usize] += 1
        }
    }
    pub fn report(&self, p: &Program) -> serde_json::Value {
        serde_json::json!({"depth":self.depth,"budget":BUDGET,"survey":self.survey,"execute":self.execute,"capped_recipes":self.plan.capped_recipes,
            "catalog":self.survey.then(||focus::Catalog::rank(self,p)), "catalog_requested_matched":self.focus.as_ref().map(|f|[f.requested,f.matched]),"stats":self.stats,"edges":self.plan.edges,"shapes":self.shapes.iter().map(|(k,n)|(k,n)).collect::<Vec<_>>(),"by_rule":self.by_rule.iter().collect::<Vec<_>>()})
    }
}
fn operand(a: &Arg, slots: &Slots, g: &Graph) -> Option<Value> {
    match a {
        Arg::Slot(i) => slots.get(i).map(|v| canon(v, &g.parents)),
        Arg::Literal(v) => Some(canon(v, &g.parents)),
    }
}
