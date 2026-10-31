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
    values: BTreeMap<usize, Value>,
    epoch: u64,
    parent: Option<FactRef>,
}
pub struct Executor {
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
    profile_queries: bool,
    query_profile: Vec<(u64, f64, u64)>,
    progress: bool,
}
/// Run-local binding workspace. Only successful matches become stored bindings;
/// failed join branches undo slot assignments instead of cloning tree maps.
struct QueryEnv {
    values: Vec<Option<Value>>,
    trail: Vec<usize>,
}
impl QueryEnv {
    fn new(slots: usize) -> Self {
        Self {
            values: vec![None; slots],
            trail: vec![],
        }
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
    let v = canon(v, &g.parents);
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
        let mut e = Self {
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
            query_indexes: QueryIndexes::default(),
            profile_queries: std::env::var_os("CELL_CPU_PROFILE").is_some(),
            query_profile: vec![(0, 0.0, 0); n],
            progress: std::env::var_os("CELL_CPU_PROGRESS").is_some(),
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
        for event in std::mem::take(&mut self.graph.events) {
            if let Ok(Some(row)) = self.graph.row(event) {
                for &rid in &self.readers[row.table] {
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
        remaining: &mut [bool],
        env: &mut QueryEnv,
        epoch: u64,
        out: &mut Vec<Binding>,
        delta: Option<(usize, u64)>,
    ) -> Result<()> {
        if !remaining.iter().any(|pending| *pending) {
            out.push(Binding {
                values: env.binding(),
                epoch,
                parent: None,
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
                if let Some(v) = value::primitive(name, &vals, output)? {
                    let mark = env.trail.len();
                    if bind(atom.args.last().unwrap(), &v, env, &self.graph) {
                        self.solve(query, remaining, env, epoch, out, delta)?;
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
                    self.graph.stats.residual_candidates += 1;
                    self.graph.stats.decoded_rows += 1;
                    let Some(r) = self.graph.row(w)? else {
                        continue;
                    };
                    // Disjoint delta expansion: the pivot is the first changed
                    // table atom in source order. Earlier atoms must be old.
                    if r.hidden
                        || delta.is_some_and(|(pivot, watermark)| {
                            chosen < pivot && r.epoch >= watermark
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
                        self.solve(query, remaining, env, epoch.max(r.epoch), out, delta)?;
                    }
                    env.rollback(mark);
                }
            }
        }
        remaining[chosen] = true;
        Ok(())
    }
    fn matches(&mut self, rid: usize, probe: bool) -> Result<Vec<Binding>> {
        let rule = self.graph.program.rules[rid].clone();
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
                let mut env =
                    QueryEnv::new(self.graph.program.templates[rule.template].sorts.len());
                if atom
                    .args
                    .iter()
                    .zip(&row.values)
                    .all(|(a, v)| bind(a, v, &mut env, &self.graph))
                {
                    let n = found.len();
                    self.solve(
                        &rule.query,
                        &mut (0..rule.query.len())
                            .map(|i| i != anchor)
                            .collect::<Vec<_>>(),
                        &mut env,
                        row.epoch,
                        &mut found,
                        None,
                    )?;
                    if let Dependency::Fact(f) = row.witness {
                        for b in &mut found[n..] {
                            b.parent = Some(f)
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
                    self.graph.stats.residual_candidates += 1;
                    self.graph.stats.decoded_rows += 1;
                    let mut env =
                        QueryEnv::new(self.graph.program.templates[rule.template].sorts.len());
                    if atom
                        .args
                        .iter()
                        .zip(&row.values)
                        .all(|(a, v)| bind(a, v, &mut env, &self.graph))
                    {
                        self.solve(
                            &rule.query,
                            &mut (0..rule.query.len())
                                .map(|i| i != pivot)
                                .collect::<Vec<_>>(),
                            &mut env,
                            row.epoch,
                            &mut found,
                            Some((pivot, watermark)),
                        )?;
                    }
                }
            }
        } else {
            self.solve(
                &rule.query,
                &mut vec![true; rule.query.len()],
                &mut QueryEnv::new(self.graph.program.templates[rule.template].sorts.len()),
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
                unique.entry(key).or_insert(b);
            }
        }
        Ok(unique.into_values().collect())
    }
    fn apply(&mut self, rid: usize, b: Binding) -> Result<()> {
        let template = self.graph.program.rules[rid].template;
        let cell = self.graph.linked_frame(template, b.values, b.parent)?;
        let ops = self.graph.program.templates[template].ops.clone();
        for (i, op) in ops.iter().enumerate() {
            match op {
                Op::Copy(..) => {}
                Op::Let(dst, Call::Table(t), aa) => {
                    if self.graph.program.tables[*t].virtual_fact {
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
                    let v = value::primitive(&name, &args, &output)?
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
                        break;
                    }
                }
                Op::Panic => return Err("source panic".into()),
            }
        }
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
        }
        let matches = work.len();
        let search_seconds = start.elapsed().as_secs_f64();
        if self.audit {
            self.binding_audit.push(json!({"ruleset":set,"bindings":work.iter().map(|(rid,b)|{let template=&self.graph.program.templates[self.graph.program.rules[*rid].template];json!({"rule":rid,"values":b.values.iter().map(|(i,v)|json!({"slot":i,"value":crate::value::exported(v,&template.sorts[*i])})).collect::<Vec<_>>()})}).collect::<Vec<_>>()}));
        }
        let phase = Instant::now();
        for (rid, b) in work {
            self.apply(rid, b)?;
        }
        let rhs_seconds = phase.elapsed().as_secs_f64();
        self.graph.stats.bindings += matches as u64;
        let phase = Instant::now();
        self.graph.repair()?;
        let repair_seconds = phase.elapsed().as_secs_f64();
        let phase = Instant::now();
        if std::env::var("CELL_CPU_QUERY_INDEX_SCOPE").as_deref() == Ok("table") {
            self.query_indexes.invalidate(&self.graph);
            self.graph.query_changes.clear();
        } else {
            self.query_indexes.update(&mut self.graph)?;
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
        let mut c = json!({"schema":"eggfuse-cell-checkpoint/v1","program":self.graph.program.artifact,"graph":self.graph.image(),"tasks":self.tasks,"watermarks":self.watermarks,"steps":self.steps});
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
        e.tasks = serde_json::from_value(checkpoint["tasks"].clone()).map_err(|x| x.to_string())?;
        e.watermarks =
            serde_json::from_value(checkpoint["watermarks"].clone()).map_err(|x| x.to_string())?;
        e.steps = serde_json::from_value(checkpoint["steps"].clone()).map_err(|x| x.to_string())?;
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
        json!({"query_profile": if self.profile_queries {Some(self.query_profile.iter().enumerate().filter(|(_,v)|v.0>0).map(|(rid,v)|json!({"rule":rid,"name":self.graph.program.templates[self.graph.program.rules[rid].template].name,"calls":v.0,"seconds":v.1,"candidates":v.2})).collect::<Vec<_>>())}else{None},"query_indexes":{"builds":self.query_indexes.builds,"decoded_facts":self.query_indexes.decoded_facts,"updated_facts":self.query_indexes.updated_facts,"peak_fact_references":self.query_indexes.peak_references,"scope":"versioned virtual-fact indexes; references only"},"query_mode":std::env::var("CELL_CPU_QUERY_MODE").unwrap_or_else(|_|"delta".into()),"backend":"cell-cpu","scope":"standalone Cell recipe executor; native frontend only; no egglog runtime linked","stats":self.graph.stats,"live_cells":self.graph.cells.len(),"stored_binding_values":self.graph.cells.values().map(|c|c.values.len()).sum::<usize>(),"virtual_constructor_facts":self.graph.cells.values().map(|c|c.facts.len()).sum::<usize>(),"function_ports":self.graph.functions.iter().filter(|f|f.live).count(),"promoted_aliases":self.graph.parents.len(),"steps":self.steps,"status":if self.tasks.is_empty(){"complete"}else{"checkpointed"}})
    }
}
