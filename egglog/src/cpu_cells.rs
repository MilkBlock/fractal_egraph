//! Native CPU multi-stage Cells. Source matching and all storage operations stay
//! in egglog. Only positive saturation cones may add witnessed successor actions.
//! Finite/probed/effectful scopes execute the original native schedule.
mod archive;
pub mod staged;
use super::*;
use crate::core::{GenericAtomTerm as T, GenericCoreAction as A, ResolvedCoreRule};
use egglog_bridge::{DefaultVal, FunctionConfig, FunctionId, MergeFn, RuleId};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

type Result<T> = std::result::Result<T, Error>;
fn err(s: impl Into<String>) -> Error {
    Error::BackendError(s.into())
}
fn string(j: &J) -> Result<&str> {
    j.as_str().ok_or_else(|| err("CPU plan: expected string"))
}
fn array(j: &J) -> Result<&Vec<J>> {
    j.as_array().ok_or_else(|| err("CPU plan: expected array"))
}
fn index(j: &J) -> Result<usize> {
    j.as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| err("CPU plan: expected index"))
}
fn span() -> Span {
    Span::Rust(Arc::new(ast::RustSpan {
        file: file!(),
        line: line!(),
        column: column!(),
    }))
}

pub fn schedule_json(s: &Schedule) -> J {
    match s {
        Schedule::Run(_, r) => {
            let mut j = json!({"run":r.ruleset});
            if let Some(f) = &r.until {
                j["until"] = json!(f.iter().map(ToString::to_string).collect::<Vec<_>>());
            }
            j
        }
        Schedule::Sequence(_, ss) => {
            json!({"sequence":ss.iter().map(schedule_json).collect::<Vec<_>>()})
        }
        Schedule::Repeat(_, n, s) => json!({"repeat":n,"body":schedule_json(s)}),
        Schedule::Saturate(_, s) => json!({"saturate":schedule_json(s)}),
    }
}
fn collect(
    eg: &EGraph,
    set: &str,
    out: &mut Vec<(String, String, ResolvedCoreRule, RuleId)>,
) -> Result<()> {
    match eg
        .rulesets
        .get(set)
        .ok_or_else(|| err("unknown CPU ruleset"))?
    {
        Ruleset::Rules(rs) => out.extend(
            rs.iter()
                .map(|(n, (r, id))| (set.into(), n.clone(), r.clone(), *id)),
        ),
        Ruleset::Combined(ss) => {
            for s in ss {
                collect(eg, s, out)?
            }
        }
    }
    Ok(())
}
fn leaves(j: &J, out: &mut BTreeSet<String>) -> Result<()> {
    if let Some(s) = j.get("run") {
        if j.get("until").is_some() || j.get("until_run").is_some() || j.get("assert").is_some() {
            return Err(err("observable Run probe"));
        }
        out.insert(string(s)?.into());
    } else if let Some(ss) = j.get("sequence") {
        for s in array(ss)? {
            leaves(s, out)?
        }
    } else if let Some(s) = j.get("saturate") {
        leaves(s, out)?
    } else {
        return Err(err("finite Repeat is not a joint saturation"));
    }
    Ok(())
}
fn subtrees(j: &J, out: &mut Vec<J>) {
    match j {
        J::Array(a) => {
            for v in a {
                subtrees(v, out)
            }
        }
        J::Object(m) => {
            if m.contains_key("saturate") {
                out.push(j.clone())
            }
            for v in m.values() {
                subtrees(v, out)
            }
        }
        _ => {}
    }
}
fn table_positive(eg: &EGraph, n: &str) -> bool {
    let f = &eg.functions[n];
    f.is_constructor()
        || f.schema.output.name() == "Unit"
            && f.decl
                .merge
                .as_ref()
                .is_some_and(|e| e.to_string() == "old")
}
fn harmless_merge(expr: Option<&ResolvedExpr>, output: &str) -> bool {
    let Some(expr) = expr else {
        return false;
    };
    if output == "Unit" && matches!(expr.to_string().as_str(), "old" | "new" | "()") {
        return true;
    }
    matches!(expr, GenericExpr::Call(_, ResolvedCall::Primitive(p), args)
        if output == "i64" && matches!(p.name(), "min" | "max") && args.len() == 2
            && ((args[0].to_string() == "old" && args[1].to_string() == "new")
             || (args[0].to_string() == "new" && args[1].to_string() == "old")))
}
fn positive(eg: &EGraph, r: &ResolvedCoreRule, id: RuleId) -> bool {
    if !eg.backend.device_rule_is_seminaive(id) {
        return false;
    }
    let ok_sort =
        |s: &ArcSort| s.is_eq_sort() || matches!(s.name(), "i64" | "bool" | "String" | "Unit");
    for a in &r.body.atoms {
        match &a.head {
            ResolvedCall::Func(f) => {
                if !table_positive(eg, &f.name) {
                    return false;
                }
            }
            ResolvedCall::Primitive(p) => {
                if p.input().iter().any(|s| s.is_eq_sort())
                    || !p.input().iter().chain([p.output()]).all(ok_sort)
                {
                    return false;
                }
            }
        }
    }
    r.head.0.iter().all(|a| match a {
        A::Let(_, _, ResolvedCall::Func(f), _) => eg.functions[&f.name].is_constructor(),
        A::Set(_, ResolvedCall::Func(f), _, _) => table_positive(eg, &f.name),
        A::LetAtomTerm(..) | A::Union(..) => true,
        _ => false,
    })
}
#[derive(Clone)]
struct Marker {
    table: FunctionId,
    params: Vec<ResolvedVar>,
    anchors: Vec<FunctionId>,
    private: bool,
}
struct Scope {
    control: J,
    variants: BTreeMap<RuleId, RuleId>,
    entries: Vec<(String, FunctionId, Vec<String>)>,
    certified: BTreeSet<RuleId>,
    rulesets: BTreeMap<String, Vec<RuleId>>,
    private_receipts: bool,
    read_tables: Vec<String>,
    has_union: bool,
    no_hidden: Arc<AtomicBool>,
}
/// Compiled native rules, native completion tables and scope activation only.
/// There is no second fact store, GPU worker, terminal replay or raw trace log.
/// Use only with the compiling EGraph; like compiled queries this is not portable
/// to unrelated graphs. Load the source artifact again for a fresh process.
pub struct CpuCells {
    scopes: Vec<Scope>,
    markers: BTreeMap<RuleId, Marker>,
    coverage: BTreeSet<RuleId>,
    inline: Arc<AtomicUsize>,
    rejected: Vec<J>,
    rejected_templates: Vec<J>,
    activations: usize,
    hidden_fallbacks: usize,
    native_runs: usize,
    optimized_runs: usize,
    hits: usize,
    unresolved: usize,
    skipped_queries: usize,
    planned_steps: usize,
    rule_reports: RunReport,
}

