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

#[path = "native_bake.rs"]
pub mod bake;
#[path = "native_binding.rs"]
mod binding;
#[path = "native_catalog.rs"]
mod catalog;
#[path = "native_debug.rs"]
pub mod debug;
#[path = "native_fractal.rs"]
mod fractal;
#[path = "native_growth.rs"]
mod growth;
#[path = "native_history.rs"]
mod history;
#[path = "native_layer_view.rs"]
mod layer_view;
#[path = "native_recursive.rs"]
mod recursive;
#[path = "native_closed_pipeline.rs"]
mod closed_pipeline;
#[path = "native_ripen.rs"]
pub mod ripen;

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
fn string(s: &str) -> Expr {
    Expr::Lit(sp(), Literal::String(s.into()))
}
fn fact(op: &str, args: Vec<Expr>) -> Command {
    Command::Action(Action::Expr(sp(), call(op, args)))
}
fn set(op: &str, id: u64, value: Expr) -> Command {
    Command::Action(Action::Set(sp(), op.into(), vec![num(id)], value))
}
fn list(items: Vec<Expr>, cons: &str, nil: &str) -> Expr {
    items
        .into_iter()
        .rev()
        .fold(call(nil, vec![]), |tail, x| call(cons, vec![x, tail]))
}
fn cref(id: u64) -> Expr {
    call("ImportedComb", vec![num(id)])
}
fn iref(id: u64) -> Expr {
    call("ImportedInstance", vec![num(id)])
}
fn vref(id: usize) -> Expr {
    call("ImportedValue", vec![num(id as u64)])
}
fn flush(eg: &mut EGraph, batch: &mut Vec<Command>) -> Result {
    if !batch.is_empty() {
        eg.run_ground_import(&std::mem::take(batch))?;
    }
    Ok(())
}
fn emit(eg: &mut EGraph, batch: &mut Vec<Command>, c: Command) -> Result {
    batch.push(c);
    if batch.len() >= 512 {
        flush(eg, batch)?;
    }
    Ok(())
}
fn load(eg: &mut EGraph, file: &Path, root: &Path) -> Result {
    let commands = eg.parse_program(
        Some(file.to_string_lossy().into_owned()),
        &crate::embedded_rules::rule_text(file)?,
    )?;
    for c in commands {
        if let Command::Include(_, p) = c {
            load(eg, &root.join(p), root)?;
        } else {
            eg.run_program(vec![c])?;
        }
    }
    Ok(())
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
    pub outputs: Vec<Output>,
    values: Vec<usize>,
    wanted: Vec<usize>,
    external: Vec<usize>,
    required: Vec<usize>,
    produced: Vec<usize>,
    unions: Vec<(usize, usize)>,
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
    preview_source: String,
    boundaries: Vec<CaptureBoundary>,
    layers: crate::coarse_smooth::LayerStore,
    datatype: String,
    datatype_name: String,
    rules: Vec<RuleInfo>,
    records: Vec<Record>,
    pool: Pool,
    events: usize,
    rounds: Option<usize>,
    trace_seconds: f64,
    trace_peak: usize,
    trace_batches: usize,
    rejected: usize,
}
impl Captured {
    fn is_coarse(&self, r: &Record) -> bool {
        self.layers
            .occurrence(r.id)
            .map(|o| {
                matches!(
                    self.layers.combs[o.comb].kind,
                    crate::coarse_smooth::CombKind::CoarseComb
                )
            })
            .unwrap_or(r.coarse)
    }
}

