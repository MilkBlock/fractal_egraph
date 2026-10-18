//! Native resumable specialization of an already matched, frozen context.
//! Full parent bodies are seeded; only opaque equality identities are renamed.
use super::*;
use egglog::ast::{Action, Fact};
use serde_json::json;
use std::time::Duration;

pub(crate) struct Baker {
    base: EGraph,
    sink: CellActionSink,
    ruleset: String,
    markers: BTreeMap<String, String>,
    environment: String,
}
pub(crate) struct Job {
    eg: EGraph,
    trace: TraceSession,
    names: Vec<String>,
    ruleset: String,
    pub steps: usize,
    pub stop: Option<String>,
    pub done: Option<Json>,
}
fn expr(eg: &EGraph, e: &Expr) -> bool {
    match e {
        Expr::Var(..) | Expr::Lit(..) => true,
        Expr::Call(_, op, args) => {
            eg.get_function(op)
                .is_some_and(|f| f.is_constructor() && !f.is_let_binding())
                && args.iter().all(|e| expr(eg, e))
        }
    }
}
impl Baker {
    pub fn new(env: &str, ruleset: &str, consumer: &str) -> Result<Self> {
        let mut eg = EGraph::default();
        let cmds = eg.parse_program(None, env)?;
        eg.run_program(cmds.clone())?;
        for c in cmds {
            if let Command::Rule { rule } = c {
                if rule.ruleset != ruleset {
                    continue;
                }
                if rule.body.is_empty()
                    || !rule.body.iter().all(|f| match f {
                        Fact::Eq(_, a, b) => expr(&eg, a) && expr(&eg, b),
                        Fact::Fact(e) => expr(&eg, e),
                    })
                    || !rule.head.0.iter().all(|a| match a {
                        Action::Let(_, _, e) | Action::Expr(_, e) => expr(&eg, e),
                        Action::Union(_, a, b) => expr(&eg, a) && expr(&eg, b),
                        _ => false,
                    })
                {
                    return Err("live Bake currently requires positive constructor rules; guards/set/merge/destructive rules need a separate transfer contract".into());
                }
            }
        }
        let mut sorts = BTreeSet::new();
        for name in eg.get_function_names() {
            let f = eg.get_function(&name).unwrap();
            if f.is_constructor() && !f.is_let_binding() {
                sorts.insert(f.schema().output.name().to_owned());
            }
        }
        let mut markers = BTreeMap::new();
        for (i, sort) in sorts.into_iter().enumerate() {
            let marker = format!("LiveBakeInput{i}");
            if eg.get_function(&marker).is_some() {
                return Err("reserved live Bake marker already exists".into());
            }
            eg.parse_and_run_program(None, &format!("(constructor {marker} (i64) {sort})"))?;
            markers.insert(sort, marker);
        }
        let sink = eg.prepare_cell_action(ruleset, consumer)?;
        if !sink.guards.is_empty() {
            return Err("live Bake cannot erase a numeric guard".into());
        }
        Ok(Self {
            base: eg,
            sink,
            ruleset: ruleset.into(),
            markers,
            environment: env.into(),
        })
    }
    pub fn start(
        &mut self,
        templates: &[Template],
        sources: &[(usize, usize)],
        raw: &[u32],
        binding: &[usize],
    ) -> Result<Job> {
        let mut eg = self.base.fork_shared();
        let mut names = vec![None::<String>; raw.len()];
        let mut param_names = BTreeMap::<(String, u32), String>::new();
        let mut lines = vec![];
        for &(tid, offset) in sources {
            let t = &templates[tid];
            for &(v, _) in &t.parameters {
                let sort = t.state["values"][v]["sort"]
                    .as_str()
                    .ok_or("missing parameter sort")?;
                let marker = self
                    .markers
                    .get(sort)
                    .ok_or("only opaque equality parameters may be specialized")?;
                let key = (sort.to_owned(), raw[offset + v]);
                let next = param_names.len();
                let name = param_names
                    .entry(key)
                    .or_insert_with(|| {
                        let n = format!("live_param{next}");
                        lines.push(format!("(let {n} ({marker} {next}))"));
                        n
                    })
                    .clone();
                names[offset + v] = Some(name);
            }
            for (i, v) in t.state["values"].as_array().unwrap().iter().enumerate() {
                if let Some(l) = v["literal"].as_str() {
                    names[offset + i] = Some(l.to_owned());
                }
            }
            let mut waiting = t.state["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| !t.markers.contains(r["op"].as_str().unwrap()))
                .collect::<Vec<_>>();
            if t.state["subsumed_rows"]
                .as_array()
                .is_some_and(|r| !r.is_empty())
            {
                return Err("hidden parent rows cannot be reseeded as live facts".into());
            }
            while !waiting.is_empty() {
                let before = waiting.len();
                let mut later = vec![];
                for r in waiting {
                    let args = r["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| names[offset + v.as_u64().unwrap() as usize].clone())
                        .collect::<Option<Vec<_>>>();
                    let Some(args) = args else {
                        later.push(r);
                        continue;
                    };
                    let term = format!("({} {})", r["op"].as_str().unwrap(), args.join(" "));
                    let out = offset + r["result"].as_u64().unwrap() as usize;
                    if let Some(old) = &names[out] {
                        lines.push(format!("(union {old} {term})"));
                    } else {
                        let n = format!("live_value{out}");
                        lines.push(format!("(let {n} {term})"));
                        names[out] = Some(n);
                    }
                }
                if later.len() == before {
                    return Err("ungrounded parent closure".into());
                }
                waiting = later;
            }
        }
        let names = names
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or("unresolved raw context")?;
        // Equality between internal values of different parents is part of the
        // specialization key too. Never assume only external parameters alias.
        let mut by_value = BTreeMap::new();
        for &(tid, offset) in sources {
            let t = &templates[tid];
            for (v, d) in t.state["values"].as_array().unwrap().iter().enumerate() {
                let index = offset + v;
                if d["literal"].is_null() {
                    if let Some(old) = by_value.insert(raw[index], index) {
                        lines.push(format!("(union {} {})", names[old], names[index]));
                    }
                }
            }
        }
        eg.parse_and_run_program(None, &lines.join("\n"))?;
        let values = binding
            .iter()
            .map(|i| {
                let e = egglog::ast::Parser::default().get_expr_from_string(None, &names[*i])?;
                Ok(eg.eval_expr(&e)?.1)
            })
            .collect::<Result<Vec<_>>>()?;
        // Use the captured full binding; do not run the original LHS again here.
        eg.apply_cell_bindings(&self.sink, &[values])?;
        Ok(Job {
            eg,
            trace: TraceSession::with_effects(),
            names,
            ruleset: self.ruleset.clone(),
            steps: 0,
            stop: None,
            done: None,
        })
    }
    pub fn environment(&self) -> &str {
        &self.environment
    }
}
impl Job {
    pub fn poll(&mut self, milliseconds: u64, facts: usize) -> Result<bool> {
        if self.done.is_some() {
            return Ok(true);
        }
        let start = Instant::now();
        self.stop = None;
        loop {
            let remaining = Duration::from_millis(milliseconds).saturating_sub(start.elapsed());
            if self.eg.num_tuples() >= facts {
                self.stop = Some("facts".into());
                return Ok(false);
            }
            if remaining.is_zero() {
                self.stop = Some("time-slice".into());
                return Ok(false);
            }
            self.trace
                .set_execution_limits(Some(remaining), Some(facts - self.eg.num_tuples()));
            let result = self.eg.step_rules_with_trace(&self.ruleset, &self.trace);
            let stopped = self.trace.execution_stop_reason().map(str::to_owned);
            self.trace.clear_execution_limits();
            self.trace.drain_completed();
            let report = result?;
            self.steps += 1;
            if let Some(reason) = stopped {
                self.stop = Some(reason);
                return Ok(false);
            }
            if !report.updated {
                self.done = Some(self.snapshot()?);
                return Ok(true);
            }
        }
    }
    fn snapshot(&mut self) -> Result<Json> {
        let mut raw_values = vec![];
        for name in &self.names {
            let e = egglog::ast::Parser::default().get_expr_from_string(None, name)?;
            raw_values.push(self.eg.eval_expr(&e)?);
        }
        let mut values = vec![];
        let mut ids = BTreeMap::new();
        let mut id = |sort: &egglog::ArcSort, v: Value| -> Result<usize> {
            let v = self.eg.get_canonical_value(v, sort);
            let key = (sort.name().to_owned(), v.rep());
            if let Some(i) = ids.get(&key) {
                return Ok(*i);
            }
            let literal = if self.eg.cell_sort_is_equality(sort.name())? {
                None
            } else {
                Some(match sort.name() {
                    "i64" => self.eg.value_to_base::<i64>(v).to_string(),
                    "String" => serde_json::to_string(
                        &self.eg.value_to_base::<egglog::sort::S>(v).to_string(),
                    )?,
                    "bool" => self.eg.value_to_base::<bool>(v).to_string(),
                    "Unit" => "()".into(),
                    _ => return Err("unsupported base literal export".into()),
                })
            };
            let i = values.len();
            values.push(json!({"sort":sort.name(),"literal":literal}));
            ids.insert(key, i);
            Ok(i)
        };
        let mut rows = vec![];
        for name in self.eg.get_function_names() {
            let f = self.eg.get_function(&name).unwrap();
            if !f.is_constructor() || f.is_let_binding() {
                continue;
            }
            let mut raw = vec![];
            self.eg.function_for_each(&name, |r| {
                if !r.subsumed {
                    raw.push(r.vals.to_vec())
                }
            })?;
            for r in raw {
                let cols = r
                    .iter()
                    .zip(f.schema().input.iter().chain([&f.schema().output]))
                    .map(|(v, s)| id(s, *v))
                    .collect::<Result<Vec<_>>>()?;
                rows.push(json!({"op":name,"args":&cols[..cols.len()-1],"result":cols.last()}));
            }
        }
        let raw_values = raw_values
            .iter()
            .map(|(sort, v)| id(sort, *v))
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"state":{"values":values,"rows":rows},"raw_values":raw_values,
            "native_steps":self.steps,"raw_names":self.names,"scope":"completed native closure of both parent bodies and the captured RHS binding; frozen positive constructor environment"}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_erase_numeric_guards_or_custom_function_updates() {
        for env in [
            "(datatype E (V i64) (N E)) (rule ((= x (V n)) (> n 0)) ((N x)) :name \"consumer\")",
            "(datatype E (V i64) (N E)) (function Score (E) i64 :merge (max old new)) (rule ((= x (V n))) ((set (Score x) n)) :name \"consumer\")",
        ] {
            assert!(Baker::new(env, "", "consumer").is_err());
        }
    }
}
