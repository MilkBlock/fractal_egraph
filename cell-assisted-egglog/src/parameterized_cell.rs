//! A fixed parameterized Cell update program, not a catalog of concrete closures.
//! Native compiled atoms supply the wiring and native RHS supply every effect.
//! Only key/output-determined programs are admitted. No original LHS query runs.
use crate::Result;
use egglog::{
    EGraph, Value,
    ast::{Command, GenericSchedule, Schedule},
    cell_assist::{CellActionSink, CellOperand},
    cell_events::{CellCommitBatch, CellCommitFeed},
};
use egglog_numeric_id::NumericId;
use serde::Serialize;
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
pub mod joint;

type Key = Vec<Value>;
type Binding = Vec<Option<Value>>;
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Read {
    Key(usize, Key),
    Output(usize, Value),
}
#[derive(Clone, PartialEq, Eq)]
struct Row {
    values: Vec<Value>,
    stamp: u32,
}
#[derive(Default, PartialEq, Eq)]
struct Rows {
    keys: BTreeMap<(usize, Key), Row>,
    outputs: BTreeMap<(usize, Value), BTreeSet<Key>>,
    hidden: BTreeSet<(usize, Key)>,
}
impl Rows {
    fn change(&mut self, table: usize, key: Key, row: Option<Row>) -> Vec<Read> {
        let id = (table, key.clone());
        if self.keys.get(&id) == row.as_ref() {
            return vec![];
        }
        let mut reads = vec![Read::Key(table, key.clone())];
        if let Some(old) = self.keys.remove(&id) {
            let out = *old.values.last().unwrap();
            reads.push(Read::Output(table, out));
            let keys = self.outputs.get_mut(&(table, out)).unwrap();
            keys.remove(&key);
            if keys.is_empty() {
                self.outputs.remove(&(table, out));
            }
        }
        if let Some(row) = row {
            let out = *row.values.last().unwrap();
            reads.push(Read::Output(table, out));
            self.outputs.entry((table, out)).or_default().insert(key);
            self.keys.insert(id, row);
        }
        reads
    }
}
#[derive(Clone, Copy, Serialize)]
enum Access {
    Key,
    Output,
}
struct Kernel {
    sink: CellActionSink,
    tables: Vec<usize>,
    seed: usize,
    steps: Vec<(usize, Access)>,
    dirty: BTreeSet<Key>,
    dependencies: BTreeMap<Key, BTreeSet<Read>>,
    subscribers: BTreeMap<Read, BTreeSet<Key>>,
    executions: usize,
    bindings: usize,
}
fn known(o: &CellOperand, b: &[bool]) -> bool {
    match o {
        CellOperand::Slot(i) => b[*i],
        CellOperand::Constant { .. } => true,
    }
}
fn value(o: &CellOperand, b: &Binding) -> Value {
    match o {
        CellOperand::Slot(i) => b[*i].unwrap(),
        CellOperand::Constant { value, .. } => *value,
    }
}
fn unify(columns: &[CellOperand], row: &[Value], b: &mut Binding) -> bool {
    for (c, v) in columns.iter().zip(row) {
        match c {
            CellOperand::Slot(i) => {
                if b[*i].is_some_and(|old| old != *v) {
                    return false;
                }
                b[*i] = Some(*v)
            }
            CellOperand::Constant { value, .. } if value != v => return false,
            _ => (),
        }
    }
    true
}
impl Kernel {
    fn compile(
        eg: &EGraph,
        sink: CellActionSink,
        tables: Vec<usize>,
        required_seed: Option<&str>,
    ) -> Result<Self> {
        let mut plans = vec![];
        for seed in 0..sink.atoms.len() {
            if required_seed.is_some_and(|name| sink.atoms[seed].function != name) {
                continue;
            }
            let mut bound = vec![false; sink.slots().len()];
            for x in &sink.atoms[seed].columns {
                if let CellOperand::Slot(i) = x {
                    bound[*i] = true
                }
            }
            let mut pending = (0..sink.atoms.len())
                .filter(|i| *i != seed)
                .collect::<BTreeSet<_>>();
            let mut steps = vec![];
            while !pending.is_empty() {
                let next = pending.iter().find_map(|i| {
                    let a = &sink.atoms[*i];
                    let n = a.columns.len() - 1;
                    if a.columns[..n].iter().all(|v| known(v, &bound)) {
                        Some((*i, Access::Key))
                    } else if eg.cell_sort_is_equality(&a.sorts[n]).ok()?
                        && known(&a.columns[n], &bound)
                    {
                        Some((*i, Access::Output))
                    } else {
                        None
                    }
                });
                let Some((i, access)) = next else { break };
                for x in &sink.atoms[i].columns {
                    if let CellOperand::Slot(i) = x {
                        bound[*i] = true
                    }
                }
                pending.remove(&i);
                steps.push((i, access));
            }
            if pending.is_empty() {
                plans.push((seed, steps))
            }
        }
        if plans.len() != 1 {
            return Err(format!(
                "{} has {} fixed seed plans; requires a different interface or ordinary matcher",
                sink.name,
                plans.len()
            )
            .into());
        }
        let (seed, steps) = plans.pop().unwrap();
        Ok(Self {
            sink,
            tables,
            seed,
            steps,
            dirty: BTreeSet::new(),
            dependencies: BTreeMap::new(),
            subscribers: BTreeMap::new(),
            executions: 0,
            bindings: 0,
        })
    }
    fn wake(&mut self, read: &Read) {
        if let Some(seeds) = self.subscribers.get(read) {
            self.dirty.extend(seeds.iter().cloned());
        }
        if let Read::Key(table, key) = read {
            if *table == self.tables[self.seed] {
                self.dirty.insert(key.clone());
            }
        }
    }
    fn read_program(
        &self,
        eg: &EGraph,
        rows: &Rows,
        step: usize,
        b: Binding,
        stamp: u32,
        watermark: u32,
        deps: &mut BTreeSet<Read>,
        out: &mut Vec<Vec<Value>>,
        lookups: &mut usize,
    ) -> Result<()> {
        if step == self.steps.len() {
            if stamp >= watermark {
                let b = b
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
                    .ok_or("unbound interface")?;
                if eg.cell_guards_hold(&self.sink, &b)? {
                    out.push(b)
                }
            }
            return Ok(());
        }
        let (i, access) = self.steps[step];
        let a = &self.sink.atoms[i];
        let table = self.tables[i];
        let n = a.columns.len() - 1;
        let read = match access {
            Access::Key => Read::Key(table, a.columns[..n].iter().map(|x| value(x, &b)).collect()),
            Access::Output => Read::Output(table, value(&a.columns[n], &b)),
        };
        deps.insert(read.clone());
        *lookups += 1;
        let keys = match read {
            Read::Key(_, k) => vec![k],
            Read::Output(_, v) => rows
                .outputs
                .get(&(table, v))
                .into_iter()
                .flatten()
                .cloned()
                .collect(),
        };
        for key in keys {
            if let Some(row) = rows.keys.get(&(table, key)) {
                let mut next = b.clone();
                if unify(&a.columns, &row.values, &mut next) {
                    self.read_program(
                        eg,
                        rows,
                        step + 1,
                        next,
                        stamp.max(row.stamp),
                        watermark,
                        deps,
                        out,
                        lookups,
                    )?;
                }
            }
        }
        Ok(())
    }
    fn matches(
        &mut self,
        eg: &EGraph,
        rows: &Rows,
        lookups: &mut usize,
    ) -> Result<Vec<Vec<Value>>> {
        let mut out = vec![];
        let watermark = eg.cell_query_watermark(&self.sink);
        for seed in std::mem::take(&mut self.dirty) {
            self.executions += 1;
            if let Some(old) = self.dependencies.remove(&seed) {
                for d in old {
                    if let Some(s) = self.subscribers.get_mut(&d) {
                        s.remove(&seed);
                        if s.is_empty() {
                            self.subscribers.remove(&d);
                        }
                    }
                }
            }
            let Some(row) = rows.keys.get(&(self.tables[self.seed], seed.clone())) else {
                continue;
            };
            let mut b = vec![None; self.sink.slots().len()];
            if !unify(&self.sink.atoms[self.seed].columns, &row.values, &mut b) {
                continue;
            }
            let mut deps = BTreeSet::new();
            self.read_program(
                eg, rows, 0, b, row.stamp, watermark, &mut deps, &mut out, lookups,
            )?;
            for d in &deps {
                self.subscribers
                    .entry(d.clone())
                    .or_default()
                    .insert(seed.clone());
            }
            self.dependencies.insert(seed, deps);
        }
        out.sort_unstable();
        if out.windows(2).any(|p| p[0] == p[1]) {
            return Err("duplicate fixed-program binding".into());
        }
        self.bindings += out.len();
        Ok(out)
    }
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub program_compilations: usize,
    pub ruleset_runs: usize,
    pub keyed_reads: usize,
    pub committed_row_events: usize,
    pub union_events: usize,
    pub bootstrap_rows: usize,
    pub native_fallback_runs: usize,
}
pub struct Runtime {
    eg: EGraph,
    kernels: Vec<Kernel>,
    sets: BTreeMap<String, Vec<usize>>,
    names: Vec<String>,
    feed: CellCommitFeed,
    rows: Rows,
    identity: Vec<(String, String)>,
    failed: bool,
    epoch: u64,
    joint: Option<joint::Joint>,
    forwarded: Option<Vec<CellCommitBatch>>,
    pub last_updated: bool,
    pub stats: Stats,
}
impl Runtime {
    pub fn new(eg: EGraph, sets: &[&str]) -> Result<Self> {
        Self::new_with_seeds(eg, sets, &BTreeMap::new())
    }
    fn new_with_seeds(
        mut eg: EGraph,
        sets: &[&str],
        seeds: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let mut sinks = vec![];
        let mut groups = BTreeMap::new();
        let mut names = BTreeSet::new();
        for set in sets {
            let mut ids = vec![];
            for name in eg.cell_rule_names(set)? {
                let sink = eg.prepare_cell_action(set, &name)?;
                if !eg.cell_query_is_seminaive(&sink) {
                    return Err("requires seminaive rules".into());
                }
                names.extend(sink.atoms.iter().map(|a| a.function.clone()));
                ids.push(sinks.len());
                sinks.push(sink);
            }
            groups.insert(set.to_string(), ids);
        }
        let names = names.into_iter().collect::<Vec<_>>();
        let mut kernels = vec![];
        for sink in sinks {
            let tables = sink
                .atoms
                .iter()
                .map(|a| names.iter().position(|n| n == &a.function).unwrap())
                .collect();
            let seed = seeds.get(&sink.name).map(String::as_str);
            kernels.push(Kernel::compile(&eg, sink, tables, seed)?);
        }
        let identity = eg.cell_rule_environment_identity();
        let (feed, initial) = eg.subscribe_cell_commits(&names)?;
        let mut r = Self {
            eg,
            kernels,
            sets: groups,
            names,
            feed,
            rows: Rows::default(),
            identity,
            failed: false,
            epoch: 0,
            joint: None,
            forwarded: None,
            last_updated: false,
            stats: Stats {
                program_compilations: 1,
                ..Stats::default()
            },
        };
        r.stats.bootstrap_rows = initial.rows.len();
        r.ingest(initial)?;
        Ok(r)
    }
    pub fn engine(&self) -> &EGraph {
        &self.eg
    }
    fn ingest(&mut self, batch: CellCommitBatch) -> Result<()> {
        if let Some(out) = &mut self.forwarded {
            out.push(batch.clone());
        }
        if let Some(mut proof) = self.joint.take() {
            proof.observes(self, &batch);
            self.joint = Some(proof);
        }
        let has_union = !batch.unions.is_empty();
        self.stats.committed_row_events += batch.rows.len();
        self.stats.union_events += batch.unions.len();
        let mut changed = vec![];
        let mut hidden_changed = false;
        for t in batch.clears {
            let before = self.rows.hidden.len();
            self.rows.hidden.retain(|(table, _)| *table != t);
            hidden_changed |= before != self.rows.hidden.len();
            let keys = self
                .rows
                .keys
                .keys()
                .filter(|(id, _)| *id == t)
                .map(|(_, k)| k.clone())
                .collect::<Vec<_>>();
            for k in keys {
                changed.extend(self.rows.change(t, k, None));
            }
        }
        for delta in batch.rows {
            let t = delta.table;
            let key = (t, delta.key.clone());
            hidden_changed |= if delta.row.is_some() && !delta.visible {
                self.rows.hidden.insert(key)
            } else {
                self.rows.hidden.remove(&key)
            };
            let row = if delta.visible {
                delta.row.map(|mut values| {
                    values.truncate(self.feed.tables[t].keys + 1);
                    Row {
                        values,
                        stamp: delta.timestamp.expect("visible native row timestamp"),
                    }
                })
            } else {
                None
            };
            changed.extend(self.rows.change(t, delta.key, row));
        }
        if has_union || hidden_changed || !changed.is_empty() {
            self.epoch = self
                .epoch
                .checked_add(1)
                .ok_or("joint generation exhausted")?;
        }
        if self.forwarded.is_none() {
            for read in changed {
                for k in &mut self.kernels {
                    k.wake(&read)
                }
            }
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<()> {
        let batch = self.eg.drain_cell_commits(&mut self.feed)?;
        // Native rebuild journals contain old-key removals and canonical row
        // replacements. Those changes also wake bindings that previously missed
        // a key/output lookup; no new concrete Cell or whole-table rescan.
        self.ingest(batch)
    }
    /// Independent diagnostic only, outside the update path. Includes row
    /// timestamps, visibility and every reverse-index entry, not just counts.
    pub fn audit_mirror(&self) -> Result<usize> {
        let mut expected = Rows::default();
        for (i, name) in self.names.iter().enumerate() {
            let n = self.feed.tables[i].keys;
            self.eg.function_for_each(name, |r| {
                if r.subsumed {
                    expected.hidden.insert((i, r.vals[..n].to_vec()));
                }
                if !r.subsumed {
                    let values = r.vals[..n + 1].to_vec();
                    expected.change(
                        i,
                        values[..n].to_vec(),
                        Some(Row {
                            values,
                            stamp: r.timestamp.rep(),
                        }),
                    );
                }
            })?;
        }
        if expected != self.rows {
            return Err("native committed-row mirror differs".into());
        }
        Ok(expected.keys.len())
    }
    pub fn input(&mut self, code: &str) -> Result<()> {
        if self.failed {
            return Err("failed update program cannot resume".into());
        }
        let cmds = self.eg.parse_program(None, code)?;
        if cmds
            .iter()
            .any(|c| !matches!(c, Command::Action(_) | Command::Check(..)))
        {
            return Err("input accepts facts/actions/checks only".into());
        }
        let result = (|| {
            self.eg.run_program(cmds)?;
            self.sync()
        })();
        if result.is_err() {
            self.failed = true
        }
        result
    }
    fn step(&mut self, s: &Schedule) -> Result<bool> {
        match s {
            GenericSchedule::Run(_, r)
                if self.sets.contains_key(&r.ruleset) && r.until.is_none() =>
            {
                self.sync()?;
                let ids = self.sets[&r.ruleset].clone();
                let mut matches = vec![];
                for id in &ids {
                    matches.push(self.kernels[*id].matches(
                        &self.eg,
                        &self.rows,
                        &mut self.stats.keyed_reads,
                    )?);
                }
                let batches = ids
                    .iter()
                    .zip(&matches)
                    .map(|(i, b)| (&self.kernels[*i].sink, b.as_slice()))
                    .collect::<Vec<_>>();
                let report = self.eg.apply_completed_cell_ruleset(&batches)?;
                self.stats.ruleset_runs += 1;
                self.sync()?;
                Ok(report.updated)
            }
            GenericSchedule::Run(..) => {
                self.stats.native_fallback_runs += 1;
                let outputs = self.eg.run_program(vec![Command::RunSchedule(s.clone())])?;
                let changed = outputs
                    .into_iter()
                    .any(|x| matches!(x,egglog::CommandOutput::RunSchedule(r) if r.updated));
                self.sync()?;
                Ok(changed)
            }
            GenericSchedule::Sequence(_, xs) => {
                let mut c = false;
                for x in xs {
                    c |= self.step(x)?;
                }
                Ok(c)
            }
            GenericSchedule::Repeat(_, n, x) => {
                let mut c = false;
                for _ in 0..*n {
                    let d = self.step(x)?;
                    c |= d;
                    if !d {
                        break;
                    }
                }
                Ok(c)
            }
            GenericSchedule::Saturate(_, x) => {
                self.sync()?;
                if self.joint.as_ref().is_some_and(|j| j.accepts(x)) {
                    let mut proof = self.joint.take().unwrap();
                    let result = (|| {
                        if let Some(updated) = proof.try_close(self)? {
                            return Ok(updated);
                        }
                        let mut changed = false;
                        loop {
                            let updated = self.step(x)?;
                            changed |= updated;
                            if !updated {
                                break;
                            }
                        }
                        proof.complete(self)?;
                        Ok(changed)
                    })();
                    self.joint = Some(proof);
                    return result;
                }
                let mut c = false;
                loop {
                    let d = self.step(x)?;
                    c |= d;
                    if !d {
                        break;
                    }
                }
                Ok(c)
            }
        }
    }
    pub fn run(&mut self, commands: &[Command]) -> Result<()> {
        if self.failed {
            return Err("failed update program cannot resume".into());
        }
        if self.eg.cell_rule_environment_identity() != self.identity {
            return Err("rule environment changed".into());
        }
        let result = (|| {
            self.last_updated = false;
            for c in commands {
                let Command::RunSchedule(s) = c else {
                    return Err("expected schedule".into());
                };
                self.last_updated |= self.step(s)?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.failed = true
        }
        result
    }
    pub fn report(&self) -> Json {
        json!({"stats":self.stats,"joint":self.joint.as_ref().map(|j|j.report()),"programs":self.sets.iter().flat_map(|(set,ids)|ids.iter().map(move |id|{let k=&self.kernels[*id];json!({"ruleset":set,"rule":k.sink.name,"seed":k.sink.atoms[k.seed].function,"accesses":k.steps.iter().map(|(i,a)|json!({"table":k.sink.atoms[*i].function,"access":a})).collect::<Vec<_>>(),"seed_evaluations":k.executions,"emitted_bindings":k.bindings,"live_seed_instances":k.dependencies.len()})})).collect::<Vec<_>>(),"watched_rows":self.rows.keys.len(),"dependency_edges":self.kernels.iter().map(|k|k.subscribers.values().map(BTreeSet::len).sum::<usize>()).sum::<usize>(),"concrete_closure_catalog_entries":0,"scope":"Parameterized native update program with optional guarded joint saturation transfers. Original schedule and native effects; unsupported boundary states use the ordinary parameterized path. Not a GPU benchmark."})
    }
}
