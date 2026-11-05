//! Compiler-owned shared entry nodes. Current parameter packets are maintained
//! once per changed input; consumers borrow them at their original source Run.
use crate::{
    graph::{Dependency, Graph, Row},
    program::*,
    value::{Value, canon},
};
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::{collections::BTreeMap, rc::Rc};
#[cfg(not(feature = "epoch-runs"))]
use {crate::graph::FactRef, std::collections::BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    nodes: Vec<Node>,
    rules: Vec<Vec<Consumer>>,
    tables: Vec<Vec<usize>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Node {
    #[cfg(feature = "row-ports")]
    #[serde(skip)]
    checks: Vec<(usize, Arg)>,
    table: usize,
    args: Vec<Arg>,
    ports: usize,
    literal_identity: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Consumer {
    node: usize,
    atom: usize,
    projection: Rc<Vec<Option<usize>>>,
}
fn identity(v: &Value) -> bool {
    match v {
        Value::Eq(_) => true,
        Value::Set(xs) => xs.iter().any(identity),
        _ => false,
    }
}
impl Plan {
    #[cfg(any(feature = "family-frontiers", feature = "family-scan"))]
    pub(crate) fn survey_nodes(&self, r: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.rules[r].iter().map(|c| (c.atom, c.node))
    }

    pub fn parse(p: &Program, raw: &J) -> Result<Self> {
        if raw.is_null() {
            {
                return Err("shared node schedule requires compiler entry_schedule; re-emit the Cell CPU artifact".into());
            }
        }
        let (s, expected) = Self::derive(p)?;
        if *raw != expected {
            return Err("shared entry schedule differs from typed query wiring/control".into());
        }
        Ok(s)
    }
    pub(crate) fn derive(p: &Program) -> Result<(Self, J)> {
        let mut s = Self {
            nodes: vec![],
            rules: vec![vec![]; p.rules.len()],
            tables: vec![vec![]; p.tables.len()],
        };
        let mut ids = BTreeMap::new();
        let mut expected: Vec<J> = vec![];
        for (rid, r) in p.rules.iter().enumerate() {
            if r.seminaive {
                for (atom, a) in r.query.iter().enumerate() {
                    let Call::Table(t) = a.call else { continue };
                    let mut slots = BTreeMap::new();
                    let mut projection = vec![None; p.templates[r.template].sorts.len()];
                    let mut pairs = vec![];
                    let mut pattern = vec![];
                    let mut args = vec![];
                    for (col, v) in a.args.iter().enumerate() {
                        match v {
                            Arg::Slot(i) => {
                                let n = slots.len();
                                let port = *slots.entry(*i).or_insert(n);
                                if projection[*i].is_none() {
                                    #[cfg(feature = "row-ports")]
                                    {
                                        projection[*i] = Some(col);
                                    }
                                    #[cfg(not(feature = "row-ports"))]
                                    {
                                        projection[*i] = Some(port);
                                    }
                                    pairs.push(json!({"slot":i,"port":port}));
                                }
                                pattern.push(Arg::Slot(port));
                                args.push(json!({"slot":port}));
                            }
                            Arg::Literal(v) => {
                                pattern.push(Arg::Literal(v.clone()));
                                args.push(
                                    p.artifact["recipe"]["rules"][rid]["query"][atom]["args"][col]
                                        .clone(),
                                );
                            }
                        }
                    }
                    let key = (t, pattern.clone());
                    let n = ids.len();
                    let id = *ids.entry(key).or_insert(n);
                    if id == s.nodes.len() {
                        s.tables[t].push(id);
                        #[cfg(feature = "row-ports")]
                        let checks = pattern
                            .iter()
                            .enumerate()
                            .filter_map(|(col, a)| match a {
                                Arg::Literal(_) => Some((col, a.clone())),
                                Arg::Slot(_) => {
                                    let previous = pattern[..col].iter().position(|x| x == a)?;
                                    Some((col, Arg::Slot(previous)))
                                }
                            })
                            .collect();
                        s.nodes.push(Node {
                            #[cfg(feature = "row-ports")]
                            checks,
                            table: t,
                            args: pattern,
                            ports: slots.len(),
                            literal_identity: a
                                .args
                                .iter()
                                .any(|a| matches!(a,Arg::Literal(v) if identity(v))),
                        });
                        expected.push(json!({"id":id,"table":p.tables[t].name,"args":args,"ports":slots.len(),"consumers":[]}));
                    }
                    expected[id]["consumers"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"rule":rid,"atom":atom,"slots":pairs}));
                    s.rules[rid].push(Consumer {
                        node: id,
                        atom,
                        projection: Rc::new(projection),
                    });
                }
            }
        }
        let expected = json!({"schema":"eggfuse-entry-schedule/v1","recipe_sha256":p.artifact["recipe"]["sha256"],"nodes":expected,"control":p.artifact["recipe"]["control"],"contract":{
            "input":"committed current row changes, coalesced before a legal consumer Run",
            "result":"complete correlated entry parameters and witness epoch",
            "delivery":"independent consumer watermarks; original RHS publication",
            "invalidation":"old/new witnesses, visibility, canonical identities and same-key forwarding",
            "scope":"table entry nodes; residual queries and guards remain exact"}});
        Ok((s, expected))
    }
}
#[cfg(feature = "row-ports")]
type SharedPacket = Packet;
#[cfg(not(feature = "row-ports"))]
type SharedPacket = Rc<Packet>;
#[derive(Clone)]
pub struct Packet {
    pub row: Rc<Row>,
    #[cfg(not(feature = "row-ports"))]
    values: Rc<Vec<Option<Value>>>,
}
#[derive(Clone)]
pub struct Seed {
    pub atom: usize,
    pub packet: SharedPacket,
    projection: Rc<Vec<Option<usize>>>,
}
/// Sparse private suffix writes over immutable compiled parameter projections.
/// No initial full-width parameter array is cloned for a scheduled consumer.
#[derive(Clone)]
pub struct Inputs {
    #[cfg(feature = "row-ports")]
    legacy_base: std::cell::OnceCell<Box<[Option<Value>]>>,
    own: Vec<Option<Value>>,
    base: Option<Seed>,
}
impl Inputs {
    pub fn owned(n: usize) -> Self {
        Self {
            #[cfg(feature = "row-ports")]
            legacy_base: Default::default(),
            own: vec![None; n],
            base: None,
        }
    }
    pub fn shared(seed: Seed) -> Self {
        Self {
            #[cfg(feature = "row-ports")]
            legacy_base: Default::default(),
            own: vec![],
            base: Some(seed),
        }
    }
    pub fn len(&self) -> usize {
        self.own
            .len()
            .max(self.base.as_ref().map_or(0, |s| s.projection.len()))
    }
    pub fn iter(&self) -> impl Iterator<Item = &Option<Value>> {
        (0..self.len()).map(move |i| &self[i])
    }
    pub fn refs(&self) -> impl Iterator<Item = Option<&Value>> {
        (0..self.len()).map(move |i| self.get(i))
    }
    /// Move private slot values into the sealed RHS input. Shared entry
    /// packets remain immutable and supply owned snapshots when needed.

