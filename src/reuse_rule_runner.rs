//! Opt-in source-rule view planning for positive constructor/relation programs.
//! Summary relations are refreshed before each finite Run step. No native facts
//! are hidden, deleted or virtualized. Planned traces expand exact source-row keys.
use super::*;
use egglog::ast::{Action, Command, Expr, Fact, GenericSchedule};

struct Lower<'a> {
    eg: &'a EGraph,
    query: Query,
    names: Vec<String>,
    vars: BTreeMap<String, usize>,
    source: &'a str,
    spans: Vec<Option<Arc<str>>>,
}
impl<'a> Lower<'a> {
    fn new(eg: &'a EGraph, source: &'a str) -> Self {
        Self {
            eg,
            source,
            query: Query {
                values: vec![],
                atoms: vec![],
                projection: vec![],
            },
            names: vec![],
            vars: BTreeMap::new(),
            spans: vec![],
        }
    }
    fn fresh(&mut self, sort: &str) -> usize {
        let mut name = format!("reuse_temp_{}", self.names.len());
        while self.source.contains(&name) {
            name.push('_');
        }
        let id = self.names.len();
        self.names.push(name);
        self.query.values.push((sort.into(), None));
        id
    }
    fn variable(&mut self, name: &str, sort: &str) -> Result<usize> {
        if let Some(i) = self.vars.get(name) {
            if self.query.values[*i].0 != sort {
                return Err("inconsistent query sort".into());
            }
            return Ok(*i);
        }
        let i = self.fresh(sort);
        self.names[i] = name.into();
        self.vars.insert(name.into(), i);
        Ok(i)
    }
    fn expression(&mut self, e: &Expr, sort: &str, result: Option<usize>) -> Result<usize> {
        match e {
            Expr::Var(_, v) => {
                let id = self.variable(v, sort)?;
                if result.is_some_and(|r| r != id) {
                    return Err("explicit variable equality uses native plan".into());
                }
                Ok(id)
            }
            Expr::Lit(_, lit) => {
                if result.is_some() {
                    return Err("literal equality uses native plan".into());
                }
                let i = self.fresh(sort);
                self.query.values[i].1 = Some(lit.to_string());
                self.names[i] = lit.to_string();
                Ok(i)
            }
            Expr::Call(span, op, args) => {
                let schema = self
                    .eg
                    .get_function(op)
                    .ok_or("primitive uses native plan")?
                    .schema()
                    .clone();
                if schema.output.name() != sort || schema.input.len() != args.len() {
                    return Err("query schema mismatch".into());
                }
                let mut columns = vec![];
                for (arg, ty) in args.iter().zip(&schema.input) {
                    columns.push(self.expression(arg, ty.name(), None)?);
                }
                let id = result.unwrap_or_else(|| self.fresh(sort));
                if sort != "Unit" && !sort.starts_with('@') {
                    columns.push(id);
                }
                self.query.atoms.push((op.clone(), columns));
                self.spans.push(match span {
                    egglog::ast::Span::Egglog(s) => {
                        Some(format!("{:?}:{}:{}", s.file.name, s.i, s.j).into())
                    }
                    _ => None,
                });
                Ok(id)
            }
        }
    }
    fn fact(&mut self, f: &Fact) -> Result<()> {
        match f {
            Fact::Eq(_, Expr::Var(_, name), expr @ Expr::Call(_, op, _))
            | Fact::Eq(_, expr @ Expr::Call(_, op, _), Expr::Var(_, name)) => {
                let sort = self
                    .eg
                    .get_function(op)
                    .ok_or("primitive equality uses native plan")?
                    .schema()
                    .output
                    .name()
                    .to_string();
                let id = self.variable(name, &sort)?;
                self.expression(expr, &sort, Some(id))?;
            }
            Fact::Fact(expr @ Expr::Call(_, op, _)) => {
                let sort = self
                    .eg
                    .get_function(op)
                    .ok_or("primitive guard uses native plan")?
                    .schema()
                    .output
                    .name()
                    .to_string();
                self.expression(expr, &sort, None)?;
            }
            _ => return Err("complex equality uses native plan".into()),
        }
        Ok(())
    }
    fn atom(&self, op: &str, args: &[usize]) -> String {
        let n = self.eg.get_function(op).unwrap().schema().input.len();
        let call = format!(
            "({op}{})",
            args[..n]
                .iter()
                .map(|i| format!(" {}", self.names[*i]))
                .collect::<String>()
        );
        if args.len() == n {
            call
        } else {
            format!("(= {} {call})", self.names[args[n]])
        }
    }
}
fn positive_expr(e: &Expr, tables: &BTreeSet<String>) -> bool {
    match e {
        Expr::Call(_, op, args) => {
            tables.contains(op) && args.iter().all(|e| positive_expr(e, tables))
        }
        _ => true,
    }
}
fn positive_action(a: &Action, tables: &BTreeSet<String>) -> bool {
    match a {
        Action::Union(_, a, b) => positive_expr(a, tables) && positive_expr(b, tables),
        Action::Let(_, _, e) | Action::Expr(_, e) => positive_expr(e, tables),
        _ => false,
    }
}
fn schedule_steps(s: &egglog::ast::Schedule, out: &mut Vec<Command>) -> Result<()> {
    match s {
        GenericSchedule::Run(_, r) if r.until.is_none() => {
            out.push(Command::RunSchedule(s.clone()))
        }
        GenericSchedule::Repeat(_, n, s) => {
            for _ in 0..*n {
                schedule_steps(s, out)?;
            }
        }
        GenericSchedule::Sequence(_, ss) => {
            for s in ss {
                schedule_steps(s, out)?;
            }
        }
        _ => {
            return Err(
                "reuse-run currently requires finite run/repeat/sequence without until".into(),
            );
        }
    }
    Ok(())
}

