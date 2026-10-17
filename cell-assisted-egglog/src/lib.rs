//! Analyze-backed partial queries. Positive finite Cell facts are certificates;
//! candidate substitutions are not certificates. All effects stay in egglog.
use egglog::cell_assist::{CellActionSink, CellOperand, CellQueryAtom};
use egglog::{
    EGraph, TraceBatch, TraceSession, Value,
    ast::{Command, Expr, Literal},
};
use egglog_numeric_id::NumericId;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

pub mod matrix;
pub mod program;
pub mod compile;
pub mod resident;
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn json(p: &Path) -> Result<Json> {
    Ok(serde_json::from_slice(&std::fs::read(p)?)?)
}

#[derive(Clone, Debug)]
struct Template {
    id: usize,
    state: Json,
    // Ordered parameter positions in the canonical state and source match names.
    parameters: Vec<(usize, String)>,
    producer: String,
    markers: BTreeSet<String>,
    literals: BTreeMap<usize, Value>,
}
#[derive(Clone)]
struct Instance {
    template: usize,
    parameters: Vec<Value>,
}
type Binding = Vec<Option<Value>>;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Lens {
    binding: Vec<Option<usize>>,
    aliases: Vec<(usize, usize)>,
}

/// Partition is explicit and exhaustive, not a heuristic ranking or a candidate
/// budget. Native atom numbers are printed by `inspect`. A Cell part can contain
/// several atoms, including atoms at non-root positions. An external part is one
/// native row (bootstrap scan once, then committed write notifications).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Part {
    pub atoms: Vec<usize>,
    pub cell: bool,
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub epochs: usize,
    pub native_matches: usize,
    pub raw_events_drained: usize,
    pub observed_instances: usize,
    pub valid_instances: usize,
    pub pending_instances: usize,
    pub cell_point_reads: usize,
    pub external_point_reads: usize,
    pub external_bootstrap_rows: usize,
    pub cell_projections: usize,
    pub candidate_pairs: usize,
    pub equality_columns: usize,
    pub accepted_bindings: usize,
    pub guard_rejected_bindings: usize,
    pub unique_dispatched_bindings: usize,
    pub assisted_full_lhs_queries: usize,
    pub audit_queries: usize,
    pub audit_native_bindings: usize,
    pub audit_uncovered_bindings: usize,
    pub audit_uncovered_per_round: Vec<usize>,
    pub native_seconds: f64,
    pub admission_seconds: f64,
    pub residual_seconds: f64,
    pub action_seconds: f64,
}

pub struct Session {
    eg: EGraph,
    sink: CellActionSink,
    ruleset: String,
    native: Vec<String>,
    templates: Vec<Template>,
    instances: Vec<Instance>,
    instance_keys: BTreeSet<(usize, Vec<Value>)>,
    // Precomputed symbolic partial query results: template, part -> slot -> state value.
    lenses: Vec<Vec<Vec<Lens>>>,
    parts: Vec<Part>,
    external: BTreeMap<String, BTreeSet<Vec<Value>>>,
    trace: TraceSession,
    audit: bool,
    pub stats: Stats,
}

/// Use the same rule environment that produced analyze. Initial facts/schedules
/// are omitted; there is no parser-based discovery of a new Cell family here.
pub fn environment(dir: &Path) -> Result<(String, Json)> {
    let history = json(&dir.join("history.json"))?;
    let src = history["source_text"]
        .as_str()
        .ok_or("analyze history needs source_text")?;
    let mut parser = EGraph::default();
    let commands = parser.parse_program(None, src)?;
    let mut text = commands
        .into_iter()
        .filter(|c| {
            !matches!(
                c,
                Command::Action(..)
                    | Command::RunSchedule(..)
                    | Command::Check(..)
                    | Command::PrintFunction(..)
                    | Command::PrintSize(..)
                    | Command::Rule { .. }
                    | Command::Rewrite(..)
                    | Command::BiRewrite(..)
            )
        })
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    // Analyze already desugared rewrites and pinned their trace names. The raw
    // Rewrite Display implementation does not preserve every explicit name.
    for rule in history["rules"]
        .as_array()
        .ok_or("missing analyzed rules")?
    {
        text.push('\n');
        text.push_str(rule["code"].as_str().ok_or("missing analyzed rule code")?);
    }
    Ok((text, history))
}