impl CpuCells {
    pub fn compile(eg: &mut EGraph, core: &J, plan: &J) -> Result<Self> {
        if eg.are_proofs_enabled() || eg.program_trace.is_some() || !eg.seminaive {
            return Err(err(
                "CPU Cells require ordinary seminaive execution without proof instrumentation",
            ));
        }
        if plan["schema"] != "eggfuse-cpu-cells/v1" || plan["core"] != *core {
            return Err(err("CPU artifact differs from typed source/control"));
        }
        let names = array(&core["rulesets"])?
            .iter()
            .map(|s| string(&s["name"]).map(str::to_owned))
            .collect::<Result<Vec<_>>>()?;
        let mut actual = eg.export_rule_program(&names).map_err(err)?;
        actual["schedule"] = core["schedule"].clone();
        if actual != *core {
            return Err(err("CPU artifact does not match this native egraph"));
        }
        let recipe = &plan["recipe"];
        let regions = &plan["regions"];
        let batch = &regions["batch_program"];
        if recipe["scope"] != "original_program" || batch["schema"] != "eggfuse-batch-program/v2" {
            return Err(err("CPU artifact scope/schema"));
        }
        let mut result = Self {
            scopes: vec![],
            markers: BTreeMap::new(),
            coverage: BTreeSet::new(),
            inline: Arc::new(AtomicUsize::new(0)),
            rejected: vec![],
            rejected_templates: vec![],
            activations: 0,
            hidden_fallbacks: 0,
            native_runs: 0,
            optimized_runs: 0,
            hits: 0,
            unresolved: 0,
            skipped_queries: 0,
            planned_steps: 0,
            rule_reports: RunReport::default(),
        };
        // Reordering constructor creation changes physical IDs and native join
        // enumeration. Even a consumer after this saturation can observe that
        // via an order-sensitive merge. A local dependency proof alone does not
        // certify full-program native endpoint preservation in that case.
        let choices = eg
            .functions
            .values()
            .filter(|f| {
                !f.is_constructor()
                    && f.schema
                        .input
                        .iter()
                        .chain([&f.schema.output])
                        .any(|s| s.is_eq_sort() || s.is_container_sort())
                    && !harmless_merge(f.decl.merge.as_ref(), f.schema.output.name())
            })
            .map(|f| f.decl.name.clone())
            .collect::<Vec<_>>();
        if !choices.is_empty() {
            result.rejected.push(json!({"reason":"order-sensitive function merge can observe native identity/enumeration changes; original native schedule", "functions":choices}));
            return Ok(result);
        }
        // A later schedule may observe this region too. Restrict admission to
        // positive components closed under all declared consumers, not just the
        // rules currently inside this saturation. This also catches scalar
        // `merge new` choices reached through a later ruleset.
        let mut global_rules = vec![];
        for (set, rs) in &eg.rulesets {
            if let Ruleset::Rules(rs) = rs {
                global_rules.extend(
                    rs.iter()
                        .map(|(name, (r, id))| (set.clone(), name.clone(), r.clone(), *id)),
                );
            }
        }
        let global_mixed = global_rules
            .iter()
            .any(|(_, _, r, id)| !positive(eg, r, *id));
        let global_safe = closed_positive_region(eg, &global_rules, global_mixed);
        let mut writers: BTreeMap<FunctionId, BTreeSet<RuleId>> = BTreeMap::new();
        for rs in eg.rulesets.values() {
            if let Ruleset::Rules(rs) = rs {
                for (_, (r, id)) in rs {
                    for a in &r.head.0 {
                        if let A::Let(_, _, ResolvedCall::Func(f), _)
                        | A::Set(_, ResolvedCall::Func(f), _, _) = a
                        {
                            writers
                                .entry(eg.functions[&f.name].backend_id)
                                .or_default()
                                .insert(*id);
                        }
                    }
                }
            }
        }
        let mut controls = vec![];
        subtrees(&core["schedule"], &mut controls);
        for (sid, s) in array(&batch["scopes"])?.iter().enumerate() {
            if !controls.contains(&s["control"]) {
                return Err(err("CPU scope not in original schedule"));
            }
            let mut sets = BTreeSet::new();
            leaves(&s["control"], &mut sets)?;
            if json!(sets.iter().collect::<Vec<_>>()) != s["rulesets"] {
                return Err(err("CPU scope rulesets differ"));
            }
            let mut rules = vec![];
            for name in &sets {
                collect(eg, name, &mut rules)?
            }
            rules.sort_by_key(|r| r.3);
            rules.dedup_by_key(|r| r.3);
            let mixed = rules.iter().any(|(_, _, r, id)| !positive(eg, r, *id));
            let mut safe = closed_positive_region(eg, &rules, mixed);
            safe.retain(|id| global_safe.contains(id));
            if safe.is_empty() {
                result.rejected.push(json!({"scope":sid,"rulesets":sets,"reason":"no closed positive region; original native schedule"}));
                continue;
            }
            let has_union = rules.iter().any(|(_, _, r, _)| {
                r.head.0.iter().any(|a| match a {
                    A::Union(..) => true,
                    A::Set(_, ResolvedCall::Func(f), _, _) => {
                        eg.functions[&f.name].is_constructor()
                    }
                    _ => false,
                })
            });
            if !array(&batch["templates"])?.iter().any(|t| {
                t["scope"].as_u64() == Some(sid as u64)
                    && t["steps"].as_array().is_some_and(|v| v.len() > 1)
            }) {
                result
                    .rejected
                    .push(json!({"scope":sid,"reason":"no multi-stage recipe"}));
                continue;
            }
            let no_hidden = Arc::new(AtomicBool::new(false));
            let planned_before = result.planned_steps;
            for (_, _, r, id) in &rules {
                if !safe.contains(id) {
                    continue;
                }
                if !result.markers.contains_key(id) {
                    let mut params = r.head.get_free_vars().into_iter().collect::<Vec<_>>();
                    params.sort_by(|a, b| a.name.cmp(&b.name));
                    let existing = effect_receipt(eg, r).filter(|(table, _)| {
                        writers
                            .get(table)
                            .is_some_and(|w| w.len() == 1 && w.contains(id))
                    });
                    let (table, private) = if let Some((table, key)) = existing {
                        params = key;
                        (table, false)
                    } else {
                        let unit = eg.backend.base_values().get(());
                        let ty = eg.backend.base_values().get_ty::<()>();
                        let schema = params
                            .iter()
                            .map(|v| v.sort.column_ty(&eg.backend))
                            .chain([ColumnTy::Base(ty)])
                            .collect();
                        let table = eg.backend.add_table(FunctionConfig {
                            schema,
                            default: DefaultVal::Const(unit),
                            merge: MergeFn::AssertEq,
                            name: format!("eggfuse_cpu_receipt_{id:?}"),
                            can_subsume: false,
                        });
                        (table, true)
                    };
                    let anchors = r
                        .body
                        .atoms
                        .iter()
                        .filter_map(|a| {
                            if let ResolvedCall::Func(f) = &a.head {
                                params
                                    .iter()
                                    .all(|v| a.args.iter().any(|x| matches!(x,T::Var(_,x) if x==v)))
                                    .then(|| eg.functions[&f.name].backend_id)
                            } else {
                                None
                            }
                        })
                        .collect();
                    result.markers.insert(
                        *id,
                        Marker {
                            table,
                            params,
                            anchors,
                            private,
                        },
                    );
                }
            }
            let allowed = safe;
            let mut variants = BTreeMap::new();
            let mut entries = vec![];
            let mut tables = BTreeSet::new();
            for (set, name, r, id) in &rules {
                if !positive(eg, r, *id) {
                    continue;
                }
                if allowed.contains(id) {
                    for a in &r.body.atoms {
                        if let ResolvedCall::Func(f) = &a.head {
                            tables.insert(f.name.clone());
                        }
                    }
                }
                let desc = format!("eggfuse.cpu.{sid}.{id:?}");
                let marker = result.markers.get(id).filter(|_| allowed.contains(id));
                let mut successor = None;
                for t in array(&batch["templates"])? {
                    if index(&t["scope"])? != sid {
                        continue;
                    }
                    let first = &array(&t["steps"])?[0];
                    let sr = &recipe["rules"][index(&first["recipe_rule"])?];
                    if sr["metadata"]["source_set"] == *set
                        && sr["metadata"]["name"] == *name
                        && array(&t["steps"])?.len() > 1
                    {
                        successor = Some(t);
                        break;
                    }
                }
                let prepared = successor
                    .map(|t| prepare(eg, core, recipe, t, r, *id, &result.markers, &allowed))
                    .transpose();
                let steps = match prepared {
                    Ok(Some(s)) => s,
                    Ok(None) => vec![],
                    Err(e) => {
                        result
                            .rejected_templates
                            .push(json!({"scope":sid,"rule":name,"reason":e.to_string()}));
                        vec![]
                    }
                };
                if steps.len() < 2 && marker.is_none() {
                    continue;
                }
                let mut guards = vec![];
                for step in steps.iter().skip(1) {
                    let mut checks = vec![];
                    let mut keys = vec![];
                    for a in &step.rule.body.atoms {
                        if let ResolvedCall::Func(f) = &a.head {
                            let table = egglog_bridge::TableAction::new(
                                &eg.backend,
                                eg.functions[&f.name].backend_id,
                            );
                            checks.push((table, a.args.len() - 1));
                            keys.extend_from_slice(&a.args[..a.args.len() - 1]);
                        }
                    }
                    let visible = no_hidden.clone();
                    let counter = result.inline.clone();
                    let unit = eg.backend.base_values().get(());
                    let id = eg
                        .backend
                        .register_external_func(Box::new(make_external_func(
                            move |state, values| {
                                if !visible.load(Ordering::Relaxed) {
                                    let mut at = 0;
                                    for (table, len) in &checks {
                                        if !table.visible_or_missing(state, &values[at..at + len]) {
                                            return None;
                                        }
                                        at += len;
                                    }
                                }
                                counter.fetch_add(1, Ordering::Relaxed);
                                Some(unit)
                            },
                        )));
                    guards.push((id, keys));
                }
                let mut tr = BackendRule::new(
                    eg.backend.new_rule(&desc, true),
                    &eg.functions,
                    &eg.type_info,
                    true,
                );
                let root = steps.first().map(|s| &s.rule).unwrap_or(r);
                tr.query(&root.body, false);
                tr.actions(&root.head)?;
                if let Some(m) = marker {
                    publish_marker(&mut tr, m, None);
                }
                for (step, (guard, keys)) in steps.iter().skip(1).zip(guards) {
                    let q = core::Query {
                        atoms: step
                            .rule
                            .body
                            .atoms
                            .iter()
                            .filter(|a| matches!(a.head, ResolvedCall::Primitive(_)))
                            .cloned()
                            .collect(),
                    };
                    tr.query(&q, false);
                    let mut args = tr.args(&keys);
                    args.push(tr.rb.egraph().base_value_constant(()));
                    let unit_type = tr.rb.egraph().base_values().get_ty::<()>();
                    tr.rb
                        .query_prim(guard, &args, ColumnTy::Base(unit_type))
                        .map_err(|e| err(e.to_string()))?;
                    tr.actions(&step.rule.head)?;
                    publish_marker(&mut tr, &result.markers[&step.id], Some(&step.subst));
                }
                result.planned_steps += steps.len().saturating_sub(1);
                let variant = tr.build();
                variants.insert(*id, variant);
                if let Some(marker) = marker {
                    entries.push((
                        desc,
                        marker.table,
                        marker.params.iter().map(|v| v.name.clone()).collect(),
                    ));
                }
            }
            if result.planned_steps == planned_before {
                result
                    .rejected
                    .push(json!({"scope":sid,"reason":"no independently proven successor"}));
                continue;
            }
            let mut rulesets = BTreeMap::new();
            for name in &sets {
                let mut native = vec![];
                collect(eg, name, &mut native)?;
                rulesets.insert(name.clone(), native.iter().map(|r| r.3).collect());
            }
            let private_receipts = allowed.iter().any(|id| result.markers[id].private);
            result.scopes.push(Scope {
                certified: allowed,
                rulesets,
                private_receipts,
                control: s["control"].clone(),
                variants,
                entries,
                read_tables: tables.into_iter().collect(),
                has_union,
                no_hidden,
            });
        }
        Ok(result)
    }
    fn clear(&self, eg: &mut EGraph) -> Result<()> {
        // Auxiliary bookkeeping is not a source Run: preserve its epoch and
        // merge ordering. Native clear bumps the table/index generation.
        for m in self.markers.values().filter(|m| m.private) {
            eg.backend.reset_auxiliary_table(m.table);
        }
        Ok(())
    }
    pub fn run(&mut self, eg: &mut EGraph, s: &Schedule) -> Result<RunReport> {
        self.walk(eg, s, None)
    }
    fn native(&mut self, eg: &mut EGraph, s: &Schedule) -> Result<RunReport> {
        let outputs = eg.run_program(vec![Command::RunSchedule(s.clone())])?;
        let report = outputs
            .into_iter()
            .find_map(|o| {
                if let CommandOutput::RunSchedule(r) = o {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap_or_default();
        self.native_runs += report.iterations.len();
        Ok(report)
    }
    fn relevant(&self, s: &Schedule) -> bool {
        if self.scopes.is_empty() {
            return false;
        }
        if matches!(s, Schedule::Saturate(..))
            && self.scopes.iter().any(|v| v.control == schedule_json(s))
        {
            return true;
        }
        match s {
            Schedule::Sequence(_, ss) => ss.iter().any(|s| self.relevant(s)),
            Schedule::Saturate(_, s) | Schedule::Repeat(_, _, s) => self.relevant(s),
            _ => false,
        }
    }
    fn walk(&mut self, eg: &mut EGraph, s: &Schedule, active: Option<usize>) -> Result<RunReport> {
        if active.is_none() && !self.relevant(s) {
            return self.native(eg, s);
        }
        if active.is_none() && matches!(s, Schedule::Saturate(..)) {
            let j = schedule_json(s);
            if let Some(id) = self.scopes.iter().position(|v| v.control == j) {
                let scope = &self.scopes[id];
                let mut hidden = false;
                {
                    for name in &scope.read_tables {
                        eg.backend
                            .for_each_while(eg.functions[name].backend_id, |row| {
                                hidden |= row.subsumed;
                                !hidden
                            });
                        if hidden {
                            break;
                        }
                    }
                }
                scope.no_hidden.store(!hidden, Ordering::Relaxed);
                if !hidden || !scope.has_union {
                    self.clear(eg)?;
                    self.coverage = self
                        .markers
                        .iter()
                        .filter(|(_, m)| m.private || eg.backend.table_size(m.table) == 0)
                        .map(|(id, _)| *id)
                        .collect();
                    self.activations += 1;
                    let answer = self.walk(eg, s, Some(id));
                    let cleanup = self.clear(eg);
                    return match answer {
                        Ok(r) => {
                            cleanup?;
                            Ok(r)
                        }
                        Err(e) => Err(e),
                    };
                }
                self.hidden_fallbacks += 1;
            }
        }
        match s {
            Schedule::Run(_, r) if active.is_some() && r.until.is_none() => {
                let scope = &self.scopes[active.unwrap()];
                let rules = &scope.rulesets[&r.ruleset];
                let mut pairs = vec![];
                let mut skipped = vec![];
                for id in rules {
                    let Some(m) = self
                        .markers
                        .get(id)
                        .filter(|_| scope.certified.contains(id))
                    else {
                        pairs.push((*id, *scope.variants.get(id).unwrap_or(id)));
                        continue;
                    };
                    let covered = eg.backend.table_size(m.table);
                    // Receipts are an injective set of parameter tuples drawn
                    // from this anchor. Equal exact cardinalities prove total
                    // coverage; no sampling or stale size estimate is used.
                    let all = covered > 0
                        && (m.params.is_empty()
                            || self.coverage.contains(id)
                                && m.anchors
                                    .iter()
                                    .any(|a| eg.backend.table_size(*a) == covered));
                    if all {
                        skipped.push(*id);
                        self.skipped_queries += 1;
                    } else {
                        pairs.push((*id, *scope.variants.get(id).unwrap_or(id)));
                    }
                }
                let source_before = scope.private_receipts.then(|| source_version(eg));
                let old = eg.backend.completed_cell_counts();
                eg.backend.set_completed_cells(scope.entries.clone(), false);
                let res = eg.backend.run_rule_variants(&pairs, &skipped);
                eg.backend.set_completed_cells(vec![], false);
                let now = eg.backend.completed_cell_counts();
                self.hits += now.0 - old.0;
                self.unresolved += now.1 - old.1;
                self.optimized_runs += 1;
                let mut iteration = res.map_err(|e| err(e.to_string()))?;
                if let Some(before) = source_before {
                    iteration.rule_set_report.changed = before != source_version(eg);
                }
                let report = RunReport::singleton(&r.ruleset, iteration);
                self.rule_reports.union(report.clone());
                eg.overall_run_report.union(report.clone());
                Ok(report)
            }
            Schedule::Sequence(_, ss) => {
                let mut r = RunReport::default();
                for s in ss {
                    r.union(self.walk(eg, s, active)?)
                }
                Ok(r)
            }
            Schedule::Repeat(_, n, body) => {
                let mut r = RunReport::default();
                for _ in 0..*n {
                    let next = self.walk(eg, body, active)?;
                    let stop = next.can_stop;
                    r.union(next);
                    if stop {
                        break;
                    }
                }
                Ok(r)
            }
            Schedule::Saturate(_, body) => {
                let mut r = RunReport::default();
                loop {
                    let next = self.walk(eg, body, active)?;
                    let updated = next.updated;
                    r.union(next);
                    if !updated {
                        break;
                    }
                }
                Ok(r)
            }
            _ => self.native(eg, s),
        }
    }
    pub fn report(&self) -> J {
        json!({"native_effect_receipts":self.markers.values().filter(|m|!m.private).count(),"private_receipt_tables":self.markers.values().filter(|m|m.private).count(),"compiled_scopes":self.scopes.len(),"compiled_variants":self.scopes.iter().map(|s|s.variants.len()).sum::<usize>(),"planned_successor_steps":self.planned_steps,"inline_successor_invocations":self.inline.load(Ordering::Relaxed),"completion_prefix_hits":self.hits,"unresolved_completion_filters":self.unresolved,"whole_rule_queries_skipped":self.skipped_queries,"scope_activations":self.activations,"hidden_union_scope_fallbacks":self.hidden_fallbacks,"optimized_runs":self.optimized_runs,"native_runs":self.native_runs,"rejected_scopes":self.rejected,"rejected_templates":self.rejected_templates,"native_rule_report":self.rule_reports})
    }
}
fn publish_marker(
    tr: &mut BackendRule<'_>,
    m: &Marker,
    subst: Option<&HashMap<ResolvedVar, ResolvedAtomTerm>>,
) {
    if !m.private {
        return;
    }
    let mut args = m
        .params
        .iter()
        .map(|v| {
            let term = subst
                .and_then(|s| s.get(v))
                .cloned()
                .unwrap_or_else(|| T::Var(span(), v.clone()));
            tr.entry(&term)
        })
        .collect::<Vec<_>>();
    args.push(tr.rb.egraph().base_value_constant(()));
    tr.rb.set(m.table, &args);
}
struct Step {
    rule: ResolvedCoreRule,
    id: RuleId,
    subst: HashMap<ResolvedVar, ResolvedAtomTerm>,
}
fn prepare(
    eg: &EGraph,
    core: &J,
    recipe: &J,
    t: &J,
    seed: &ResolvedCoreRule,
    seed_id: RuleId,
    markers: &BTreeMap<RuleId, Marker>,
    allowed: &BTreeSet<RuleId>,
) -> Result<Vec<Step>> {
    let steps = array(&t["steps"])?;
    let first = &recipe["rules"][index(&steps[0]["recipe_rule"])?];
    let mut globals = vec![];
    for (i, s) in array(&t["slots"])?.iter().enumerate() {
        if s["definition"] == "receipt" && s["sort"]["name"] == "Unit" {
            globals.push(T::Literal(span(), Literal::Unit));
            continue;
        }
        let sort = eg
            .type_info
            .get_sort_by_name(string(&s["sort"]["name"])?)
            .ok_or_else(|| err("CPU slot sort"))?
            .clone();
        let name = if i < index(&t["input_width"])? {
            string(&first["slots"][i]["name"])?.into()
        } else {
            format!("@eggfuse/cpu/{seed_id:?}/{i}")
        };
        globals.push(T::Var(
            span(),
            ResolvedVar {
                name,
                sort,
                is_global_ref: false,
            },
        ));
    }
    let mut facts = vec![];
    let mut aliases = HashMap::default();
    let mut output = vec![];
    let mut defined = (0..index(&t["input_width"])?).collect::<BTreeSet<_>>();
    for (si, step) in steps.iter().enumerate() {
        let spec = &recipe["rules"][index(&step["recipe_rule"])?];
        let set = string(&spec["metadata"]["source_set"])?;
        let name = string(&spec["metadata"]["name"])?;
        let Ruleset::Rules(rs) = eg
            .rulesets
            .get(set)
            .ok_or_else(|| err("CPU source rule set"))?
        else {
            return Err(err("CPU source set must be concrete"));
        };
        let (original, id) = rs.get(name).ok_or_else(|| err("CPU source rule"))?;
        if si > 0 && (!allowed.contains(id) || !markers.contains_key(id)) {
            continue;
        }
        if si == 0 && *id != seed_id {
            return Err(err("CPU seed identity"));
        }
        let raw = array(&core["rulesets"])?
            .iter()
            .flat_map(|s| s["rules"].as_array().into_iter().flatten())
            .find(|r| r["name"] == name && r["source_set"] == set)
            .ok_or_else(|| err("CPU source rule missing"))?;
        let mut vars = BTreeMap::new();
        for a in array(&raw["body"])? {
            for v in array(&a["args"])? {
                if let Some(n) = v["var"].as_str() {
                    vars.insert(n.to_owned(), v["sort"].clone());
                }
            }
        }
        let query_names = vars.keys().cloned().collect::<BTreeSet<_>>();
        for a in array(&raw["head"])? {
            if let Some(n) = a["dst"].as_str() {
                vars.insert(n.into(), a["sort"].clone());
            }
        }
        let slots = array(&spec["slots"])?;
        if slots
            .iter()
            .map(|s| string(&s["name"]))
            .collect::<Result<BTreeSet<_>>>()?
            .len()
            != slots.len()
        {
            return Err(err("CPU duplicate variable slot"));
        }
        if slots.len() != vars.len() {
            return Err(err("CPU slot coverage"));
        }
        if slots.len() != array(&step["mapping"])?.len() {
            return Err(err("CPU mapping width"));
        }
        let mut subst = HashMap::default();
        for (i, s) in slots.iter().enumerate() {
            let name = string(&s["name"])?;
            if vars.get(name) != Some(&s["sort"]) {
                return Err(err("CPU slot does not match source variable"));
            }
            let old = ResolvedVar {
                name: name.into(),
                sort: eg
                    .type_info
                    .get_sort_by_name(string(&s["sort"]["name"])?)
                    .ok_or_else(|| err("CPU sort"))?
                    .clone(),
                is_global_ref: false,
            };
            let m = &step["mapping"][i];
            let term = if m.get("slot").is_some() {
                globals
                    .get(index(&m["slot"])?)
                    .ok_or_else(|| err("CPU slot out of bounds"))?
                    .clone()
            } else {
                let l = match string(&m["sort"]["name"])? {
                    "i64" => Literal::Int(m["literal"].as_i64().ok_or_else(|| err("CPU literal"))?),
                    "bool" => Literal::Bool(m["literal"].as_i64() == Some(1)),
                    "String" => Literal::String(string(&m["literal"])?.into()),
                    "Unit" => Literal::Unit,
                    _ => return Err(err("CPU literal type")),
                };
                T::Literal(span(), l)
            };
            if term.output().name() != old.sort.name() {
                return Err(err("CPU mapping sort"));
            }
            subst.insert(old, term);
        }
        for (i, s) in slots.iter().enumerate() {
            if query_names.contains(string(&s["name"])?) {
                if let Some(n) = step["mapping"][i]["slot"].as_u64() {
                    defined.insert(n as usize);
                }
            }
        }
        for a in array(&raw["head"])? {
            if let Some(dst) = a["dst"].as_str() {
                let pos = slots
                    .iter()
                    .position(|s| s["name"] == dst)
                    .ok_or_else(|| err("CPU destination slot"))?;
                let m = &step["mapping"][pos];
                let n = index(&m["slot"])?;
                if !defined.insert(n) {
                    return Err(err("CPU mapping overwrites an existing slot"));
                }
            }
        }
        let r = rename(original, &subst)?;
        if si == 0 {
            if r.body.atoms != seed.body.atoms {
                return Err(err("CPU seed binding mapping differs"));
            }
            facts.extend(r.body.atoms.iter().filter_map(|a| {
                if let ResolvedCall::Func(f) = &a.head {
                    Some((f.name.clone(), a.args.clone()))
                } else {
                    None
                }
            }));
        } else {
            for a in &r.body.atoms {
                if let ResolvedCall::Func(f) = &a.head {
                    let args = a
                        .args
                        .iter()
                        .map(|a| resolve(a, &aliases))
                        .collect::<Vec<_>>();
                    if !facts.iter().any(|(n, vs)| {
                        *n == f.name
                            && vs
                                .iter()
                                .map(|a| resolve(a, &aliases))
                                .eq(args.iter().cloned())
                    }) {
                        return Ok(output);
                    }
                }
            }
        }
        for a in &r.head.0 {
            match a {
                A::LetAtomTerm(_, v, x) => {
                    aliases.insert(T::Var(span(), v.clone()), resolve(x, &aliases));
                }
                A::Let(_, v, ResolvedCall::Func(f), args) => {
                    let mut row = args.clone();
                    row.push(T::Var(span(), v.clone()));
                    facts.push((f.name.clone(), row));
                }
                A::Set(_, ResolvedCall::Func(f), args, v) => {
                    let mut row = args.clone();
                    row.push(v.clone());
                    facts.push((f.name.clone(), row));
                }
                _ => {}
            }
        }
        output.push(Step {
            rule: r,
            id: *id,
            subst,
        });
    }
    Ok(output)
}
fn resolve(
    x: &ResolvedAtomTerm,
    aliases: &HashMap<ResolvedAtomTerm, ResolvedAtomTerm>,
) -> ResolvedAtomTerm {
    let mut x = x.clone();
    while let Some(y) = aliases.get(&x) {
        if *y == x {
            break;
        }
        x = y.clone()
    }
    x
}

// CoreActions::subst prepends aliases for inlining expressions; it is not an
// SSA alpha-renamer (in particular it reads RHS destinations before definition).
fn rename(
    r: &ResolvedCoreRule,
    m: &HashMap<ResolvedVar, ResolvedAtomTerm>,
) -> Result<ResolvedCoreRule> {
    let term = |x: &ResolvedAtomTerm| {
        if let T::Var(_, v) = x {
            m.get(v).cloned().unwrap_or_else(|| x.clone())
        } else {
            x.clone()
        }
    };
    let var = |v: &ResolvedVar| match m.get(v) {
        Some(T::Var(_, v)) => Ok(v.clone()),
        _ => Err(err("CPU effect destination must be a fresh variable")),
    };
    let mut out = r.clone();
    for a in &mut out.body.atoms {
        a.args = a.args.iter().map(&term).collect();
    }
    out.head.0 = r
        .head
        .0
        .iter()
        .map(|a| {
            Ok(match a {
                A::Let(s, v, f, xs) => A::Let(
                    s.clone(),
                    var(v)?,
                    f.clone(),
                    xs.iter().map(&term).collect(),
                ),
                A::LetAtomTerm(s, v, x) => A::LetAtomTerm(s.clone(), var(v)?, term(x)),
                A::Set(s, f, xs, v) => A::Set(
                    s.clone(),
                    f.clone(),
                    xs.iter().map(&term).collect(),
                    term(v),
                ),
                A::Union(s, x, y) => A::Union(s.clone(), term(x), term(y)),
                _ => return Err(err("CPU action not admitted")),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(out)
}

// A single native constructor (or Unit relation update) is already a complete
// record of its own idempotent effect. Reuse that row instead of duplicating it.
fn effect_receipt(eg: &EGraph, r: &ResolvedCoreRule) -> Option<(FunctionId, Vec<ResolvedVar>)> {
    let mut aliases = HashMap::default();
    let mut found = None;
    let query = r
        .body
        .atoms
        .iter()
        .flat_map(|a| a.args.iter())
        .filter_map(|x| {
            if let T::Var(_, v) = x {
                Some(v.clone())
            } else {
                None
            }
        })
        .collect::<HashSet<_>>();
    for a in &r.head.0 {
        match a {
            A::LetAtomTerm(_, v, x) => {
                aliases.insert(T::Var(span(), v.clone()), resolve(x, &aliases));
            }
            A::Let(_, _, ResolvedCall::Func(f), args) if eg.functions[&f.name].is_constructor() => {
                if found.is_some() {
                    return None;
                }
                found = Some((eg.functions[&f.name].backend_id, args.clone()));
            }
            A::Set(_, ResolvedCall::Func(f), args, T::Literal(_, Literal::Unit))
                if !eg.functions[&f.name].is_constructor() && table_positive(eg, &f.name) =>
            {
                if found.is_some() {
                    return None;
                }
                found = Some((eg.functions[&f.name].backend_id, args.clone()));
            }
            _ => return None,
        }
    }
    let (id, args) = found?;
    let mut params = vec![];
    for x in args {
        let T::Var(_, v) = resolve(&x, &aliases) else {
            return None;
        };
        if !query.contains(&v) || params.contains(&v) {
            return None;
        }
        params.push(v);
    }
    Some((id, params))
}

// Forward-closed regions prevent early facts from influencing a nonmonotone
// consumer. Destructive writers invalidate both witnesses and effect receipts.
// Mixed scopes retain their original native rules; only this subset is fused.
fn closed_positive_region(
    eg: &EGraph,
    rules: &[(String, String, ResolvedCoreRule, RuleId)],
    mixed: bool,
) -> BTreeSet<RuleId> {
    let mut readers: BTreeMap<String, BTreeSet<RuleId>> = BTreeMap::new();
    let mut reads = BTreeMap::new();
    let mut writes = BTreeMap::new();
    let mut destructive = BTreeSet::new();
    let mut unknown = false;
    let mut safe = BTreeSet::new();
    for (_, _, r, id) in rules {
        let mut rs = BTreeSet::new();
        let mut ws = BTreeSet::new();
        let mut union = false;
        for a in &r.body.atoms {
            if let ResolvedCall::Func(f) = &a.head {
                rs.insert(f.name.clone());
            }
        }
        for a in &r.head.0 {
            match a {
                A::Let(_, _, ResolvedCall::Func(f), _) => {
                    rs.insert(f.name.clone());
                    if eg.functions[&f.name].is_constructor() {
                        ws.insert(f.name.clone());
                    }
                }
                A::Set(_, ResolvedCall::Func(f), _, _) => {
                    ws.insert(f.name.clone());
                    union |= eg.functions[&f.name].is_constructor();
                }
                A::Union(..) => union = true,
                A::Change(_, _, ResolvedCall::Func(f), _) => {
                    destructive.insert(f.name.clone());
                    ws.insert(f.name.clone());
                }
                A::Let(_, _, ResolvedCall::Primitive(p), _) => {
                    unknown |= !matches!(
                        p.name(),
                        "+" | "-"
                            | "*"
                            | "/"
                            | "%"
                            | "min"
                            | "max"
                            | "not"
                            | "and"
                            | "or"
                            | "xor"
                            | "=>"
                            | "bool-="
                            | "bool-<"
                            | "bool->"
                            | "bool-<="
                            | "bool->="
                    );
                }
                _ => {}
            }
        }
        let mut seen = BTreeSet::new();
        let mut pending = ws.iter().cloned().collect::<Vec<_>>();
        if union {
            pending.extend(
                eg.functions
                    .iter()
                    .filter(|(_, f)| {
                        !f.is_constructor()
                            && f.schema
                                .input
                                .iter()
                                .any(|s| s.is_eq_sort() || s.is_container_sort())
                    })
                    .map(|(n, _)| n.clone()),
            );
        }
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if let Some(e) = &eg.functions[&name].decl.merge {
                merge_footprint(eg, e, &mut rs, &mut ws, &mut pending, &mut unknown);
            }
        }
        // Naive primitives can read arbitrary tables outside their explicit LHS.
        unknown |= !eg.backend.device_rule_is_seminaive(*id);
        for t in &rs {
            readers.entry(t.clone()).or_default().insert(*id);
        }
        reads.insert(*id, rs);
        writes.insert(*id, ws);
        if positive(eg, r, *id) && (!mixed || !union) {
            safe.insert(*id);
        }
    }
    if mixed && unknown {
        return BTreeSet::new();
    }
    loop {
        let bad = safe
            .iter()
            .copied()
            .filter(|id| {
                reads[id]
                    .iter()
                    .chain(writes[id].iter())
                    .any(|t| destructive.contains(t))
                    || writes[id].iter().any(|t| {
                        readers
                            .get(t)
                            .is_some_and(|rs| rs.iter().any(|r| !safe.contains(r)))
                    })
            })
            .collect::<Vec<_>>();
        if bad.is_empty() {
            break;
        }
        for id in bad {
            safe.remove(&id);
        }
    }
    safe
}

fn source_version(eg: &EGraph) -> (Vec<core_relations::TableVersion>, usize) {
    (
        eg.functions
            .values()
            .map(|f| eg.backend.native_function_version(f.backend_id))
            .collect(),
        eg.native_union_count(),
    )
}

fn merge_footprint(
    eg: &EGraph,
    e: &ResolvedExpr,
    reads: &mut BTreeSet<String>,
    writes: &mut BTreeSet<String>,
    pending: &mut Vec<String>,
    unknown: &mut bool,
) {
    if let GenericExpr::Call(_, c, args) = e {
        match c {
            ResolvedCall::Func(f) => {
                reads.insert(f.name.clone());
                if eg.functions[&f.name].is_constructor() && writes.insert(f.name.clone()) {
                    pending.push(f.name.clone());
                }
            }
            ResolvedCall::Primitive(p) => {
                *unknown |= !matches!(
                    p.name(),
                    "+" | "-"
                        | "*"
                        | "/"
                        | "%"
                        | "min"
                        | "max"
                        | "not"
                        | "and"
                        | "or"
                        | "xor"
                        | "=>"
                );
            }
        }
        for x in args {
            merge_footprint(eg, x, reads, writes, pending, unknown)
        }
    }
}
