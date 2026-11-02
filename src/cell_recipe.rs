//! Generic analyzer Cell IR. Syntax DAG sharing never caches mutable reads.
//! Native replay and endpoint materialization have different authorizations.
use crate::pipeline::Result;
use egglog::{
    EGraph,
    ast::{Action, Actions, Change, Command, Expr, Fact, Literal, Rule, RunConfig, Schedule, Span},
};
use serde_json::{Value as J, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
fn sp() -> Span {
    Span::Rust(std::sync::Arc::new(egglog::ast::RustSpan {
        file: file!(),
        line: line!(),
        column: column!(),
    }))
}
fn arr(x: &J) -> Result<&Vec<J>> {
    x.as_array().ok_or_else(|| "expected recipe array".into())
}
fn num(x: &J) -> Result<usize> {
    Ok(x.as_u64().ok_or("expected recipe index")? as usize)
}
fn text(x: &J) -> Result<&str> {
    x.as_str().ok_or_else(|| "expected recipe text".into())
}
fn declaration(c: &Command) -> bool {
    matches!(
        c,
        Command::Sort { .. }
            | Command::Datatype { .. }
            | Command::Datatypes { .. }
            | Command::Constructor { .. }
            | Command::Function { .. }
            | Command::Relation { .. }
            | Command::AddRuleset(..)
            | Command::UnstableCombinedRuleset(..)
    )
}
#[derive(Default)]
pub struct Library {
    expressions: Vec<J>,
    expression_ids: BTreeMap<String, usize>,
    commands: Vec<J>,
    command_ids: BTreeMap<String, usize>,
    environments: Vec<Vec<usize>>,
    environment_ids: BTreeMap<(Vec<usize>, Option<String>), usize>,
    cells: BTreeMap<usize, J>,
    scopes: Vec<J>,
    scope_ids: BTreeMap<String, usize>,
    dirty: bool,
    seen: BTreeMap<usize, String>,
    source_schedule: Option<J>,
    stage_events: Vec<J>,
    device_programs: Vec<J>,
    device_program_ids: BTreeMap<String, usize>,
    device_instances: BTreeMap<usize, J>,
    device_rejected: BTreeMap<usize, String>,
    device_source: Option<J>,
}
struct Encoding<'a> {
    lib: &'a mut Library,
    markers: BTreeMap<String, String>,
    inputs: Vec<J>,
    input_ids: BTreeMap<(String, i64), usize>,
}
impl Encoding<'_> {
    fn expr(&mut self, e: &Expr) -> Result<usize> {
        let v = match e {
            Expr::Var(_, n) => json!({"op":"variable","name":n}),
            Expr::Lit(_, l) => match l {
                Literal::Int(n) => json!({"op":"literal","sort":"i64","value":n.to_string()}),
                Literal::Float(n) => {
                    json!({"op":"literal","sort":"f64","bits":n.0.to_bits().to_string()})
                }
                Literal::String(s) => json!({"op":"literal","sort":"String","value":s}),
                Literal::Bool(b) => json!({"op":"literal","sort":"bool","value":b}),
                Literal::Unit => json!({"op":"literal","sort":"Unit"}),
            },
            Expr::Call(_, name, args) => {
                if let Some(sort) = self.markers.get(name).cloned() {
                    if let [Expr::Lit(_, Literal::Int(index))] = args.as_slice() {
                        let key = (name.clone(), *index);
                        let next = self.inputs.len();
                        let slot = *self.input_ids.entry(key).or_insert_with(|| {
                            self.inputs
                                .push(json!({"marker":name,"index":index,"sort":sort}));
                            next
                        });
                        return self
                            .lib
                            .intern_expr(json!({"op":"input","slot":slot,"sort":sort}));
                    }
                }
                json!({"op":"call","name":name,"args":args.iter().map(|a|self.expr(a)).collect::<Result<Vec<_>>>()?})
            }
        };
        self.lib.intern_expr(v)
    }
    fn fact(&mut self, f: &Fact) -> Result<J> {
        Ok(match f {
            Fact::Eq(_, a, b) => json!({"op":"equal","args":[self.expr(a)?,self.expr(b)?]}),
            Fact::Fact(e) => json!({"op":"predicate","expr":self.expr(e)?}),
        })
    }
    fn action(&mut self, a: &Action) -> Result<J> {
        Ok(match a {
            Action::Let(_, name, e) => json!({"op":"let","name":name,"expr":self.expr(e)?}),
            Action::Expr(_, e) => json!({"op":"eval","expr":self.expr(e)?}),
            Action::Union(_, a, b) => json!({"op":"union","args":[self.expr(a)?,self.expr(b)?]}),
            Action::Set(_, name, args, v) => {
                json!({"op":"set","table":name,"args":args.iter().map(|a|self.expr(a)).collect::<Result<Vec<_>>>()?,"value":self.expr(v)?})
            }
            Action::Change(_, kind, name, args) => {
                json!({"op":if *kind==Change::Subsume{"subsume"}else{"delete"},"table":name,"args":args.iter().map(|a|self.expr(a)).collect::<Result<Vec<_>>>()?})
            }
            Action::Panic(_, message) => json!({"op":"panic","message":message}),
        })
    }
    fn schedule(&mut self, s: &Schedule) -> Result<J> {
        Ok(match s {
            Schedule::Run(_, r) => {
                json!({"op":"run","ruleset":r.ruleset,"until":r.until.as_ref().map(|fs|fs.iter().map(|f|self.fact(f)).collect::<Result<Vec<_>>>()).transpose()?})
            }
            Schedule::Repeat(_, n, b) => json!({"op":"repeat","count":n,"body":self.schedule(b)?}),
            Schedule::Saturate(_, b) => json!({"op":"saturate","body":self.schedule(b)?}),
            Schedule::Sequence(_, bs) => {
                json!({"op":"sequence","body":bs.iter().map(|b|self.schedule(b)).collect::<Result<Vec<_>>>()?})
            }
        })
    }
    fn command(&mut self, c: &Command) -> Result<usize> {
        let v = match c {
            Command::Action(a) => self.action(a)?,
            Command::Check(_, fs) => {
                json!({"op":"check","facts":fs.iter().map(|f|self.fact(f)).collect::<Result<Vec<_>>>()?})
            }
            Command::Fail(_, c) => json!({"op":"expect_failure","command":self.command(c)?}),
            Command::Rule { rule } => {
                json!({"op":"rule","name":rule.name,"ruleset":rule.ruleset,"naive":rule.naive,"locals":"scoped to this rule invocation","conditions":rule.body.iter().map(|f|self.fact(f)).collect::<Result<Vec<_>>>()?,"actions":rule.head.0.iter().map(|a|self.action(a)).collect::<Result<Vec<_>>>()?})
            }
            Command::RunSchedule(s) => json!({"op":"schedule","schedule":self.schedule(s)?}),
            c if declaration(c) => {
                let merge = if let Command::Function { merge: Some(e), .. } = c {
                    Some(self.expr(e)?)
                } else {
                    None
                };
                json!({"op":"declaration","source":c.to_string(),"merge_expression":merge})
            }
            _ => return Err(format!("unsupported operational command in Cell recipe: {c}").into()),
        };
        self.lib.intern_command(v)
    }
}
impl Library {
    pub fn observe_device_source(&mut self, source: Option<&J>) {
        if self.device_source.is_none() && source.is_some() {
            self.device_source = source.cloned();
            self.dirty = true;
        }
    }
    /// Capture the already prepared native state once, before local saturation.
    /// This adds no second preparation/replay and never exports a terminal cache.
    pub fn register_device(&mut self, unit: usize, eg: &EGraph, rulesets: &[String]) {
        if self.device_instances.contains_key(&unit) || self.device_rejected.contains_key(&unit) {
            return;
        }
        let result = (|| -> Result<()> {
            let mut core = eg
                .export_device_program(rulesets)
                .map_err(|e| format!("{e}"))?;
            if let Some(runtime)=self.cells.get(&unit).and_then(|c|c["native_runtime"].as_str()) {
                crate::structural_key::source::attach_device_runtime(&mut core,runtime)?;
            }
            let initial = core.as_object_mut().unwrap().remove("initial").unwrap();
            core["schedule"] = json!([{"saturate":{"sequence":rulesets.iter().map(|name|json!({"run":name})).collect::<Vec<_>>()}}]);
            core["assertion_tables"] = json!([]);
            let key = serde_json::to_string(&core)?;
            let next = self.device_programs.len();
            let id = *self.device_program_ids.entry(key).or_insert_with(|| {
                self.device_programs.push(core);
                next
            });
            self.device_instances.insert(unit,json!({"unit":unit,"program":id,"initial":initial,
                "inputs":self.cells.get(&unit).map(|c|c["inputs"].clone()),
                "scope":"actual initialized isolated ripen session","live_substitution_authorized":false}));
            Ok(())
        })();
        if let Err(e) = result {
            self.device_rejected.insert(unit, e.to_string());
        }
        self.dirty = true;
    }
    pub fn observe_schedule(&mut self, source: &str, events: Vec<J>) -> Result<()> {
        if self.source_schedule.is_none() {
            self.source_schedule = Some(crate::cell_schedule::source_program(source)?);
            self.dirty = true;
        }
        if self.stage_events != events {
            self.stage_events = events;
            self.dirty = true;
        }
        Ok(())
    }
    fn intern_expr(&mut self, v: J) -> Result<usize> {
        let k = serde_json::to_string(&v)?;
        let n = self.expressions.len();
        Ok(*self.expression_ids.entry(k).or_insert_with(|| {
            self.expressions.push(v);
            n
        }))
    }
    fn intern_command(&mut self, v: J) -> Result<usize> {
        let k = serde_json::to_string(&v)?;
        let n = self.commands.len();
        Ok(*self.command_ids.entry(k).or_insert_with(|| {
            self.commands.push(v);
            n
        }))
    }
    pub fn register(
        &mut self,
        unit: usize,
        parents: &[usize],
        origin: &J,
        source: &str,
        validation: &str,
    ) -> Result<()> {
        if self.cells.contains_key(&unit) {
            return Ok(());
        }
        let runtime = crate::structural_key::source::declaration(source)?.map(str::to_owned);
        if crate::structural_key::source::declaration(validation)? != runtime.as_deref() {
            return Err("Cell source/validation native runtime differs".into());
        }
        let mut eg = EGraph::default();
        let source = crate::visual_rule::surface_program(eg.parse_program(None, source)?);
        let validation = crate::visual_rule::surface_program(eg.parse_program(None, validation)?);
        let markers = origin["parameter_markers"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(sort, name)| {
                        name.as_str().map(|name| (name.to_owned(), sort.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut e = Encoding {
            lib: self,
            markers,
            inputs: vec![],
            input_ids: BTreeMap::new(),
        };
        let mut env = vec![];
        let mut setup = vec![];
        let mut checks = vec![];
        let mut sets = BTreeSet::new();
        let mut prefix = true;
        for c in &source {
            if let Command::Rule { rule } = c {
                sets.insert(rule.ruleset.clone());
            }
            let id = e.command(c)?;
            if matches!(c, Command::Check(..)) {
                checks.push(id);
                prefix = false;
            } else if prefix && (declaration(c) || matches!(c, Command::Rule { .. })) {
                env.push(id);
            } else {
                setup.push(id);
                prefix = false;
            }
        }
        let validation = validation
            .iter()
            .map(|c| e.command(c))
            .collect::<Result<Vec<_>>>()?;
        let inputs = e.inputs;
        let n = self.environments.len();
        let environment = *self.environment_ids.entry((env.clone(), runtime.clone())).or_insert_with(|| {
            self.environments.push(env);
            n
        });
        self.dirty = true;
        self.cells.insert(unit,json!({"unit":unit,"parents":parents,"environment":environment,"native_runtime":runtime,"inputs":inputs,"prepare":setup,"final_checks":checks,"validation":validation,
            "records":origin["members"],"stage_boundaries":origin["replay_plan"],"stage_boundary_kind":"application replay; original Run boundaries are in stage_events","external_injections":origin["staged_injections"],"relative_binding":origin["reuse_recipe"],"interface":origin["reuse_interface_terms"],"numeric_conditions":origin["analysis_program"],
            "closure":{"op":"saturate","body":{"op":"sequence","body":sets.into_iter().map(|s|json!({"op":"run","ruleset":s,"until":null})).collect::<Vec<_>>()}},
            "program_status":"ready","completion_status":"not_observed","scope":"isolated native ripen; not the tier0 schedule","live_substitution_authorized":false}));
        Ok(())
    }
    pub fn observe_jobs(&mut self, jobs: &[J]) -> Result<()> {
        for job in jobs {
            let unit = job["candidate_id"].as_u64().ok_or("job missing Cell ID")? as usize;
            let key = serde_json::to_string(&(job["state"].clone(), job["export"].clone()))?;
            if self.seen.get(&unit) == Some(&key) {
                continue;
            }
            self.seen.insert(unit, key);
            self.dirty = true;
            if !self.cells.contains_key(&unit) {
                self.dirty = true;
                self.cells.insert(unit,json!({"unit":unit,"program_status":"unavailable","reason":job["reason"],"live_substitution_authorized":false}));
            }
            let cell = self.cells.get_mut(&unit).unwrap();
            cell["ripen_state"] = job["state"].clone();
            cell["registered"] = json!(job["cell_unit"].is_number());
            if job["state"] != "Saturated" {
                cell["completion_status"] = json!("not_saturated");
                continue;
            }
            let Some(folder) = job["folder"].as_str() else {
                continue;
            };
            let folder = Path::new(folder);
            let (file, certified) = if folder.join("saturated-rule-composition.json").exists() {
                ("saturated-rule-composition.json", true)
            } else {
                ("observed-saturated-state.json", false)
            };
            let path = folder.join(file);
            if !path.exists() {
                cell["completion_status"] = json!("state_unavailable");
                cell["completion_reason"] = job["export"].clone();
                continue;
            }
            let state: J = serde_json::from_slice(&std::fs::read(path)?)?;
            let initial = ["initial-state.json", "trigger-state.json"]
                .into_iter()
                .find_map(|n| std::fs::read(folder.join(n)).ok())
                .map(|b| serde_json::from_slice::<J>(&b))
                .transpose()?;
            if certified {
                let report: J = serde_json::from_slice(&std::fs::read(folder.join("ripen.json"))?)?;
                crate::reuse_graph::RipSatEgraph::from_native_export(
                    serde_json::from_value(state.clone())?,
                    &report,
                )?;
            }
            let mut packet = completion(&state, initial.as_ref(), certified)?;
            let scope = packet.as_object_mut().unwrap().remove("scope").unwrap();
            let scope_key = serde_json::to_string(&scope)?;
            let next = self.scopes.len();
            let scope_id = *self.scope_ids.entry(scope_key).or_insert_with(|| {
                self.scopes.push(scope);
                next
            });
            packet["scope_ref"] = json!(scope_id);
            if let Some(guard) = packet["initial_guard"].as_object_mut() {
                guard.remove("local_ids");
                guard.remove("scope");
            }
            packet["boundary"]=json!(cell["inputs"].as_array().into_iter().flatten().enumerate().map(|(slot,input)| {
                let find=|s:&J|s["rows"].as_array().into_iter().flatten().find(|r|r["op"]==input["marker"] && r["args"].as_array().is_some_and(|a|a.len()==1) && r["args"][0].as_u64().is_some_and(|i|s["values"][i as usize]["literal"]==input["index"].to_string())).map(|r|r["result"].clone());
                json!({"input":slot,"sort":input["sort"],"initial_slot":initial.as_ref().and_then(find),"final_slot":find(&state)})
            }).collect::<Vec<_>>());
            cell["completion"] = packet;
            cell["completion_status"] = json!(if certified {
                "certified_endpoint"
            } else {
                "observed_endpoint_only"
            });
            cell["completion_reason"] = job["export"].clone();
        }
        Ok(())
    }
    pub fn unavailable(&mut self, unit: usize, reason: String) {
        self.dirty = true;
        self.cells.insert(unit, json!({"unit":unit,"program_status":"unavailable","reason":reason,"live_substitution_authorized":false}));
    }
    pub fn report(&self) -> J {
        let mut counts = BTreeMap::<String, usize>::new();
        for c in self.cells.values() {
            *counts
                .entry(c["program_status"].as_str().unwrap_or("unknown").into())
                .or_default() += 1;
            *counts
                .entry(c["completion_status"].as_str().unwrap_or("none").into())
                .or_default() += 1;
        }
        json!({"schema":"analyzer-cell-recipes/v1","counts":counts,"cell_count":self.cells.len(),"environment_count":self.environments.len(),"registered_cell_count":self.cells.values().filter(|c|c["registered"]==true).count(),"registered_with_program":self.cells.values().filter(|c|c["registered"]==true&&c["program_status"]=="ready").count(),"file":"cell-recipes.json","expression_nodes":self.expressions.len(),"instruction_nodes":self.commands.len(),"scope":"all prepared Cells; endpoint reconstruction is distinct from certified reusable closure"})
    }
    pub fn save(&mut self, path: &Path) -> Result<()> {
        if !self.dirty && path.exists() {
            return Ok(());
        }
        let mut v = self.report();
        v["expressions"] = json!(self.expressions);
        v["instructions"] = json!(self.commands);
        v["environments"] = json!(self.environments);
        v["scopes"] = json!(self.scopes);
        v["source_schedule"] = json!(self.source_schedule);
        v["stage_events"] = json!(self.stage_events);
        v["cells"] = json!(self.cells.values().collect::<Vec<_>>());
        crate::pipeline::write_json(path, &v)?;
        crate::pipeline::write_json(
            &path.with_file_name("cell-device-recipes.json"),
            &json!({
                "schema":"analyzer-device-cells/v1","programs":self.device_programs,
                "source_program":self.device_source,"source_schedule":self.source_schedule,
                "instances":self.device_instances.iter().map(|(unit,instance)|{let mut v=instance.clone();v["observed_completion"]=self.cells.get(unit).map(|c|c["ripen_state"].clone()).unwrap_or(J::Null);v}).collect::<Vec<_>>(),
                "rejected":self.device_rejected.iter().map(|(unit,reason)|json!({"unit":unit,"reason":reason})).collect::<Vec<_>>(),
                "contract":"exact initialized local programs; no endpoint substitution; does not authorize global skipping"
            }),
        )?;
        self.dirty = false;
        Ok(())
    }
}

mod codec;
mod device;
mod endpoint;
pub use codec::Program;
pub use endpoint::{completion, public_snapshot, snapshot_graph};
#[cfg(test)]
mod tests;