/// Real tier0 LHS substitution, restricted to positive, finite schedules.
/// Unsupported mutation semantics are rejected before running either program.
pub fn run(source: &str, library: &super::super::library::Library) -> Result<Value> {
    run_observed(source, library, false).map(|(report, _)| report)
}
pub fn run_observed(
    source: &str,
    library: &super::super::library::Library,
    observe: bool,
) -> Result<(Value, BTreeSet<String>)> {
    let start = std::time::Instant::now();
    let mut observer = super::view_observer::Observer::default();
    observer.known = library.observed_grafts();
    let trace = egglog::TraceSession::with_dependencies();
    let mut eg = EGraph::default();
    let commands = crate::visual_rule::surface_program(eg.parse_program(None, source)?);
    let mut equality_sorts = BTreeSet::new();
    let mut tables = BTreeSet::new();
    for c in &commands {
        match c {
            Command::Datatype { name, variants, .. } => {
                equality_sorts.insert(name.clone());
                tables.extend(variants.iter().map(|v| v.name.clone()))
            }
            Command::Relation { name, .. } => {
                tables.insert(name.clone());
            }
            _ => {}
        }
    }
    let globals: BTreeSet<_> = commands
        .iter()
        .filter_map(|c| match c {
            Command::Action(Action::Let(_, name, _)) => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut normalized = vec![];
    for (i, c) in commands.into_iter().enumerate() {
        if matches!(c, Command::Rewrite(_, _, true)) {
            return Err("reuse-run does not support subsuming rewrites".into());
        }
        let c = crate::visual_rule::normalize(c, i);
        match &c {
            Command::Rule{rule} if !rule.naive && rule.head.0.iter().all(|a|positive_action(a,&tables))=>{},
            Command::Action(a) if positive_action(a,&tables)=>{},
            Command::Datatype{..}|Command::Relation{..}|Command::AddRuleset(..)|Command::Check(..)=>{},
            Command::RunSchedule(s)=>{schedule_steps(s,&mut normalized)?;continue;},
            _=>return Err("reuse-run supports positive constructors/relations, union, checks and finite schedules only".into()),
        }
        normalized.push(c);
    }
    let mut planner = SourcePlanner::new(&mut eg, library, source, globals.clone())?;
    planner.observer.known = observer.known;
    for c in normalized {
        match c {
            Command::Rule { rule } => planner.install(&mut eg, rule, observe.then_some(&trace))?,
            Command::RunSchedule(s) => {
                planner.refresh(&mut eg)?;
                if observe {
                    eg.run_program_with_trace(vec![Command::RunSchedule(s)], &trace)?;
                    planner.observer.consume(&eg, &trace.drain_completed())?;
                } else {
                    eg.run_program(vec![Command::RunSchedule(s)])?;
                }
            }
            c => {
                eg.run_program(vec![c])?;
            }
        }
    }
    let optimized_seconds = start.elapsed().as_secs_f64();
    // Independent native program; comparisons below include every user table.
    let mut reference = EGraph::default();
    let start = std::time::Instant::now();
    reference.parse_and_run_program(None, source)?;
    let reference_seconds = start.elapsed().as_secs_f64();
    let a = snapshot(&mut eg, &tables, source, &equality_sorts, &globals)?;
    let b = snapshot(&mut reference, &tables, source, &equality_sorts, &globals)?;
    let equivalent = matches!(
        crate::saturated_rule_composition::compare(&a, &b)?,
        crate::saturated_rule_composition::Comparison::Equivalent { .. }
    );
    if !equivalent {
        return Err("optimized rule execution differs from native reference".into());
    }
    Ok((
        json!({"scope":"positive finite native tier0 rule execution; optional native summary graft tracing; no virtual native rows","plans":planner.plans,"fallback_rules":planner.fallback,"views":planner.shared.len(),"reference_equivalent":equivalent,"optimized_seconds":optimized_seconds,"reference_seconds":reference_seconds,"grafts":if observe {planner.observer.report()}else{Value::Null}}),
        planner.observer.known,
    ))
}
/// Reusable source planner shared by the runner and the native capture pipeline.
pub(crate) struct SourcePlanner {
    source: String,
    globals: BTreeSet<String>,
    viewset: String,
    baked: BTreeSet<Key>,
    shared: BTreeMap<Key, String>,
    observer: super::view_observer::Observer,
    plans: Vec<Value>,
    fallback: Vec<Value>,
    rule_index: usize,
}
impl SourcePlanner {
    pub(crate) fn supports_capture(commands: &[Command]) -> bool {
        let tables: BTreeSet<_> = commands
            .iter()
            .flat_map(|c| match c {
                Command::Datatype { variants, .. } => {
                    variants.iter().map(|v| v.name.clone()).collect::<Vec<_>>()
                }
                Command::Relation { name, .. } => vec![name.clone()],
                _ => vec![],
            })
            .collect();
        commands.iter().all(|c| match c {
            Command::Rule { rule } => {
                !rule.naive && rule.head.0.iter().all(|a| positive_action(a, &tables))
            }
            Command::Action(a) => positive_action(a, &tables),
            Command::RunSchedule(GenericSchedule::Run(_, r)) => r.until.is_none(),
            Command::RunSchedule(GenericSchedule::Repeat(_, _, s)) => {
                matches!(s.as_ref(),GenericSchedule::Run(_,r) if r.until.is_none())
            }
            Command::Datatype { .. }
            | Command::Relation { .. }
            | Command::AddRuleset(..)
            | Command::Check(..) => true,
            _ => false,
        })
    }
    pub(crate) fn new(
        eg: &mut EGraph,
        library: &super::super::library::Library,
        source: &str,
        globals: BTreeSet<String>,
    ) -> Result<Self> {
        let mut viewset = "reuse_views".to_string();
        while source.contains(&viewset) {
            viewset.push('_');
        }
        eg.parse_and_run_program(None, &format!("(ruleset {viewset})"))?;
        let baked = library
            .fragments()
            .iter()
            .map(|q| q.key(&q.atoms).map(|x| x.0))
            .collect::<Result<_>>()?;
        let mut observer = super::view_observer::Observer::default();
        observer.known = library.observed_grafts();
        Ok(Self {
            source: source.into(),
            globals,
            viewset,
            baked,
            shared: BTreeMap::new(),
            observer,
            plans: vec![],
            fallback: vec![],
            rule_index: 0,
        })
    }
    pub(crate) fn install(
        &mut self,
        eg: &mut EGraph,
        mut rule: egglog::ast::Rule,
        trace: Option<&egglog::TraceSession>,
    ) -> Result<()> {
        let source = self.source.as_str();
        let globals = &self.globals;
        let viewset = &self.viewset;
        let baked = &self.baked;
        let shared = &mut self.shared;
        let observer = &mut self.observer;
        let plans = &mut self.plans;
        let fallback = &mut self.fallback;
        let rule_index = &mut self.rule_index;

        if rule.name.is_empty() {
            rule.name = format!("{viewset}_source_{}", observer.rules.len());
        }
        observer.rules.insert(rule.name.clone(), rule.to_string());
        let mut lower = Lower::new(&eg, source);
        let lowered = rule.body.iter().try_for_each(|f| lower.fact(f));
        if let Err(e) = lowered {
            fallback.push(json!({"rule":rule.name,"reason":e.to_string()}));
            eg.run_program(vec![Command::Rule { rule }])?;
            return Ok(());
        }
        if lower
            .vars
            .values()
            .any(|i| lower.query.values[*i].0.starts_with('@'))
        {
            fallback.push(
                json!({"rule":rule.name,"reason":"named private relation output uses native plan"}),
            );
            eg.run_program(vec![Command::Rule { rule }])?;
            return Ok(());
        }
        if lower
            .names
            .iter()
            .any(|name| globals.contains(name) || name == "_")
        {
            fallback.push(json!({"rule":rule.name,"reason":"global-bound LHS uses native plan"}));
            eg.run_program(vec![Command::Rule { rule }])?;
            return Ok(());
        }
        // Match baked table order; no arbitrary-subgraph search.
        let mut order: Vec<_> = (0..lower.query.atoms.len()).collect();
        order.sort_by(|a, b| lower.query.atoms[*a].0.cmp(&lower.query.atoms[*b].0));
        lower.query.atoms = order
            .iter()
            .map(|i| lower.query.atoms[*i].clone())
            .collect();
        lower.spans = order.iter().map(|i| lower.spans[*i].clone()).collect();
        let mut chosen = None;
        for n in (2..=lower.query.atoms.len()).rev() {
            let (key, vars) = lower.query.key(&lower.query.atoms[..n])?;
            if baked.contains(&key) {
                chosen = Some((n, key, vars));
                break;
            }
        }
        let Some((n, key, vars)) = chosen else {
            eg.run_program(vec![Command::Rule { rule }])?;
            return Ok(());
        };
        // Native lowering can eliminate a call-result variable used as another
        // atom's key. Summary columns preserve it, but residual-only results do
        // not necessarily retain their source name in the compiled SymbolMap.
        // Until read plans can follow those aliases, keep the original rule:
        // an optimized match without complete source provenance loses history.
        let call_results: BTreeSet<_> = lower
            .query
            .atoms
            .iter()
            .filter_map(|(op, cols)| {
                let arity = eg.get_function(op).unwrap().schema().input.len();
                cols.get(arity).copied()
            })
            .collect();
        let missing_source_key = lower.query.atoms.iter().any(|(op, cols)| {
            let arity = eg.get_function(op).unwrap().schema().input.len();
            cols[..arity]
                .iter()
                .any(|v| call_results.contains(v) && !vars.contains(v))
        });
        if missing_source_key {
            fallback.push(json!({"rule":rule.name,"reason":"residual call-result key may be eliminated from source witness bindings"}));
            eg.run_program(vec![Command::Rule { rule }])?;
            return Ok(());
        }
        // A keyed lookup cannot multiply the first atom's rows. Avoid
        // materializing a Cartesian/fan-out view without arbitrary caps.
        let arities: Vec<_> = lower.query.atoms[..n]
            .iter()
            .map(|(op, _)| eg.get_function(op).unwrap().schema().input.len())
            .collect();
        let nonexpanding = key_bound(&key, &arities);
        if !nonexpanding {
            fallback.push(json!({"rule":rule.name,"reason":"fragment has an unbound lookup key and may multiply summary rows"}));
            eg.run_program(vec![Command::Rule { rule }])?;
            return Ok(());
        }
        let facts: Vec<_> = lower
            .query
            .atoms
            .iter()
            .map(|(op, args)| lower.atom(op, args))
            .collect();
        let args = vars
            .iter()
            .map(|v| lower.names[*v].clone())
            .collect::<Vec<_>>()
            .join(" ");
        let types = vars
            .iter()
            .map(|v| lower.query.values[*v].0.clone())
            .collect::<Vec<_>>()
            .join(" ");
        let mut view = shared
            .get(&key)
            .cloned()
            .unwrap_or_else(|| format!("{viewset}_{}", shared.len()));
        while source.contains(&view) {
            view.push('_');
        }
        let new = !shared.contains_key(&key);
        observer.views.insert(
            view.clone(),
            (
                key.clone(),
                lower.query.atoms[..n]
                    .iter()
                    .map(|(op, _)| eg.get_function(op).unwrap().schema().input.len())
                    .collect(),
            ),
        );
        let summary = format!("({view} {args})");
        let body = format!("{} {}", summary, facts[n..].join(" "));
        let span = rule.span.clone();
        let mut parser = egglog::ast::Parser::default();
        let source_reads = lower
            .query
            .atoms
            .iter()
            .zip(&lower.spans)
            .map(|((op, cols), span)| {
                let arity = eg.get_function(op).unwrap().schema().input.len();
                let keys = cols[..arity]
                    .iter()
                    .map(|i| match &lower.query.values[*i].1 {
                        Some(l) => parser.get_expr_from_string(None, l).map_err(Into::into),
                        None => Ok(Expr::Var(rule.span.clone(), lower.names[*i].clone())),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok((op.clone(), keys, span.clone()))
            })
            .collect::<Result<Vec<_>>>()?;
        // Call-result variables are determined by keyed row witnesses. Native
        // lowering normally eliminates their source names; exposing a summary
        // column must not turn them into new external ripen parameters.
        let defined: BTreeSet<_> = rule
            .body
            .iter()
            .filter_map(|f| match f {
                Fact::Eq(_, Expr::Var(_, v), Expr::Call(..))
                | Fact::Eq(_, Expr::Call(..), Expr::Var(_, v)) => Some(v),
                _ => None,
            })
            .collect();
        let visible = lower
            .vars
            .keys()
            .filter(|v| !defined.contains(v))
            .cloned()
            .collect();
        drop(lower);
        if new {
            eg.parse_and_run_program(None,&format!("(relation {view} ({types})) (rule ({}) ({summary}) :ruleset {viewset} :name \"{viewset}_fill_{rule_index}\")",facts[..n].join(" ")))?;
            shared.insert(key, view.clone());
        }
        let parsed = eg.parse_program(None, &format!("(rule ({body}) ())"))?;
        let Command::Rule { rule: replacement } = &parsed[0] else {
            unreachable!()
        };
        if let Some(trace) = trace {
            eg.register_source_read_plan(trace, &rule.name, &view, source_reads, visible)?;
        }
        rule.body = replacement.body.clone();
        rule.span = span;
        plans.push(json!({"rule":rule.name,"original_atoms":facts.len(),"new_atoms":1+facts.len()-n,"view":view,"all_binding_columns":vars.len(),"summary_row_bound":"at most the first atom table row count"}));
        eg.run_program(vec![Command::Rule { rule }])?;
        *rule_index += 1;

        Ok(())
    }
    pub(crate) fn refresh(&self, eg: &mut EGraph) -> Result<()> {
        if !self.shared.is_empty() {
            eg.parse_and_run_program(None, &format!("(run {} 1)", self.viewset))?;
        }
        Ok(())
    }
    pub(crate) fn consume(&mut self, eg: &EGraph, batch: &egglog::TraceBatch) -> Result<()> {
        self.observer.consume(eg, batch)
    }
    pub(crate) fn report(&self) -> Value {
        json!({"plans":self.plans,"fallback_rules":self.fallback,"views":self.shared.len(),"grafts":self.observer.report()})
    }
    pub(crate) fn signatures(&self) -> BTreeSet<String> {
        self.observer.known.clone()
    }
}
fn snapshot(
    eg: &mut EGraph,
    tables: &BTreeSet<String>,
    source: &str,
    equality_sorts: &BTreeSet<String>,
    globals: &BTreeSet<String>,
) -> Result<SaturatedRuleComposition> {
    use crate::saturated_rule_composition::{Row, Vertex};
    let mut ids = BTreeMap::new();
    let mut values = vec![];
    let mut rows = vec![];
    for table in tables {
        let f = eg.get_function(table).ok_or("missing user table")?;
        let schema = f.schema();
        let sorts: Vec<_> = schema.input.iter().chain([&schema.output]).collect();
        let mut raw = vec![];
        eg.function_for_each(table, |r| raw.push((r.vals.to_vec(), r.subsumed)))?;
        for (raw, hidden) in raw {
            if hidden {
                return Err("hidden row unsupported in positive runner".into());
            }
            let mut columns = vec![];
            for (v, s) in raw.iter().zip(&sorts) {
                let canonical = eg.get_canonical_value(*v, s);
                let key = (s.name().to_string(), canonical);
                let next = values.len();
                let id = if let Some(i) = ids.get(&key) {
                    *i
                } else {
                    let literal =
                        match s.name() {
                            "i64" => Some(eg.value_to_base::<i64>(*v).to_string()),
                            "String" => Some(serde_json::to_string(
                                &eg.value_to_base::<egglog::sort::S>(*v).to_string(),
                            )?),
                            "bool" => Some(eg.value_to_base::<bool>(*v).to_string()),
                            "Unit" => Some("()".into()),
                            s if equality_sorts.contains(s)
                                || (s.starts_with('@') && s == schema.output.name()) =>
                            {
                                None
                            }
                            _ => return Err(format!(
                                "snapshot verifier does not support base sort {} in table {table}",
                                s.name()
                            )
                            .into()),
                        };
                    values.push(Vertex {
                        sort: s.name().into(),
                        literal,
                    });
                    ids.insert(key, next);
                    next
                };
                columns.push(id);
            }
            let result = columns.pop().ok_or("empty row")?;
            rows.push(Row {
                op: table.clone(),
                args: columns,
                result,
            });
        }
    }
    let mut ports = BTreeMap::new();
    let mut parser = egglog::ast::Parser::default();
    for name in globals {
        let expr = parser.get_expr_from_string(None, name)?;
        let (sort, value) = eg.eval_expr(&expr)?;
        let key = (
            sort.name().to_string(),
            eg.get_canonical_value(value, &sort),
        );
        let id = if let Some(i) = ids.get(&key) {
            *i
        } else {
            let literal = match sort.name() {
                "i64" => Some(eg.value_to_base::<i64>(value).to_string()),
                "String" => Some(serde_json::to_string(
                    &eg.value_to_base::<egglog::sort::S>(value).to_string(),
                )?),
                "bool" => Some(eg.value_to_base::<bool>(value).to_string()),
                "Unit" => Some("()".into()),
                s if equality_sorts.contains(s) => None,
                _ => return Err("unsupported global sort".into()),
            };
            let i = values.len();
            values.push(Vertex {
                sort: sort.name().into(),
                literal,
            });
            ids.insert(key, i);
            i
        };
        ports.insert(name.clone(), id);
    }
    let state = SaturatedRuleComposition {
        version: 2,
        local_ids: vec![],
        scope: json!({"source":source,"observation_only":true}),
        values,
        rows,
        subsumed_rows: vec![],
        ports,
    };
    state.validate()?;
    Ok(state)
}

fn key_bound(key: &Key, arities: &[usize]) -> bool {
    let Some((_, first)) = key.atoms.first() else {
        return false;
    };
    if key.atoms.len() != arities.len() {
        return false;
    }
    let mut bound: BTreeSet<_> = first.iter().copied().collect();
    key.atoms
        .iter()
        .zip(arities)
        .skip(1)
        .all(|((_, cols), arity)| {
            if *arity > cols.len() {
                return false;
            }
            let yes = cols[..*arity]
                .iter()
                .all(|i| bound.contains(i) || key.values.get(*i).is_some_and(|v| v.1.is_some()));
            bound.extend(cols.iter().copied());
            yes
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unbound_keys_without_a_size_heuristic() {
        let mut k = Key {
            values: vec![("E".into(), None); 4],
            atoms: vec![("A".into(), vec![0, 1]), ("B".into(), vec![2, 3])],
        };
        assert!(!key_bound(&k, &[1, 1]));
        k.atoms[1].1 = vec![0, 3];
        assert!(key_bound(&k, &[1, 1]));
        k.atoms[1].1 = vec![2, 3];
        k.values[2] = ("i64".into(), Some("1".into()));
        assert!(key_bound(&k, &[1, 1]));
    }
}