fn capture(
    source: &Path,
    rounds: Option<usize>,
    online: Option<(&mut EGraph, &Path, &rayon::ThreadPool)>,
) -> Result<Captured> {
    let text = std::fs::read_to_string(source)?;
    capture_text_with_sink(&source.to_string_lossy(), &text, rounds, online, None)
}
fn capture_with_sink(
    source: &Path,
    rounds: Option<usize>,
    online: Option<(&mut EGraph, &Path, &rayon::ThreadPool)>,
    sink: Option<&mut dyn FnMut(&mut Captured) -> Result>,
) -> Result<Captured> {
    let text = std::fs::read_to_string(source)?;
    capture_text_with_sink(&source.to_string_lossy(), &text, rounds, online, sink)
}
fn declaration_sort(commands: &[Command]) -> Result<String> {
    let names=commands.iter().filter_map(|c| match c {Command::Datatype{name,..} | Command::Sort{name,presort_and_args:None,..}=>Some(name.clone()), _=>None}).collect::<Vec<_>>();
    if names.len()>1 || commands.iter().any(|c| matches!(c,Command::Datatypes{..})) {
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
    mut online: Option<(&mut EGraph, &Path, &rayon::ThreadPool)>,
    mut sink: Option<&mut dyn FnMut(&mut Captured) -> Result>,
) -> Result<Captured> {
    let started = Instant::now();
    let mut eg = EGraph::default();
    let mut commands = eg.parse_program(Some(name.to_owned()), text)?;
    declaration_sort(&commands)?;
    let declarations: Vec<_> = commands.iter().filter(|c| matches!(c,
        Command::Datatype {..} | Command::Relation {..} | Command::Function{..}
        | Command::Sort{..} | Command::Constructor {..})).cloned().collect();
    let datatype_name = declaration_sort(&declarations)?;
    let datatype = declarations.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
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
        preview_source: text.to_owned(),
        boundaries: vec![],
        layers: Default::default(),
        datatype,
        datatype_name,
        rules,
        records: vec![],
        pool: Pool::default(),
        events: 0,
        rounds: Some(0),
        trace_seconds: 0.0,
        trace_peak: 0,
        trace_batches: 0,
        rejected: 0,
    };
    let mut inserted = (0, 0);
    let mut producers = BTreeMap::new();
    let mut count = 0;
    let mut known = true;
    for command in commands {
        if let Command::RunSchedule(egglog::ast::GenericSchedule::Repeat(_, n, inner)) = &command {
            if matches!(**inner, egglog::ast::GenericSchedule::Run(..)) {
                for _ in 0..*n {
                    let round_start = Instant::now();
                    eg.run_program_with_trace(
                        vec![Command::RunSchedule((**inner).clone())],
                        &trace,
                    )?;
                    let tier0_seconds = round_start.elapsed().as_secs_f64();
                    let collect_start = Instant::now();
                    count += 1;
                    collect(&eg, &trace, &mut c, &mut producers)?;
                    c.boundaries.push(CaptureBoundary {
                        ripen: None,
                        kind: if known { "round" } else { "execution-boundary" }.into(),
                        round: known.then_some(count),
                        end: c.records.len(),
                    });
                    if let Some(sink) = sink.as_mut() {
                        sink(&mut c)?;
                    }
                    let collect_seconds = collect_start.elapsed().as_secs_f64();
                    let tier1_start = Instant::now();
                    if let Some((tier1, root, worker)) = online.as_mut() {
                        worker
                            .install(|| {
                                build_tier1(&mut c, tier1, root, &mut inserted)
                                    .map_err(|e| e.to_string())
                            })
                            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                        eprintln!(
                            "[online] tier1 contains {} applications before next round",
                            c.records.len()
                        );
                    }
                    eprintln!(
                        "[perf-round] round={count} tier0={tier0_seconds:.6} collect={collect_seconds:.6} tier1={:.6}",
                        tier1_start.elapsed().as_secs_f64()
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
        eg.run_program_with_trace(vec![command], &trace)?;
        if schedule {
            collect(&eg, &trace, &mut c, &mut producers)?;
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
    collect(&eg, &trace, &mut c, &mut producers)?;
    if c.boundaries.last().is_none_or(|b| b.end != c.records.len()) {
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
    if let Some((tier1, root, worker)) = online.as_mut() {
        worker
            .install(|| build_tier1(&mut c, tier1, root, &mut inserted).map_err(|e| e.to_string()))
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    }
    c.rounds = known.then_some(count);
    c.trace_seconds = started.elapsed().as_secs_f64();
    Ok(c)
}

// Only direct committed producer certificates survive a batch, not WriteEvents.
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
    let rules = &c.rules;
    let names: BTreeMap<_, _> = rules
        .iter()
        .enumerate()
        .map(|(i, r)| (r.rule.name.clone(), i))
        .collect();
    let batch = trace.drain_completed();
    let batch_len = batch.len();
    c.trace_peak = c.trace_peak.max(batch_len);
    c.trace_batches += usize::from(batch_len > 0);
    if trace.buffered_event_count() != 0 {
        return Err("raw trace buffers were not drained at execution boundary".into());
    }
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
    for w in &writes {
        if w.outcome == WriteOutcome::Inserted && w.rebuild_of.is_none() {
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
    for r in batch.reads {
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
    let mut imported: BTreeMap<_, _> = records.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
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
        if named.iter().any(|(n,_)| !variable_sorts[rule].contains_key(n)) { continue; }
        named.sort_by(|a, b| a.0.cmp(&b.0));
        let mut values = vec![];
        let mut outputs = vec![];
        let mut inputs = vec![];
        for (n, v) in &named {
            let sort = variable_sorts[rule]
                .get(n)
                .ok_or_else(|| format!("unmapped binding sort: {n}"))?
                .clone();
            let literal=if sort.as_ref()=="i64" {Some(eg.value_to_base::<i64>(*v).to_string())}else{None};
            let token=pool.intern(Token {sort,key:Key::Value(s,*v)});
            if let Some(literal)=literal {pool.literals.insert(token,literal);}
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
        let mut parents = BTreeSet::new();
        for r in rs.iter() {
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
            outputs,
            values,
            wanted,
            external,
            required,
            produced,
            unions: union_tokens,
            coarse,
            comb: None,
            instance: None,
            extension: 0,
        });
    }
    // Certificates in rolled-back scopes cannot serve future reads. A read's
    // live origin still must explicitly reference this exact committed row.
    let current_scope = resets.len();
    producers
        .retain(|_, p| scope(p.match_id) == current_scope && imported.contains_key(&p.match_id));
    eprintln!(
        "[trace] consumed {batch_len} raw events; raw buffers drained; {} producer certificates",
        producers.len()
    );
    c.rejected = events - records.len();
    c.events = events;
    Ok(())
}

fn update_layers(c: &mut Captured) -> Result {
    c.layers.ripen = c.boundaries.last().and_then(|b| b.ripen.clone());
    use crate::coarse_smooth::{Apply, Effect, RelativeBinding};
    for r in c.records.iter().skip(c.layers.occurrences.len()) {
        let apply = Apply {
            event: r.id,
            rule: c.rules[r.rule].rule.to_string(),
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
        c.layers
            .push(apply)
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    }
    Ok(())
}

fn build_tier1(
    c: &mut Captured,
    eg: &mut EGraph,
    _root: &Path,
    inserted: &mut (usize, usize),
) -> Result {
    let stage_start = Instant::now();
    if eg.get_function("ImportedComb").is_some()
        && *inserted == (c.pool.values.len(), c.records.len())
    {
        return Ok(());
    }
    if eg.get_function("ImportedComb").is_none() {
        eg.parse_and_run_program(None, crate::layer_bridge::SCHEMA)?;
        eg.parse_and_run_program(None,"(function ImportedComb (i64) Comb :no-merge) (function ImportedInstance (i64) Instance :no-merge) (function ImportedValue (i64) Val :no-merge)")?;
    }
    update_layers(c)?;
    let mut batch = vec![];
    emit(eg, &mut batch, fact("Empty", vec![]))?;
    for (id, t) in c.pool.values.iter().enumerate().skip(inserted.0) {
        emit(
            eg,
            &mut batch,
            set(
                "ImportedValue",
                id as u64,
                call("V", vec![string(&t.sort), string(&t.label())]),
            ),
        )?;
    }
    for r in c.records.iter().skip(inserted.1) {
        let parent_expr = list(
            if r.parents.is_empty() {
                vec![call("Empty", vec![])]
            } else {
                r.parents.iter().map(|p| cref(c.records[*p].id)).collect()
            },
            "MoreParents",
            "NoParents",
        );
        let ports = r
            .ports
            .iter()
            .enumerate()
            .map(|(slot, p)| {
                let sort = &c.pool.values[r.wanted[slot]].sort;
                match p {
                    Port::Parent(k, j) => {
                        let p = call(
                            "ParentPort",
                            vec![num(*k as u64), num(*j as u64), string(sort)],
                        );
                        if c.is_coarse(r) {
                            call("Local", vec![p])
                        } else {
                            p
                        }
                    }
                    Port::External(k) => call("External", vec![num(*k as u64), string(sort)]),
                }
            })
            .collect();
        let binding = list(
            ports,
            if c.is_coarse(r) { "PCons" } else { "RCons" },
            if c.is_coarse(r) { "PNil" } else { "RNil" },
        );
        emit(
            eg,
            &mut batch,
            set(
                "ImportedComb",
                r.id,
                call(
                    if c.is_coarse(r) {
                        "CoarseComb"
                    } else {
                        "SmoothComb"
                    },
                    vec![
                        parent_expr,
                        call("Rule", vec![string(&c.rules[r.rule].rule.name)]),
                        binding,
                    ],
                ),
            ),
        )?;
        emit(
            eg,
            &mut batch,
            set(
                "ImportedInstance",
                r.id,
                call("Occurrence", vec![num(r.id), cref(r.id)]),
            ),
        )?;
        for (slot, p) in r.parents.iter().enumerate() {
            emit(
                eg,
                &mut batch,
                fact(
                    "ParentAt",
                    vec![iref(r.id), num(slot as u64), iref(c.records[*p].id)],
                ),
            )?;
        }
        for (relation, values) in [("OutputAt", &r.values), ("ExternalAt", &r.external)] {
            for (slot, v) in values.iter().enumerate() {
                emit(
                    eg,
                    &mut batch,
                    fact(relation, vec![iref(r.id), num(slot as u64), vref(*v)]),
                )?;
            }
        }
        let effect = |id| call("RowFact", vec![vref(id)]);
        for v in &r.produced {
            emit(
                eg,
                &mut batch,
                fact("Produced", vec![iref(r.id), effect(*v)]),
            )?;
        }
        for v in &r.required {
            if matches!(
                c.pool.values[*v].key,
                Key::External(..) | Key::Replay(_, true)
            ) {
                emit(
                    eg,
                    &mut batch,
                    fact("ExternalFact", vec![iref(r.id), effect(*v)]),
                )?;
            }
        }
        for (a, b) in &r.unions {
            emit(
                eg,
                &mut batch,
                fact(
                    "Produced",
                    vec![iref(r.id), call("Equal", vec![vref(*a), vref(*b)])],
                ),
            )?;
        }
        emit(
            eg,
            &mut batch,
            fact(
                "Requires",
                vec![
                    iref(r.id),
                    list(
                        r.required.iter().map(|v| effect(*v)).collect(),
                        "ECons",
                        "ENil",
                    ),
                ],
            ),
        )?;
    }
    for (index, r) in c.records.iter().enumerate().skip(inserted.1) {
        emit(
            eg,
            &mut batch,
            fact(
                "Binding",
                vec![
                    iref(r.id),
                    list(r.wanted.iter().map(|v| vref(*v)).collect(), "ACons", "ANil"),
                ],
            ),
        )?;
        emit(
            eg,
            &mut batch,
            fact("SupportsUse", vec![iref(r.id), iref(r.id)]),
        )?;
        for (slot, p) in r.parents.iter().enumerate() {
            emit(
                eg,
                &mut batch,
                fact(
                    "LinkedParent",
                    vec![iref(r.id), num(slot as u64), iref(c.records[*p].id)],
                ),
            )?;
            emit(
                eg,
                &mut batch,
                fact(
                    "ParentTemplate",
                    vec![cref(r.id), num(slot as u64), cref(c.records[*p].id)],
                ),
            )?;
        }
        // Legacy reducers inspect inherited effects. The Rust store itself keeps
        // effects only at their origin and resolves this closure on demand.
        for effect in c.layers.provides(&[index]) {
            let e = match effect {
                crate::coarse_smooth::Effect::RowFact(v) => call("RowFact", vec![vref(v)]),
                crate::coarse_smooth::Effect::Equal(a, b) => call("Equal", vec![vref(a), vref(b)]),
            };
            emit(eg, &mut batch, fact("Provides", vec![iref(r.id), e]))?;
        }
    }
    flush(eg, &mut batch)?;
    *inserted = (c.pool.values.len(), c.records.len());
    // Read handles for the data-only Tier2 projection. No Tier1 saturation.
    let mut native = BTreeMap::new();
    eg.function_for_each("Occurrence", |r| {
        native.insert(
            eg.value_to_base::<i64>(r.vals[0]) as u64,
            (r.vals[1], r.vals[2]),
        );
    })?;
    for r in &mut c.records {
        let &(comb, instance) = native.get(&r.id).ok_or("missing projected occurrence")?;
        r.comb = Some(comb);
        r.instance = Some(instance);
    }
    if let Some(path) = std::env::var_os("EGG_LAYOUT_SUPPORT_SNAPSHOT") {
        let pairs: BTreeSet<_> = c.records.iter().map(|r| (r.id, r.id)).collect();
        serde_json::to_writer(
            std::io::BufWriter::new(std::fs::File::create(path)?),
            &pairs,
        )?;
    }
    eprintln!(
        "[perf-layers] build_and_project={:.6} records={} definitions={} coarse_layers={} smooth_layers={}",
        stage_start.elapsed().as_secs_f64(),
        c.records.len(),
        c.layers.combs.len(),
        c.layers.coarse_layers.len(),
        c.layers.smooth_layers.len()
    );
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ExtensionKey {
    rule: usize,
    coarse: bool,
    inputs: Vec<String>,
    routes: Vec<String>,
}
struct Extensions {
    keys: Vec<ExtensionKey>,
    known: Vec<bool>,
}
fn role(c: &Captured, parent: usize, slot: usize) -> Option<String> {
    let p = &c.records[parent];
    let r = &c.rules[p.rule];
    let s = match &p.outputs[slot] {
        Output::Var(n) => format!("variable:{n}"),
        Output::Column(span, col) => {
            let (path, e) = r.calls.get(span)?;
            let Expr::Call(_, op, _) = e else {
                return None;
            };
            format!("{path}:{op}:{col}")
        }
        Output::Row(span) => format!("row:{}", r.calls.get(span)?.0),
    };
    Some(format!("{}:{s}", r.rule.name))
}
fn build_tier2(c: &mut Captured, eg: &mut EGraph, root: &Path) -> Result<(Extensions, Vec<Json>)> {
    update_tier2(c, eg, root, &mut Tier2State::default())
}
#[derive(Default)]
struct Tier2State {
    keys: Vec<ExtensionKey>,
    seen: BTreeMap<ExtensionKey, usize>,
    known: Vec<bool>,
    inserted: usize,
    loaded: bool,
}
fn update_tier2(
    c: &mut Captured,
    eg: &mut EGraph,
    root: &Path,
    state: &mut Tier2State,
) -> Result<(Extensions, Vec<Json>)> {
    if !state.loaded {
        load(eg, &root.join("rules/tier2.egg"), root)?;
        load(eg, &root.join("rules/higher.egg"), root)?;
        eg.parse_and_run_program(
            None,
            "(function ImportedExtension (i64) Extension :no-merge)",
        )?;
        state.loaded = true;
    }
    let Tier2State {
        keys,
        seen,
        known,
        inserted,
        ..
    } = state;
    let mut batch = vec![];
    for index in *inserted..c.records.len() {
        let r = &c.records[index];
        let mut valid = true;
        let mut routes = vec![];
        let mut exprs = vec![];
        for (slot, p) in r.ports.iter().enumerate() {
            let sort = &c.pool.values[r.wanted[slot]].sort;
            match p {
                Port::External(k) => {
                    routes.push(format!("external:{k}:{sort}"));
                    exprs.push(call("Outside", vec![num(*k as u64), string(sort)]));
                }
                Port::Parent(k, j) => {
                    let role = role(c, r.parents[*k], *j).unwrap_or_else(|| {
                        valid = false;
                        format!("unmapped:{j}")
                    });
                    routes.push(format!("parent:{k}:{role}:{sort}"));
                    exprs.push(call(
                        "ParentRole",
                        vec![num(*k as u64), string(&role), string(sort)],
                    ));
                }
            }
        }
        let inputs = r
            .inputs
            .iter()
            .map(|i| match i {
                Input::Var(n) => format!("var:{n}"),
                Input::Read(s) => c.rules[r.rule]
                    .calls
                    .get(s)
                    .map(|x| x.0.clone())
                    .unwrap_or_else(|| {
                        valid = false;
                        "unmapped".into()
                    }),
            })
            .collect();
        let key = ExtensionKey {
            rule: r.rule,
            coarse: c.is_coarse(r),
            inputs,
            routes,
        };
        let ext = if let Some(&id) = seen.get(&key) {
            id
        } else {
            let id = keys.len();
            let schema = format!(
                "{}\ncoarse={}\n{:?}",
                c.rules[r.rule].rule,
                c.is_coarse(r),
                key.inputs
            );
            emit(
                eg,
                &mut batch,
                set(
                    "ImportedExtension",
                    id as u64,
                    call(
                        "Extend",
                        vec![
                            string(&c.rules[r.rule].rule.name),
                            list(exprs, "Next", "End"),
                            call("Schema", vec![string(&schema)]),
                        ],
                    ),
                ),
            )?;
            known.push(valid);
            keys.push(key.clone());
            seen.insert(key, id);
            id
        };
        emit(
            eg,
            &mut batch,
            fact(
                "At",
                vec![num(r.id), call("ImportedExtension", vec![num(ext as u64)])],
            ),
        )?;
        for (slot, p) in r.parents.iter().enumerate() {
            emit(
                eg,
                &mut batch,
                fact(
                    "Before",
                    vec![num(c.records[*p].id), num(r.id), num(slot as u64)],
                ),
            )?;
        }
        if c.is_coarse(r) {
            emit(
                eg,
                &mut batch,
                fact(
                    "NeedsBoundary",
                    vec![call("ImportedExtension", vec![num(ext as u64)])],
                ),
            )?;
        }
        c.records[index].extension = ext;
    }
    flush(eg, &mut batch)?;
    for r in c.records.iter().skip(*inserted) {
        if c.is_coarse(r) || r.parents.len() != 1 || !known[r.extension] {
            continue;
        }
        let ports = r
            .ports
            .iter()
            .enumerate()
            .map(|(slot, p)| match p {
                Port::Parent(k, j) => call(
                    "ParentPort",
                    vec![
                        num(*k as u64),
                        num(*j as u64),
                        string(&c.pool.values[r.wanted[slot]].sort),
                    ],
                ),
                _ => unreachable!(),
            })
            .collect();
        emit(
            eg,
            &mut batch,
            fact(
                "UnaryStep",
                vec![
                    cref(c.records[r.parents[0]].id),
                    cref(r.id),
                    call("ImportedExtension", vec![num(r.extension as u64)]),
                    list(ports, "RCons", "RNil"),
                ],
            ),
        )?;
    }
    flush(eg, &mut batch)?;
    eg.parse_and_run_program(
        None,
        "(run-schedule (saturate (run tier2))) (run-schedule (saturate (run higher)))",
    )?;
    let mut native = vec![None; keys.len()];
    eg.function_for_each("ImportedExtension", |r| {
        native[eg.value_to_base::<i64>(r.vals[0]) as usize] = Some(r.vals[1])
    })?;
    let native: Vec<_> = native.into_iter().map(Option::unwrap).collect();
    let ext_ids: BTreeMap<_, _> = native.iter().enumerate().map(|(i, v)| (*v, i)).collect();
    let cid = |v| {
        eg.value_to_class_id(eg.get_sort_by_name("Comb").unwrap(), v)
            .to_string()
    };
    let mut depths = BTreeMap::new();
    eg.function_for_each("Depth", |r| {
        depths.insert(r.vals[1], eg.value_to_base::<i64>(r.vals[0]));
    })?;
    let mut h = BTreeMap::new();
    eg.function_for_each("FractalComb",|r|{h.insert(r.vals[4],json!({"count":depths[&r.vals[0]],"operator":format!("ext_{:04}",ext_ids[&r.vals[1]]),"ctx":cid(r.vals[2]),"represents":[]}));})?;
    eg.function_for_each("Represents", |r| {
        h.get_mut(&r.vals[0]).unwrap()["represents"]
            .as_array_mut()
            .unwrap()
            .push(json!(cid(r.vals[1])))
    })?;
    *inserted = c.records.len();
    Ok((
        Extensions {
            keys: keys.clone(),
            known: known.clone(),
        },
        h.into_values().collect(),
    ))
}

fn pretty(e: &Expr) -> String {
    match e {
        Expr::Var(_, n) => n.clone(),
        Expr::Lit(_, v) => v.to_string(),
        Expr::Call(_, op, args) => {
            let a: Vec<_> = args.iter().map(pretty).collect();
            if a.len() == 2 {
                if let Some(symbol) = match op.as_str() {
                    "Add" => Some("+"),
                    "Sub" => Some("−"),
                    "Mul" => Some("·"),
                    "Div" => Some("/"),
                    "Pow" => Some("^"),
                    _ => None,
                } {
                    return format!("({} {symbol} {})", a[0], a[1]);
                }
            }
            if op == "Integral" && a.len() == 2 {
                return format!("∫({}) d{}", a[0], a[1]);
            }
            if op == "Diff" && a.len() == 2 {
                return format!("D({}, {})", a[1], a[0]);
            }
            format!("{op}({})", a.join(", "))
        }
    }
}
/// Egglog syntax for an expression the lane changed, so the fractal preview can
/// unroll a lane by re-parsing and substituting it (the display form `pretty`
/// prints is not parseable: infix `·`, prefix `+(n, 1)`).
fn egglog(e: &Expr) -> String {
    match e {
        Expr::Var(_, n) => n.clone(),
        Expr::Lit(_, v) => v.to_string(),
        Expr::Call(_, op, args) => {
            let a: Vec<_> = args.iter().map(egglog).collect();
            if a.is_empty() {
                format!("({op})")
            } else {
                format!("({op} {})", a.join(" "))
            }
        }
    }
}
fn update_text(c: &Captured, index: usize) -> Vec<String> {
    let r = &c.records[index];
    r.inputs
        .iter()
        .zip(&r.ports)
        .filter_map(|(input, port)| {
            let Input::Var(n) = input else {
                return None;
            };
            let value = match port {
                Port::External(_) => "外部接口".into(),
                Port::Parent(p, j) => {
                    let parent = &c.records[r.parents[*p]];
                    match &parent.outputs[*j] {
                        Output::Var(n) => n.clone(),
                        Output::Column(span, col) => match c.rules[parent.rule].calls.get(span) {
                            Some((_, Expr::Call(_, op, args))) => {
                                if *col < args.len() {
                                    egglog(&args[*col])
                                } else {
                                    format!("({op} …)")
                                }
                            }
                            _ => "未映射输出".into(),
                        },
                        Output::Row(_) => "行见证".into(),
                    }
                }
            };
            Some(format!("{n} ← {value}"))
        })
        .collect()
}
fn view(c: &Captured, eg: &EGraph, extensions: &Extensions, higher: &[Json]) -> Result<Json> {
    let cid = |v| {
        eg.value_to_class_id(eg.get_sort_by_name("Comb").unwrap(), v)
            .to_string()
    };
    let eligible =
        |r: &Record| !c.is_coarse(r) && r.parents.len() == 1 && extensions.known[r.extension];
    let mut children: Vec<Vec<usize>> = vec![vec![]; c.records.len()];
    let mut starts = vec![];
    for (i, r) in c.records.iter().enumerate().filter(|(_, r)| eligible(r)) {
        let p = r.parents[0];
        if eligible(&c.records[p]) && c.records[p].extension == r.extension {
            children[p].push(i);
        } else {
            starts.push(i);
        }
    }
    let mut paths = vec![];
    let mut todo: Vec<_> = starts.into_iter().map(|i| vec![i]).collect();
    while let Some(path) = todo.pop() {
        let last = *path.last().unwrap();
        if children[last].is_empty() {
            if path.len() >= 2 {
                paths.push(path);
            }
            continue;
        }
        for child in &children[last] {
            let mut p = path.clone();
            p.push(*child);
            todo.push(p);
        }
    }
    paths.sort_by_key(|p| (std::cmp::Reverse(p.len()), c.records[p[0]].id));
    let mut needed = BTreeSet::new();
    let mut lanes = vec![];
    for (n, path) in paths.iter().enumerate() {
        let first = &c.records[path[0]];
        let trigger = first.parents[0];
        let op = format!("ext_{:04}", first.extension);
        let context = cid(c.records[trigger].comb.unwrap());
        let target = cid(c.records[*path.last().unwrap()].comb.unwrap());
        if !higher.iter().any(|h| {
            h["count"].as_u64() == Some(path.len() as u64)
                && h["operator"] == op
                && h["ctx"] == context
                && h["represents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|x| x == &target)
        }) {
            return Err("view path is not a native FractalComb".into());
        }
        needed.insert(trigger);
        needed.extend(path);
        lanes.push(json!({"id":format!("chain_{}",n+1),"rule":c.rules[first.rule].rule.name,"operator":op,"trigger":c.records[trigger].id,"events":path.iter().map(|i|c.records[*i].id).collect::<Vec<_>>(),"higher":format!("FractalComb(Depth({}), {}, {}, initial_binding)",path.len(),op,context),"update":update_text(c,path[0])}));
    }
    let mut nodes = serde_json::Map::new();
    for index in needed {
        let r = &c.records[index];
        let rule = &c.rules[r.rule].rule;
        let equation = match (rule.body.first(), rule.head.0.first()) {
            (Some(Fact::Eq(_, _, lhs)), Some(Action::Union(_, _, rhs))) => {
                format!("{} ⇒ {}", pretty(lhs), pretty(rhs))
            }
            _ => rule.to_string(),
        };
        let lowered = crate::native_lower::lower(&c.records, &c.rules, index).and_then(|lowered| {
            let mut check = EGraph::default();
            check
                .parse_and_run_program(None, &format!("{}\n{}", c.datatype, lowered.code))
                .map_err(|e| e.to_string())?;
            Ok(lowered)
        });
        nodes.insert(r.id.to_string(),json!({"event":r.id,"comb":format!("${}",cid(r.comb.unwrap())),"rule":rule.name,"kind":if c.is_coarse(r){"CoarseComb"}else{"SmoothComb"},"source":rule.to_string(),"equation":equation,"combined":lowered.as_ref().ok().map(|x|&x.code),"source_steps":lowered.as_ref().ok().map(|x|x.steps.iter().map(|i|c.rules[c.records[*i].rule].rule.name.clone()).collect::<Vec<_>>()),"reason":lowered.as_ref().err(),"tier1":format!("{}(parents={:?}, rule={}, ports={:?})",if c.is_coarse(r){"CoarseComb"}else{"SmoothComb"},r.parents.iter().map(|p|c.records[*p].id).collect::<Vec<_>>(),rule.name,r.ports),"parent_events":r.parents.iter().map(|p|c.records[*p].id).collect::<Vec<_>>(),"external_routes":r.ports.iter().filter_map(|p|if let Port::External(k)=p{Some(*k)}else{None}).collect::<Vec<_>>(),"source_routes":extensions.keys[r.extension].routes}));
    }
    let visible: BTreeSet<_> = nodes
        .values()
        .map(|n| n["comb"].as_str().unwrap().to_owned())
        .collect();
    let mut original: BTreeSet<_> = c.records.iter().filter_map(|r| r.comb).collect();
    original.extend(eg.lookup_function("Empty", &[]));
    let total = original.len();
    let with_views = eg.get_size("Empty")
        + eg.get_size("SmoothComb")
        + eg.get_size("CoarseComb")
        + eg.get_size("FractalComb");
    Ok(
        json!({"capture":{"rounds":c.rounds},"stats":{"higher_rules":higher.len(),"maximal_chains":lanes.len(),"applications":paths.iter().map(Vec::len).sum::<usize>(),"visible_unique_contexts":visible.len(),"total_comb_templates":total,"comb_templates_with_views":with_views},"lanes":lanes,"nodes":nodes,"scope":"Single-process native analysis; only witnessed finite paths are displayed. Raw events are drained after completed rounds; producer/equality indexes and analysis graphs remain resident."}),
    )
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn render(root: &Path, out: &Path, data: &Json, native: bool) -> Result {
    let mut template =
        std::fs::read_to_string(root.join("experiments/tier2/fractal_view_template.html"))?;
    if native {
        template = template
            .replace("../tier1_extract/index.html#", "index.html#")
            .replace("../tier1_extract/index.html", "index.html");
    }
    if out.join("rounds/manifest.json").exists() {
        let absolute = out.canonicalize()?;
        let link=absolute.strip_prefix(root).ok().filter(|p|p.starts_with("out")).map(|p| {
            let query=p.to_string_lossy().bytes().map(|b|if b.is_ascii_alphanumeric()||b"/-_.".contains(&b){(b as char).to_string()}else{format!("%{b:02X}")}).collect::<String>();
            format!("<p><a href=\"http://127.0.0.1:8080/?layer_run={query}\">在 tools 调试网页打开逐轮 Layer / Fractal / DOT</a></p></header>")
        }).unwrap_or_else(||"<p>逐轮数据位于 rounds/；请在 tools 调试网页载入分析目录。</p></header>".into());
        template = template.replace("</header>", &link);
    }
    let payload = serde_json::to_string(data)?.replace('<', "\\u003c");
    std::fs::write(
        out.join("fractal.html"),
        template.replace("__DATA__", &payload),
    )?;
    if native {
        let mut html="<!doctype html><meta charset=\"utf-8\"><title>Native combined rules</title><style>body{font:16px system-ui;max-width:1100px;margin:32px auto}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:#eef3ed;padding:16px}details{padding:12px}</style><h1>单进程分析 · 已显示组合</h1><p><a href=\"fractal.html\">返回 fractal 视图</a></p>".to_string();
        for n in data["nodes"].as_object().unwrap().values() {
            let name = n["comb"].as_str().unwrap();
            html += &format!(
                "<details id=\"{}\"><summary>{}</summary><pre>{}</pre><pre>{}</pre></details>",
                escape(name.trim_start_matches('$')),
                escape(name),
                escape(n["source"].as_str().unwrap()),
                escape(
                    n["combined"]
                        .as_str()
                        .or(n["reason"].as_str())
                        .unwrap_or("no new effect")
                )
            );
        }
        html += "<script>function focus(){let x=document.getElementById(location.hash.slice(1));if(x){x.open=true;x.scrollIntoView()}}onhashchange=focus;focus()</script>";
        std::fs::write(out.join("index.html"), html)?;
    }
    Ok(())
}
/// All three egraphs/analyses execute in this process. Only final results are serialized.
pub fn run(root: &Path, source: &Path, rounds: Option<usize>, out: &Path) -> Result<Json> {
    run_with_options(root, source, rounds, out, false, false, None)
}
pub fn run_with_options(
    root: &Path,
    source: &Path,
    rounds: Option<usize>,
    out: &Path,
    online: bool,
    save_history: bool,
    replay: Option<&Path>,
) -> Result<Json> {
    let source = if replay.is_some() {
        source.to_path_buf()
    } else {
        source.canonicalize()?
    };
    if out.exists() {
        return Err(format!("output already exists: {}", out.display()).into());
    }
    std::fs::create_dir_all(out)?;
    let started = Instant::now();
    let mut status = json!({"status":"running","phase":if replay.is_some() {"history"} else {"tier0"},"build_mode":if online {"online"} else {"offline"},"history_input":replay,"save_history":save_history,"mode":"native-single-process","pid":std::process::id(),"source":source,"requested_rounds":rounds,"subprocesses":0});
    let marker = out.join("run.json");
    std::fs::write(&marker, serde_json::to_string_pretty(&status)?)?;
    let mut result = (|| -> Result<Json> {
        let worker = rayon::ThreadPoolBuilder::new().num_threads(1).build()?;
        let mut tier1 = EGraph::default();
        let mut layer_exporter = layer_view::Exporter::with_closed(out)?;
        let mut c = if let Some(path) = replay {
            history::read(path)?
        } else if online {
            capture_with_sink(
                &source,
                rounds,
                Some((&mut tier1, root, &worker)),
                Some(&mut |c| layer_exporter.capture(c, out)),
            )?
        } else {
            capture(&source, rounds, None)?
        };
        if save_history {
            history::save(&out.join("history.json"), &source, &c)?;
        }
        let mut times = BTreeMap::new();
        times.insert(
            if online {
                "tier0_capture_and_online_tier1"
            } else {
                "tier0_and_typed_capture"
            },
            c.trace_seconds,
        );
        eprintln!(
            "[native] {} events, {} eligible applications",
            c.events,
            c.records.len()
        );
        // Isolate metadata work in one Rayon worker; tier-0 kept its normal pool.
        let base_status = status.clone();
        let phase_marker = marker.clone();
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build()?;
        pool.install(move || -> std::result::Result<Json,String> {
            (|| -> Result<Json> {
        let phase=|name:&str|->Result {let mut s=base_status.clone();s["phase"]=json!(name);std::fs::write(&phase_marker,serde_json::to_vec(&s)?)?;eprintln!("[native] {name}");Ok(())};
        phase("tier1")?;
        let mut eg=tier1;let stage=Instant::now();if !online || replay.is_some() { build_tier1(&mut c,&mut eg,root,&mut (0,0))?; }times.insert("tier1",stage.elapsed().as_secs_f64());
        if !online || replay.is_some() {layer_exporter.replay(&c,out)?;}
        phase("tier2")?;
        let stage=Instant::now();let(ext,higher)=build_tier2(&mut c,&mut eg,root)?;times.insert("tier2",stage.elapsed().as_secs_f64());
        phase("fractal-views")?;
        let stage=Instant::now();let fractal_views=fractal::build(&c,&mut eg,&ext,root)?;times.insert("fractal_views",stage.elapsed().as_secs_f64());
        let recursive_start=Instant::now();let recursive=if std::env::var_os("EGG_LAYOUT_DISCOVER_RECURSION").is_some() || std::env::var_os("EGG_LAYOUT_TEMPLATE_CATALOG").is_some(){recursive::build(&c,&ext,&mut eg,root,&fractal_views)?}else{Json::Null};times.insert("recursive_patterns",recursive_start.elapsed().as_secs_f64());
        let growth_start=Instant::now();let growth=if std::env::var_os("EGG_LAYOUT_DISCOVER_GROWTH").is_some(){growth::analyze(&c,&fractal_views)}else{Json::Null};times.insert("dependency_growth",growth_start.elapsed().as_secs_f64());
        phase("view")?;
        let stage=Instant::now();let mut data=view(&c,&eg,&ext,&higher)?;
        for row in fractal_views["views"].as_array().unwrap() { let key=row["event"].to_string(); if let Some(node)=data["nodes"].get_mut(&key) { node["fractal_view"]=row.clone(); } }
        for row in fractal_views["fact_history"].as_array().unwrap() { let key=row["endpoint_event"].to_string(); if let Some(node)=data["nodes"].get_mut(&key) { node["fractal_facts"]=row.clone(); } }times.insert("selected_lowering_and_view",stage.elapsed().as_secs_f64());
        let summary=json!({"events":c.events,"imported_events":c.records.len(),"excluded_events":c.rejected,"extensions":ext.keys.len(),"higher_rules":higher.len(),"executed_rounds":c.rounds,"timings_seconds":times,"wall_seconds":started.elapsed().as_secs_f64(),"subprocesses":0,"intermediate_trace_files":0,"raw_trace_peak_batch_events":c.trace_peak,"raw_trace_batches":c.trace_batches,"raw_trace_remaining_events":0,"build_mode":if online {"online"} else {"offline"},"history_saved":save_history,"history_replayed":replay.is_some(),"layer_visualization":"tools/egglog_debugger (load analysis directory)","round_manifest":"rounds/manifest.json","tier1_backend":"rust-coarse-smooth-layers","layer_definitions":c.layers.combs.len(),"coarse_layers":c.layers.coarse_layers.len(),"smooth_layers":c.layers.smooth_layers.len(),"scope":"Rust coarse/smooth layers with a data-only egglog projection for existing Tier2 consumers; tier-0 executes only during recapture. Online mode imports at simple run-round boundaries; complex schedules are imported at completion. Raw trace events are drained at completed execution boundaries; compact producer/equality indexes and analysis graphs remain resident. History contains resolved eligible applications, not rejected/raw trace events. Reduce rules are loaded; arbitrary accumulator summaries are not inferred."});
        let reuse_ablation=if std::env::var_os("EGG_LAYOUT_REUSE_ABLATION").is_some(){json!({"scope":"End-to-end online admission ablation; the restricted arm admits only the backward layer slice stopping at coarse introductions. Learning and selection both use this gate.","interface_only":c.layers.reuse.report()["stats"],"layer_restricted":crate::comb_reuse::ReuseStore::replay(&c.layers,true).report()["stats"]})}else{Json::Null};
        let report=json!({"reuse_ablation":reuse_ablation,"layer_patterns":layer_exporter.summary(),"layers":c.layers.report(),"status":"complete","summary":summary,"higher_rules":higher,"fractal_views":fractal_views,"dependency_growth":growth,"recursive_patterns":recursive,"view":data});
        render(root,out,&report["view"],true)?;
        Ok(report)
            })().map_err(|e|e.to_string())
        }).map_err(|e|e.into())
    })();
    if let Ok(r) = &mut result {
        std::fs::write(
            out.join("layers.json"),
            serde_json::to_vec_pretty(&r["layers"])?,
        )?;
        r["summary"]["wall_seconds"] = json!(started.elapsed().as_secs_f64());
        std::fs::write(out.join("analysis.json"), serde_json::to_vec(r)?)?;
        if !r["recursive_patterns"].is_null() {
            std::fs::write(
                out.join("recursive_patterns.json"),
                serde_json::to_vec_pretty(&r["recursive_patterns"])?,
            )?;
            let template =
                std::fs::read_to_string(root.join("experiments/recursive_patterns/viewer.html"))?;
            let data = serde_json::to_string(&r["recursive_patterns"])?.replace('<', r"\u003c");
            std::fs::write(
                out.join("recursive_patterns.html"),
                template.replace("__RECURSIVE_DATA__", &data),
            )?;
        }
        if !r["dependency_growth"].is_null() {
            std::fs::write(
                out.join("dependency_growth.json"),
                serde_json::to_vec_pretty(&r["dependency_growth"])?,
            )?;
            let template =
                std::fs::read_to_string(root.join("experiments/dependency_growth/viewer.html"))?;
            let data = serde_json::to_string(&r["dependency_growth"])?.replace('<', "\\u003c");
            std::fs::write(
                out.join("dependency_growth.html"),
                template.replace("__GROWTH_DATA__", &data),
            )?;
        }
    }
    if let Ok(bytes) = std::fs::read(&marker) {
        if let Ok(s) = serde_json::from_slice::<Json>(&bytes) {
            status["phase"] = s["phase"].clone();
        }
    }
    match &result {
        Ok(r) => {
            status["phase"] = json!("complete");
            status["status"] = json!("complete");
            status["summary"] = r["summary"].clone();
            status["executed_rounds"] = r["summary"]["executed_rounds"].clone();
            status["match_events"] = r["summary"]["events"].clone();
            status["viewer"] = json!("fractal.html");
        }
        Err(e) => {
            status["status"] = json!("failed");
            status["error"] = json!(e.to_string());
        }
    }
    let saved = std::fs::write(marker, serde_json::to_string_pretty(&status)?);
    if result.is_ok() {
        saved?;
    } else if let Err(e) = saved {
        eprintln!("could not save failure status: {e}");
    }
    result
}
/// Re-render a completed final result, without secretly rerunning Python stages.
pub fn reuse(root: &Path, out: Option<&Path>) -> Result {
    if let Some(out) = out {
        let native = out.join("analysis.json");
        if native.exists() {
            let report: Json = serde_json::from_slice(&std::fs::read(native)?)?;
            if report["status"] != "complete" {
                return Err("not a complete native analysis".into());
            }
            render(root, out, &report["view"], true)?;
            println!("{}", out.join("fractal.html").display());
        } else {
            let state: Json = serde_json::from_slice(&std::fs::read(out.join("run.json"))?)?;
            if state["status"] != "complete" {
                return Err("not a completed legacy result".into());
            }
            let dir = out.join("experiments/tier2");
            let data: Json =
                serde_json::from_slice(&std::fs::read(dir.join("fractal_view.json"))?)?;
            render(root, &dir, &data, false)?;
            println!("{}", dir.join("fractal.html").display());
        }
    } else {
        let dir = root.join("experiments/tier2");
        let data: Json = serde_json::from_slice(&std::fs::read(dir.join("fractal_view.json"))?)?;
        render(root, &dir, &data, false)?;
        println!("{}", dir.join("fractal.html").display());
    }
    println!("Reused completed view; no inference or capture was rerun.");
    Ok(())
}
