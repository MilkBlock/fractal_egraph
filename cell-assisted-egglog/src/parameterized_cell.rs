//! A fixed parameterized Cell update program, not a catalog of concrete closures.
//! Native compiled atoms supply the wiring and native RHS supply every effect.
//! Typed key/output/partial-key programs retain complete bindings and native guards.
//! No original LHS query runs inside these providers.
use crate::Result;
use egglog::{
    EGraph, Value,
    ast::{Command, GenericSchedule, Schedule},
    cell_assist::{CellActionSink, CellOperand},
    cell_events::{CellCommitBatch, CellCommitFeed},
};
use egglog_numeric_id::NumericId;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;
use serde_json::{Value as Json, json};
use smallvec::{SmallVec, smallvec};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
pub mod artifact;
pub mod joint;
mod comparison;
mod scalar;
pub mod boundary;

type Key = SmallVec<[Value; 4]>;
type Binding = SmallVec<[Option<Value>; 16]>;
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Read {
    Key(usize, Key),
    Output(usize, Value),
    Columns(usize, u64, Key),
}
type Reads = SmallVec<[Read; 4]>;
#[derive(Clone, PartialEq, Eq)]
struct Row {
    values: Vec<Value>,
    stamp: u32,
}
/// Partition the existing keyed mirror, without a second membership index.
/// Closure construction visits only its table, while hot keyed reads stay hashed.
#[derive(Default)]
struct TableRows(Vec<FxHashMap<(usize, Key), Arc<Row>>>);
impl TableRows {
    fn get(&self, key: &(usize, Key)) -> Option<&Arc<Row>> {
        self.0.get(key.0)?.get(key)
    }
    fn contains_key(&self, key: &(usize, Key)) -> bool { self.get(key).is_some() }
    fn insert(&mut self, key: (usize, Key), row: Arc<Row>) {
        self.0.resize_with(self.0.len().max(key.0 + 1), FxHashMap::default);
        self.0[key.0].insert(key, row);
    }
    fn remove(&mut self, key: &(usize, Key)) -> Option<Arc<Row>> {
        self.0.get_mut(key.0)?.remove(key)
    }
    fn keys(&self) -> impl Iterator<Item = &(usize, Key)> {
        self.0.iter().flat_map(|t| t.keys())
    }
    fn len(&self) -> usize { self.0.iter().map(|t| t.len()).sum() }
    fn table(&self, t: usize) -> impl Iterator<Item = (&(usize, Key), &Arc<Row>)> {
        self.0.get(t).into_iter().flat_map(|t| t.iter())
    }
}
impl std::ops::Index<&(usize, Key)> for TableRows {
    type Output = Arc<Row>;
    fn index(&self, key: &(usize, Key)) -> &Arc<Row> { self.get(key).unwrap() }
}
impl PartialEq for TableRows {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.0.iter().enumerate().all(|(i, t)| {
            if t.is_empty() { other.0.get(i).is_none_or(|t| t.is_empty()) }
            else { other.0.get(i) == Some(t) }
        })
    }
}
impl Eq for TableRows {}
#[derive(Default, PartialEq, Eq)]
struct ProjectedRows { by_stamp:BTreeMap<u32,BTreeSet<Key>>, len:usize }
impl ProjectedRows {
    fn insert(&mut self,stamp:u32,key:Key){if self.by_stamp.entry(stamp).or_default().insert(key){self.len+=1;}}
    fn remove(&mut self,stamp:u32,key:&Key){
        if let Some(keys)=self.by_stamp.get_mut(&stamp){
            if keys.remove(key){self.len-=1;}
            if keys.is_empty(){self.by_stamp.remove(&stamp);}
        }
    }
    fn keys(&self,since:u32)->impl Iterator<Item=&Key>{self.by_stamp.range(since..).flat_map(|(_,keys)|keys.iter())}
}
#[derive(Default, PartialEq, Eq)]
struct Rows {
    keys: TableRows,
    outputs: FxHashMap<(usize, Value), BTreeSet<Key>>,
    hidden: BTreeSet<(usize, Key)>,
    reverse: BTreeSet<usize>,
    projections: BTreeSet<(usize,u64)>,
    projected: FxHashMap<(usize,u64,Key),ProjectedRows>,
}
fn projected_key(row:&[Value], mask:u64)->Key {
    row.iter().enumerate().filter_map(|(i,v)|(i<64 && mask&(1u64<<i)!=0).then_some(*v)).collect()
}
impl Rows {
    // Traversals used for closure construction retain their previous exact
    // order. Hot keyed reads use one hash lookup, not a tree walk.
    fn table(&self, table: usize) -> Vec<(&(usize, Key), &Arc<Row>)> {
        let mut rows = self
            .keys
            .table(table)
            .collect::<Vec<_>>();
        rows.sort_unstable_by(|a, b| a.0.cmp(b.0));
        rows
    }