    pub fn materialize(&self) -> Vec<Option<Value>> {
        self.refs().map(|v| v.cloned()).collect()
    }
    pub fn owned_mut(&mut self) -> &mut [Option<Value>] {
        assert!(self.base.is_none());
        &mut self.own
    }
}
impl Inputs {
    pub fn get(&self, slot: usize) -> Option<&Value> {
        if let Some(v) = self.own.get(slot).and_then(Option::as_ref) {
            return Some(v);
        }
        let s = self.base.as_ref()?;
        let port = s.projection.get(slot).copied().flatten()?;
        #[cfg(feature = "row-ports")]
        {
            s.packet.row.values.get(port)
        }
        #[cfg(not(feature = "row-ports"))]
        {
            s.packet.values[port].as_ref()
        }
    }
    pub fn set(&mut self, slot: usize, v: Option<Value>) {
        if self.own.len() <= slot {
            self.own.resize(slot + 1, None)
        }
        self.own[slot] = v;
    }
}
#[cfg(feature = "row-ports")]
impl std::ops::Index<usize> for Inputs {
    type Output = Option<Value>;
    fn index(&self, i: usize) -> &Self::Output {
        if let Some(v) = self.own.get(i).filter(|v| v.is_some()) {
            return v;
        }
        let Some(s) = &self.base else { return &None };
        if s.projection.get(i).is_none_or(Option::is_none) {
            return &None;
        }
        &self.legacy_base.get_or_init(|| {
            s.projection
                .iter()
                .map(|port| port.map(|c| s.packet.row.values[c].clone()))
                .collect()
        })[i]
    }
}
#[cfg(not(feature = "row-ports"))]
impl std::ops::Index<usize> for Inputs {
    type Output = Option<Value>;
    fn index(&self, i: usize) -> &Self::Output {
        if let Some(v) = self.own.get(i).filter(|v| v.is_some()) {
            return v;
        }
        if let Some(s) = &self.base {
            if let Some(Some(port)) = s.projection.get(i) {
                return &s.packet.values[*port];
            }
        }
        &None
    }
}
impl std::ops::IndexMut<usize> for Inputs {
    fn index_mut(&mut self, i: usize) -> &mut Self::Output {
        if self.own.len() <= i {
            self.own.resize(i + 1, None)
        }
        &mut self.own[i]
    }
}

