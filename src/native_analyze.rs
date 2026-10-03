//! Single-process Math analysis. Native events are consumed as Rust data;
//! no trace JSON, generated input files, pipes, or Python processes.
use crate::pipeline::Result;
use egglog::{
    EGraph, RuleActionOutcome, TraceSession, Value, WriteOutcome,
    ast::{Action, Command, Expr, Fact, Literal, Rule, Span},
};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};
// `std::time::Instant` is not implemented on wasm32-unknown-unknown.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

#[path = "native_cs.rs"]
mod cs;
#[path = "native_debug.rs"]
pub mod debug;
#[path = "native_history.rs"]
mod history;
#[path = "native_layer_view.rs"]
mod layer_view;
#[path = "native_ripen.rs"]
pub mod ripen;
#[path = "native_saturated_rule_composition_pipeline.rs"]
mod saturated_rule_composition_pipeline;
#[path = "native_weak_dominate.rs"]
mod weak_dominate;
fn sp() -> Span {
    Span::Rust(Arc::new(egglog::ast::RustSpan {
        file: file!(),
        line: line!(),
        column: column!(),
    }))
}
fn call(op: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(sp(), op.into(), args)
}
fn num(n: u64) -> Expr {
    Expr::Lit(sp(), Literal::Int(n.try_into().expect("id exceeds i64")))
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Value(usize, Value),
    Write(usize, u64),
    External(usize, String, Vec<Value>),
    Replay(String, bool),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Token {
    sort: Arc<str>,
    key: Key,
}
impl Token {
    fn label(&self) -> String {
        match &self.key {
            Key::Replay(label, _) => label.clone(),
            Key::Value(s, v) => format!("{s}:{v:?}"),
            Key::Write(s, w) => format!("{s}:write:{w}"),
            Key::External(s, t, row) => format!(
                "{s}:external:{}",
                json!([t, row.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>()])
            ),
        }
    }
}
#[derive(Default)]
struct Pool {
    ids: BTreeMap<Token, usize>,
    literals: BTreeMap<usize, String>,
    values: Vec<Token>,
}
impl Pool {
    fn intern(&mut self, t: Token) -> usize {
        if let Some(&i) = self.ids.get(&t) {
            return i;
        }
        let i = self.values.len();
        self.values.push(t.clone());
        self.ids.insert(t, i);
        i
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Port {
    Parent(usize, usize),
    External(usize),
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Input {
    Var(String),
    Read(Arc<str>),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Output {
    Var(String),
    Column(Arc<str>, usize),
    Row(Arc<str>),
}
#[derive(Clone)]
pub struct RuleInfo {
    pub rule: Rule,
    pub calls: BTreeMap<Arc<str>, (String, Expr)>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Record {
    pub id: u64,
    pub rule: usize,
    pub parents: Vec<usize>,
    pub ports: Vec<Port>,
    pub inputs: Vec<Input>,
    /// Constructor output identities from committed LHS row witnesses, in read order.
    #[serde(default)]
    read_results: Vec<Option<usize>>,
    pub outputs: Vec<Output>,
    values: Vec<usize>,
    wanted: Vec<usize>,
    external: Vec<usize>,
    required: Vec<usize>,
    produced: Vec<usize>,
    unions: Vec<(usize, usize)>,
    #[serde(default)]
    pub updated_rows: bool,
    pub coarse: bool,
    #[serde(skip)]
    pub comb: Option<Value>,
    #[serde(skip)]
    pub instance: Option<Value>,
    pub extension: usize,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct CaptureBoundary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ripen: Option<crate::coarse_smooth::RipenFeedback>,
    kind: String,
    round: Option<usize>,
    end: usize,
}
struct Captured {
    reuse_plan: Json,
    reuse_signatures: BTreeSet<String>,
    changes: Vec<crate::closure_contract::Change>,
    last_scope: usize,
    preview_source: String,
    boundaries: Vec<CaptureBoundary>,
    layers: crate::coarse_smooth::LayerStore,
    applies: Vec<crate::coarse_smooth::Apply>,
    datatype: String,
    datatype_name: String,
    rules: Vec<RuleInfo>,
    records: Vec<Record>,
    record_index: BTreeMap<u64, usize>,
    pool: Pool,
    events: usize,
    rounds: Option<usize>,
    trace_seconds: f64,
    trace_peak: usize,
    trace_batches: usize,
    rejected: usize,
}
fn declaration_sort(commands: &[Command]) -> Result<String> {
    let names = commands
        .iter()
        .filter_map(|c| match c {
            Command::Datatype { name, .. }
            | Command::Sort {
                name,
                presort_and_args: None,
                ..
            } => Some(name.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if names.len() > 1
        || commands
            .iter()
            .any(|c| matches!(c, Command::Datatypes { .. }))
    {
        return Err("multi-datatype analysis is not yet supported".into());
    }
    Ok(names.first().cloned().unwrap_or_else(|| "Unit".into()))
}
/// Same as `capture_with_sink`, but for source text already in memory (the wasm
/// build has no filesystem).
fn capture_text_with_sink(
    name: &str,
    text: &str,
    rounds: Option<usize>,
    sink: Option<&mut dyn FnMut(&mut Captured) -> Result>,
) -> Result<Captured> {
    capture_text_with_planner(name,text,rounds,sink,None)
}
fn capture_text_with_planner(
    name:&str,text:&str,rounds:Option<usize>,mut sink:Option<&mut dyn FnMut(&mut Captured)->Result>,
    library:Option<&crate::reuse_graph::library::Library>,
)->Result<Captured> {
    let started = Instant::now();
    let mut eg = EGraph::default();
    let mut commands =
        crate::visual_rule::surface_program(eg.parse_program(Some(name.to_owned()), text)?);
    declaration_sort(&commands)?;
    let declarations: Vec<_> = commands
        .iter()
        .filter(|c| {
            matches!(
                c,
                Command::Datatype { .. }
                    | Command::Relation { .. }
                    | Command::Function { .. }
                    | Command::Sort { .. }
                    | Command::Constructor { .. }
            )
        })
        .cloned()
        .collect();
    let datatype_name = declaration_sort(&declarations)?;
    let datatype = declarations
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    if commands.iter().any(|c| matches!(c, Command::Include(..))) {
        return Err("native analysis does not yet accept included source programs".into());
    }
    if let Some(n) = rounds {
        let indices: Vec<_> = commands
            .iter()
            .enumerate()
            .filter_map(|(i, c)| matches!(c, Command::RunSchedule(_)).then_some(i))
            .collect();
        if n == 0 || indices.len() != 1 {
            return Err("--rounds requires one simple (run N)".into());
        }
        match &mut commands[indices[0]] {
            Command::RunSchedule(egglog::ast::GenericSchedule::Repeat(_, count, inner))
                if matches!(**inner, egglog::ast::GenericSchedule::Run(..)) =>
            {
                *count = n
            }
            _ => return Err("cannot override a complex schedule".into()),
        }
    }
    let mut rules = vec![];
    let mut names = BTreeMap::new();
    for c in &mut commands {
        *c = crate::visual_rule::normalize(c.clone(), rules.len());
        if let Command::Rule { rule } = c {
            if rule.name.is_empty() {
                rule.name = format!("R{}", rules.len());
            }
            if names.insert(rule.name.clone(), rules.len()).is_some() {
                return Err("rule names must be unique".into());
            }
            let calls = crate::visual_rule::positions(rule)
                .into_iter()
                .map(|(span, path, _)| {
                    let expression = crate::visual_rule::owned_expression_at(rule, &path)
                        .unwrap()
                        .clone();
                    (Arc::from(span), (path, expression))
                })
                .collect();
            rules.push(RuleInfo {
                rule: rule.clone(),
                calls,
            });
        }
    }
    let trace = TraceSession::with_dependencies();
    let mut c = Captured {
        reuse_plan: Json::Null,
        reuse_signatures: BTreeSet::new(),
        changes: vec![],
        last_scope: 0,
        preview_source: text.to_owned(),
        boundaries: vec![],
        layers: Default::default(),
        applies: vec![],
        datatype,
        datatype_name,
        rules,
        records: vec![],
        record_index: BTreeMap::new(),
        pool: Pool::default(),
        events: 0,
        rounds: Some(0),
        trace_seconds: 0.0,
        trace_peak: 0,
        trace_batches: 0,
        rejected: 0,
    };
    use crate::reuse_graph::planner::rule_runner::SourcePlanner;
    let mut planner=if let Some(library)=library {
        if SourcePlanner::supports_capture(&commands) {
            let globals=commands.iter().filter_map(|c|match c {Command::Action(Action::Let(_,n,_))=>Some(n.clone()),_=>None}).collect();
            Some(SourcePlanner::new(&mut eg,library,text,globals)?)
        } else {c.reuse_plan=json!({"status":"native_fallback","reason":"program is outside the positive finite planner subset"});None}
    } else {None};
    let mut producers = BTreeMap::new();
    let mut count = 0;
    let mut known = true;
    for command in commands {
        if let Command::RunSchedule(egglog::ast::GenericSchedule::Repeat(_, n, inner)) = &command {
            if matches!(**inner, egglog::ast::GenericSchedule::Run(..)) {
                for _ in 0..*n {
                    let round_start = Instant::now();
                    if let Some(p)=&planner {p.refresh(&mut eg)?;}
                    eg.run_program_with_trace(
                        vec![Command::RunSchedule((**inner).clone())],
                        &trace,
                    )?;
                    let tier0_seconds = round_start.elapsed().as_secs_f64();
                    let collect_start = Instant::now();
                    count += 1;
                    collect_with_planner(&eg, &trace, &mut c, &mut producers,planner.as_mut())?;
                    c.boundaries.push(CaptureBoundary {
                        ripen: None,
                        kind: if known { "round" } else { "execution-boundary" }.into(),
                        round: known.then_some(count),
                        end: c.records.len(),
                    });
                    let collect_seconds = collect_start.elapsed().as_secs_f64();
                    let sink_start = Instant::now();
                    if let Some(sink) = sink.as_mut() {
                        sink(&mut c)?;
                    }
                    let sink_seconds = sink_start.elapsed().as_secs_f64();
                    eprintln!(
                        "[perf-round] round={count} tier0={tier0_seconds:.6} collect={collect_seconds:.6} analysis_and_output={sink_seconds:.6}"
                    );
                    eprintln!("[native] round {count} completed");
                }
                continue;
            }
        }
        if matches!(command, Command::RunSchedule(_)) {
            known = false;
        }
        let schedule = matches!(command, Command::RunSchedule(_));
        let external_change = match &command {
            Command::Action(Action::Set(_, op, _, _) | Action::Change(_, _, op, _)) => {
                Some((false, vec![op.clone()]))
            }
            Command::Action(Action::Union(..)) => Some((true, vec![])),
            Command::Action(Action::Expr(_, e) | Action::Let(_, _, e)) => {
                fn calls(e: &Expr, out: &mut Vec<String>) {
                    if let Expr::Call(_, op, args) = e {
                        out.push(op.clone());
                        for a in args {
                            calls(a, out);
                        }
                    }
                }
                let mut tables = vec![];
                calls(e, &mut tables);
                if tables.is_empty() {
                    None
                } else {
                    Some((false, tables))
                }
            }
            _ => None,
        };
        if schedule {if let Some(p)=&planner {p.refresh(&mut eg)?;}}
        match command {
            Command::Rule{rule} if planner.is_some()=>planner.as_mut().unwrap().install(&mut eg,rule,Some(&trace))?,
            command=>{eg.run_program_with_trace(vec![command], &trace)?;}
        }
        if let Some((reset, tables)) = external_change {
            c.changes.push(crate::closure_contract::Change {
                boundary: c.boundaries.len() + 1,
                reset,
                tables,
                ..Default::default()
            });
        }
        if schedule {
            collect_with_planner(&eg, &trace, &mut c, &mut producers,planner.as_mut())?;
            c.boundaries.push(CaptureBoundary {
                ripen: None,
                kind: "execution-boundary".into(),
                round: None,
                end: c.records.len(),
            });
            if let Some(sink) = sink.as_mut() {
                sink(&mut c)?;
            }
        }
    }
    collect_with_planner(&eg, &trace, &mut c, &mut producers,planner.as_mut())?;
    if c.boundaries.last().is_none_or(|b| b.end != c.records.len())
        || c.changes
            .last()
            .is_some_and(|e| e.boundary > c.boundaries.len())
    {
        c.boundaries.push(CaptureBoundary {
            ripen: None,
            kind: "final".into(),
            round: None,
            end: c.records.len(),
        });
    }
    if let Some(sink) = sink.as_mut() {
        sink(&mut c)?;
    }
    c.rounds = known.then_some(count);
    c.trace_seconds = started.elapsed().as_secs_f64();
    Ok(c)
}

// Only direct committed producer certificates survive a batch, not WriteEvents.
// 看上去是个节点但实际上又不是，因为他这里只有一个table
struct Producer {
    match_id: u64,
    table: String,
    row: Vec<Value>,
}
fn collect(
    eg: &EGraph,
    trace: &TraceSession,
    c: &mut Captured,
    producers: &mut BTreeMap<u64, Producer>,
) -> Result {
    collect_with_planner(eg,trace,c,producers,None)
}
fn collect_with_planner(eg:&EGraph,trace:&TraceSession,c:&mut Captured,producers:&mut BTreeMap<u64,Producer>,planner:Option<&mut crate::reuse_graph::planner::rule_runner::SourcePlanner>)->Result {
    let prior_scope = c.last_scope;
    let rules = &c.rules;
    let names: BTreeMap<_, _> = rules
        .iter()
        .enumerate()
        .map(|(i, r)| (r.rule.name.clone(), i))
        .collect();
    let batch = trace.drain_completed();
    if let Some(planner)=planner {planner.consume(eg,&batch)?;c.reuse_plan=planner.report();c.reuse_signatures=planner.signatures();}
    let batch_len = batch.len();
    c.trace_peak = c.trace_peak.max(batch_len);
    c.trace_batches += usize::from(batch_len > 0);
    if trace.buffered_event_count() != 0 {
        return Err("raw trace buffers were not drained at execution boundary".into());
    }
    let raw_unions = batch.unions.clone();
    let invalidations = batch.invalidations.clone();
    let matches = batch.matches;
    let events = c.events + matches.len();
    let resets = trace.scope_resets();
    let scope = |id| resets.partition_point(|r| *r < id);
    let survived: BTreeSet<_> = batch
        .actions
        .into_iter()
        .filter(|x| x.outcome == RuleActionOutcome::Survived)
        .map(|x| x.match_event_id)
        .collect();
    let writes = batch.writes;
    let mut added_producers = vec![];
    for w in &writes {
        if w.outcome == WriteOutcome::Inserted && w.rebuild_of.is_none() {
            added_producers.push(w.event_id);
            producers.insert(
                w.event_id,
                Producer {
                    match_id: w.match_event_id,
                    table: format!("{:?}", w.table),
                    row: w.actual.clone(),
                },
            );
        }
    }
    let mut actions: BTreeMap<u64, Vec<_>> = BTreeMap::new();
    for w in &writes {
        actions.entry(w.match_event_id).or_default().push(w);
    }
    let mut unions: BTreeMap<u64, Vec<_>> = BTreeMap::new();
    for u in batch.unions {
        if u.displaced.is_some() {
            unions
                .entry(u.match_event_id)
                .or_default()
                .push((u.lhs, u.rhs));
        }
    }
    let mut reads: BTreeMap<u64, Vec<_>> = BTreeMap::new();
    let mut schemas = BTreeMap::new();
    for (table, name) in trace.table_names() {
        if let Some(f) = eg.get_function(&name) {
            let schema = f.schema();
            schemas.insert(
                format!("{table:?}"),
                (
                    name,
                    schema.input.len(),
                    schema
                        .input
                        .iter()
                        .chain(std::iter::once(&schema.output))
                        .map(|s| Arc::<str>::from(s.name()))
                        .collect::<Vec<_>>(),
                ),
            );
        }
    }
    for r in batch.reads.into_iter().filter(|r|!r.planner_summary) {
        reads.entry(r.match_event_id).or_default().push(r);
    }
    let variable_sorts: Vec<BTreeMap<String, Arc<str>>> = rules
        .iter()
        .map(|r| {
            let mut sorts = BTreeMap::new();
            for (_, e) in r.calls.values() {
                if let Expr::Call(_, op, args) = e {
                    if let Some(f) = eg.get_function(op) {
                        for (a, sort) in args.iter().zip(&f.schema().input) {
                            if let Expr::Var(_, n) = a {
                                sorts.insert(n.clone(), Arc::<str>::from(sort.name()));
                            }
                        }
                    }
                }
            }
            for f in &r.rule.body {
                if let Fact::Eq(_, Expr::Var(_, n), Expr::Call(_, op, _)) = f {
                    if let Some(f) = eg.get_function(op) {
                        sorts.insert(n.clone(), Arc::<str>::from(f.schema().output.name()));
                    }
                }
            }
            sorts
        })
        .collect();
    let pool = &mut c.pool;
    let records = &mut c.records;
    let imported = &mut c.record_index;
    let mut ordered = matches.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|m| m.event_id);
    for m in ordered {
        let Some(&rule) = names.get(m.rule.as_ref()) else {
            continue;
        };
        let ws = actions.entry(m.event_id).or_default();
        ws.sort_by_key(|w| (w.source_span.clone(), format!("{:?}", w.table), w.event_id));
        if !survived.contains(&m.event_id)
            || !m.physical_witness_complete
            || !(ws.iter().any(|w| w.outcome != WriteOutcome::Unsupported)
                || unions.contains_key(&m.event_id))
        {
            continue;
        }
        let s = scope(m.event_id);
        let datatype_name = c.datatype_name.clone();
        let value_token = |v| Token {
            sort: Arc::from(datatype_name.as_str()),
            key: Key::Value(s, v),
        };
        let mut named = m
            .bindings
            .iter()
            .filter_map(|b| {
                b.name
                    .as_ref()
                    .filter(|n| !n.starts_with('@'))
                    .map(|n| (n.to_string(), b.value))
            })
            .collect::<Vec<_>>();
        // A missing sort is an unsupported trace witness, not an execution error.
        if named
            .iter()
            .any(|(n, _)| !variable_sorts[rule].contains_key(n))
        {
            continue;
        }
        named.sort_by(|a, b| a.0.cmp(&b.0));
        let mut values = vec![];
        let mut outputs = vec![];
        let mut inputs = vec![];
        for (n, v) in &named {
            let sort = variable_sorts[rule]
                .get(n)
                .ok_or_else(|| format!("unmapped binding sort: {n}"))?
                .clone();
            let literal = if sort.as_ref() == "i64" {
                Some(eg.value_to_base::<i64>(*v).to_string())
            } else {
                None
            };
            let token = pool.intern(Token {
                sort,
                key: Key::Value(s, *v),
            });
            if let Some(literal) = literal {
                pool.literals.insert(token, literal);
            }
            values.push(token);
            outputs.push(Output::Var(n.clone()));
            inputs.push(Input::Var(n.clone()));
        }
        for w in ws
            .iter()
            .filter(|w| w.outcome != WriteOutcome::Unsupported && w.rebuild_of.is_none())
        {
            if let (Some(span), Some((_, arity, sorts))) =
                (&w.source_span, schemas.get(&format!("{:?}", w.table)))
            {
                for (col, v) in w.actual.iter().take(arity + 1).enumerate() {
                    values.push(pool.intern(Token {
                        sort: sorts[col].clone(),
                        key: Key::Value(s, *v),
                    }));
                    outputs.push(Output::Column(span.clone(), col));
                }
            }
        }
        let mut produced = vec![];
        for w in ws
            .iter()
            .filter(|w| w.outcome == WriteOutcome::Inserted && w.rebuild_of.is_none())
        {
            let name = schemas
                .get(&format!("{:?}", w.table))
                .map(|x| x.0.to_string())
                .unwrap_or_else(|| format!("{:?}", w.table));
            let token = pool.intern(Token {
                sort: Arc::from(format!("Fact:{name}")),
                key: Key::Write(s, w.event_id),
            });
            produced.push(token);
            if let Some(span) = &w.source_span {
                values.push(token);
                outputs.push(Output::Row(span.clone()));
            }
        }
        let rs = reads.entry(m.event_id).or_default();
        rs.sort_by_key(|r| (r.source_span.clone(), r.table_name.clone(), r.event_id));
        let mut required = vec![];
        let mut read_parents = vec![];
        let mut read_results = vec![];
        let mut parents = BTreeSet::new();
        for r in rs.iter() {
            read_results.push(
                schemas
                    .get(&format!("{:?}", r.table))
                    .and_then(|(_, arity, sorts)| {
                        (sorts[*arity].as_ref() == datatype_name)
                            .then(|| r.row.get(*arity))
                            .flatten()
                    })
                    .map(|v| pool.intern(value_token(*v))),
            );
            let valid = r
                .producer_match_event_id
                .zip(r.producer_write_event_id)
                .and_then(|(p, w)| {
                    let row = producers.get(&w)?;
                    let &parent = imported.get(&p)?;
                    (row.match_id == p
                        && row.table == format!("{:?}", r.table)
                        && row.row == r.row
                        && w < r.event_id
                        && p < m.event_id
                        && scope(p) == s)
                        .then_some((parent, w))
                });
            let name = r
                .table_name
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("{:?}", r.table));
            let key = if let Some((parent, w)) = valid {
                parents.insert(parent);
                read_parents.push(Some(parent));
                Key::Write(s, w)
            } else {
                read_parents.push(None);
                Key::External(s, format!("{:?}", r.table), r.row.clone())
            };
            required.push(pool.intern(Token {
                sort: Arc::from(format!("Fact:{name}")),
                key,
            }));
            inputs.push(Input::Read(
                r.source_span
                    .clone()
                    .unwrap_or_else(|| Arc::from("unmapped")),
            ));
        }
        let parents: Vec<usize> = parents.into_iter().collect();
        let mut wanted = values[..named.len()].to_vec();
        wanted.extend(&required);
        let mut ports = vec![];
        let mut external = vec![];
        for (slot, v) in wanted.iter().enumerate() {
            let preferred = slot.checked_sub(named.len()).and_then(|i| read_parents[i]);
            let found = parents
                .iter()
                .enumerate()
                .filter(|(_, p)| preferred.is_none_or(|x| x == **p))
                .find_map(|(k, p)| {
                    records[*p]
                        .values
                        .iter()
                        .position(|x| x == v)
                        .map(|j| (k, j))
                });
            if let Some((p, j)) = found {
                ports.push(Port::Parent(p, j));
            } else {
                let j = external.iter().position(|x| x == v).unwrap_or_else(|| {
                    external.push(*v);
                    external.len() - 1
                });
                ports.push(Port::External(j));
            }
        }
        let union_tokens = unions
            .remove(&m.event_id)
            .unwrap_or_default()
            .into_iter()
            .map(|(a, b)| (pool.intern(value_token(a)), pool.intern(value_token(b))))
            .collect();
        let coarse = parents.is_empty() || ports.iter().any(|p| matches!(p, Port::External(_)));
        imported.insert(m.event_id, records.len());
        records.push(Record {
            id: m.event_id,
            rule,
            parents,
            ports,
            inputs,
            read_results,
            outputs,
            values,
            wanted,
            external,
            required,
            produced,
            unions: union_tokens,
            updated_rows: ws.iter().any(|w| w.outcome == WriteOutcome::Updated && w.rebuild_of.is_none()),
            coarse,
            comb: None,
            instance: None,
            extension: 0,
        });
    }
    let mut change = crate::closure_contract::Change {
        boundary: c.boundaries.len() + 1,
        reset: resets.len() != c.last_scope,
        ..Default::default()
    };
    c.last_scope = resets.len();
    for w in &writes {
        if !matches!(w.outcome, WriteOutcome::Inserted | WriteOutcome::Updated) {
            continue;
        }
        if let Some((name, _, sorts)) = schemas.get(&format!("{:?}", w.table)) {
            for (v, sort) in w.actual.iter().zip(sorts) {
                if sort.as_ref() != "Unit" {
                    change.tokens.push(pool.intern(Token {
                        sort: sort.clone(),
                        key: Key::Value(scope(w.match_event_id), *v),
                    }));
                }
            }
            if w.outcome == WriteOutcome::Updated {
                change.tables.push(name.to_string());
            }
        } else {
            change.reset = true;
        }
    }
    for u in raw_unions.into_iter().filter(|u| u.displaced.is_some()) {
        let tokens = [u.lhs, u.rhs].map(|v| {
            pool.intern(Token {
                sort: Arc::from(c.datatype_name.as_str()),
                key: Key::Value(scope(u.match_event_id), v),
            })
        });
        change.unions.push((tokens[0], tokens[1]));
    }
    for invalid in invalidations {
        if let Some(p) = producers.get(&invalid.write_event_id) {
            if let Some((name, _, _)) = schemas.get(&p.table) {
                change.tables.push(name.to_string());
            } else {
                change.reset = true;
            }
        } else {
            change.reset = true;
        } // Unknown row ownership: invalidate conservatively.
    }
    // Subsume may not have a row producer. Its target table is still known.
    for m in &matches {
        if survived.contains(&m.event_id) {
            if let Some(&r) = names.get(m.rule.as_ref()) {
                for a in &rules[r].rule.head.0 {
                    if let Action::Change(_, _, op, _) = a {
                        change.tables.push(op.clone());
                    }
                }
            }
        }
    }
    change.tokens.sort();
    change.tokens.dedup();
    change.tables.sort();
    change.tables.dedup();
    if change.reset
        || !change.tokens.is_empty()
        || !change.tables.is_empty()
        || !change.unions.is_empty()
    {
        c.changes.push(change);
    }
    // Certificates in rolled-back scopes cannot serve future reads. A read's
    // live origin still must explicitly reference this exact committed row.
    let current_scope = resets.len();
    if prior_scope != current_scope {
        producers.retain(|_, p| {
            scope(p.match_id) == current_scope && imported.contains_key(&p.match_id)
        });
    } else {
        for id in added_producers {
            if producers.get(&id).is_some_and(|p| {
                scope(p.match_id) != current_scope || !imported.contains_key(&p.match_id)
            }) {
                producers.remove(&id);
            }
        }
    }
    eprintln!(
        "[trace] consumed {batch_len} raw events; raw buffers drained; {} producer certificates",
        producers.len()
    );
    c.rejected = events - records.len();
    c.events = events;
    Ok(())
}

fn update_applies(c: &mut Captured) -> Result {
    use crate::coarse_smooth::{Apply, Effect, RelativeBinding};
    let rule_text: Vec<_> = c.rules.iter().map(|r| r.rule.to_string()).collect();
    for r in c.records.iter().skip(c.applies.len()) {
        let apply = Apply {
            event: r.id,
            rule: rule_text[r.rule].clone(),
            input_roles: r
                .inputs
                .iter()
                .map(|input| match input {
                    Input::Var(n) => format!("var:{n}"),
                    Input::Read(span) => format!(
                        "read:{}",
                        c.rules[r.rule]
                            .calls
                            .get(span)
                            .map(|x| x.0.as_str())
                            .unwrap_or(span)
                    ),
                })
                .collect(),
            output_roles: r
                .outputs
                .iter()
                .map(|output| match output {
                    Output::Var(n) => format!("var:{n}"),
                    Output::Row(span) => format!(
                        "row:{}",
                        c.rules[r.rule]
                            .calls
                            .get(span)
                            .map(|x| x.0.as_str())
                            .unwrap_or(span)
                    ),
                    Output::Column(span, col) => format!(
                        "column:{}:{col}",
                        c.rules[r.rule]
                            .calls
                            .get(span)
                            .map(|x| x.0.as_str())
                            .unwrap_or(span)
                    ),
                })
                .collect(),
            parents: r.parents.clone(),
            binding: r
                .ports
                .iter()
                .map(|p| match p {
                    Port::Parent(parent, output) => RelativeBinding::ParentPort {
                        parent: *parent,
                        output: *output,
                    },
                    Port::External(slot) => RelativeBinding::External { slot: *slot },
                })
                .collect(),
            wanted: r.wanted.clone(),
            outputs: r.values.clone(),
            external: r.external.clone(),
            required: r.required.iter().copied().map(Effect::RowFact).collect(),
            external_facts: r
                .required
                .iter()
                .filter(|v| {
                    matches!(
                        c.pool.values[**v].key,
                        Key::External(..) | Key::Replay(_, true)
                    )
                })
                .copied()
                .map(Effect::RowFact)
                .collect(),
            produced: r
                .produced
                .iter()
                .copied()
                .map(Effect::RowFact)
                .chain(r.unions.iter().map(|(a, b)| Effect::Equal(*a, *b)))
                .collect(),
        };
        c.applies.push(apply);
    }
    Ok(())
}

fn update_layers(c: &mut Captured) -> Result {
    update_applies(c)?;
    c.layers.ripen = c.boundaries.last().and_then(|b| b.ripen.clone());
    if !crate::graph_render::graphs_enabled()
        && std::env::var("EGG_LAYOUT_COMPOSITION_DISCOVERY").as_deref()!=Ok("layers")
        && std::env::var("EGG_LAYOUT_LAYER_VIEW").as_deref()!=Ok("1") {return Ok(());}
    for apply in &c.applies[c.layers.occurrences.len()..] {
        c.layers.push(apply.clone()).map_err(|e|->Box<dyn std::error::Error>{e.into()})?;
    }
    Ok(())
}

fn execute(
    name: &str,
    text: &str,
    rounds: Option<usize>,
    out: &Path,
    replay: Option<&Path>,
    save_history: bool,
    persist_frames: bool,
    stream_frames: bool,
    observation: crate::ripen_observation::Limits,
    emit: &mut dyn FnMut(Json) -> Result,
    reuse_library:Option<&Path>,
    optimize:bool,
) -> Result<Json> {
    std::fs::create_dir_all(out)?;
    let render_batch = crate::graph_render::Batch::start()?;
    let detailed_trace = crate::graph_render::graphs_enabled()
        && std::env::var("EGG_LAYOUT_FULL_TRACE").as_deref() == Ok("1");
    eprintln!(
        "ripen limits: {} ms, {} facts; detailed trace: {}",
        observation
            .milliseconds
            .map_or("off".into(), |n| n.to_string()),
        observation.facts.map_or("off".into(), |n| n.to_string()),
        detailed_trace
    );
    let mut pipeline =
        saturated_rule_composition_pipeline::Pipeline::new(out, name.into(), observation,reuse_library)?;
    let mut manifest = vec![];
    let mut emitted = 0;
    let mut boundary = 0;
    let mut boundary_emit = |c: &mut Captured| -> Result {
        update_layers(c)?;
        boundary += 1;
        for r in c.records[emitted..].iter().filter(|_| detailed_trace) {
            let rule = &c.rules[r.rule].rule;
            let mut row = debug::render(rule);
            let (line, end) = debug::location(rule);
            row["kind"] = json!("application");
            row["id"] = json!(format!("apply:{}", r.id));
            row["event"] = json!(r.id);
            row["rule"] = json!(rule.name);
            row["source_line"] = json!(line);
            row["end_line"] = json!(end);
            row["boundary"] = json!(boundary);
            row["parents"] = json!(r.parents);
            emit(row)?;
        }
        emitted = c.records.len();
        pipeline.step(c, &c.layers, boundary)?;
        if !persist_frames && !stream_frames {
            return Ok(());
        }
        let b = c.boundaries.last().cloned().unwrap_or(CaptureBoundary {
            ripen: None,
            kind: "capture".into(),
            round: None,
            end: c.records.len(),
        });
        let mut frame = layer_view::snapshot(&c.layers, &b, boundary, &c.preview_source)?;
        frame["saturated_rule_composition"] = pipeline.report().clone();
        frame["ripen_output_directory"] = json!(out);
        if let Some(dot) = frame["saturated_rule_composition"]["catalog"]["dot"].as_str() {
            frame["dots"]["saturated_rule_composition"] = json!(dot);
        }
        if persist_frames {
            layer_view::persist(out, &frame, &mut manifest)?;
        }
        emit(frame)
    };
    let c = if let Some(path) = replay {
        let mut c = history::read(path)?;
        let boundaries=std::mem::take(&mut c.boundaries);
        if boundaries.is_empty() {boundary_emit(&mut c)?;} else {
            let mut records=std::mem::take(&mut c.records).into_iter();
            let mut changes=std::mem::take(&mut c.changes).into_iter().peekable();
            for (index,boundary) in boundaries.into_iter().enumerate() {
                while c.records.len()<boundary.end {c.records.push(records.next().ok_or("incomplete replay boundary")?);}
                while changes.peek().is_some_and(|change|change.boundary<=index+1) {c.changes.push(changes.next().unwrap());}
                c.boundaries.push(boundary);
                boundary_emit(&mut c)?;
            }
            c.changes.extend(changes);
        }
        c
    } else {
        let library=if optimize {Some(crate::reuse_graph::library::Library::load(reuse_library.ok_or("optimization requires a reuse library")?)?)}else{None};
        if let Some(library)=&library {capture_text_with_planner(name,text,rounds,Some(&mut boundary_emit),Some(library))?} else {capture_text_with_sink(name,text,rounds,Some(&mut boundary_emit))?}
    };
    drop(boundary_emit);
    if save_history {
        history::save(&out.join("history.json"), Path::new(name), &c)?;
    }
    // All cells keep their native engines. Drain fairly; an infinite cell stays
    // Running and can be interrupted by the caller, never marked saturated.
    while pipeline.has_work() {
        boundary += 1;
        pipeline.step(&c, &c.layers, boundary)?;
        if stream_frames {
            let q = pipeline.report().clone();
            let mut progress = json!({"kind":"ripen_progress"});
            progress["saturated_rule_composition"] = q;
            emit(progress)?;
        }
    }
    if persist_frames || stream_frames {
        let b = CaptureBoundary {
            ripen: None,
            kind: if pipeline.report()["counts"]["ObservedUnfinished"]
                .as_u64()
                .unwrap_or(0)
                > 0
            {
                "ripen-observation"
            } else {
                "ripen-complete"
            }
            .into(),
            round: None,
            end: c.records.len(),
        };
        boundary += 1;
        let mut frame = layer_view::snapshot(&c.layers, &b, boundary, &c.preview_source)?;
        frame["saturated_rule_composition"] = pipeline.report().clone();
        if let Some(dot) = frame["saturated_rule_composition"]["catalog"]["dot"].as_str() {
            frame["dots"]["saturated_rule_composition"] = json!(dot);
        }
        if persist_frames {
            layer_view::persist(out, &frame, &mut manifest)?;
        }
        emit(frame)?;
    }
    let finish_started = Instant::now();
    let mut report = json!({"status":if pipeline.report()["counts"]["ObservedUnfinished"].as_u64().unwrap_or(0)>0 {"observed_unfinished"}else{"complete"},"summary":{"applications":c.records.len(),"logical_matches":c.events,"excluded":c.rejected,"scope":"native history -> CS -> ripen -> equivalence -> weak dominate"},"layers":c.layers.report(),"saturated_rule_composition":null});
    report["reuse_plan"]=c.reuse_plan.clone();
    report["saturated_rule_composition"] = pipeline.take_report();
    // Opt-in through the graph-output switch: leave graphless native timing alone.
    if crate::graph_render::graphs_enabled() {
        let specs = c.pool.values.iter().enumerate().map(|(i,t)|crate::slotted_layer::TokenSpec {
            sort: t.sort.to_string(), literal:c.pool.literals.get(&i).cloned(),
        }).collect::<Vec<_>>();
        let slotted = crate::slotted_layer::LayerCanon::build(&c.layers, &specs)?;
        report["slotted_canon"] = slotted.report().clone();
        std::fs::write(out.join("slotted-canon.json"), serde_json::to_vec_pretty(slotted.report())?)?;
        std::fs::write(out.join("slotted-canon.dsl"), slotted.to_dsl())?;
        for (name,dot) in [("slotted-canon.dot",slotted.to_dot()),
            ("slotted-egraph.dot",crate::slotted_dot::to_dot(&slotted.egraph))] {
            let path=out.join(name);
            std::fs::write(&path,dot)?;
            crate::graph_render::enqueue(&path)?;
        }
    }
    std::fs::write(out.join("analysis.json"), serde_json::to_vec(&report)?)?;
    std::fs::write(
        out.join("layers.json"),
        serde_json::to_vec(&c.layers.report())?,
    )?;
    if std::env::var_os("EGG_LAYOUT_PROFILE").is_some() {
        eprintln!(
            "[perf-finish] output_seconds={:.6}",
            finish_started.elapsed().as_secs_f64()
        );
    }
    let cleanup_started = Instant::now();
    pipeline.remember_grafts(c.reuse_signatures.clone());
    pipeline.save_reuse_library()?;
    pipeline.release_engines();
    if std::env::var_os("EGG_LAYOUT_PROFILE").is_some() {
        eprintln!(
            "[perf-finish] cleanup_seconds={:.6}",
            cleanup_started.elapsed().as_secs_f64()
        );
    }
    emit(json!({"kind":"rendering","message":"Analysis complete; rendering SVGs in parallel"}))?;
    render_batch.finish()?;
    Ok(report)
}
pub fn run(
    source: &Path,
    out: &Path,
    rounds: Option<usize>,
    replay: Option<&Path>,
    save_history: bool,
) -> Result<Json> {
    run_with_observation(
        source,
        out,
        rounds,
        replay,
        save_history,
        crate::ripen_observation::Limits::from_env()?,
    )
}
pub fn run_with_observation(
    source: &Path,
    out: &Path,
    rounds: Option<usize>,
    replay: Option<&Path>,
    save_history: bool,
    observation: crate::ripen_observation::Limits,
) -> Result<Json> {
    run_with_reuse(source,out,rounds,replay,save_history,observation,None)
}
pub fn run_with_reuse(
    source:&Path,out:&Path,rounds:Option<usize>,replay:Option<&Path>,save_history:bool,
    observation:crate::ripen_observation::Limits,reuse_library:Option<&Path>,
)->Result<Json> {
    run_configured(source,out,rounds,replay,save_history,observation,reuse_library,false)
}
pub fn run_configured(source:&Path,out:&Path,rounds:Option<usize>,replay:Option<&Path>,save_history:bool,observation:crate::ripen_observation::Limits,reuse_library:Option<&Path>,optimize:bool)->Result<Json> {
    if optimize && (reuse_library.is_none()||replay.is_some()){return Err("planned capture requires a source and reuse library, not history replay".into());}
    observation.validate()?;
    if out.exists() {
        return Err("analysis output already exists".into());
    }
    let text = if replay.is_some() {
        String::new()
    } else {
        std::fs::read_to_string(source)?
    };
    execute(
        &source.to_string_lossy(),
        &text,
        rounds,
        out,
        replay,
        save_history,
        crate::graph_render::graphs_enabled(),
        false,
        observation,
        &mut |_| Ok(()),
        reuse_library,
        optimize,
    )
}

#[cfg(test)]
mod incremental_history_tests {
    use super::*;
    #[test]
    fn producer_index_does_not_reuse_rolled_back_facts() {
        let c = capture_text_with_sink(
            "scope-regression.egg",
            r#"
            (datatype E (A i64) (B i64) (C i64))
            (ruleset first) (ruleset second)
            (rewrite (A x) (B x) :ruleset first :name "make-b")
            (rewrite (B x) (C x) :ruleset second :name "make-c")
            (A 1)
            (push)
            (run first 1)
            (pop)
            (run second 1)
            (run first 1)
            (run second 1)
        "#,
            None,
            None,
        )
        .unwrap();
        assert_eq!(c.record_index.len(), c.records.len());
        let make_b: Vec<_> = c
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| c.rules[r.rule].rule.name == "make-b")
            .map(|(i, _)| i)
            .collect();
        let make_c: Vec<_> = c
            .records
            .iter()
            .filter(|r| c.rules[r.rule].rule.name == "make-c")
            .collect();
        assert_eq!(make_b.len(), 2);
        assert_eq!(make_c.len(), 1);
        assert!(make_c[0].parents.contains(&make_b[1]));
        assert!(!make_c[0].parents.contains(&make_b[0]));
    }
}
