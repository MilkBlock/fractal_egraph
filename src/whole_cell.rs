//! Executable WHOLE RipSatCell transitions, not query-fragment certificates.
//! Native saturation is confined to Bake. Hot execution runs a compiled delta
//! through the native action-only sink and obtains its complete slot frame by
//! a typed receipt. Original smooth queries remain installed but are not run.
pub mod automatic;
pub mod catalog;

use crate::{
    pipeline::Result,
    reuse_graph::{ReuseGraph, RipSatEgraph, RipSatId},
    saturated_rule_composition::{
        self as graph, Comparison, Row, SaturatedRuleComposition as State, Vertex,
    },
};
use egglog::{
    ArcSort, Core, EGraph, Primitive, Value, WritePrim, WriteState,
    ast::{Action, Command, Expr, Fact, Span},
    cell_assist::CellActionSink,
    constraint::{SimpleTypeConstraint, TypeConstraint},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub sort: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    pub parent: Option<String>,
    pub parameters: Vec<Parameter>,
    /// A native constructor expression over previous parameters, new parameters
    /// and `parent` (the source Cell's root). This supplies the coarse context.
    pub graft: String,
    /// Complete positive seed forest, including equality preconditions.
    #[serde(default)]
    pub premises: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub ruleset: String,
    pub stages: Vec<Stage>,
}