#[cfg(feature = "epoch-runs")]
#[path = "epoch_runs.rs"]
mod epoch_runs;
#[derive(Default)]
struct State {
    #[cfg(feature = "epoch-runs")]
    planned_counts: bool,
    cold: bool,
    #[cfg(not(feature = "epoch-runs"))]
    pending: BTreeSet<Dependency>,
    #[cfg(feature = "epoch-runs")]
    pending: rustc_hash::FxHashSet<Dependency>,
    rows: HashMap<Dependency, SharedPacket>,
    #[cfg(not(feature = "epoch-runs"))]
    epochs: BTreeSet<(u64, Dependency)>,
    #[cfg(feature = "epoch-runs")]
    epochs: epoch_runs::Epochs,
    epoch_counts: Option<BTreeMap<u64, usize>>,
}
impl State {
    fn insert_epoch(&mut self, epoch: u64, w: Dependency) {
        if self.epochs.insert((epoch, w))
            && let Some(counts) = &mut self.epoch_counts
        {
            *counts.entry(epoch).or_default() += 1;
        }
    }
    fn remove_epoch(&mut self, epoch: u64, w: Dependency) {
        if self.epochs.remove(&(epoch, w))
            && let Some(counts) = &mut self.epoch_counts
        {
            let count = counts.get_mut(&epoch).expect("indexed epoch");
            *count -= 1;
            if *count == 0 {
                counts.remove(&epoch);
            }
        }
    }
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub predicate_checks: u64,
    node_refreshes: u64,
    changed_headers: u64,
    node_notifications: u64,
    cold_rows: u64,
    row_decodes: u64,
    parameters_copied: u64,
    parameter_packets_reused: u64,
    consumer_calls: u64,
    consumer_seed_deliveries: u64,
    borrowed_slots: u64,
    retired_packets: u64,
}
pub struct Scheduler {
    pub enabled: bool,
    plan: Plan,
    states: Vec<State>,
    rows: HashMap<Dependency, Option<Rc<Row>>>,
    unions: u64,
    pub stats: Stats,
}
/// Immutable node contents after refresh, plus this consumer's delta frontier.
#[derive(Clone, Copy)]
pub(crate) struct Batch<'a> {
    state: &'a State,
    consumer: &'a Consumer,
    watermark: u64,
    pub table: usize,
}
impl Batch<'_> {
    #[cfg(any(
        feature = "projection-shared",
        feature = "family-overlap",
        feature = "family-frontiers",
        feature = "family-scan"
    ))]
    pub fn node(&self) -> usize {
        self.consumer.node
    }
    #[cfg(any(
        feature = "projection-semijoin",
        feature = "projection-shared",
        feature = "family-frontiers",
        feature = "family-scan"
    ))]
    pub fn watermark(&self) -> u64 {
        self.watermark
    }
    pub fn atom(&self) -> usize {
        self.consumer.atom
    }
    pub fn len(&self) -> usize {
        self.state.rows.len()
    }
    #[cfg(feature = "epoch-runs")]
    pub fn delta(&self) -> impl Iterator<Item = &(u64, Dependency)> {
        self.state.epochs.range(self.watermark)
    }
    #[cfg(not(feature = "epoch-runs"))]
    pub fn delta(&self) -> std::collections::btree_set::Range<'_, (u64, Dependency)> {
        self.state
            .epochs
            .range((self.watermark, Dependency::Fact(FactRef { cell: 0, op: 0 }))..)
    }
    #[cfg(feature = "epoch-runs")]
    pub fn delta_len(&self) -> Option<usize> {
        self.state
            .planned_counts
            .then(|| self.state.epochs.counts(self.watermark).sum())
    }
    #[cfg(not(feature = "epoch-runs"))]
    pub fn delta_len(&self) -> Option<usize> {
        Some(
            self.state
                .epoch_counts
                .as_ref()?
                .range(self.watermark..)
                .map(|(_, n)| n)
                .sum(),
        )
    }
    #[cfg(feature = "epoch-runs")]
    pub fn delta_exceeds(&self, budget: usize, bins: &mut u64) -> bool {
        let mut total = 0;
        self.state.epochs.counts(self.watermark).any(|n| {
            *bins += 1;
            total += n;
            total > budget
        })
    }
    #[cfg(not(feature = "epoch-runs"))]
    pub fn delta_exceeds(&self, budget: usize, bins: &mut u64) -> bool {
        let counts = self.state.epoch_counts.as_ref().expect("planned entry");
        let mut total = 0;
        for (_, n) in counts.range(self.watermark..) {
            *bins += 1;
            total += n;
            if total > budget {
                return true;
            }
        }
        false
    }
    pub fn packet(&self, w: Dependency) -> Option<&SharedPacket> {
        self.state
            .rows
            .get(&w)
            .filter(|p| p.row.epoch >= self.watermark)
    }
    pub fn seed(&self, packet: &SharedPacket) -> Seed {
        Seed {
            atom: self.consumer.atom,
            packet: packet.clone(),
            projection: self.consumer.projection.clone(),
        }
    }
}
impl Scheduler {
    pub fn new(p: &Program, enabled: bool) -> Self {
        Self::with_epoch_counts(p, enabled, false)
    }
    pub(crate) fn with_epoch_counts(p: &Program, enabled: bool, planned: bool) -> Self {
        let plan = p.entry_schedule.clone();
        let states = plan
            .nodes
            .iter()
            .map(|_| State {
                cold: true,
                #[cfg(feature = "epoch-runs")]
                planned_counts: enabled && planned,
                epoch_counts: (enabled && planned && !cfg!(feature = "epoch-runs"))
                    .then(BTreeMap::new),
                ..Default::default()
            })
            .collect();
        Self {
            enabled,
            plan,
            states,
            rows: HashMap::default(),
            unions: 0,
            stats: Stats::default(),
        }
    }
    pub fn accepts(&self, rid: usize) -> bool {
        self.enabled && self.plan.rules.get(rid).is_some_and(|x| !x.is_empty())
    }
    /// Called before query-change records are consumed. Only committed rows and
    /// exact same-key forwarding retirements enter the shared pending domains.
    pub fn publish(&mut self, g: &Graph) {
        self.publish_except(g, |_| false);
    }
    pub(crate) fn publish_except(&mut self, g: &Graph, dedicated: impl Fn(usize) -> bool) {
        if !self.enabled {
            return;
        }
        self.rows.clear();
        // A same-key repair retires the old witness and touches the survivor.
        for (table, changes) in g.changes.tables() {
            if dedicated(table) {
                continue;
            }
            self.stats.changed_headers += changes.len() as u64;
            for &id in &self.plan.tables[table] {
                let state = &mut self.states[id];
                if !state.cold {
                    let before = state.pending.len();
                    state.pending.extend(changes.keys().copied());
                    self.stats.node_notifications += (state.pending.len() - before) as u64;
                }
            }
        }
        if self.unions != g.stats.unions {
            for (id, n) in self.plan.nodes.iter().enumerate() {
                if n.literal_identity {
                    self.states[id].cold = true;
                }
            }
            self.unions = g.stats.unions;
        }
    }
    pub fn seeds(&mut self, g: &Graph, rid: usize, watermark: u64) -> Result<Vec<Seed>> {
        self.seeds_where(g, rid, watermark, |_, _, _| true)
    }
    pub(crate) fn seeds_where(
        &mut self,
        g: &Graph,
        rid: usize,
        watermark: u64,
        mut allow: impl FnMut(usize, usize, Dependency) -> bool,
    ) -> Result<Vec<Seed>> {
        self.batches(g, rid, watermark, |batch, out| {
            for &(_, w) in batch.delta() {
                if allow(batch.atom(), batch.table, w) {
                    out.push(batch.seed(&batch.state.rows[&w]));
                }
            }
            Ok(())
        })
    }
    pub(crate) fn terminal_batch(
        &mut self,
        g: &Graph,
        rid: usize,
        atom: usize,
        watermark: u64,
    ) -> Result<Batch<'_>> {
        let i = self.plan.rules[rid]
            .iter()
            .position(|c| c.atom == atom)
            .ok_or("missing terminal entry")?;
        let node = self.plan.rules[rid][i].node;
        self.refresh(g, node)?;
        Ok(Batch {
            state: &self.states[node],
            consumer: &self.plan.rules[rid][i],
            watermark,
            table: self.plan.nodes[node].table,
        })
    }
    pub(crate) fn batches(
        &mut self,
        g: &Graph,
        rid: usize,
        watermark: u64,
        mut visit: impl FnMut(Batch<'_>, &mut Vec<Seed>) -> Result<()>,
    ) -> Result<Vec<Seed>> {
        self.stats.consumer_calls += 1;
        let mut out = vec![];
        for c in self.plan.rules[rid].clone() {
            self.refresh(g, c.node)?;
            let start = out.len();
            visit(
                Batch {
                    state: &self.states[c.node],
                    consumer: &c,
                    watermark,
                    table: self.plan.nodes[c.node].table,
                },
                &mut out,
            )?;
            let count = (out.len() - start) as u64;
            self.stats.consumer_seed_deliveries += count;
            self.stats.borrowed_slots += count * self.plan.nodes[c.node].ports as u64;
        }
        Ok(out)
    }
    fn refresh(&mut self, g: &Graph, id: usize) -> Result<()> {
        let s = &mut self.states[id];
        if !s.cold && s.pending.is_empty() {
            return Ok(());
        }
        self.stats.node_refreshes += 1;
        let n = &self.plan.nodes[id];
        let full = s.cold;
        s.cold = false;
        let pending = if full {
            self.stats.cold_rows += g.witness_count(n.table) as u64;
            s.pending.clear();
            g.witnesses(n.table)
        } else {
            #[cfg(feature = "epoch-runs")]
            {
                let mut v: Vec<_> = s.pending.drain().collect();
                v.sort_unstable();
                v
            }
            #[cfg(not(feature = "epoch-runs"))]
            {
                std::mem::take(&mut s.pending).into_iter().collect()
            }
        };
        if full {
            s.epochs.clear();
            if let Some(counts) = &mut s.epoch_counts {
                counts.clear();
            }
        }
        let mut old = if full {
            std::mem::take(&mut s.rows)
        } else {
            HashMap::default()
        };
        for w in pending {
            let previous = if full {
                old.remove(&w)
            } else {
                s.rows.remove(&w)
            };
            if let Some(p) = &previous {
                s.remove_epoch(p.row.epoch, w);
            }
            let row = if let Some(row) = self.rows.get(&w) {
                row.clone()
            } else {
                self.stats.row_decodes += 1;
                let row = g.row(w)?.map(Rc::new);
                self.rows.insert(w, row.clone());
                row
            };
            let Some(row) = row.filter(|r| r.table == n.table && !r.hidden) else {
                self.stats.retired_packets += u64::from(previous.is_some());
                continue;
            };
            self.stats.predicate_checks += 1;
            if !n.literal_identity
                && let Some(_previous) = previous.as_ref().filter(|p| p.row.values == row.values)
            {
                self.stats.parameter_packets_reused += 1;
                s.insert_epoch(row.epoch, w);
                #[cfg(feature = "row-ports")]
                s.rows.insert(w, Packet { row });
                #[cfg(not(feature = "row-ports"))]
                s.rows.insert(
                    w,
                    Rc::new(Packet {
                        row,
                        values: _previous.values.clone(),
                    }),
                );
                continue;
            }
            #[cfg(feature = "row-ports")]
            {
                let valid = n.checks.iter().all(|(col, check)| match check {
                    Arg::Slot(other) => row.values[*other] == row.values[*col],
                    Arg::Literal(v) => canon(v, &g.parents) == row.values[*col],
                });
                if valid {
                    s.insert_epoch(row.epoch, w);
                    s.rows.insert(w, Packet { row });
                } else {
                    self.stats.retired_packets += u64::from(previous.is_some());
                }
            }
            #[cfg(not(feature = "row-ports"))]
            {
                let mut values = vec![None; n.ports];
                let valid = n.args.iter().zip(row.values.iter()).all(|(a, v)| match a {
                    Arg::Literal(l) => canon(l, &g.parents) == *v,
                    Arg::Slot(i) => match &values[*i] {
                        Some(old) => old == v,
                        None => {
                            self.stats.parameters_copied += 1;
                            values[*i] = Some(v.clone());
                            true
                        }
                    },
                });
                if valid {
                    let values = if let Some(p) =
                        previous.as_ref().filter(|p| p.values.as_ref() == &values)
                    {
                        self.stats.parameter_packets_reused += 1;
                        p.values.clone()
                    } else {
                        Rc::new(values)
                    };
                    s.insert_epoch(row.epoch, w);
                    s.rows.insert(w, Rc::new(Packet { row, values }));
                } else {
                    self.stats.retired_packets += u64::from(previous.is_some());
                }
            }
        }
        self.stats.retired_packets += old.len() as u64;
        Ok(())
    }
    #[cfg(feature = "rhs-work-audit")]
    pub fn seed_deliveries(&self) -> u64 {
        self.stats.consumer_seed_deliveries
    }
    pub fn report(&self) -> J {
        json!({"enabled":self.enabled,"nodes":self.plan.nodes.len(),"stats":self.stats,"live_packets":self.states.iter().map(|s|s.rows.len()).sum::<usize>(),"scope":"compiled shared entry predicates; consumer watermarks and complete residual/RHS remain source scheduled"})
    }
}
