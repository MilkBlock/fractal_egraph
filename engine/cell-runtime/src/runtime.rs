use crate::{
    graph::*,
    program::*,
    value::{self, Value, canon},
};
use serde_json::{Value as J, json};
use crate::schedule::{self, Task};
use std::collections::{BTreeMap, BTreeSet};
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
}
fn operand(a: &Arg, env: &BTreeMap<usize, Value>, g: &Graph) -> Option<Value> {
    match a {
        Arg::Literal(v) => Some(canon(v, &g.parents)),
        Arg::Slot(i) => env.get(i).map(|v| canon(v, &g.parents)),
    }
}
fn bind(a: &Arg, v: &Value, env: &mut BTreeMap<usize, Value>, g: &Graph) -> bool {
    let v = canon(v, &g.parents);
    match a {
        Arg::Literal(l) => canon(l, &g.parents) == v,
        Arg::Slot(i) => {
            if let Some(old) = env.get(i) {
                canon(old, &g.parents) == v
            } else {
                env.insert(*i, v);
                true
            }
        }
    }
}
impl Executor {
    pub fn new(graph: Graph, audit: bool) -> Self {
        let mut readers = vec![vec![]; graph.program.tables.len()];
        for (rid, r) in graph.program.rules.iter().enumerate() {
            for a in &r.query {
                if let Call::Table(t) = a.call {
                    if !readers[t].contains(&rid) {
                        readers[t].push(rid)
                    }
                }
            }
        }
        let n = graph.program.rules.len();
        let tasks=schedule::initial(&graph.program.control);
        let initial_audit=if audit {graph.export().ok()} else {None};
        let mut e = Self {
            graph,
            steps: vec![],
            watermarks: vec![0; n],
            dirty: (0..n).collect(),
            pending: vec![BTreeSet::new(); n],
            readers,
            audit,
            snapshots: vec![],
            binding_audit: vec![],initial_audit,
            tasks,
        };
        e.deliver();
        e
    }
    fn deliver(&mut self) {
        for event in std::mem::take(&mut self.graph.events) {
            if let Ok(Some(row)) = self.graph.row(event) {
                for &rid in &self.readers[row.table] {
                    self.dirty.insert(rid);
                    if self.graph.program.rules[rid].direct.is_some() {
                        self.pending[rid].insert(event);
                    }
                }
            }
        }
    }
    fn solve(
        &mut self,
        query: &[Atom],
        remaining: Vec<usize>,
        env: BTreeMap<usize, Value>,
        epoch: u64,
        out: &mut Vec<Binding>,
    ) -> Result<()> {
        if remaining.is_empty() {
            out.push(Binding {
                values: env,
                epoch,
                parent: None,
            });
            return Ok(());
        }
        let chosen = remaining
            .iter()
            .filter_map(|&i| {
                let a = &query[i];
                match a.call {
                    Call::Primitive(..) => a.args[..a.args.len() - 1]
                        .iter()
                        .all(|v| operand(v, &env, &self.graph).is_some())
                        .then_some((0usize, i)),
                    Call::Table(t) => {
                        let bound = a.args[..a.args.len() - 1]
                            .iter()
                            .all(|v| operand(v, &env, &self.graph).is_some());
                        Some((
                            if bound {
                                1
                            } else {
                                self.graph.witnesses(t).len() + 2
                            },
                            i,
                        ))
                    }
                }
            })
            .min()
            .map(|(_, i)| i)
            .ok_or("unbound primitive query cycle")?;
        let rest = remaining
            .into_iter()
            .filter(|i| *i != chosen)
            .collect::<Vec<_>>();
        let atom = &query[chosen];
        match &atom.call {
            Call::Primitive(name, output) => {
                let vals = atom.args[..atom.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, &env, &self.graph).ok_or("unbound primitive input"))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                if let Some(v) = value::primitive(name, &vals, output)? {
                    let mut e = env;
                    if bind(atom.args.last().unwrap(), &v, &mut e, &self.graph) {
                        self.solve(query, rest, e, epoch, out)?;
                    }
                }
            }
            Call::Table(t) => {
                let key = atom.args[..atom.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, &env, &self.graph))
                    .collect::<Option<Vec<_>>>();
                let witnesses = if let Some(key) = key {
                    self.graph
                        .lookup(*t, &key)?
                        .into_iter()
                        .map(|r| r.witness)
                        .collect()
                } else {
                    self.graph.witnesses(*t)
                };
                for w in witnesses {
                    self.graph.stats.residual_candidates += 1;
                    self.graph.stats.decoded_rows += 1;
                    let Some(r) = self.graph.row(w)? else {
                        continue;
                    };
                    if r.hidden {
                        continue;
                    }
                    let mut e = env.clone();
                    if atom
                        .args
                        .iter()
                        .zip(&r.values)
                        .all(|(a, v)| bind(a, v, &mut e, &self.graph))
                    {
                        self.solve(query, rest.clone(), e, epoch.max(r.epoch), out)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn matches(&mut self, rid: usize, probe: bool) -> Result<Vec<Binding>> {
        let rule = self.graph.program.rules[rid].clone();
        if !probe && rule.seminaive && !self.dirty.remove(&rid) {
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
                let mut env = BTreeMap::new();
                if atom
                    .args
                    .iter()
                    .zip(&row.values)
                    .all(|(a, v)| bind(a, v, &mut env, &self.graph))
                {
                    let n = found.len();
                    self.solve(
                        &rule.query,
                        (0..rule.query.len()).filter(|i| *i != anchor).collect(),
                        env,
                        row.epoch,
                        &mut found,
                    )?;
                    if let Dependency::Fact(f) = row.witness {
                        for b in &mut found[n..] {
                            b.parent = Some(f)
                        }
                    }
                    self.graph.stats.direct_bindings += (found.len() - n) as u64;
                }
            }
        } else {
            self.solve(
                &rule.query,
                (0..rule.query.len()).collect(),
                BTreeMap::new(),
                0,
                &mut found,
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
        for (i, op) in ops.into_iter().enumerate() {
            match op {
                Op::Copy(..) => {}
                Op::Let(dst, Call::Table(t), aa) => {
                    if self.graph.program.tables[t].virtual_fact {
                        self.graph.construct(cell, i, t, &aa, dst)?;
                    } else {
                        let key = self.graph.arguments(cell, &aa)?;
                        let row = self.graph.lookup(t, &key)?.ok_or_else(|| {
                            format!(
                                "missing function read {}",
                                self.graph.program.tables[t].name
                            )
                        })?;
                        self.graph
                            .cells
                            .get_mut(&cell)
                            .unwrap()
                            .values
                            .insert(dst, row.values.last().unwrap().clone());
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
                        .insert(dst, v);
                }
                Op::Set(t, aa, v) => self.graph.set(cell, i, t, &aa, &v)?,
                Op::Union(a, b) => {
                    let a = self.graph.argument(cell, &a)?;
                    let b = self.graph.argument(cell, &b)?;
                    self.graph.enqueue_union(a, b)?;
                }
                Op::Hide(t, aa, delete) => {
                    let key = self.graph.arguments(cell, &aa)?;
                    if !self.graph.hide(t, &key, delete)? {
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
            let bindings = self.matches(rid, false)?;
            self.watermarks[rid] = self.graph.epoch;
            work.extend(bindings.into_iter().map(|b| (rid, b)));
        }
        let matches = work.len();
        if self.audit {self.binding_audit.push(json!({"ruleset":set,"bindings":work.iter().map(|(rid,b)|{let template=&self.graph.program.templates[self.graph.program.rules[*rid].template];json!({"rule":rid,"values":b.values.iter().map(|(i,v)|json!({"slot":i,"value":crate::value::exported(v,&template.sorts[*i])})).collect::<Vec<_>>()})}).collect::<Vec<_>>()}));}
        for (rid, b) in work {
            self.apply(rid, b)?;
        }
        self.graph.stats.bindings += matches as u64;
        self.graph.repair()?;
        self.deliver();
        self.graph.collect_garbage();
        let changed = self.graph.changed;
        self.steps.push(json!({"ordinal":self.steps.len()+1,"ruleset":set,"path":path,"iterations":iterations,"epoch":self.graph.epoch,"logical_matches":matches,"changed":changed,"live_cells":self.graph.cells.len(),"elapsed_seconds":start.elapsed().as_secs_f64()}));
        if self.audit {
            self.snapshots.push(self.graph.export()?);
        }
        Ok(changed)
    }
    pub fn execute(&mut self) -> Result<()> { self.execute_until(None) }
    pub fn execute_until(&mut self, stop: Option<usize>) -> Result<()> {
        while !self.tasks.is_empty() {
            if stop.is_some_and(|n| self.steps.len() >= n) { break; }
            match self.tasks.pop().unwrap() {
                Task::Eval { node, path, iterations } => {
                    if let Some(set) = node["run"].as_str() {
                        if let Some(probe) = node["until_run"].as_str() { if self.probe(probe)? { continue; } }
                        if node["assert"].as_bool() == Some(true) {
                            if !self.probe(set)? { return Err(format!("source check failed: {set}")); }
                            continue;
                        }
                        if !node["until"].is_null() || !node["assert_table"].is_null() { return Err("unlowered source probe".into()); }
                        self.run(set, &path, &iterations)?;
                    } else if let Some(seq) = node["sequence"].as_array() {
                        for (i, child) in seq.iter().enumerate().rev() {
                            let mut p = path.clone(); p.push(i);
                            self.tasks.push(Task::Eval { node: child.clone(), path: p, iterations: iterations.clone() });
                        }
                    } else if let Some(count) = node["repeat"].as_u64() {
                        if count > 0 { self.tasks.push(Task::Repeat { body: node["body"].clone(), count, next: 0, start: self.steps.len(), path, iterations }); }
                    } else if !node["saturate"].is_null() {
                        let body=node["saturate"].clone();let mut it=iterations.clone();it.push(0);
                        self.tasks.push(Task::Saturate{body:body.clone(),iteration:0,start:self.steps.len(),path:path.clone(),iterations});
                        self.tasks.push(Task::Eval{node:body,path,iterations:it});
                    } else { return Err("unknown schedule instruction".into()); }
                }
                Task::Repeat{body,count,next,start,path,iterations} => {
                    if next>0 && !self.steps[start..].iter().any(|s|s["changed"]==true) {continue;}
                    if next<count {let mut it=iterations.clone();it.push(next);
                        self.tasks.push(Task::Repeat{body:body.clone(),count,next:next+1,start:self.steps.len(),path:path.clone(),iterations});
                        self.tasks.push(Task::Eval{node:body,path,iterations:it});
                    }
                }
                Task::Saturate{body,iteration,start,path,iterations} => {
                    if self.steps[start..].iter().any(|s|s["changed"]==true) {
                        let iteration=iteration.checked_add(1).ok_or("saturation occurrence exhausted")?;let mut it=iterations.clone();it.push(iteration);
                        self.tasks.push(Task::Saturate{body:body.clone(),iteration,start:self.steps.len(),path:path.clone(),iterations});
                        self.tasks.push(Task::Eval{node:body,path,iterations:it});
                    }
                }
            }
        }
        Ok(())
    }
    pub fn checkpoint(&self) -> J {
        let mut c=json!({"schema":"eggfuse-cell-checkpoint/v1","program":self.graph.program.artifact,"graph":self.graph.image(),"tasks":self.tasks,"watermarks":self.watermarks,"steps":self.steps});
        c["sha256"]=crate::checksum::digest(&c).into();c
    }
    pub fn resume(program: Program, mut checkpoint: J, audit: bool) -> Result<Self> {
        let checksum=checkpoint.as_object_mut().ok_or("checkpoint object")?.remove("sha256").ok_or("checkpoint checksum missing")?;
        if checksum!=crate::checksum::digest(&checkpoint){return Err("checkpoint checksum differs".into());}
        if checkpoint["schema"]!="eggfuse-cell-checkpoint/v1" || checkpoint["program"]!=program.artifact {return Err("checkpoint program differs".into());}
        let graph=Graph::restore(program,&checkpoint["graph"])?;let mut e=Self::new(graph,audit);
        e.tasks=serde_json::from_value(checkpoint["tasks"].clone()).map_err(|x|x.to_string())?;
        e.watermarks=serde_json::from_value(checkpoint["watermarks"].clone()).map_err(|x|x.to_string())?;
        e.steps=serde_json::from_value(checkpoint["steps"].clone()).map_err(|x|x.to_string())?;
        if e.watermarks.len()!=e.graph.program.rules.len() || e.watermarks.iter().any(|v|*v>e.graph.epoch) || e.steps.len() as u64!=e.graph.epoch {return Err("checkpoint stage metadata invalid".into());}
        for task in &e.tasks {let start=match task {Task::Repeat{start,..}|Task::Saturate{start,..}=>*start,_=>0};if start>e.steps.len(){return Err("checkpoint continuation invalid".into());}}
        Ok(e)
    }
    pub fn report(&self) -> J {
        json!({"backend":"cell-cpu","scope":"standalone Cell recipe executor; native frontend only; no egglog runtime linked","stats":self.graph.stats,"live_cells":self.graph.cells.len(),"stored_binding_values":self.graph.cells.values().map(|c|c.values.len()).sum::<usize>(),"virtual_constructor_facts":self.graph.cells.values().map(|c|c.facts.len()).sum::<usize>(),"function_ports":self.graph.functions.iter().filter(|f|f.live).count(),"promoted_aliases":self.graph.parents.len(),"steps":self.steps,"status":if self.tasks.is_empty(){"complete"}else{"checkpointed"}})
    }
}