impl Session {
    /// Backend conformance entry: all atoms read explicit rows of this native
    /// snapshot. No Cells are inferred here and this is not a Cell coverage claim.
    pub fn for_query_validation(mut eg: EGraph, ruleset: &str, consumer: &str) -> Result<Self> {
        let sink = eg.prepare_cell_action(ruleset, consumer)?;
        let parts = (0..sink.atoms.len())
            .map(|i| Part {
                atoms: vec![i],
                cell: false,
            })
            .collect::<Vec<_>>();
        let mut external = sink
            .atoms
            .iter()
            .map(|a| (a.function.clone(), BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        let mut stats = Stats::default();
        for (name, keys) in &mut external {
            let width = eg.get_function(name).unwrap().schema().input.len();
            eg.function_for_each(name, |r| {
                keys.insert(r.vals[..width].to_vec());
                stats.external_bootstrap_rows += 1;
            })?;
        }
        Ok(Self {
            eg,
            sink,
            ruleset: ruleset.into(),
            native: vec![],
            templates: vec![],
            instances: vec![],
            instance_keys: BTreeSet::new(),
            lenses: vec![],
            parts,
            external,
            trace: TraceSession::with_dependencies(),
            audit: true,
            stats,
        })
    }

    pub fn new(
        dir: &Path,
        ruleset: &str,
        consumer: &str,
        native: Vec<String>,
        parts: Vec<Part>,
    ) -> Result<Self> {
        if native.iter().any(|r| r == consumer) {
            return Err("native and assisted lanes overlap".into());
        }
        let (env, history) = environment(dir)?;
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &env)?;
        let sink = eg.prepare_cell_action(ruleset, consumer)?;
        let mut coverage = vec![0; sink.atoms.len()];
        if !parts.iter().any(|p| p.cell) {
            return Err("at least one part must use certified Cells".into());
        }
        for p in &parts {
            if p.atoms.is_empty() || (!p.cell && p.atoms.len() != 1) {
                return Err("empty part or multi-atom external part".into());
            }
            for &a in &p.atoms {
                *coverage.get_mut(a).ok_or("bad atom index")? += 1;
            }
        }
        if coverage.iter().any(|n| *n != 1) {
            return Err("partition must cover every LHS atom exactly once".into());
        }
        let catalog = json(&dir.join("catalog/catalog.json"))?;
        let mut templates = vec![];
        let mut seen = BTreeSet::new();
        for trigger in catalog["triggers"]
            .as_array()
            .ok_or("missing catalog triggers")?
        {
            let origin = &trigger["binding_origin"];
            let Some(members) = origin["comb_members"].as_array() else {
                continue;
            };
            if members.len() != 1 {
                continue;
            }
            let Some(producer) = members[0]["rule_name"].as_str() else {
                continue;
            };
            if producer != consumer && !native.iter().any(|r| r == producer) {
                continue;
            }
            let Some(id) = trigger["saturated_rule_composition"].as_u64() else {
                continue;
            };
            let id = id as usize;
            let state = json(&dir.join(format!("catalog/states/state-{id:04}.json")))?;
            // No interpretation of symbolic analysis fields as guaranteed facts.
            if state["scope"]["symbolic_size_conditions"] == true {
                continue;
            }
            let markers: BTreeSet<_> = origin["parameter_markers"]
                .as_object()
                .into_iter()
                .flat_map(|m| m.values())
                .chain([&origin["parameter_marker"]])
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect();
            let record = &history["records"]
                [members[0]["record"].as_u64().ok_or("missing record")? as usize];
            let mut parameters = vec![];
            let mut valid = true;
            for term in origin["reuse_interface_terms"]["lower"]
                .as_array()
                .ok_or("missing Cell interface")?
            {
                let sv = origin["symbolic_values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|v| v["term"] == *term);
                let Some(token) = sv.and_then(|v| v["token"].as_u64()) else {
                    valid = false;
                    break;
                };
                let var = record["outputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(record["values"].as_array().unwrap())
                    .find_map(|(role, value)| {
                        if value.as_u64() == Some(token) {
                            role["Var"].as_str()
                        } else {
                            None
                        }
                    });
                let Some(var) = var else {
                    valid = false;
                    break;
                };
                let e = egglog::ast::Parser::default()
                    .get_expr_from_string(None, term.as_str().unwrap())?;
                let Expr::Call(_, op, args) = e else {
                    valid = false;
                    break;
                };
                let Some(Expr::Lit(_, Literal::Int(n))) = args.first() else {
                    valid = false;
                    break;
                };
                let row = state["rows"].as_array().unwrap().iter().find(|r| {
                    r["op"] == op
                        && r["args"].as_array().is_some_and(|a| a.len() == 1)
                        && state["values"][r["args"][0].as_u64().unwrap() as usize]["literal"]
                            == n.to_string()
                });
                let Some(row) = row else {
                    valid = false;
                    break;
                };
                parameters.push((row["result"].as_u64().unwrap() as usize, var.to_owned()));
            }
            // The representative trigger's value coordinates must agree with its
            // canonical state. Nonidentity adapters need a separate transport proof.
            let identity = trigger["value_map"].as_array().is_some_and(|m| {
                m.iter()
                    .enumerate()
                    .all(|(i, x)| x.as_u64() == Some(i as u64))
            });
            if !valid
                || !identity
                || parameters.is_empty()
                || !seen.insert((id, producer.to_owned(), parameters.clone()))
            {
                continue;
            }
            let mut literals = BTreeMap::new();
            for (i, v) in state["values"].as_array().unwrap().iter().enumerate() {
                if let Some(lit) = v["literal"].as_str() {
                    let e = egglog::ast::Parser::default().get_expr_from_string(None, lit)?;
                    if !matches!(e, Expr::Lit(..)) {
                        valid = false;
                        break;
                    }
                    let (s, val) = eg.eval_expr(&e)?;
                    if s.name() != v["sort"].as_str().unwrap() {
                        valid = false;
                        break;
                    }
                    literals.insert(i, val);
                }
            }
            if valid {
                templates.push(Template {
                    id,
                    state,
                    parameters,
                    producer: producer.into(),
                    markers,
                    literals,
                })
            }
        }
        if templates.is_empty() {
            return Err(
                "no supported completed analyze Cell with source-match parameter mapping".into(),
            );
        }
        let lenses = templates
            .iter()
            .map(|t| {
                parts
                    .iter()
                    .map(|p| if p.cell { project(t, &sink, p) } else { vec![] })
                    .collect()
            })
            .collect();
        let external = parts
            .iter()
            .filter(|p| !p.cell)
            .map(|p| (sink.atoms[p.atoms[0]].function.clone(), BTreeSet::new()))
            .collect();
        Ok(Self {
            eg,
            sink,
            ruleset: ruleset.into(),
            native,
            templates,
            instances: vec![],
            instance_keys: BTreeSet::new(),
            lenses,
            parts,
            external,
            trace: TraceSession::with_dependencies(),
            audit: false,
            stats: Stats::default(),
        })
    }
    pub fn engine(&self) -> &EGraph {
        &self.eg
    }
    pub fn set_audit(&mut self, enabled: bool) {
        self.audit = enabled;
    }
    pub fn describe(&self) -> Json {
        serde_json::json!({"rule":self.sink.name,"native_rules":self.native,"slots":self.sink.slots(),"atoms":self.sink.atoms.iter().enumerate().map(|(i,a)|serde_json::json!({"index":i,"function":a.function,"columns":format!("{:?}",a.columns),"sorts":a.sorts})).collect::<Vec<_>>(),
            "guards":format!("{:?}",self.sink.guards),"parts":self.parts,"templates":self.templates.iter().enumerate().map(|(i,t)|serde_json::json!({"state":t.id,"producer":t.producer,"parameters":t.parameters,"projection_counts":self.lenses[i].iter().map(Vec::len).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
    fn observe(&mut self, batch: TraceBatch) {
        self.stats.raw_events_drained += batch.len();
        for m in batch.matches {
            for (tid, t) in self.templates.iter().enumerate().filter(|(_, t)| {
                t.producer == m.rule.strip_prefix("cell-action:").unwrap_or(&m.rule)
            }) {
                let params: Option<Vec<_>> = t
                    .parameters
                    .iter()
                    .map(|(_, n)| {
                        m.bindings
                            .iter()
                            .find(|b| b.name.as_deref() == Some(n.as_str()))
                            .map(|b| b.value)
                    })
                    .collect();
                if let Some(parameters) = params {
                    if self.instance_keys.insert((tid, parameters.clone())) {
                        self.instances.push(Instance {
                            template: tid,
                            parameters,
                        });
                    }
                }
            }
        }
        for w in batch.writes {
            let Some(name) = self.eg.cell_table_name(w.table) else {
                continue;
            };
            let Some(rows) = self.external.get_mut(name) else {
                continue;
            };
            let n = self.eg.get_function(name).unwrap().schema().input.len();
            if w.actual.len() > n {
                rows.insert(w.actual[..n].to_vec());
            }
        }
        self.stats.observed_instances = self.instances.len();
    }
    pub fn input(&mut self, text: &str) -> Result<()> {
        let cmds = self.eg.parse_program(None, text)?;
        if cmds
            .iter()
            .any(|c| !matches!(c, Command::Action(..) | Command::Check(..)))
        {
            return Err("input updates must be actions/checks; rule environment is frozen".into());
        }
        self.eg.run_cell_input_with_trace(cmds, &self.trace)?;
        self.stats.epochs += 1;
        self.observe(self.trace.drain_completed());
        // Seed external relations even if a top-level action has no row receipt.
        // This is the single bootstrap table scan, never a consumer LHS query.
        if self.stats.epochs == 1 {
            for (name, rows) in &mut self.external {
                let arity = self.eg.get_function(name).unwrap().schema().input.len();
                self.eg.function_for_each(name, |r| {
                    if !r.subsumed {
                        rows.insert(r.vals[..arity].to_vec());
                        self.stats.external_bootstrap_rows += 1;
                    }
                })?;
            }
        }
        Ok(())
    }
    pub fn round(&mut self, backend: &mut dyn matrix::Matcher) -> Result<bool> {
        let t = Instant::now();
        let report =
            self.eg
                .step_cell_native_rules(&self.ruleset, &self.native, Some(&self.trace))?;
        self.stats.native_seconds += t.elapsed().as_secs_f64();
        self.stats.native_matches += report.num_matches_per_rule.values().sum::<usize>();
        self.stats.epochs += 1;
        self.observe(self.trace.drain_completed());
        let t = Instant::now();
        // Every round currently revalidates point witnesses after rebuild. No
        // stale guarantee is carried across union, subsume or set. The batch is
        // immutable until all GPU work has completed and bindings are accepted.
        let mut fragments = vec![BTreeSet::<Binding>::new(); self.parts.len()];
        self.stats.valid_instances = 0;
        self.stats.pending_instances = 0;
        for inst in &self.instances {
            let tid = inst.template;
            let template = &self.templates[tid];
            let values = resolve(
                &self.eg,
                template,
                &inst.parameters,
                &mut self.stats.cell_point_reads,
            );
            let Some(values) = values else {
                self.stats.pending_instances += 1;
                continue;
            };
            self.stats.valid_instances += 1;
            for (p, lenses) in self.lenses[tid].iter().enumerate() {
                for lens in lenses {
                    if lens.aliases.iter().any(|(a, b)| values[*a] != values[*b]) {
                        continue;
                    }
                    let binding = lens.binding.iter().map(|v| v.map(|i| values[i])).collect();
                    fragments[p].insert(binding);
                    self.stats.cell_projections += 1;
                }
            }
        }
        for (pi, p) in self.parts.iter().enumerate().filter(|(_, p)| !p.cell) {
            let atom = &self.sink.atoms[p.atoms[0]];
            for key in &self.external[&atom.function] {
                self.stats.external_point_reads += 1;
                if let Some(row) = self.eg.cell_visible_row(&atom.function, key) {
                    if let Some(binding) = bind_atom(atom, &row, self.sink.slots().len()) {
                        fragments[pi].insert(binding);
                    }
                }
            }
        }
        self.stats.admission_seconds += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let mut joined: Vec<Binding> = vec![vec![None; self.sink.slots().len()]];
        for fragment in fragments {
            let right: Vec<_> = fragment.into_iter().collect();
            if joined.is_empty() || right.is_empty() {
                joined.clear();
                break;
            }
            let shared: Vec<_> = (0..joined[0].len())
                .filter(|&i| joined[0][i].is_some() && right[0][i].is_some())
                .collect();
            let left_keys: Vec<Vec<u32>> = joined
                .iter()
                .map(|b| shared.iter().map(|i| b[*i].unwrap().rep()).collect())
                .collect();
            let right_keys: Vec<Vec<u32>> = right
                .iter()
                .map(|b| shared.iter().map(|i| b[*i].unwrap().rep()).collect())
                .collect();
            self.stats.candidate_pairs += joined
                .len()
                .checked_mul(right.len())
                .ok_or("pair count overflow")?;
            self.stats.equality_columns += shared.len();
            let pairs = backend.match_rows(&left_keys, &right_keys)?;
            let mut next = BTreeSet::new();
            for (a, b) in pairs {
                let mut row = joined.get(a).ok_or("bad backend left index")?.clone();
                let other = right.get(b).ok_or("bad backend right index")?;
                // Mandatory exact CPU boundary check of returned pairs. This is
                // O(accepted * interface), not a repeat of the Cartesian search.
                if shared.iter().any(|i| row[*i] != other[*i]) {
                    return Err("backend returned a false positive".into());
                }
                for (x, y) in row.iter_mut().zip(other) {
                    if x.is_none() {
                        *x = *y
                    }
                }
                next.insert(row);
            }
            joined = next.into_iter().collect();
        }
        self.stats.residual_seconds += t.elapsed().as_secs_f64();
        let residual_candidates = joined.len();
        let bindings = joined
            .into_iter()
            .map(|b| {
                b.into_iter()
                    .collect::<Option<Vec<_>>>()
                    .ok_or("incomplete RHS binding")
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let bindings = bindings
            .into_iter()
            .filter_map(|b| match self.eg.cell_guards_hold(&self.sink, &b) {
                Ok(true) => Some(Ok(b)),
                Ok(false) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.stats.guard_rejected_bindings += residual_candidates - bindings.len();
        self.stats.accepted_bindings += bindings.len();
        self.stats.unique_dispatched_bindings += bindings.len();
        if self.audit {
            let expected: BTreeSet<_> = self
                .eg
                .audit_cell_matches(&self.sink)?
                .into_iter()
                .collect();
            let actual: BTreeSet<_> = bindings.iter().cloned().collect();
            if !actual.is_subset(&expected) {
                return Err("assisted bindings are not native matches".into());
            }
            self.stats.audit_queries += 1;
            self.stats.audit_native_bindings += expected.len();
            let uncovered = expected.difference(&actual).count();
            self.stats.audit_uncovered_bindings += uncovered;
            self.stats.audit_uncovered_per_round.push(uncovered);
        }
        let t = Instant::now();
        self.eg
            .apply_cell_bindings_with_trace(&self.sink, &bindings, Some(&self.trace))?;
        let effects = self.trace.drain_completed();
        let changed = effects.unions.iter().any(|u| u.displaced.is_some())
            || effects.writes.iter().any(|w| {
                self.eg.cell_table_name(w.table).is_some()
                    && matches!(
                        w.outcome,
                        egglog::WriteOutcome::Inserted | egglog::WriteOutcome::Updated
                    )
            });
        self.observe(effects);
        self.stats.action_seconds += t.elapsed().as_secs_f64();
        // No native full-LHS consumer rule runs on this lane.
        Ok(report.updated || changed)
    }
}

fn bind_atom(atom: &CellQueryAtom, row: &[Value], slots: usize) -> Option<Binding> {
    if row.len() != atom.columns.len() {
        return None;
    }
    let mut binding = vec![None; slots];
    for (term, &v) in atom.columns.iter().zip(row) {
        match term {
            CellOperand::Slot(i) => {
                if binding[*i].is_some_and(|x| x != v) {
                    return None;
                }
                binding[*i] = Some(v)
            }
            CellOperand::Constant { value, .. } if *value != v => return None,
            _ => (),
        }
    }
    Some(binding)
}

// Offline only: finite positive query projection on the analyzed symbolic Cell.
// All matching rows and alias alternatives are retained, including non-root rows.
fn project(t: &Template, sink: &CellActionSink, p: &Part) -> Vec<Lens> {
    let hidden: BTreeSet<_> = t.state["subsumed_rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Json::as_u64)
        .collect();
    let mut solutions = vec![Lens {
        binding: vec![None; sink.slots().len()],
        aliases: vec![],
    }];
    for &a in &p.atoms {
        let atom = &sink.atoms[a];
        let mut next = BTreeSet::new();
        for (ri, r) in t.state["rows"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(i, r)| !hidden.contains(&(*i as u64)) && r["op"] == atom.function)
        {
            let _ = ri;
            let cols: Vec<_> = r["args"]
                .as_array()
                .unwrap()
                .iter()
                .chain([&r["result"]])
                .map(|v| v.as_u64().unwrap() as usize)
                .collect();
            if cols.len() != atom.columns.len() {
                continue;
            }
            for old in &solutions {
                let mut row = old.clone();
                let mut valid = true;
                for ((term, &v), sort) in atom.columns.iter().zip(&cols).zip(&atom.sorts) {
                    if t.state["values"][v]["sort"] != *sort {
                        valid = false;
                        break;
                    }
                    match term {
                        CellOperand::Slot(i) => {
                            if let Some(x) = row.binding[*i] {
                                if x != v {
                                    row.aliases.push((x, v));
                                }
                            } else {
                                row.binding[*i] = Some(v)
                            }
                        }
                        CellOperand::Constant { text, .. } => {
                            if t.state["values"][v]["literal"] != *text {
                                valid = false;
                                break;
                            }
                        }
                    }
                }
                if valid {
                    next.insert(row);
                }
            }
        }
        solutions = next.into_iter().collect();
    }
    solutions
}

fn resolve(eg: &EGraph, t: &Template, params: &[Value], reads: &mut usize) -> Option<Vec<Value>> {
    let mut values = vec![None; t.state["values"].as_array()?.len()];
    for (&i, &v) in &t.literals {
        values[i] = Some(v)
    }
    for ((i, _), v) in t.parameters.iter().zip(params) {
        let sort = eg.get_sort_by_name(t.state["values"][*i]["sort"].as_str()?)?;
        let v = eg.get_canonical_value(*v, sort);
        if values[*i].is_some_and(|old| old != v) {
            return None;
        }
        values[*i] = Some(v);
    }
    let hidden: BTreeSet<_> = t.state["subsumed_rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Json::as_u64)
        .collect();
    let mut pending: Vec<_> = t.state["rows"]
        .as_array()?
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            !hidden.contains(&(*i as u64)) && !t.markers.contains(r["op"].as_str().unwrap())
        })
        .map(|(_, r)| r)
        .collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut later = vec![];
        for r in pending {
            let args: Option<Vec<_>> = r["args"]
                .as_array()?
                .iter()
                .map(|v| values[v.as_u64()? as usize])
                .collect();
            let Some(args) = args else {
                later.push(r);
                continue;
            };
            *reads += 1;
            let row = eg.cell_visible_row(r["op"].as_str()?, &args)?;
            let v = *row.last()?;
            let out = r["result"].as_u64()? as usize;
            if values[out].is_some_and(|old| old != v) {
                return None;
            }
            values[out] = Some(v);
        }
        if later.len() == before {
            return None;
        }
        pending = later;
    }
    values.into_iter().collect()
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    #[test]
    fn new_input_aliases_enable_an_internal_nonroot_projection() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None,"(datatype E (V i64) (Pair E E) (Box E)) (relation Hit (E)) (rule ((Pair x x)) ((Hit x)) :name \"diagonal\") (Box (Pair (V 10) (V 11)))").unwrap();
        let sink = eg.prepare_cell_action("", "diagonal").unwrap();
        let state = serde_json::json!({"values":[{"sort":"E"},{"sort":"E"},{"sort":"E"},{"sort":"E"}],"rows":[{"op":"Pair","args":[0,1],"result":2},{"op":"Box","args":[2],"result":3}]});
        let template = Template {
            id: 0,
            state,
            parameters: vec![(0, "a".into()), (1, "b".into())],
            producer: "unused".into(),
            markers: BTreeSet::new(),
            literals: BTreeMap::new(),
        };
        let lenses = project(
            &template,
            &sink,
            &Part {
                atoms: vec![0],
                cell: true,
            },
        );
        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].aliases, vec![(0, 1)]);
        let mut eval = |s: &str| {
            let e = egglog::ast::Parser::default()
                .get_expr_from_string(None, s)
                .unwrap();
            eg.eval_expr(&e).unwrap().1
        };
        let params = vec![eval("(V 10)"), eval("(V 11)")];
        let before = resolve(&eg, &template, &params, &mut 0).unwrap();
        assert_ne!(before[0], before[1]);
        eg.parse_and_run_program(None, "(union (V 10) (V 11))")
            .unwrap();
        let after = resolve(&eg, &template, &params, &mut 0).unwrap();
        assert_eq!(after[0], after[1]);
        assert_eq!(eg.audit_cell_matches(&sink).unwrap().len(), 1);
    }
}