    fn change(&mut self, table: usize, key: Key, row: Option<Row>) -> Vec<Read> {
        let id = (table, key.clone());
        if self.keys.get(&id).map(Arc::as_ref) == row.as_ref() {
            return vec![];
        }
        let mut reads = vec![Read::Key(table, key.clone())];
        for &(t,mask) in self.projections.range((table,0)..=(table,u64::MAX)) {
            if let Some(old)=self.keys.get(&id) {
                let k=projected_key(&old.values,mask);
                let bucket=self.projected.get_mut(&(t,mask,k.clone())).unwrap();
                bucket.remove(old.stamp,&key);
                if bucket.len==0{self.projected.remove(&(t,mask,k.clone()));}
                reads.push(Read::Columns(t,mask,k));
            }
            if let Some(new)=&row {
                let k=projected_key(&new.values,mask);
                self.projected.entry((t,mask,k.clone())).or_default().insert(new.stamp,key.clone());
                reads.push(Read::Columns(t,mask,k));
            }
        }
        if let Some(old) = self
            .keys
            .remove(&id)
            .filter(|_| self.reverse.contains(&table))
        {
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
            if self.reverse.contains(&table) {
                reads.push(Read::Output(table, out));
                self.outputs.entry((table, out)).or_default().insert(key);
            }
            self.keys.insert(id, Arc::new(row));
        }
        reads
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
enum Access {
    Key,
    Output,
    Columns(u64),
}
/// Confirmed ports on the single-path prefix of one live seed. Immutable rows are shared
/// with the current mirror, not copied into an application history. An output
/// with several alternatives ends the prefix; its complete branching search is
/// still performed. Rebuild invalidates changed ports; a changed computed key
/// cannot reuse the old row. Independent unchanged operands remain reusable.
struct PortPath {
    seed: Arc<Row>,
    ports: Vec<Option<(Read, Option<Arc<Row>>)>>,
    visited: usize,
    payload: Option<(usize, Vec<(usize, Value)>, u32)>,
}
impl PortPath {
    fn invalidate(&mut self, read: &Read) {
        for port in &mut self.ports {
            if port.as_ref().is_some_and(|(r, _)| r == read) {
                *port = None;
                self.payload = None;
            }
        }
    }
    fn save(&mut self, step: usize, read: Read, row: Option<Arc<Row>>) {
        if step == self.ports.len() {
            self.ports.push(Some((read, row)));
        } else {
            self.ports[step] = Some((read, row));
        }
    }
}
struct Kernel {
    sink: CellActionSink,
    tables: Vec<usize>,
    seed: usize,
    steps: Vec<(usize, Access)>,
    dirty: BTreeSet<Key>,
    dependencies: FxHashMap<Key, Reads>,
    subscribers: FxHashMap<Read, FxHashSet<Key>>,
    executions: usize,
    bindings: usize,
    watch: bool,
    slot_count: usize,
    retire_guard_failures: bool,
    retired: BTreeSet<Key>,
    retired_total: usize,
    noop_union_column: Option<usize>,
    identity_noops: usize,
    reuse_ports: bool,
    paths: FxHashMap<Key, PortPath>,
    reused_reads: usize,
    index_reads: usize,
    resumed_seeds: usize,
    rows_examined: std::cell::Cell<usize>,
    delta_rows_skipped: std::cell::Cell<usize>,
    payload_layout: Option<(usize, Vec<usize>)>,
    payload_resumes: std::cell::Cell<usize>,
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
        Ok(Self::from_wiring(sink, tables, seed, steps))
    }
    fn from_wiring(
        sink: CellActionSink,
        tables: Vec<usize>,
        seed: usize,
        steps: Vec<(usize, Access)>,
    ) -> Self {
        let slot_count = sink.slots().len();
        Self {
            sink,
            tables,
            seed,
            steps,
            dirty: BTreeSet::new(),
            dependencies: FxHashMap::default(),
            subscribers: FxHashMap::default(),
            executions: 0,
            bindings: 0,
            watch: true,
            slot_count,
            retire_guard_failures: false,
            retired: BTreeSet::new(),
            retired_total: 0,
            noop_union_column: None,
            identity_noops: 0,
            reuse_ports: true,
            paths: FxHashMap::default(),
            reused_reads: 0,
            index_reads: 0,
            resumed_seeds: 0,
            rows_examined: std::cell::Cell::new(0),
            delta_rows_skipped: std::cell::Cell::new(0),
            payload_layout: None,
            payload_resumes: std::cell::Cell::new(0),
        }
    }
    fn wake(&mut self, read: &Read) {
        if let Some(seeds) = self.subscribers.get(read) {
            self.dirty.extend(seeds.iter().cloned());
            for seed in seeds {
                if let Some(path) = self.paths.get_mut(seed) {
                    path.invalidate(read);
                }
            }
        }
        if let Read::Key(table, key) = read {
            if *table == self.tables[self.seed] {
                self.paths.remove(key);
                if !self.retire_guard_failures || !self.retired.contains(key) {
                    self.dirty.insert(key.clone());
                }
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
        deps: &mut Reads,
        out: &mut Vec<Vec<Value>>,
        lookups: &mut usize,
        rejections: &mut usize,
    ) -> Result<()> {
        self.read_program_reusing(
            eg, rows, step, b, stamp, watermark, deps, out, lookups, rejections, None, &mut 0,
        )
    }
    fn read_keys<'a>(&self,eg:&EGraph,rows:&Rows,keys:impl Iterator<Item=&'a Key>,table:usize,
        step:usize,b:&Binding,stamp:u32,watermark:u32,deps:&mut Reads,out:&mut Vec<Vec<Value>>,
        lookups:&mut usize,rejections:&mut usize)->Result<()> {
        let a=&self.sink.atoms[self.steps[step].0];
        for key in keys {
            let row=&rows.keys[&(table,key.clone())];
            self.rows_examined.set(self.rows_examined.get()+1);
            let mut next=b.clone();
            if unify(&a.columns,&row.values,&mut next){
                self.read_program(eg,rows,step+1,next,stamp.max(row.stamp),watermark,deps,out,lookups,rejections)?;
            }
        }
        Ok(())
    }
    fn read_program_reusing(
        &self,
        eg: &EGraph,
        rows: &Rows,
        step: usize,
        b: Binding,
        stamp: u32,
        watermark: u32,
        deps: &mut Reads,
        out: &mut Vec<Vec<Value>>,
        lookups: &mut usize,
        rejections: &mut usize,
        mut path: Option<&mut PortPath>,
        reused: &mut usize,
    ) -> Result<()> {
        let mut b = b;
        let mut stamp = stamp;
        let mut step = step;
        if step == 0 && self.payload_layout.is_some() {
            if let Some(p) = path.as_mut() {
                if let Some((prefix, values, old_stamp)) = &p.payload {
                    let required = self.sink.atoms[self.seed].columns.iter()
                        .chain(self.steps[..*prefix].iter()
                            .flat_map(|(i, _)| self.sink.atoms[*i].columns.iter()))
                        .filter_map(|x| match x { CellOperand::Slot(i) => Some(*i), _ => None })
                        .collect::<BTreeSet<_>>();
                    let complete = required.iter().all(|i|
                        b[*i].is_some() || values.iter().any(|(slot, _)| slot == i));
                    if complete && p.ports.len() >= *prefix
                        && p.ports[..*prefix].iter().all(Option::is_some)
                    {
                        for (i, v) in values { b[*i] = Some(*v); }
                        for port in &p.ports[..*prefix] {
                            deps.push(port.as_ref().unwrap().0.clone());
                        }
                        stamp = stamp.max(*old_stamp);
                        step = *prefix;
                        p.visited = *prefix;
                        self.payload_resumes.set(self.payload_resumes.get() + 1);
                    }
                }
            }
        }
        loop {
            if let (Some((prefix, slots)), Some(p)) = (&self.payload_layout, path.as_mut()) {
                if step > 0 && step <= *prefix && p.ports.len() >= step
                    && p.ports[..step].iter().all(Option::is_some)
                {
                    p.payload = Some((step,
                        slots.iter().filter_map(|i| b[*i].map(|v| (*i, v))).collect(), stamp));
                }
            }
            if step == self.steps.len() {
                if stamp >= watermark {
                    let b = b
                        .into_iter()
                        .collect::<Option<Vec<_>>>()
                        .ok_or("unbound interface")?;
                    if self.sink.native_residuals || eg.cell_guards_hold(&self.sink, &b)? {
                        out.push(b)
                    } else {
                        *rejections += 1
                    }
                }
                return Ok(());
            }
            let (i, access) = self.steps[step];
            let a = &self.sink.atoms[i];
            let table = self.tables[i];
            let n = a.columns.len() - 1;
            let read = match access {
                Access::Key => {
                    Read::Key(table, a.columns[..n].iter().map(|x| value(x, &b)).collect())
                }
                Access::Output => Read::Output(table, value(&a.columns[n], &b)),
                Access::Columns(mask)=>Read::Columns(table,mask,a.columns.iter().enumerate().filter_map(|(i,o)|
                    (mask&(1u64<<i)!=0).then(||value(o,&b))).collect()),
            };
            deps.push(read.clone());
            // Committed notifications invalidate changed ports. Recompute the
            // key from current bindings before reusing a row: an x update may
            // change later keys but need not invalidate an independent y port.
            if let Some(p) = path.as_mut() {
                p.visited = step + 1;
            }
            if let Some((old_read, row)) = path
                .as_ref()
                .and_then(|p| p.ports.get(step))
                .and_then(Option::as_ref)
            {
                if old_read == &read {
                    *reused += 1;
                    let Some(row) = row else { return Ok(()) };
                    self.rows_examined.set(self.rows_examined.get()+1);
                    if !unify(&a.columns, &row.values, &mut b) {
                        return Ok(());
                    }
                    stamp = stamp.max(row.stamp);
                    step += 1;
                    continue;
                }
            }
            *lookups += 1;
            if let Read::Columns(_,mask,key)=&read {
                if step+1==self.steps.len() && stamp<watermark {
                    // Every preceding witness is old. Only the terminal port's
                    // delta can create a new binding. Do not cache this filtered
                    // subset as if it were the complete current port.
                    if let Some(p)=path.as_mut(){p.visited=step;}
                    if let Some(bucket)=rows.projected.get(&(table,*mask,key.clone())) {
                        let mut visited=0;
                        for key in bucket.keys(watermark) {
                            visited+=1;self.rows_examined.set(self.rows_examined.get()+1);
                            let row=&rows.keys[&(table,key.clone())];let mut next=b.clone();
                            if unify(&a.columns,&row.values,&mut next){self.read_program(eg,rows,step+1,next,stamp.max(row.stamp),watermark,deps,out,lookups,rejections)?;}
                        }
                        self.delta_rows_skipped.set(self.delta_rows_skipped.get()+bucket.len-visited);
                    }
                    return Ok(());
                }
            }
            let row = match &read {
                Read::Key(_, k) => rows.keys.get(&(table, k.clone())),
                Read::Output(_,v) => {
                    let Some(keys) = rows.outputs.get(&(table,*v)) else {
                        if let Some(p) = path.as_mut() {
                            p.save(step, read, None);
                        }
                        return Ok(());
                    };
                    if keys.len() == 1 {
                        rows.keys.get(&(table, keys.first().unwrap().clone()))
                    } else {
                        if let Some(p) = path.as_mut() {
                            p.visited = step;
                        }
                        self.read_keys(eg,rows,keys.iter(),table,step,&b,stamp,watermark,deps,out,lookups,rejections)?;
                        return Ok(());
                    }
                }
                Read::Columns(_,mask,key)=>{
                    let Some(keys)=rows.projected.get(&(table,*mask,key.clone())) else{
                        if let Some(p)=path.as_mut(){p.save(step,read,None);}
                        return Ok(());
                    };
                    if keys.len==1 {rows.keys.get(&(table,keys.keys(0).next().unwrap().clone()))}
                    else{
                        if let Some(p)=path.as_mut(){p.visited=step;}
                        self.read_keys(eg,rows,keys.keys(0),table,step,&b,stamp,watermark,deps,out,lookups,rejections)?;
                        return Ok(());
                    }
                }
            };
            if let Some(p) = path.as_mut() {
                p.save(step, read, row.cloned());
            }
            let Some(row) = row else { return Ok(()) };
            self.rows_examined.set(self.rows_examined.get()+1);
            if !unify(&a.columns, &row.values, &mut b) {
                return Ok(());
            }
            stamp = stamp.max(row.stamp);
            step += 1;
        }
    }

    fn install_dependencies(&mut self, seed: Key, deps: Option<Reads>) {
        if self.dependencies.get(&seed) == deps.as_ref() {
            return;
        }
        let old = self.dependencies.remove(&seed).unwrap_or_default();
        let empty = Reads::new();
        let new = deps.as_ref().unwrap_or(&empty);
        for d in old.iter().filter(|d| new.binary_search(d).is_err()) {
            if let Some(s) = self.subscribers.get_mut(d) {
                s.remove(&seed);
                if s.is_empty() {
                    self.subscribers.remove(d);
                }
            }
        }
        for d in new.iter().filter(|d| old.binary_search(d).is_err()) {
            self.subscribers
                .entry(d.clone())
                .or_default()
                .insert(seed.clone());
        }
        if let Some(deps) = deps {
            self.dependencies.insert(seed, deps);
        }
    }
    fn matches(
        &mut self,
        eg: &EGraph,
        rows: &Rows,
        lookups: &mut usize,
    ) -> Result<Vec<Vec<Value>>> {
        let mut out = vec![];
        let reads_before = *lookups;
        let watermark = eg.cell_query_watermark(&self.sink);
        for seed in std::mem::take(&mut self.dirty) {
            self.executions += 1;
            let Some(row) = rows.keys.get(&(self.tables[self.seed], seed.clone())) else {
                self.paths.remove(&seed);
                self.install_dependencies(seed, None);
                continue;
            };
            if self
                .noop_union_column
                .is_some_and(|column| row.values[column] == *row.values.last().unwrap())
            {
                self.identity_noops += 1;
                self.paths.remove(&seed);
                self.install_dependencies(seed, None);
                continue;
            }
            let mut b = smallvec![None; self.slot_count];
            if !unify(&self.sink.atoms[self.seed].columns, &row.values, &mut b) {
                self.paths.remove(&seed);
                self.install_dependencies(seed, None);
                continue;
            }
            let mut deps = Reads::new();
            let before = out.len();
            let mut rejections = 0;
            let mut path = (self.reuse_ports && !self.steps.is_empty()).then(|| {
                self.paths
                    .remove(&seed)
                    .filter(|p| Arc::ptr_eq(&p.seed, row))
                    .unwrap_or_else(|| PortPath {
                        seed: row.clone(),
                        ports: vec![],
                        visited: 0,
                        payload: None,
                    })
            });
            if let Some(p) = path.as_mut() {
                p.visited = 0;
            }
            let mut reused = 0;
            self.read_program_reusing(
                eg,
                rows,
                0,
                b,
                row.stamp,
                watermark,
                &mut deps,
                &mut out,
                lookups,
                &mut rejections,
                path.as_mut(),
                &mut reused,
            )?;
            self.reused_reads += reused;
            self.resumed_seeds += usize::from(reused > 0);
            if let Some(p) = path.as_mut() {
                // Unvisited ports no longer have subscriptions. Never retain
                // them across an early failed/missing read or a new branch.
                p.ports.truncate(p.visited);
            }
            if self.retire_guard_failures && out.len() == before && rejections > 0 {
                deps.clear();
                self.retired.insert(seed.clone());
                self.retired_total += 1;
            } else if let Some(path) = path.filter(|p| !p.ports.is_empty()) {
                self.paths.insert(seed.clone(), path);
            }
            deps.sort_unstable();
            deps.dedup();
            self.install_dependencies(seed, Some(deps));
        }
        out.sort_unstable();
        if out.windows(2).any(|p| p[0] == p[1]) {
            return Err("duplicate fixed-program binding".into());
        }
        self.index_reads += *lookups - reads_before;
        self.bindings += out.len();
        Ok(out)
    }
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub program_compilations: usize,
    pub loaded_programs: usize,
    pub ruleset_runs: usize,
    pub keyed_reads: usize,
    pub committed_row_events: usize,
    pub union_events: usize,
    pub bootstrap_rows: usize,
    pub native_fallback_runs: usize,
    pub frontier_seconds: f64,
    pub effect_seconds: f64,
    pub ingest_seconds: f64,
}
pub struct Runtime {
    eg: EGraph,
    kernels: Vec<Kernel>,
    wake_tables: Vec<Vec<usize>>,
    sets: BTreeMap<String, Vec<usize>>,
    names: Vec<String>,
    feed: CellCommitFeed,
    rows: Rows,
    identity: u64,
    failed: bool,
    epoch: u64,
    joint: Option<joint::Joint>,
    comparison: Option<comparison::Scope>,
    scalar: BTreeMap<String, scalar::Group>,
    boundary: BTreeMap<String,Vec<usize>>,
    forwarded: Option<Vec<CellCommitBatch>>,
    pub last_updated: bool,
    pub stats: Stats,
}
impl Runtime {
    pub fn new(eg: EGraph, sets: &[&str]) -> Result<Self> {
        Self::new_with_seeds(eg, sets, &BTreeMap::new())
    }
    fn new_with_seeds(eg: EGraph, sets: &[&str], seeds: &BTreeMap<String, String>) -> Result<Self> {
        Self::new_with_scalar(eg, sets, seeds, vec![])
    }
    fn new_with_scalar(
        eg: EGraph,
        sets: &[&str],
        seeds: &BTreeMap<String, String>,
        specs: Vec<scalar::Spec>,
    ) -> Result<Self> {
        Self::new_with_plans(eg, sets, seeds, specs, None)
    }
    fn new_with_plans(
        eg: EGraph,
        sets: &[&str],
        seeds: &BTreeMap<String, String>,
        specs: Vec<scalar::Spec>,
        plans: Option<&[artifact::Program]>,
    ) -> Result<Self> {
        Self::new_with_boundaries(eg,sets,seeds,specs,plans,&[])
    }
    fn new_with_boundaries(
        mut eg:EGraph,sets:&[&str],seeds:&BTreeMap<String,String>,specs:Vec<scalar::Spec>,
        plans:Option<&[artifact::Program]>,boundaries:&[boundary::Spec],
    )->Result<Self>{
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
        let mut scalar = BTreeMap::<String, scalar::Group>::new();
        let mut scalar_seeds = BTreeMap::new();
        for spec in specs {
            names.insert(spec.field.clone());
            let sink = eg.prepare_cell_candidate_action(&spec.set, &spec.name)?;
            if !eg.cell_query_is_seminaive(&sink) {
                return Err("scalar transfer requires seminaive rule".into());
            }
            names.extend(sink.atoms.iter().map(|a| a.function.clone()));
            let plan = scalar::Plan::compile(&spec, &sink)?;
            scalar_seeds.insert(sinks.len(), spec.seed.clone());
            scalar
                .entry(spec.set)
                .or_default()
                .plans
                .push((sinks.len(), plan));
            sinks.push(sink);
        }
        for (set, g) in &mut scalar {
            let selected = g
                .plans
                .iter()
                .map(|(id, _)| sinks[*id].name.clone())
                .collect::<BTreeSet<_>>();
            g.others = eg
                .cell_rule_names(set)?
                .into_iter()
                .filter(|n| !selected.contains(n))
                .collect();
        }
        let mut supplied=groups.iter().flat_map(|(set,ids)|ids.iter().map(|i|(set.clone(),sinks[*i].name.clone())))
            .chain(scalar.iter().flat_map(|(set,g)|g.plans.iter().map(|(i,_)|(set.clone(),sinks[*i].name.clone())))).collect::<BTreeSet<_>>();
        let mut boundary=BTreeMap::<String,Vec<usize>>::new();
        let mut boundary_seeds=BTreeMap::new();
        for spec in boundaries {
            if !supplied.insert((spec.ruleset.clone(),spec.rule.clone())){return Err("boundary rule is already supplied by a Cell program".into())}
            let sink=eg.prepare_cell_action(&spec.ruleset,&spec.rule)?;
            if !eg.cell_query_is_seminaive(&sink) || !sink.primitives.is_empty() || sink.native_residuals {
                return Err("boundary program requires a seminaive structural query".into())
            }
            names.extend(sink.atoms.iter().map(|a|a.function.clone()));
            boundary_seeds.insert(sinks.len(),spec.clone());
            boundary.entry(spec.ruleset.clone()).or_default().push(sinks.len());sinks.push(sink);
        }
        let names = names.into_iter().collect::<Vec<_>>();
        if plans.is_some_and(|p| p.len() != sinks.len()) {
            return Err("transfer must cover every installed rule exactly once".into());
        }
        let mut used_plans = BTreeSet::new();
        let mut kernels = vec![];
        for (id, sink) in sinks.into_iter().enumerate() {
            let tables = sink
                .atoms
                .iter()
                .map(|a| names.iter().position(|n| n == &a.function).unwrap())
                .collect();
            let seed = scalar_seeds
                .get(&id)
                .or_else(|| seeds.get(&sink.name))
                .map(String::as_str);
            if let Some(spec)=boundary_seeds.get(&id) {
                kernels.push(boundary::install(&eg,sink,tables,spec)?);
            } else if let Some(plans) = plans {
                let (index, plan) = plans
                    .iter()
                    .enumerate()
                    .find(|(_, p)| {
                        p.rule == sink.name
                            && groups.get(&p.ruleset).is_some_and(|ids| ids.contains(&id))
                    })
                    .ok_or("missing transferred rule or mismatched ruleset")?;
                if !used_plans.insert(index) {
                    return Err("duplicate transferred rule".into());
                }
                kernels.push(plan.instantiate(&eg, sink, tables)?);
            } else {
                kernels.push(Kernel::compile(&eg, sink, tables, seed)?);
            }
        }
        let mut wake_tables = vec![vec![]; names.len()];
        let mut reverse = BTreeSet::new();
        let mut projections=BTreeSet::new();
        for (id, k) in kernels.iter().enumerate() {
            for table in k.tables.iter().copied().collect::<BTreeSet<_>>() {
                wake_tables[table].push(id)
            }
            for (atom, access) in &k.steps {
                if matches!(access, Access::Output) {
                    reverse.insert(k.tables[*atom]);
                }
                if let Access::Columns(mask)=access {projections.insert((k.tables[*atom],*mask));}
            }
        }
        let identity = eg.cell_rule_environment_stamp();
        let (feed, initial) = eg.subscribe_cell_commits(&names)?;
        let mut r = Self {
            eg,
            kernels,
            wake_tables,
            sets: groups,
            names,
            feed,
            rows: Rows {
                reverse,
                projections,
                ..Rows::default()
            },
            identity,
            failed: false,
            epoch: 0,
            joint: None,
            comparison: None,
            scalar,
            boundary,
            forwarded: None,
            last_updated: false,
            stats: Stats {
                program_compilations: usize::from(plans.is_none()),
                loaded_programs: plans.map_or(0, |p| p.len()),
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
    /// Diagnostic ablation: disable saved port wiring without changing rule
    /// selection, schedules, bindings or native effects.
    pub fn set_port_reuse(&mut self, enabled: bool) {
        for kernel in &mut self.kernels {
            kernel.reuse_ports = enabled;
            kernel.paths.clear();
        }
    }
    fn ingest(&mut self, mut batch: CellCommitBatch) -> Result<()> {
        if batch.rows.is_empty() && batch.clears.is_empty() && batch.unions.is_empty() {
            return Ok(());
        }
        let forward = self.forwarded.is_some();
        if let Some(mut proof) = self.joint.take() {
            proof.observes(self, &batch);
            self.joint = Some(proof);
        }
        let has_union = !batch.unions.is_empty();
        self.stats.committed_row_events += batch.rows.len();
        self.stats.union_events += batch.unions.len();
        let mut changed = vec![];
        let mut hidden_changed = false;
        for &t in &batch.clears {
            let before = self.rows.hidden.len();
            self.rows.hidden.retain(|(table, _)| *table != t);
            hidden_changed |= before != self.rows.hidden.len();
            let keys = self
                .rows
                .keys
                .table(t)
                .map(|((_, k), _)| k.clone())
                .collect::<Vec<_>>();
            for k in keys {
                let reads = self.rows.change(t, k, None);
                changed.extend(reads);
            }
        }
        for delta in &mut batch.rows {
            let t = delta.table;
            let key = (t, Key::from_slice(&delta.key));
            hidden_changed |= if delta.row.is_some() && !delta.visible {
                self.rows.hidden.insert(key)
            } else {
                self.rows.hidden.remove(&key)
            };
            let row = if delta.visible {
                // Only the mirror's live payload needs a second owner. Move
                // the original event, including its keys, to the GPU consumer.
                (if forward { delta.row.clone() } else { delta.row.take() }).map(|mut values| {
                    values.truncate(self.feed.tables[t].keys + 1);
                    Row {
                        values,
                        stamp: delta.timestamp.expect("visible native row timestamp"),
                    }
                })
            } else {
                None
            };
            let reads = self.rows.change(t, Key::from_slice(&delta.key), row);
            changed.extend(reads);
        }
        if has_union || hidden_changed || !changed.is_empty() {
            self.epoch = self
                .epoch
                .checked_add(1)
                .ok_or("joint generation exhausted")?;
        }
        {
            for read in changed {
                let table = match &read {
                    Read::Key(t, _) | Read::Output(t, _) | Read::Columns(t,..) => *t,
                };
                for id in &self.wake_tables[table] {
                    let k = &mut self.kernels[*id];
                    if k.watch {
                        k.wake(&read)
                    }
                }
            }
        }
        if let Some(out) = &mut self.forwarded { out.push(batch); }
        Ok(())
    }
    fn sync(&mut self) -> Result<()> {
        let started = std::time::Instant::now();
        let batch = self.eg.drain_cell_commits(&mut self.feed)?;
        // Native rebuild journals contain old-key removals and canonical row
        // replacements. Those changes also wake bindings that previously missed
        // a key/output lookup; no new concrete Cell or whole-table rescan.
        let result = self.ingest(batch);
        self.stats.ingest_seconds += started.elapsed().as_secs_f64();
        result
    }
    /// Independent diagnostic only, outside the update path. Includes row
    /// timestamps, visibility and every reverse-index entry, not just counts.
    pub fn audit_mirror(&self) -> Result<usize> {
        let mut expected = Rows {
            reverse: self.rows.reverse.clone(),
            ..Rows::default()
        };
        for (i, name) in self.names.iter().enumerate() {
            let n = self.feed.tables[i].keys;
            self.eg.function_for_each(name, |r| {
                if r.subsumed {
                    expected
                        .hidden
                        .insert((i, SmallVec::from_slice(&r.vals[..n])));
                }
                if !r.subsumed {
                    let values = r.vals[..n + 1].to_vec();
                    expected.change(
                        i,
                        SmallVec::from_slice(&values[..n]),
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
                if r.until.is_none() && self.scalar.contains_key(&r.ruleset) =>
            {
                Ok(self.run_scalar(&r.ruleset)?.updated)
            }
            GenericSchedule::Run(_, r)
                if self.sets.contains_key(&r.ruleset) && r.until.is_none() =>
            {
                self.sync()?;
                let started = std::time::Instant::now();
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
                self.stats.frontier_seconds += started.elapsed().as_secs_f64();
                let started = std::time::Instant::now();
                let report = self.eg.apply_completed_cell_ruleset(&batches)?;
                self.stats.effect_seconds += started.elapsed().as_secs_f64();
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
                if self.comparison.is_some() && comparison::Scope::accepts(x) {
                    let mut scope = self.comparison.take().unwrap();
                    let result = scope.run(self);
                    self.comparison = Some(scope);
                    return result;
                }
                if self.enter_scalar_scope(x) {
                    let result = (|| {
                        let mut c = false;
                        loop {
                            let d = self.step(x)?;
                            c |= d;
                            if !d {
                                break;
                            }
                        }
                        Ok(c)
                    })();
                    self.leave_scalar_scope(x);
                    return result;
                }
                if self.joint.as_ref().is_some_and(|j| j.accepts(x)) {
                    let mut proof = self.joint.take().unwrap();
                    let result = (|| {
                        if let Some(updated) = proof.try_close(self)? {
                            return Ok(updated);
                        }
                        proof.run_dataflow(self, x)
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
        if self.eg.cell_rule_environment_stamp() != self.identity {
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
        // Scalar programs are not in `sets`. Omitting them hid the main
        // ordinary-Run query provider and made per-Cell reuse misleading.
        let owners = self.sets.iter().flat_map(|(set,ids)|ids.iter().map(move |id|(*id,(set,"dataflow"))))
            .chain(self.scalar.iter().flat_map(|(set,g)|g.plans.iter().map(move |(id,_)|(*id,(set,"scalar_query")))))
            .chain(self.boundary.iter().flat_map(|(set,ids)|ids.iter().map(move |id|(*id,(set,"boundary_query")))))
            .collect::<BTreeMap<_,_>>();
        let programs=self.kernels.iter().enumerate().map(|(id,k)| {
            let (set,kind)=owners[&id];
            json!({"program_id":id,"kind":kind,"ruleset":set,"rule":k.sink.name,
                "seed":k.sink.atoms[k.seed].function,
                "accesses":k.steps.iter().map(|(i,a)|json!({"table":k.sink.atoms[*i].function,"access":a})).collect::<Vec<_>>(),
                "seed_evaluations":k.executions,"emitted_bindings":k.bindings,
                "live_seed_instances":k.dependencies.len(),"retired_guard_seeds":k.retired_total,
                "identity_noop_seeds":k.identity_noops,"index_reads":k.index_reads,
                "candidate_rows":k.rows_examined.get(),
                "terminal_delta_rows_skipped":k.delta_rows_skipped.get(),
                "payload_layout":k.payload_layout,"payload_resumes":k.payload_resumes.get(),
                "reused_port_reads":k.reused_reads,"resumed_seed_evaluations":k.resumed_seeds,
                "stored_port_paths":k.paths.len()})
        }).collect::<Vec<_>>();
        json!({"stats":self.stats,"scalar_cells":self.scalar.iter().map(|(set,g)|json!({"ruleset":set,"rules":g.plans.len(),"stats":g.stats,"scope_allowed":g.scope.allowed,"closure":g.scope.stats})).collect::<Vec<_>>(),"joint":self.joint.as_ref().map(|j|j.report()),"program_telemetry_version":2,"programs":programs,"port_reuse":{"enabled":self.kernels.iter().all(|k|k.reuse_ports),"index_reads":self.kernels.iter().map(|k|k.index_reads).sum::<usize>(),"reused_reads":self.kernels.iter().map(|k|k.reused_reads).sum::<usize>(),"resumed_seed_evaluations":self.kernels.iter().map(|k|k.resumed_seeds).sum::<usize>(),"stored_paths":self.kernels.iter().map(|k|k.paths.len()).sum::<usize>(),"shared_row_handles":self.kernels.iter().flat_map(|k|k.paths.values()).map(|p|p.ports.iter().filter(|x|x.as_ref().is_some_and(|(_,r)|r.is_some())).count()).sum::<usize>()},"watched_rows":self.rows.keys.len(),"dependency_edges":self.kernels.iter().map(|k|k.subscribers.values().map(|s|s.len()).sum::<usize>()).sum::<usize>(),"concrete_closure_catalog_entries":0,"scope":"Parameterized native update program with optional guarded joint saturation transfers. Original schedule and native effects; unsupported boundary states use the ordinary parameterized path. Not a GPU benchmark."})
    }
}
