//! Native Cell/external-fragment events -> shared matrix guards/RHS/min-max ->
//! one native set per changed canonical key. The original consumer LHS is not run
//! except by the optional oracle. Mutable numbers never become closure constants.
use super::*;
use crate::{matrix_service::Service, numeric as n, program as ir};
use egglog::{
    ast::Action,
    cell_assist::{CellI64Check, CellI64Group, CellI64Proposal},
    cell_events::{CellCommitFeed, CellProducerTap},
};
use serde_json::json;

struct Compiler {
    exprs: Vec<n::Expr>,
    inputs: Vec<usize>,
    slots: Vec<(String, String)>,
}
impl Compiler {
    fn expression(&mut self, e: &Expr) -> Result<usize> {
        let op = match e {
            Expr::Var(_, name) => {
                let slot = self
                    .slots
                    .iter()
                    .position(|(s, _)| s == name)
                    .ok_or("numeric variable absent from LHS")?;
                if self.slots[slot].1 != "i64" {
                    return Err("numeric expression needs i64 payloads".into());
                }
                let index = self.inputs.iter().position(|i| *i == slot).unwrap();
                n::Expr::Input { slot: index }
            }
            Expr::Lit(_, Literal::Int(value)) => n::Expr::Const { value: *value },
            Expr::Call(_, op, args) if args.len() == 2 => {
                let a = self.expression(&args[0])?;
                let b = self.expression(&args[1])?;
                match op.as_str() {
                    "+" => n::Expr::Add { a, b },
                    "-" => n::Expr::Sub { a, b },
                    "*" => n::Expr::Mul { a, b },
                    "min" => n::Expr::Min { a, b },
                    "max" => n::Expr::Max { a, b },
                    _ => return Err(format!("unsupported numeric operator {op}").into()),
                }
            }
            _ => return Err("unsupported numeric expression".into()),
        };
        // Whole shared expression columns, not one AST copy per match.
        let key = serde_json::to_value(&op)?;
        if let Some(i) = self
            .exprs
            .iter()
            .position(|e| serde_json::to_value(e).unwrap() == key)
        {
            return Ok(i);
        }
        let i = self.exprs.len();
        self.exprs.push(op);
        Ok(i)
    }
}
#[derive(Default, Serialize)]
pub struct VerificationStats {
    pub enabled: bool,
    pub batches: usize,
    pub native_bindings: usize,
    pub proposals: usize,
    pub groups: usize,
    pub failures: usize,
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub native_matches: usize,
    pub producer_events: usize,
    pub row_events: usize,
    pub union_events: usize,
    pub source_point_reads: usize,
    pub external_point_reads: usize,
    pub channels: usize,
    pub accepted_bindings: usize,
    pub native_sets: usize,
    pub quiet_rounds: usize,
    pub audit_uncovered: Vec<usize>,
    pub verification: VerificationStats,
    pub marker_work: Json,
    pub physical_layout: Json,
    pub round_work: Vec<Json>,
    pub cartesian_combinations_bound: u64,
    pub device: Json,
}
struct Frame {
    part: usize,
    binding: Binding,
}
pub struct NumericSession {
    source: Session,
    tap: CellProducerTap,
    fields: Vec<Vec<String>>,
    feed: CellCommitFeed,
    writer: CellActionSink,
    function: String,
    merge: n::Merge,
    keys: Vec<CellOperand>,
    value_expr: usize,
    guards: Vec<(usize, usize, n::Compare)>,
    numeric_slots: Vec<usize>,
    device: Service,
    program: ir::Program,
    identities: BTreeMap<(String, u32), u32>,
    frames: BTreeMap<usize, Frame>,
    active: BTreeSet<usize>,
    offers: BTreeSet<Vec<usize>>,
    bindings: Vec<Vec<Value>>,
    group_keys: Vec<Vec<Value>>,
    retired: Vec<usize>,
    epoch: usize,
    acknowledged: BTreeSet<usize>,
    checker: Option<CellI64Check>,
    check_failed: bool,
    markers: crate::marker_layout::Markers,
    indexed_markers: bool,
    condition_ids: BTreeMap<(ir::Wire, ir::Wire), usize>,
    pub stats: Stats,
}
impl NumericSession {
    pub fn new(mut source: Session, env: &str, worker: &Path) -> Result<Self> {
        if source.stats.epochs != 0 {
            return Err("attach numeric matrix before input".into());
        }
        for name in ["+", "-", "*", "min", "max"] {
            if source.eg.get_function(name).is_some() {
                return Err(
                    format!("shadowed arithmetic operator {name} needs resolved lowering").into(),
                );
            }
        }
        let commands = source.eg.parse_program(None, env)?;
        let rule = commands
            .iter()
            .find_map(|c| match c {
                Command::Rule { rule }
                    if rule.name == source.sink.name && rule.ruleset == source.ruleset =>
                {
                    Some(rule)
                }
                _ => None,
            })
            .ok_or("consumer source missing")?;
        let [Action::Set(_, function, key_exprs, rhs)] = &rule.head.0[..] else {
            return Err("numeric matrix consumer must have exactly one set action; extra effects stay native".into());
        };
        let f = source
            .eg
            .get_function(function)
            .ok_or("unknown target function")?;
        if f.is_constructor() || f.schema().output.name() != "i64" {
            return Err("numeric merge target must be a custom i64 function".into());
        }
        let merge = commands
            .iter()
            .find_map(|c| match c {
                Command::Function { name, merge, .. } if name == function => merge.as_ref(),
                _ => None,
            })
            .ok_or("numeric merge requires an explicit min/max expression")?;
        let Expr::Call(_, op, args) = merge else {
            return Err("order-dependent/custom merge is not a numeric reduction".into());
        };
        let names = args
            .iter()
            .filter_map(|e| match e {
                Expr::Var(_, v) => Some(v.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if args.len() != 2 || names != BTreeSet::from(["old", "new"]) {
            return Err("only exact min/max(old,new) merges are reduced".into());
        }
        let merge = match op.as_str() {
            "min" => n::Merge::Min,
            "max" => n::Merge::Max,
            _ => return Err("custom merge needs a separate algebra/ordering proof".into()),
        };
        let slots = source
            .sink
            .slots()
            .iter()
            .map(|(n, s)| (n.to_string(), s.to_string()))
            .collect::<Vec<_>>();
        let numeric_slots = slots
            .iter()
            .enumerate()
            .filter(|(_, (_, s))| s == "i64")
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let mut compiler = Compiler {
            exprs: vec![],
            inputs: numeric_slots.clone(),
            slots: slots.clone(),
        };
        let value_expr = compiler.expression(rhs)?;
        let mut guards = vec![];
        for g in &source.sink.guards {
            let mut exprs = vec![];
            for operand in &g.operands {
                let expression = match operand {
                    CellOperand::Slot(i) => Expr::Var(rule.span.clone(), slots[*i].0.clone()),
                    CellOperand::Constant { text, .. } => {
                        egglog::ast::Parser::default().get_expr_from_string(None, text)?
                    }
                };
                exprs.push(compiler.expression(&expression)?);
            }
            guards.push((
                exprs[0],
                exprs[1],
                match g.opcode {
                    0 => n::Compare::Lt,
                    1 => n::Compare::Gt,
                    2 => n::Compare::Le,
                    3 => n::Compare::Ge,
                    _ => return Err("unknown native guard opcode".into()),
                },
            ));
        }
        let keys = key_exprs
            .iter()
            .map(|e| match e {
                Expr::Var(_, name) => slots
                    .iter()
                    .position(|(s, _)| s == name)
                    .map(CellOperand::Slot)
                    .ok_or_else(|| "function key absent from binding".into()),
                Expr::Lit(..) => {
                    let (_, value) = source.eg.eval_expr(e)?;
                    Ok(CellOperand::Constant {
                        value,
                        text: e.to_string(),
                    })
                }
                _ => Err("computed function keys require explicit native construction".into()),
            })
            .collect::<Result<Vec<_>>>()?;
        let fields = source
            .native
            .iter()
            .map(|r| {
                source
                    .templates
                    .iter()
                    .filter(|t| t.producer == *r)
                    .flat_map(|t| t.parameters.iter().map(|(_, v)| v.clone()))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let tap = source.eg.prepare_cell_producers(
            &source.ruleset,
            &source
                .native
                .iter()
                .cloned()
                .zip(fields.clone())
                .collect::<Vec<_>>(),
        )?;
        let mut tables = source
            .sink
            .atoms
            .iter()
            .map(|a| a.function.clone())
            .chain([function.clone()])
            .collect::<BTreeSet<_>>();
        for t in &source.templates {
            for r in t.state["rows"].as_array().unwrap() {
                let op = r["op"].as_str().unwrap();
                if !t.markers.contains(op) {
                    tables.insert(op.into());
                }
            }
        }
        let (feed, initial) = source
            .eg
            .subscribe_cell_commits(&tables.into_iter().collect::<Vec<_>>())?;
        if !initial.rows.is_empty() {
            return Err("subscribe numeric matrix before input facts".into());
        }
        let writer_name = "CellMatrixNumericWrite";
        let writer_rules = "cell-matrix-numeric-commit";
        if source.eg.get_function(writer_name).is_some() {
            return Err("reserved numeric writer name exists".into());
        }
        let key_sorts = source
            .eg
            .get_function(function)
            .unwrap()
            .schema()
            .input
            .iter()
            .map(|s| s.name().to_owned())
            .collect::<Vec<_>>();
        let vars = (0..key_sorts.len())
            .map(|i| format!("k{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        source.eg.parse_and_run_program(None,&format!("(ruleset {writer_rules}) (function {writer_name} ({} i64) Unit :merge old) (rule (({writer_name} {vars} value)) ((set ({function} {vars}) value)) :ruleset {writer_rules} :name \"matrix-numeric-commit\")",key_sorts.join(" ")))?;
        let writer = source
            .eg
            .prepare_cell_action(writer_rules, "matrix-numeric-commit")?;
        let width = slots.len().max(1);
        let program = ir::Program {
            version: 1,
            bits: 32,
            slots: width,
            nodes: (0..width)
                .map(|slot| ir::Node {
                    op: ir::Op::Port,
                    args: vec![],
                    slot,
                })
                .collect(),
            cells: vec![],
            templates: (0..source.parts.len())
                .map(|i| json!({"kind":"typed partial-query fragment","part":source.parts[i]}))
                .collect(),
            conditions: vec![],
            channels: vec![],
            stages: vec![vec![]],
            updates: vec![],
            numeric: Some(n::Plan {
                exprs: compiler.exprs,
                ..Default::default()
            }),
            evidence: json!({"source":"analyze Cell projections and current native function rows","scope":"one original set action, exact i64 guards/arithmetic, min/max merge; native canonical keys pinned during dispatch"}),
        };
        let marker_columns = source
            .parts
            .iter()
            .map(|p| {
                p.atoms
                    .iter()
                    .flat_map(|i| source.sink.atoms[*i].columns.iter())
                    .filter_map(|v| match v {
                        CellOperand::Slot(i) => Some(*i),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            })
            .collect();
        Ok(Self {
            source,
            tap,
            fields,
            feed,
            writer,
            function: function.clone(),
            merge,
            keys,
            value_expr,
            guards,
            numeric_slots,
            device: Service::new(worker)?,
            program,
            identities: BTreeMap::new(),
            frames: BTreeMap::new(),
            active: BTreeSet::new(),
            offers: BTreeSet::new(),
            bindings: vec![],
            group_keys: vec![],
            retired: vec![],
            epoch: 0,
            acknowledged: BTreeSet::new(),
            checker: None,
            check_failed: false,
            markers: crate::marker_layout::Markers::new(marker_columns),
            indexed_markers: false,
            condition_ids: BTreeMap::new(),
            stats: Stats::default(),
        })
    }
    pub fn engine(&self) -> &EGraph {
        &self.source.eg
    }
    pub fn configure_layout(
        &mut self,
        marker: &str,
        planner: crate::layout::Planner,
    ) -> Result<()> {
        if self.epoch != 0 || !self.program.cells.is_empty() {
            return Err("configure layout before execution".into());
        }
        self.indexed_markers = match marker {
            "indexed" => true,
            "cartesian" => false,
            _ => return Err("marker layout must be indexed or cartesian".into()),
        };
        self.device.planner = planner;
        Ok(())
    }
    /// Optional strict native oracle. Off means no checker query, RHS replay or
    /// shadow graph. A failed check poisons this session before any batch write.
    pub fn set_gpu_check(&mut self, enabled: bool) -> Result<()> {
        if self.check_failed {
            return Err("GPU check failed; start a new session".into());
        }
        if enabled && self.checker.is_none() {
            self.checker = Some(
                self.source
                    .eg
                    .prepare_cell_i64_check(&self.source.ruleset, &self.source.sink)?,
            );
        } else if !enabled {
            if let Some(checker) = self.checker.take() {
                self.source.eg.free_cell_i64_check(checker);
            }
        }
        self.stats.verification.enabled = enabled;
        Ok(())
    }
    pub fn input(&mut self, text: &str) -> Result<()> {
        let c = self.source.eg.parse_program(None, text)?;
        if c.iter()
            .any(|c| !matches!(c, Command::Action(..) | Command::Check(..)))
        {
            return Err("numeric input keeps the rule environment frozen".into());
        }
        self.source.eg.run_program(c)?;
        Ok(())
    }
    fn observe(&mut self, rule: usize, values: &[Value]) {
        for (tid, t) in self
            .source
            .templates
            .iter()
            .enumerate()
            .filter(|(_, t)| t.producer == self.source.native[rule])
        {
            let p = t
                .parameters
                .iter()
                .map(|(_, n)| {
                    self.fields[rule]
                        .iter()
                        .position(|f| f == n)
                        .map(|i| values[i])
                })
                .collect::<Option<Vec<_>>>();
            if let Some(parameters) = p {
                if self.source.instance_keys.insert((tid, parameters.clone())) {
                    self.source.instances.push(Instance {
                        template: tid,
                        parameters,
                    });
                }
            }
        }
    }
    fn fragments(&mut self) -> Result<Vec<Vec<usize>>> {
        let mut parts = vec![BTreeSet::new(); self.source.parts.len()];
        for inst in &self.source.instances {
            let t = &self.source.templates[inst.template];
            if let Some(v) = resolve(
                &self.source.eg,
                t,
                &inst.parameters,
                &mut self.stats.source_point_reads,
            ) {
                for (pi, lenses) in self.source.lenses[inst.template].iter().enumerate() {
                    for l in lenses {
                        if l.aliases.iter().all(|(a, b)| v[*a] == v[*b]) {
                            parts[pi].insert(
                                l.binding
                                    .iter()
                                    .map(|x| x.map(|i| v[i]))
                                    .collect::<Binding>(),
                            );
                        }
                    }
                }
            }
        }
        for (pi, p) in self
            .source
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.cell)
        {
            let atom = &self.source.sink.atoms[p.atoms[0]];
            for key in &self.source.external[&atom.function] {
                self.stats.external_point_reads += 1;
                if let Some(row) = self.source.eg.cell_visible_row(&atom.function, key) {
                    if let Some(b) = bind_atom(atom, &row, self.source.sink.slots().len()) {
                        parts[pi].insert(b);
                    }
                }
            }
        }
        let mut old = self
            .active
            .iter()
            .map(|i| ((self.frames[i].part, self.frames[i].binding.clone()), *i))
            .collect::<BTreeMap<_, _>>();
        let mut active = BTreeSet::new();
        let mut choices = vec![];
        for (pi, bindings) in parts.into_iter().enumerate() {
            let mut ids = vec![];
            for binding in bindings {
                let key = (pi, binding.clone());
                let id = if let Some(id) = old.get(&key) {
                    *id
                } else {
                    let mut codes = vec![];
                    for (i, v) in binding.iter().enumerate() {
                        let code = if let Some(v) = v {
                            let key = (self.source.sink.slots()[i].1.to_owned(), v.rep());
                            let next = u32::try_from(self.identities.len() + 1)?;
                            *self.identities.entry(key).or_insert(next)
                        } else {
                            0
                        };
                        codes.push(code);
                    }
                    let partition = (0..codes.len())
                        .map(|i| codes.iter().position(|x| *x == codes[i]).unwrap())
                        .collect();
                    let id = self.program.cells.len();
                    self.program.cells.push(ir::Cell {
                        template: pi,
                        binding: Some(codes),
                        partition,
                    });
                    self.frames.insert(id, Frame { part: pi, binding });
                    old.insert(key, id);
                    id
                };
                ids.push(id);
                active.insert(id);
            }
            choices.push(ids);
        }
        self.retired = self.active.difference(&active).copied().collect();
        self.active = active;
        Ok(choices)
    }
    fn audit_current(&mut self, decisions: &[Json]) -> Result<()> {
        if !self.source.audit {
            return Ok(());
        }
        let expected = self
            .source
            .eg
            .audit_cell_matches(&self.source.sink)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        let actual = decisions
            .iter()
            .enumerate()
            .filter(|(i, d)| {
                **d == 1
                    && self.program.channels[*i]
                        .sources
                        .iter()
                        .all(|c| self.active.contains(c))
            })
            .map(|(i, _)| self.bindings[i].clone())
            .collect::<BTreeSet<_>>();
        if !actual.is_subset(&expected) {
            return Err("numeric matrix produced a non-native match".into());
        }
        self.stats
            .audit_uncovered
            .push(expected.difference(&actual).count());
        Ok(())
    }
    fn verify_gpu(&mut self, result: &Json) -> Result<()> {
        let Some(checker) = &self.checker else {
            return Ok(());
        };
        let decisions = result["decisions"]
            .as_array()
            .ok_or("GPU check: missing decisions")?;
        if decisions.len() != self.program.channels.len() {
            return Err("GPU check: decision shape mismatch".into());
        }
        let active = |i: usize| {
            self.program.channels[i]
                .sources
                .iter()
                .all(|c| self.active.contains(c))
        };
        let accepted = decisions
            .iter()
            .enumerate()
            .filter(|(i, d)| **d == 1 && active(*i))
            .map(|(i, _)| self.bindings[i].clone())
            .collect::<Vec<_>>();
        let plan = self.program.numeric.as_ref().unwrap();
        let numeric = &result["numeric"];
        let values = numeric["values"]
            .as_array()
            .ok_or("GPU check: missing numeric values")?;
        let versions = numeric["versions"]
            .as_array()
            .ok_or("GPU check: missing numeric versions")?;
        let proposals = numeric["proposals"]
            .as_array()
            .ok_or("GPU check: missing proposals")?;
        let statuses = numeric["status"]
            .as_array()
            .ok_or("GPU check: missing proposal statuses")?;
        if values.len() != plan.groups.len()
            || versions.len() != plan.groups.len()
            || proposals.len() != plan.writes.len()
            || statuses.len() != plan.writes.len()
        {
            return Err("GPU check: numeric shape mismatch".into());
        }
        let mut grouped = (0..plan.groups.len())
            .map(|_| vec![])
            .collect::<Vec<Vec<CellI64Proposal>>>();
        for (w, write) in plan.writes.iter().enumerate() {
            if self.acknowledged.contains(&write.group) {
                continue;
            }
            let c = write.channel;
            if decisions[c] == 1 && active(c) {
                if statuses[w] != 1 {
                    return Err(format!("GPU check: missing proposal {w}").into());
                }
                grouped[write.group].push(CellI64Proposal {
                    binding: self.bindings[c].clone(),
                    value: proposals[w].as_i64().ok_or("GPU check: invalid RHS i64")?,
                });
            } else if statuses[w] == 1 {
                return Err(
                    format!("GPU check: proposal {w} has no active accepted binding").into(),
                );
            }
        }
        let mut groups = vec![];
        for (g, proposals) in grouped.into_iter().enumerate() {
            if self.acknowledged.contains(&g) {
                continue;
            }
            let value = if values[g].is_null() {
                None
            } else {
                Some(values[g].as_i64().ok_or("GPU check: invalid merged i64")?)
            };
            let initial = plan.groups[g].initial;
            let version = versions[g].as_u64().ok_or("GPU check: invalid version")?;
            // Each group is immutable and reduced once, including hot resume.
            if version != u64::from(value != initial) {
                return Err(format!("GPU check: group {g} change/version mismatch").into());
            }
            groups.push(CellI64Group {
                key: self.group_keys[g].clone(),
                initial,
                proposals,
                result: value,
            });
        }
        let report =
            self.source
                .eg
                .check_cell_i64_batch(&self.source.sink, checker, &accepted, &groups)?;
        self.stats.verification.batches += 1;
        self.stats.verification.native_bindings += report.native_bindings;
        self.stats.verification.proposals += report.proposals;
        self.stats.verification.groups += report.groups;
        if self.source.audit {
            self.stats.audit_uncovered.push(0);
        }
        Ok(())
    }
    fn check_before_commit(&mut self, result: &Json) -> Result<()> {
        if let Err(e) = self.verify_gpu(result) {
            self.check_failed = true;
            self.stats.verification.failures += 1;
            self.stats.device = result.clone();
            return Err(e);
        }
        Ok(())
    }
    pub fn round(&mut self) -> Result<bool> {
        if self.check_failed {
            return Err("GPU check failed; start a new session".into());
        }
        let native = self
            .source
            .eg
            .step_cell_producers(&self.tap, &self.source.ruleset)?;
        self.stats.native_matches += native.num_matches_per_rule.values().sum::<usize>();
        let delta = self.source.eg.drain_cell_commits(&mut self.feed)?;
        let events = self.tap.drain();
        self.stats.row_events += delta.rows.len();
        self.stats.union_events += delta.unions.len();
        self.stats.producer_events += events.len();
        if delta.rows.is_empty()
            && delta.unions.is_empty()
            && delta.clears.is_empty()
            && events.is_empty()
        {
            self.stats.quiet_rounds += 1;
            return Ok(native.updated);
        }
        for e in events {
            self.observe(e.rule, &e.values)
        }
        for table in delta.clears {
            let name = &self.feed.tables[table].name;
            if let Some(keys) = self.source.external.get_mut(name) {
                keys.clear();
            }
        }
        for row in delta.rows {
            let name = &self.feed.tables[row.table].name;
            if let Some(keys) = self.source.external.get_mut(name) {
                if row.visible {
                    keys.insert(row.key);
                } else {
                    keys.remove(&row.key);
                }
            }
        }
        self.epoch += 1;
        let choices = self.fragments()?;
        let old_channels = self.program.channels.len();
        self.stats.cartesian_combinations_bound =
            self.stats.cartesian_combinations_bound.saturating_add(
                choices
                    .iter()
                    .fold(1u64, |a, c| a.saturating_mul(c.len() as u64)),
            );
        let combinations = if self.indexed_markers {
            let combinations = self.markers.combinations(
                self.active
                    .iter()
                    .map(|id| (*id, self.frames[id].part, &self.frames[id].binding)),
            );
            self.stats.marker_work = serde_json::to_value(&self.markers.stats)?;
            combinations
        } else {
            let mut combinations = vec![vec![]];
            for choices in choices {
                let mut next = vec![];
                for c in combinations {
                    for i in &choices {
                        let mut c = c.clone();
                        c.push(*i);
                        next.push(c);
                    }
                }
                combinations = next;
            }
            combinations
        };
        let mut groups = BTreeMap::new();
        let slots = self.source.sink.slots();
        for sources in combinations {
            if !self.offers.insert(sources.clone()) {
                continue;
            }
            let mut bound = vec![None; slots.len()];
            let mut equalities = vec![];
            for cell in &sources {
                for (slot, v) in self.frames[cell].binding.iter().enumerate() {
                    if let Some(v) = v {
                        if let Some((prior, _)) = bound[slot] {
                            equalities.push((ir::Wire(prior, slot), ir::Wire(*cell, slot)));
                        } else {
                            bound[slot] = Some((*cell, *v));
                        }
                    }
                }
            }
            let binding = bound
                .into_iter()
                .map(|v| {
                    v.map(|(_, v)| v)
                        .ok_or("numeric partition lacks a native variable")
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut key = vec![];
            let schema = self
                .source
                .eg
                .get_function(&self.function)
                .unwrap()
                .schema();
            for (k, sort) in self.keys.iter().zip(&schema.input) {
                let value = match k {
                    CellOperand::Slot(i) => binding[*i],
                    CellOperand::Constant { value, .. } => *value,
                };
                key.push(self.source.eg.get_canonical_value(value, sort));
            }
            let group = if let Some(g) = groups.get(&key) {
                *g
            } else {
                let initial = self
                    .source
                    .eg
                    .lookup_function_row(&self.function, &key)
                    .map(|r| self.source.eg.value_to_base::<i64>(r[key.len()]));
                let g = self.group_keys.len();
                self.group_keys.push(key.clone());
                self.program
                    .numeric
                    .as_mut()
                    .unwrap()
                    .groups
                    .push(n::Group {
                        scope: format!("{}:native-boundary:{}", self.function, self.epoch),
                        key: key.iter().map(|v| v.rep()).collect(),
                        merge: self.merge,
                        initial,
                    });
                groups.insert(key, g);
                g
            };
            let ci = self.program.channels.len();
            let mut req = vec![];
            for (left, right) in equalities {
                let key = if left < right {
                    (left, right)
                } else {
                    (right, left)
                };
                let condition = if self.indexed_markers {
                    self.condition_ids.get(&key).copied()
                } else {
                    None
                };
                let condition = condition.unwrap_or_else(|| {
                    let i = self.program.conditions.len();
                    self.program.conditions.push(ir::Condition { left, right });
                    if self.indexed_markers {
                        self.condition_ids.insert(key, i);
                    }
                    i
                });
                req.push(ir::Requirement {
                    condition,
                    depth: 0,
                });
            }
            self.program.channels.push(ir::Channel {
                label: format!("{}:{ci}", self.source.sink.name),
                sources,
                alternatives: vec![req],
                target: None,
            });
            self.program.stages[0].push(ci);
            let n = self.program.numeric.as_mut().unwrap();
            n.inputs.push(
                self.numeric_slots
                    .iter()
                    .map(|i| self.source.eg.value_to_base::<i64>(binding[*i]))
                    .collect(),
            );
            n.state_bindings.push(vec![]);
            n.guards
                .extend(self.guards.iter().map(|(left, right, compare)| n::Guard {
                    channel: ci,
                    left: *left,
                    right: *right,
                    compare: *compare,
                    depth: 0,
                }));
            n.writes.push(n::Write {
                channel: ci,
                group,
                value: self.value_expr,
            });
            self.bindings.push(binding);
            self.stats.channels += 1;
        }
        if self.program.cells.is_empty()
            || (self.program.channels.len() == old_channels && self.retired.is_empty())
        {
            if self.checker.is_some() {
                let result = if self.stats.device.is_null() {
                    json!({"decisions":[], "numeric":{"values":[], "versions":[], "proposals":[], "status":[]}})
                } else {
                    self.stats.device.clone()
                };
                self.check_before_commit(&result)?;
                return Ok(native.updated);
            }
            let decisions = self.stats.device["decisions"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            self.audit_current(&decisions)?;
            return Ok(native.updated);
        }
        // Every old scalar/key snapshot stays immutable. A changed native row
        // creates new fragments/channels and a new canonical-key group version.
        self.device.install(&self.program)?;
        self.stats.physical_layout = self.device.layout_report.clone();
        let result = self.device.step(false, &self.retired)?;
        self.stats.round_work.push(json!({"new_channels":self.program.channels.len()-old_channels,"levels":result["levels"],"layout_work":result["layout_work"],"matrix_bytes":result["matrix_bytes"],"host_count_reads":result["host_count_reads"]}));
        if result["numeric"]["arithmetic_error"] == true {
            self.stats.device = result;
            return Err(
                "checked i64 RHS failed; no numeric group from this batch committed".into(),
            );
        }
        let decisions = result["decisions"]
            .as_array()
            .ok_or("missing numeric decisions")?;
        self.check_before_commit(&result)?;
        if self.checker.is_none() {
            self.audit_current(decisions)?;
        }
        self.stats.accepted_bindings += decisions[old_channels..]
            .iter()
            .filter(|d| **d == 1)
            .count();
        let mut writes = vec![];
        for g in 0..self.group_keys.len() {
            if self.acknowledged.contains(&g) {
                continue;
            }
            if result["numeric"]["versions"][g] == 0 {
                continue;
            }
            let value = result["numeric"]["values"][g]
                .as_i64()
                .ok_or("missing merged i64")?;
            let key = &self.group_keys[g];
            let initial = self.program.numeric.as_ref().unwrap().groups[g].initial;
            let current = self
                .source
                .eg
                .lookup_function_row(&self.function, key)
                .map(|r| self.source.eg.value_to_base::<i64>(r[key.len()]));
            if current != initial {
                return Err("native numeric snapshot changed during matrix dispatch".into());
            }
            let numeric = self.source.eg.base_to_value(value);
            let unit = self.source.eg.base_to_value(());
            let mut row = vec![];
            for (name, sort) in self.writer.slots() {
                let v = if name == "value" {
                    numeric
                } else if let Some(i) = name.strip_prefix('k').and_then(|i| i.parse::<usize>().ok())
                {
                    *key.get(i).ok_or("writer key mapping")?
                } else if sort == "Unit" {
                    unit
                } else {
                    return Err(format!("writer has an unexpected operand {name}:{sort}").into());
                };
                row.push(v);
            }
            writes.push(row);
        }
        let count = writes.len();
        if count > 0 {
            self.source.eg.apply_cell_bindings(&self.writer, &writes)?;
        }
        self.acknowledged.extend(0..self.group_keys.len());
        self.stats.native_sets += count;
        self.stats.device = result;
        Ok(native.updated || count > 0)
    }
}