#[derive(Clone, Serialize)]
struct Step {
    name: String,
    source: Option<usize>,
    target: usize,
    parameters: Vec<Parameter>,
    input_types: Vec<Vertex>,
    input_partition: Vec<usize>,
    input_to_target: Vec<usize>,
    inherited_rows: usize,
    emitted_rows: usize,
    union_actions: usize,
    actions: Vec<String>,
    graft: String,
    premises: Vec<String>,
    bake: Json,
}
/// Constructed only by native Bake plus independent delta replay verification.
/// Saved JSON is an inspection artifact, not an unchecked executable loader.
#[derive(Clone, Serialize)]
pub struct Library {
    certificates: crate::certificate::Book,
    environment: String,
    ruleset: String,
    constructors: Vec<String>,
    inert: BTreeSet<String>,
    smooth_rules: Vec<String>,
    states: Vec<RipSatEgraph>,
    steps: Vec<Step>,
}
struct Snapshot {
    graph: State,
    raw: Vec<Value>,
}
struct Baked {
    eg: EGraph,
    frame: Vec<Value>,
    state: usize,
    ports: BTreeMap<String, Value>,
}
fn symbol(s: &str) -> Result<()> {
    if s.is_empty()
        || s.starts_with("wc_")
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        || s.starts_with(|c: char| c.is_ascii_digit())
    {
        return Err(format!("invalid/reserved whole-Cell name {s}").into());
    }
    Ok(())
}
fn positive(e: &Expr, ctors: &BTreeSet<String>) -> bool {
    match e {
        Expr::Var(..) | Expr::Lit(..) => true,
        Expr::Call(_, op, args) => ctors.contains(op) && args.iter().all(|a| positive(a, ctors)),
    }
}
fn calls(e: &Expr, out: &mut BTreeSet<String>) {
    if let Expr::Call(_, op, args) = e {
        out.insert(op.clone());
        for a in args {
            calls(a, out);
        }
    }
}
fn environment(
    src: &str,
    ruleset: &str,
) -> Result<(EGraph, Vec<String>, BTreeSet<String>, Vec<String>)> {
    let mut eg = EGraph::default();
    let cmds = eg.parse_program(None, src)?;
    if let Some(command) = cmds.iter().find(|c| {
        !matches!(
            c,
            Command::Datatype { .. }
                | Command::Constructor { .. }
                | Command::AddRuleset(..)
                | Command::Sort {
                    presort_and_args: None,
                    ..
                }
                | Command::Rule { .. }
                | Command::Rewrite(..)
                | Command::BiRewrite(..)
        )
    }) {
        return Err(format!(
            "whole-Cell proof does not cover this declaration/effect: {}",
            command.to_string().chars().take(240).collect::<String>()
        )
        .into());
    }
    eg.run_program(cmds.clone())?;
    let constructors = eg
        .get_function_names()
        .into_iter()
        .filter(|n| eg.get_function(n).unwrap().is_constructor())
        .collect::<BTreeSet<_>>();
    if constructors.iter().any(|n| n.starts_with("wc_")) {
        return Err("reserved whole-Cell namespace".into());
    }
    let mut read = BTreeSet::new();
    for c in cmds {
        match c {
            Command::Datatype { .. }
            | Command::Constructor { .. }
            | Command::AddRuleset(..)
            | Command::Sort {
                presort_and_args: None,
                ..
            } => (),
            Command::Rewrite(set, r, subsume) => {
                if set != ruleset
                    || subsume
                    || !r.conditions.is_empty()
                    || !positive(&r.lhs, &constructors)
                    || !positive(&r.rhs, &constructors)
                {
                    return Err("whole-Cell rewrite requires a positive, non-subsuming rule in the selected ruleset".into());
                }
                let mut lhs = BTreeSet::new();
                calls(&r.lhs, &mut lhs);
                if lhs.is_empty() {
                    return Err("rewrite needs a constructor premise".into());
                }
                read.extend(lhs);
            }
            Command::BiRewrite(set, r) => {
                if set != ruleset
                    || !r.conditions.is_empty()
                    || !positive(&r.lhs, &constructors)
                    || !positive(&r.rhs, &constructors)
                {
                    return Err("unsupported bidirectional rewrite".into());
                }
                let (mut lhs, mut rhs) = (BTreeSet::new(), BTreeSet::new());
                calls(&r.lhs, &mut lhs);
                calls(&r.rhs, &mut rhs);
                if lhs.is_empty() || rhs.is_empty() {
                    return Err("bidirectional rewrite needs constructor premises".into());
                }
                read.extend(lhs);
                read.extend(rhs);
            }
            Command::Rule { rule } => {
                if rule.ruleset != ruleset || rule.body.is_empty() {
                    return Err(
                        "whole-Cell Bake requires one positive ruleset and nonempty premises"
                            .into(),
                    );
                }
                let mut body_calls = BTreeSet::new();
                for f in &rule.body {
                    match f {
                        Fact::Eq(_, a, b)
                            if positive(a, &constructors) && positive(b, &constructors) =>
                        {
                            calls(a, &mut body_calls);
                            calls(b, &mut body_calls);
                        }
                        Fact::Fact(e) if positive(e, &constructors) => calls(e, &mut body_calls),
                        _ => return Err(
                            "numeric/negative guards require a separate whole-Cell transfer proof"
                                .into(),
                        ),
                    }
                }
                if body_calls.is_empty() {
                    return Err("zero-table premise is not a local Cell rule".into());
                }
                read.extend(body_calls);
                for a in &rule.head.0 {
                    if !match a {
                        Action::Expr(_, e) | Action::Let(_, _, e) => positive(e, &constructors),
                        Action::Union(_, a, b) => {
                            positive(a, &constructors) && positive(b, &constructors)
                        }
                        _ => false,
                    } {
                        return Err("set/merge/destructive/primitive effects lack a whole-Cell transfer proof".into());
                    }
                }
            }
            _ => {
                return Err(
                    "whole-Cell environment must contain declarations and positive rules only"
                        .into(),
                );
            }
        }
    }
    let rules = eg.cell_rule_names(ruleset)?;
    if rules.is_empty() {
        return Err("empty smooth ruleset".into());
    }
    let inert = constructors.difference(&read).cloned().collect();
    Ok((eg, constructors.into_iter().collect(), inert, rules))
}
fn literal(eg: &EGraph, s: &ArcSort, v: Value) -> Result<Option<String>> {
    if s.is_eq_sort() {
        return Ok(None);
    }
    Ok(Some(match s.name() {
        "i64" => eg.value_to_base::<i64>(v).to_string(),
        "bool" => eg.value_to_base::<bool>(v).to_string(),
        "String" => serde_json::to_string(&eg.value_to_base::<egglog::sort::S>(v).to_string())?,
        "Unit" => "()".into(),
        n => return Err(format!("unsupported literal/container sort {n}").into()),
    }))
}
fn snapshot(
    eg: &EGraph,
    names: &[String],
    scope: Json,
    ports: &BTreeMap<String, (String, Value)>,
) -> Result<Snapshot> {
    let (mut values, mut raw, mut ids) = (vec![], vec![], BTreeMap::new());
    let mut slot = |sort: &ArcSort, v: Value| -> Result<usize> {
        let v = eg.get_canonical_value(v, sort);
        let key = (sort.name().to_owned(), v);
        if let Some(i) = ids.get(&key) {
            return Ok(*i);
        }
        let i = values.len();
        values.push(Vertex {
            sort: sort.name().into(),
            literal: literal(eg, sort, v)?,
        });
        raw.push(v);
        ids.insert(key, i);
        Ok(i)
    };
    let mut named = BTreeMap::new();
    for (n, (s, v)) in ports {
        named.insert(
            n.clone(),
            slot(eg.get_sort_by_name(s).ok_or("unknown port sort")?, *v)?,
        );
    }
    let mut rows = BTreeSet::new();
    for name in names {
        let f = eg.get_function(name).ok_or("missing constructor")?;
        let mut stored = vec![];
        eg.function_for_each(name, |r| stored.push((r.vals.to_vec(), r.subsumed)))?;
        for (vs, hidden) in stored {
            if hidden {
                return Err("whole-Cell closure cannot contain subsumed rows".into());
            }
            let c = vs
                .iter()
                .zip(f.schema().input.iter().chain([&f.schema().output]))
                .map(|(v, s)| slot(s, *v))
                .collect::<Result<Vec<_>>>()?;
            rows.insert(Row {
                op: name.clone(),
                args: c[..c.len() - 1].to_vec(),
                result: *c.last().unwrap(),
            });
        }
    }
    Ok(Snapshot {
        graph: State {
            version: 2,
            local_ids: vec![],
            scope,
            values,
            rows: rows.into_iter().collect(),
            subsumed_rows: vec![],
            ports: named,
        },
        raw,
    })
}
fn canonical_frame(eg: &EGraph, types: &[Vertex], values: &[Value]) -> Result<Vec<Value>> {
    if types.len() != values.len() {
        return Err("whole-Cell frame width".into());
    }
    types
        .iter()
        .zip(values)
        .map(|(t, v)| {
            Ok(eg.get_canonical_value(
                *v,
                eg.get_sort_by_name(&t.sort).ok_or("missing frame sort")?,
            ))
        })
        .collect()
}
fn partition(types: &[Vertex], vs: &[Value]) -> Vec<usize> {
    let mut first = BTreeMap::new();
    types
        .iter()
        .zip(vs)
        .enumerate()
        .map(|(i, (t, v))| *first.entry((t.sort.as_str(), *v)).or_insert(i))
        .collect()
}
fn saturate(eg: &mut EGraph, ruleset: &str) -> Result<Json> {
    let now = Instant::now();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut steps = 0;
    let mut updated = false;
    loop {
        let r = eg.step_rules(ruleset)?;
        updated |= r.updated;
        steps += 1;
        for (n, c) in r.num_matches_per_rule {
            *counts.entry(n.to_string()).or_default() += c;
        }
        if !r.updated {
            break;
        }
    }
    Ok(
        json!({"native_complete":true,"updated":updated,"can_stop":!updated,"last_step_updated":false,"steps":steps,"smooth_matches":counts,"seconds":now.elapsed().as_secs_f64()}),
    )
}
fn scope(env: &str, ruleset: &str) -> Json {
    json!({"semantics":"positive-constructor-equality/v1","environment":env,"ruleset":ruleset,"boundary":"typed opaque inputs; complete saturation only"})
}

fn recipe(
    target: &State,
    source: Option<&State>,
    map: &[usize],
) -> Result<(Vec<String>, usize, usize, usize)> {
    let mut actions = vec![];
    let mut known = BTreeSet::new();
    let mut unions = 0;
    for (i, to) in map.iter().enumerate() {
        if known.insert(*to) {
            actions.push(format!("(let v{to} in{i})"));
        } else {
            actions.push(format!("(union v{to} in{i})"));
            unions += 1;
        }
    }
    for (i, v) in target.values.iter().enumerate() {
        if let Some(l) = &v.literal {
            if known.insert(i) {
                actions.push(format!("(let v{i} {l})"));
            }
        }
    }
    let retained = source
        .map(|s| {
            s.rows
                .iter()
                .map(|r| Row {
                    op: r.op.clone(),
                    args: r.args.iter().map(|i| map[*i]).collect(),
                    result: map[r.result],
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let target_rows = target.rows.iter().cloned().collect::<BTreeSet<_>>();
    if !retained.is_subset(&target_rows) {
        return Err("parent body is not wholly retained in target".into());
    }
    let inherited = retained.len();
    let mut pending = target_rows
        .difference(&retained)
        .cloned()
        .collect::<Vec<_>>();
    let created = pending.len();
    let mut tmp = 0;
    while !pending.is_empty() {
        let before = pending.len();
        let mut next = vec![];
        for r in pending {
            if !r.args.iter().all(|i| known.contains(i)) {
                next.push(r);
                continue;
            }
            let expr = format!(
                "({} {})",
                r.op,
                r.args
                    .iter()
                    .map(|i| format!("v{i}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            if known.insert(r.result) {
                actions.push(format!("(let v{} {expr})", r.result));
            } else {
                actions.push(format!("(let made{tmp} {expr})"));
                actions.push(format!("(union v{} made{tmp})", r.result));
                tmp += 1;
                unions += 1;
            }
        }
        if next.len() == before {
            return Err("ungrounded target SCC: cannot compile a constructor/union delta".into());
        }
        pending = next;
    }
    if known.len() != target.values.len() {
        return Err("unresolved whole-Cell output slots".into());
    }
    Ok((actions, inherited, created, unions))
}

#[derive(Clone)]
struct Receipt {
    name: String,
    types: Vec<ArcSort>,
    rows: Arc<Mutex<Vec<Vec<Value>>>>,
}
impl Primitive for Receipt {
    fn name(&self) -> &str {
        &self.name
    }
    fn get_type_constraints(&self, s: &Span) -> Box<dyn TypeConstraint> {
        SimpleTypeConstraint::new(&self.name, self.types.clone(), s.clone()).into_box()
    }
}
impl WritePrim for Receipt {
    fn apply<'a, 'db>(&self, state: WriteState<'a, 'db>, args: &[Value]) -> Option<Value> {
        self.rows.lock().unwrap().push(args.to_vec());
        Some(state.base_values().get(()))
    }
}
struct Emitter {
    sink: CellActionSink,
    receipt: Arc<Mutex<Vec<Vec<Value>>>>,
    inputs: usize,
    outputs: usize,
    code: String,
}
impl Emitter {
    fn new(eg: &mut EGraph, step: &Step, target: &State, id: usize) -> Result<Self> {
        let name = format!("wc_receipt_{id}");
        let rows = Arc::new(Mutex::new(vec![]));
        let types = target
            .values
            .iter()
            .chain(&step.input_types)
            .map(|t| {
                eg.get_sort_by_name(&t.sort)
                    .cloned()
                    .ok_or("missing emitter type")
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut signature = types;
        signature.push(eg.get_sort_by_name("Unit").cloned().ok_or("missing Unit")?);
        eg.add_write_primitive(
            Receipt {
                name: name.clone(),
                types: signature,
                rows: rows.clone(),
            },
            None,
        );
        let input_names = (0..step.input_types.len())
            .map(|i| format!("in{i}"))
            .collect::<Vec<_>>();
        let output_names = (0..target.values.len())
            .map(|i| format!("v{i}"))
            .chain(input_names.clone())
            .collect::<Vec<_>>();
        let code = format!(
            "(ruleset wc_emit_{id})\n(function wc_gate_{id} ({}) i64 :merge old)\n(rule ((= 0 (wc_gate_{id} {})))\n  ({}\n   ({name} {})) :ruleset wc_emit_{id} :name \"wc_delta_{id}\")",
            step.input_types
                .iter()
                .map(|t| t.sort.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            input_names.join(" "),
            step.actions.join("\n   "),
            output_names.join(" ")
        );
        eg.parse_and_run_program(None, &code)?;
        let sink = eg.prepare_cell_action(&format!("wc_emit_{id}"), &format!("wc_delta_{id}"))?;
        Ok(Self {
            sink,
            receipt: rows,
            inputs: input_names.len(),
            outputs: target.values.len(),
            code,
        })
    }
    fn apply(&self, eg: &mut EGraph, args: &[Value]) -> Result<(Vec<Value>, Vec<Value>, Json)> {
        let (mut frames, report) = self.apply_many(eg, &[args.to_vec()])?;
        let (out, old) = frames.pop().ok_or("missing native receipt")?;
        Ok((out, old, report))
    }
    fn apply_many(
        &self,
        eg: &mut EGraph,
        args: &[Vec<Value>],
    ) -> Result<(Vec<(Vec<Value>, Vec<Value>)>, Json)> {
        if args.iter().any(|a| a.len() != self.inputs) {
            return Err("transition input count".into());
        }
        let bindings = args
            .iter()
            .map(|args| {
                self.sink
                    .slots()
                    .iter()
                    .map(|(n, _)| {
                        n.strip_prefix("in")
                            .and_then(|x| x.parse::<usize>().ok())
                            .and_then(|i| args.get(i))
                            .copied()
                            .ok_or("unexpected macro query slot")
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.receipt.lock().unwrap().clear();
        let report = eg.apply_cell_bindings(&self.sink, &bindings)?;
        let mut rows = self.receipt.lock().unwrap();
        if rows.len() != args.len() || rows.iter().any(|r| r.len() != self.outputs + self.inputs) {
            return Err("missing or duplicate native commit receipt".into());
        }
        let receipts = rows.len();
        let frames = std::mem::take(&mut *rows)
            .into_iter()
            .map(|row| (row[..self.outputs].to_vec(), row[self.outputs..].to_vec()))
            .collect();
        Ok((
            frames,
            json!({"macro_action_matches":report.num_matches_per_rule,"updated":report.updated,"frame_receipts":receipts,"returned_target_slots":self.outputs}),
        ))
    }
}

impl Library {
    /// Check the current executable-certificate contract without saturation,
    /// candidate discovery or constructing an unchecked library.
    pub fn validate_environment(src: &str, ruleset: &str) -> Result<()> {
        environment(src, ruleset).map(|_| ())
    }
    pub fn bake(src: &str, plan: &Plan) -> Result<Self> {
        let (base, names, inert, rules) = environment(src, &plan.ruleset)?;
        let mut lib = Self {
            certificates: Default::default(),
            environment: src.into(),
            ruleset: plan.ruleset.clone(),
            constructors: names,
            inert,
            smooth_rules: rules,
            states: vec![],
            steps: vec![],
        };
        let mut registry = ReuseGraph::default();
        let mut baked = BTreeMap::<String, Baked>::new();
        for spec in &plan.stages {
            symbol(&spec.name)?;
            if baked.contains_key(&spec.name) {
                return Err("duplicate stage name".into());
            }
            let parent = spec
                .parent
                .as_ref()
                .map(|p| baked.get(p).ok_or("parent must be an earlier baked stage"))
                .transpose()?;
            let mut eg = parent.map(|p| p.eg.clone()).unwrap_or_else(|| base.clone());
            let mut ports = parent.map(|p| p.ports.clone()).unwrap_or_default();
            let mut input_types = parent
                .map(|p| lib.states[p.state].graph().values.clone())
                .unwrap_or_default();
            let mut args = parent.map(|p| p.frame.clone()).unwrap_or_default();
            for p in &spec.parameters {
                symbol(&p.name)?;
                if p.name == "parent" || ports.contains_key(&p.name) {
                    return Err("reserved or already introduced parameter".into());
                }
                if !eg.cell_sort_is_equality(&p.sort)? {
                    return Err(
                        "free ports must be equality-sort values; primitive literals are retained"
                            .into(),
                    );
                }
                let marker = format!("wc_input_{}", p.sort);
                if eg.get_function(&marker).is_none() {
                    eg.parse_and_run_program(
                        None,
                        &format!("(constructor {marker} (i64) {})", p.sort),
                    )?;
                }
                eg.parse_and_run_program(
                    None,
                    &format!("(let wc_param_{} ({marker} {}))", p.name, ports.len()),
                )?;
                let ex = egglog::ast::Parser::default()
                    .get_expr_from_string(None, &format!("wc_param_{}", p.name))?;
                let v = eg.eval_expr(&ex)?.1;
                args.push(v);
                input_types.push(Vertex {
                    sort: p.sort.clone(),
                    literal: None,
                });
                ports.insert(p.name.clone(), v);
            }
            let before = eg.clone();
            let args = canonical_frame(&eg, &input_types, &args)?;
            let aliases = partition(&input_types, &args);
            let mut ex = egglog::ast::Parser::default().get_expr_from_string(None, &spec.graft)?;
            if !positive(&ex, &lib.constructors.iter().cloned().collect()) {
                return Err("graft must be a positive constructor expression".into());
            }
            ex = ex.visit_exprs(&mut |e| {
                if let Expr::Var(s, n) = &e {
                    if n == "parent" {
                        if let Some(p) = &spec.parent {
                            return Expr::Var(s.clone(), format!("wc_root_{p}"));
                        }
                    }
                    if ports.contains_key(n) {
                        return Expr::Var(s.clone(), format!("wc_param_{n}"));
                    }
                    e
                } else {
                    e
                }
            });
            let seed_commands=eg.parse_program(None,&spec.premises.join("\n"))?;
            for command in seed_commands {
                let Command::Action(mut action)=command else{return Err("seed premises must be constructor/equality facts".into())};
                let bind=|e:Expr|e.visit_exprs(&mut |e|match e {
                    Expr::Var(s,n) if n=="parent"=>spec.parent.as_ref().map(|p|Expr::Var(s.clone(),format!("wc_root_{p}"))).unwrap_or(Expr::Var(s,n)),
                    Expr::Var(s,n) if ports.contains_key(&n)=>Expr::Var(s,format!("wc_param_{n}")), e=>e});
                match &mut action {
                    Action::Expr(_,e)=>{if !positive(e,&lib.constructors.iter().cloned().collect()){return Err("non-positive seed premise".into())}*e=bind(e.clone());},
                    Action::Union(_,a,b)=>{if !positive(a,&lib.constructors.iter().cloned().collect())||!positive(b,&lib.constructors.iter().cloned().collect()){return Err("non-positive seed equality".into())}*a=bind(a.clone());*b=bind(b.clone());},
                    _=>return Err("seed premise has an unsupported effect".into()),
                }
                eg.run_program(vec![Command::Action(action)])?;
            }
            eg.parse_and_run_program(None, &format!("(let wc_root_{} {ex})", spec.name))?;
            let done = saturate(&mut eg, &plan.ruleset)?;
            let root = eg.eval_expr(
                &egglog::ast::Parser::default()
                    .get_expr_from_string(None, &format!("wc_root_{}", spec.name))?,
            )?;
            let mut boundary = BTreeMap::new();
            for (n, v) in &ports {
                let e = egglog::ast::Parser::default()
                    .get_expr_from_string(None, &format!("wc_param_{n}"))?;
                let (s, _) = eg.eval_expr(&e)?;
                boundary.insert(format!("input:{n}"), (s.name().into(), *v));
            }
            boundary.insert("root".into(), (root.0.name().into(), root.1));
            let snap = snapshot(&eg, &lib.constructors, scope(src, &plan.ruleset), &boundary)?;
            let complete = json!({"ripen":{"state":"Saturated","updated":false},"checks":"passed","native_stop_reason":null,"saturated_rule_composition":{"status":"exported","values":snap.graph.values.len(),"facts":snap.graph.rows.len()}});
            let body = RipSatEgraph::from_native_export(snap.graph.clone(), &complete)?;
            let intern = registry.intern_body(body)?;
            let target = intern.id.0;
            if intern.inserted {
                lib.states
                    .push(registry.body(RipSatId(target)).unwrap().clone());
            }
            let mut frame = vec![None; snap.raw.len()];
            for (i, v) in snap.raw.iter().enumerate() {
                frame[intern.value_map[i]] = Some(*v);
            }
            let frame = frame
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or("incomplete canonical frame")?;
            let desc = lib.states[target].graph();
            let input_to_target = args
                .iter()
                .zip(&input_types)
                .map(|(v, t)| {
                    let canon = eg.get_canonical_value(*v, &eg.get_sort_by_name(&t.sort).unwrap());
                    frame
                        .iter()
                        .enumerate()
                        .find(|(i, x)| desc.values[*i].sort == t.sort && **x == canon)
                        .map(|(i, _)| i)
                        .ok_or("inherited value missing from target closure")
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let source = parent.map(|p| p.state);
            let (actions, inherited_rows, emitted_rows, union_actions) = recipe(
                desc,
                source.map(|i| lib.states[i].graph()),
                &input_to_target,
            )?;
            let mut step = Step {
                name: spec.name.clone(),
                source,
                target,
                parameters: spec.parameters.clone(),
                input_types,
                input_partition: aliases,
                input_to_target,
                inherited_rows,
                emitted_rows,
                union_actions,
                actions,
                graft: spec.graft.clone(),
                premises: spec.premises.clone(),
                bake: done,
            };
            // Verify the compiled delta on a fresh parent snapshot, never on the
            // already saturated target. All original smooth rules stay installed.
            let mut replay = before;
            let emit = Emitter::new(&mut replay, &step, desc, lib.steps.len())?;
            let (out, old, _) = emit.apply(&mut replay, &args)?;
            let out = validate_receipt(&replay, &step, desc, &out, &old)?;
            let named = desc
                .ports
                .iter()
                .map(|(n, i)| (n.clone(), (desc.values[*i].sort.clone(), out[*i])))
                .collect();
            let actual = snapshot(
                &replay,
                &lib.constructors,
                scope(src, &plan.ruleset),
                &named,
            )?;
            if !matches!(
                graph::compare(desc, &actual.graph)?,
                Comparison::Equivalent { .. }
            ) {
                return Err("compiled whole-Cell delta disagrees with native saturation".into());
            }
            step.bake["delta_replay_verified"] = json!(true);
            if target == lib.certificates.documents.len() {
                lib.certificates.checked(crate::certificate::Document::finite(
                    &plan.ruleset,json!({"state":target,"scope":desc.scope,"ports":desc.ports}),json!({"native_saturated":true,"delta_replay_verified":true,"entries":[]})));
            }
            lib.certificates.documents[target].evidence["entries"].as_array_mut().unwrap().push(
                json!({"stage":step.name,"graft":step.graft,"input_partition":step.input_partition,"bake":step.bake}));
            lib.certificates.marker(target,format!("state:{target}"),
                desc.ports.iter().map(|(n,i)|(n.clone(),desc.values[*i].sort.clone())).collect(),
                vec!["typed complete receipt".into(),"input alias partition".into(),"positive schema/ruleset scope".into(),"visible ownership coverage".into()])?;
            lib.steps.push(step);
            baked.insert(
                spec.name.clone(),
                Baked {
                    eg,
                    frame,
                    state: target,
                    ports,
                },
            );
        }
        Ok(lib)
    }
    /// Export native-certified complete bodies for the existing positive query
    /// projection reader. This does not add an unchecked macro-program loader.
    pub fn export_projection_catalog(&self, directory: &std::path::Path) -> Result<()> {
        if directory.exists() {
            return Err("Cell projection catalog directory already exists".into());
        }
        let catalog = directory.join("catalog");
        std::fs::create_dir_all(catalog.join("states"))?;
        for (id, state) in self.states.iter().enumerate() {
            crate::pipeline::write_json(
                &catalog.join(format!("states/state-{id:04}.json")),
                state.graph(),
            )?;
        }
        crate::pipeline::write_json(
            &catalog.join("catalog.json"),
            &json!({
                "schema":"saturated-rule-composition-catalog/v1",
                "saturated_rule_compositions":self.states.len(),
                "scope":"Native whole-Cell saturation plus independent delta replay. Exported only for positive witnessed query projection; not an unchecked executable macro library.",
                "triggers":self.steps.iter().map(|s|json!({
                    "saturated_rule_composition":s.target,
                    "binding_origin":{"kind":"native-whole-cell-bake","stage":s.name,"graft":s.graft,"parameters":s.parameters,"bake":s.bake}
                })).collect::<Vec<_>>()
            }),
        )?;
        crate::pipeline::write_json(&directory.join("bake.json"), self)?;
        crate::pipeline::write_json(&directory.join("certificates.json"), &self.certificates.report())?;
        Ok(())
    }
    pub fn describe(&self) -> Json {
        serde_json::to_value(self).unwrap()
    }
    pub fn body(&self, id: usize) -> &State {
        self.states[id].graph()
    }
}

fn validate_receipt(
    eg: &EGraph,
    step: &Step,
    target: &State,
    out: &[Value],
    old: &[Value],
) -> Result<Vec<Value>> {
    let out = canonical_frame(eg, &target.values, out)?;
    let old = canonical_frame(eg, &step.input_types, old)?;
    if partition(&target.values, &out) != (0..out.len()).collect::<Vec<_>>() {
        return Err("unbaked target alias specialization; no target Cell published".into());
    }
    if old
        .iter()
        .zip(&step.input_to_target)
        .any(|(v, i)| *v != out[*i])
    {
        return Err("native commit did not establish the inherited equality map".into());
    }
    for (t, v) in target.values.iter().zip(&out) {
        if t.literal.is_some()
            && literal(eg, eg.get_sort_by_name(&t.sort).unwrap(), *v)? != t.literal
        {
            return Err("native receipt changed a retained literal".into());
        }
    }
    Ok(out)
}

#[derive(Clone, Serialize)]
pub struct Instance {
    pub state: usize,
    pub revision: usize,
    #[serde(skip)]
    frame: Vec<Value>,
}
impl Instance {
    pub fn frame(&self) -> &[Value] {
        &self.frame
    }
}
pub struct Runtime {
    eg: EGraph,
    lib: Library,
    emitters: Vec<Emitter>,
    original: Vec<(CellActionSink, u32)>,
    current: Option<Instance>,
    failed: bool,
    pub events: Vec<Json>,
}
impl Runtime {
    pub fn new(mut lib: Library, inputs: &str) -> Result<Self> {
        let (mut eg, _, _, _) = environment(&lib.environment, &lib.ruleset)?;
        let cmds = eg.parse_program(None, inputs)?;
        for c in &cmds {
            if !match c {
                Command::Action(Action::Let(_, n, e)) => {
                    !n.starts_with("wc_") && positive(e, &lib.inert)
                }
                Command::Action(Action::Expr(_, e)) => positive(e, &lib.inert),
                _ => false,
            } {
                return Err("runtime input forest must use inert constructors; active context needs another certified graft".into());
            }
        }
        eg.run_program(cmds)?;
        let mut emitters = vec![];
        for (i, s) in lib.steps.iter().enumerate() {
            emitters.push(Emitter::new(&mut eg, s, lib.states[s.target].graph(), i)?);
        }
        let mut original = vec![];
        for name in &lib.smooth_rules {
            let sink = eg.prepare_cell_action(&lib.ruleset, name)?;
            let watermark = eg.cell_query_watermark(&sink);
            original.push((sink, watermark));
        }
        lib.certificates.bind_environment(&eg);
        Ok(Self {
            eg,
            lib,
            emitters,
            original,
            current: None,
            failed: false,
            events: vec![],
        })
    }
    pub fn engine(&self) -> &EGraph {
        &self.eg
    }
    pub fn current(&self) -> Option<&Instance> {
        self.current.as_ref()
    }
    pub fn root(&self) -> Option<Value> {
        let i = self.current.as_ref()?;
        Some(i.frame[self.lib.states[i.state].graph().ports["root"]])
    }
    pub fn compiled_egg(&self) -> String {
        self.emitters
            .iter()
            .map(|e| e.code.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    pub fn advance(&mut self, name: &str, bindings: &BTreeMap<String, String>) -> Result<Json> {
        if !self.lib.certificates.current(&self.eg) {return Err("whole-Cell certificate environment changed".into());}
        if self.failed {
            return Err("failed whole-Cell runtime cannot resume".into());
        }
        let id = self
            .lib
            .steps
            .iter()
            .position(|s| s.name == name)
            .ok_or("unknown whole-Cell transition")?;
        let step = self.lib.steps[id].clone();
        if step.source != self.current.as_ref().map(|s| s.state) {
            return Err("wrong source RipSatCell state".into());
        }
        if bindings.keys().cloned().collect::<BTreeSet<_>>()
            != step.parameters.iter().map(|p| p.name.clone()).collect()
        {
            return Err("external parameter names do not match transition".into());
        }
        let mut args = self
            .current
            .as_ref()
            .map(|s| s.frame.clone())
            .unwrap_or_default();
        for p in &step.parameters {
            let e =
                egglog::ast::Parser::default().get_expr_from_string(None, &bindings[&p.name])?;
            if !matches!(e, Expr::Var(..)) {
                return Err("bind predeclared input names; do not construct untracked context during a transition".into());
            }
            let (s, v) = self.eg.eval_expr(&e)?;
            if s.name() != p.sort {
                return Err("external parameter sort mismatch".into());
            }
            args.push(v);
        }
        let args = canonical_frame(&self.eg, &step.input_types, &args)?;
        if partition(&step.input_types, &args) != step.input_partition {
            return Err(
                "unbaked input alias specialization; no transition effects submitted".into(),
            );
        }
        let now = Instant::now();
        let attempt = (|| {
            let (out, old, report) = self.emitters[id].apply(&mut self.eg, &args)?;
            if self
                .original
                .iter()
                .any(|(s, w)| self.eg.cell_query_watermark(s) != *w)
            {
                return Err("an original smooth query ran during macro execution".into());
            }
            let frame = validate_receipt(
                &self.eg,
                &step,
                self.lib.states[step.target].graph(),
                &out,
                &old,
            )?;
            let revision = self.current.as_ref().map_or(0, |x| x.revision + 1);
            self.current = Some(Instance {
                state: step.target,
                revision,
                frame,
            });
            Ok(
                json!({"stage":name,"source":step.source,"target":step.target,"revision":revision,"inherited_rows":step.inherited_rows,"emitted_rows":step.emitted_rows,"union_actions":step.union_actions,"native":report,"seconds":now.elapsed().as_secs_f64(),"frame_source":"typed native commit receipt","smooth_match_counts":self.smooth_counts(),"smooth_query_watermarks":self.original.iter().map(|(s,w)|json!({"rule":s.name,"before":w,"after":self.eg.cell_query_watermark(s)})).collect::<Vec<_>>()}),
            )
        })();
        match attempt {
            Ok(e) => {
                self.events.push(e.clone());
                Ok(e)
            }
            Err(e) => {
                self.failed = true;
                self.current = None;
                Err(e)
            }
        }
    }
    pub fn smooth_counts(&self) -> BTreeMap<String, usize> {
        self.lib
            .smooth_rules
            .iter()
            .map(|n| {
                (
                    n.clone(),
                    self.events
                        .iter()
                        .map(|e| {
                            e["native"]["macro_action_matches"][n].as_u64().unwrap_or(0) as usize
                        })
                        .sum(),
                )
            })
            .collect()
    }
    pub fn full_snapshot(&self) -> Result<State> {
        let i = self.current.as_ref().ok_or("no current Cell")?;
        let d = self.lib.states[i.state].graph();
        Ok(snapshot(
            &self.eg,
            &self.lib.constructors,
            scope(&self.lib.environment, &self.lib.ruleset),
            &BTreeMap::from([(
                "root".into(),
                (d.values[d.ports["root"]].sort.clone(), self.root().unwrap()),
            )]),
        )?
        .graph)
    }
    pub fn verify_against(&self, native: &EGraph, root: Value) -> Result<Comparison> {
        let actual = self.full_snapshot()?;
        let sort = actual.values[actual.ports["root"]].sort.clone();
        let expected = snapshot(
            native,
            &self.lib.constructors,
            actual.scope.clone(),
            &BTreeMap::from([("root".into(), (sort, root))]),
        )?
        .graph;
        graph::compare(&expected, &actual)
    }
    /// Audit only: clone, run the original smooth matcher, then compare ALL
    /// original constructor rows and equality classes. Never used to advance.
    pub fn audit_saturation(&self) -> Result<Json> {
        let before = self.full_snapshot()?;
        let mut eg = self.eg.clone();
        let report = saturate(&mut eg, &self.lib.ruleset)?;
        let root = self.root().unwrap();
        let sort = before.values[before.ports["root"]].sort.clone();
        let after = snapshot(
            &eg,
            &self.lib.constructors,
            before.scope.clone(),
            &BTreeMap::from([("root".into(), (sort, root))]),
        )?
        .graph;
        let eq = graph::compare(&before, &after)?;
        if !matches!(eq, Comparison::Equivalent { .. }) {
            return Err("macro target was not saturated under original rules".into());
        }
        Ok(json!({"audit_original_matcher":report,"graph_unchanged":true}))
    }
}

/// End-to-end acceptance runner. Bake, the native reference and post-run audits
/// use separate engines; none of their matches are attributed to hot execution.
pub fn run_checked(source: &str, plan: &Plan, inputs: &str, out: &std::path::Path) -> Result<Json> {
    if out.exists() {
        return Err("whole-Cell output directory already exists".into());
    }
    std::fs::create_dir_all(out)?;
    let start = Instant::now();
    let lib = Library::bake(source, plan)?;
    let bake_seconds = start.elapsed().as_secs_f64();
    crate::pipeline::write_json(&out.join("library.json"), &lib)?;
    let mut run = Runtime::new(lib.clone(), inputs)?;
    std::fs::write(
        out.join("compiled-deltas.egg"),
        format!(
            "; wc_receipt_* are typed Rust write primitives returning the whole target frame.\n; The wc_gate_* LHS is a dispatch signature; the action-only API bypasses it.\n{source}\n{}",
            run.compiled_egg()
        ),
    )?;
    let (mut native, _, _, _) = environment(source, &plan.ruleset)?;
    native.parse_and_run_program(None, inputs)?;
    let mut original = format!("{source}\n{inputs}\n");
    let mut expressions = BTreeMap::<String, Expr>::new();
    let mut stages = vec![];
    for spec in &plan.stages {
        let ex = egglog::ast::Parser::default().get_expr_from_string(None, &spec.graft)?;
        let ex = ex.visit_exprs(&mut |e| {
            if matches!(&e,Expr::Var(_,n) if n=="parent") {
                spec.parent
                    .as_ref()
                    .and_then(|p| expressions.get(p))
                    .cloned()
                    .unwrap_or(e)
            } else {
                e
            }
        });
        expressions.insert(spec.name.clone(), ex.clone());
        for command in native.parse_program(None,&spec.premises.join("\n"))? {
            let Command::Action(mut action)=command else{return Err("invalid reference seed premise".into())};
            let replace=|e:Expr|e.visit_exprs(&mut |e|if matches!(&e,Expr::Var(_,n) if n=="parent") {
                spec.parent.as_ref().and_then(|p|expressions.get(p)).cloned().unwrap_or(e)
            }else{e});
            match &mut action {Action::Expr(_,e)=>*e=replace(e.clone()),Action::Union(_,a,b)=>{*a=replace(a.clone());*b=replace(b.clone());},_=>return Err("invalid reference premise action".into())}
            let c=Command::Action(action);original+=&format!("{c}\n");native.run_program(vec![c])?;
        }
        let graft = format!("(let wc_reference_{} {ex})", spec.name);
        original += &format!("{graft}\n(run-schedule (saturate {}))\n", plan.ruleset);
        native.parse_and_run_program(None, &graft)?;
        let reference = saturate(&mut native, &plan.ruleset)?;
        let runtime = run.advance(
            &spec.name,
            &spec
                .parameters
                .iter()
                .map(|p| (p.name.clone(), p.name.clone()))
                .collect(),
        )?;
        let root = native.eval_expr(
            &egglog::ast::Parser::default()
                .get_expr_from_string(None, &format!("wc_reference_{}", spec.name))?,
        )?;
        let expected = snapshot(
            &native,
            &lib.constructors,
            scope(source, &plan.ruleset),
            &BTreeMap::from([("root".into(), (root.0.name().into(), root.1))]),
        )?
        .graph;
        let actual = run.full_snapshot()?;
        crate::pipeline::write_json(&out.join(format!("{}-native.json", spec.name)), &expected)?;
        crate::pipeline::write_json(&out.join(format!("{}-cell.json", spec.name)), &actual)?;
        let comparison = graph::compare(&expected, &actual)?;
        if !matches!(comparison, Comparison::Equivalent { .. }) {
            return Err(format!(
                "{}: full facts/equality mismatch: {comparison:?}",
                spec.name
            )
            .into());
        }
        let audit = run.audit_saturation()?;
        let extract = |eg: &EGraph| -> Result<Json> {
            let outputs = eg
                .clone()
                .parse_and_run_program(None, &format!("(extract {ex})"))?;
            for o in outputs {
                if let egglog::CommandOutput::ExtractBest(dag, cost, term) = o {
                    return Ok(json!({"cost":cost,"term":dag.to_string(term)}));
                }
            }
            Err("missing native extraction result".into())
        };
        let native_extraction = extract(&native)?;
        let cell_extraction = extract(run.engine())?;
        if native_extraction["cost"] != cell_extraction["cost"] {
            return Err("extraction cost differs".into());
        }
        let state = run.current().unwrap().state;
        stages.push(json!({"stage":spec.name,"cell_state":state,"cell_rows":lib.body(state).rows.len(),"whole_graph_enodes":actual.rows.len(),"whole_graph_eclasses":actual.values.iter().filter(|v|v.literal.is_none()).count(),"native":reference,"runtime":runtime,"exact_full_graph":comparison,"saturation_audit":audit,"native_extraction":native_extraction,"cell_extraction":cell_extraction}));
    }
    std::fs::write(out.join("original.egg"), original)?;
    let report = json!({"schema":"whole-ripsat-execution/v1","completed":true,"bake_seconds":bake_seconds,"source_environment":source,"plan":plan,"runtime_input":inputs,"stages":stages,
        "scope":"Actual native constructor/union effects from whole-closure deltas. No original smooth queries in hot execution. Full original graphs compared after each saturation boundary. CPU/native backend, not a GPU or general eggcc speedup claim.",
        "boundary":"one positive constructor/union ruleset; inert input forest; exact observed alias partition; complete saturation only; no arbitrary finite-run substitution"});
    crate::pipeline::write_json(&out.join("report.json"), &report)?;
    Ok(report)
}
