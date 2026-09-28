//! Executable RipSat macros for positive constructor rewrites. Native matching
//! supplies bindings; finite symbolic closures replace (not accompany) source
//! RHSs. Boundary/union matches keep using native semi-naive queries.
use crate::{pipeline::Result, reuse_graph::ripsat_runtime::saturate};
use egglog::{
    EGraph, Value as Native,
    ast::{Command, Expr, GenericSchedule},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

#[path = "reuse_evolution.rs"]
pub mod evolution;

#[derive(Clone, Serialize, Deserialize)]
pub struct RipSatCell {
    pub rule: usize,
    pub actions: String,
    pub rows: usize,
    pub classes: usize,
    pub proof_matches: usize,
    pub slots: Vec<(String, String)>,
    pub template: Vec<(String, Vec<usize>)>,
}
/// Compatibility name for existing v2 Bake files and downstream code.
pub type Macro = RipSatCell;
#[path = "reuse_cell_transitions.rs"]
pub mod transitions;
#[path = "reuse_cell_machine.rs"]
mod cell_machine;

#[derive(Default, Serialize, Deserialize)]
pub struct Bake {
    pub schema: String,
    pub environment: Vec<String>,
    pub macros: BTreeMap<usize, Macro>,
    pub unfinished: BTreeMap<usize, String>,
}
struct Rule {
    lhs: Expr,
    rhs: Expr,
}
fn expr(e: &Expr, vars: &BTreeMap<String, String>) -> String {
    match e {
        Expr::Var(_, s) => vars.get(s).cloned().unwrap_or_else(|| s.clone()),
        Expr::Lit(_, l) => l.to_string(),
        Expr::Call(_, op, a) => format!(
            "({op}{})",
            a.iter()
                .map(|e| format!(" {}", expr(e, vars)))
                .collect::<String>()
        ),
    }
}
fn eval(eg: &mut EGraph, s: &str) -> Result<Native> {
    let e = egglog::ast::Parser::default().get_expr_from_string(None, s)?;
    Ok(eg.eval_expr(&e)?.1)
}
fn vars(eg: &EGraph, e: &Expr, ty: &str, out: &mut BTreeMap<String, String>) -> Result<()> {
    match e {
        Expr::Var(_, v) => {
            if ty != "Math" {
                return Err(
                    "primitive-valued variable requires a conditional closure; native fallback"
                        .into(),
                );
            }
            out.insert(v.clone(), ty.into());
        }
        Expr::Lit(..) => {}
        Expr::Call(_, op, a) => {
            let f = eg.get_function(op).ok_or(
                "primitive in expression is not supported by the positive constructor runtime",
            )?;
            if f.schema().output.name() != ty || f.schema().input.len() != a.len() {
                return Err("schema mismatch".into());
            }
            for (a, t) in a.iter().zip(&f.schema().input) {
                vars(eg, a, t.name(), out)?;
            }
        }
    }
    Ok(())
}
fn learn(environment: &[String], rule: &Rule, id: usize) -> Result<Macro> {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, &environment.join("\n"))?;
    eg.parse_and_run_program(None, "(constructor AdaptiveInput (i64) Math)")?;
    let mut names = BTreeMap::new();
    vars(&eg, &rule.lhs, "Math", &mut names)?;
    let substitutions: BTreeMap<_, _> = names
        .keys()
        .enumerate()
        .map(|(i, n)| (n.clone(), format!("(AdaptiveInput {i})")))
        .collect();
    let mut anchors = vec![];
    for (n, t) in &substitutions {
        anchors.push((eval(&mut eg, t)?, n.clone()));
    }
    let root = eval(&mut eg, &expr(&rule.lhs, &substitutions))?;
    anchors.push((root, "adaptive_root".into()));
    let proof_matches = saturate(&mut eg)?;
    eg.parse_and_run_program(
        None,
        &format!(
            "(check (= {} {}))",
            expr(&rule.lhs, &substitutions),
            expr(&rule.rhs, &substitutions)
        ),
    )?;
    let math = eg
        .get_function("AdaptiveInput")
        .unwrap()
        .schema()
        .output
        .clone();
    let mut known: BTreeMap<Native, String> = BTreeMap::new();
    let mut actions = vec![];
    for (v, n) in anchors {
        let v = eg.get_canonical_value(v, &math);
        if let Some(old) = known.get(&v) {
            if old != &n {
                actions.push(format!("(union {n} {old})"));
            }
        } else {
            known.insert(v, n);
        }
    }
    let mut rows = vec![];
    let mut classes = BTreeSet::new();
    let mut tables = eg.get_function_names();
    tables.sort();
    for op in tables {
        if op == "AdaptiveInput" {
            continue;
        }
        let f = eg.get_function(&op).unwrap();
        if f.schema().output.name() != "Math" {
            return Err("non-constructor output in closure".into());
        }
        eg.function_for_each(&op, |r| {
            rows.push((op.clone(), r.vals.to_vec(), r.subsumed))
        })?;
    }
    let count = rows.len();
    let body_rows = rows.clone();
    let mut pending = rows;
    while !pending.is_empty() {
        let before = pending.len();
        let mut later = vec![];
        for (op, row, hidden) in pending {
            if hidden {
                return Err("subsumed closure cannot be installed as positive macro".into());
            }
            let f = eg.get_function(&op).unwrap();
            let mut args = vec![];
            let mut ready = true;
            for (v, t) in row.iter().zip(&f.schema().input) {
                let s = match t.name() {
                    "Math" => known.get(&eg.get_canonical_value(*v, t)).cloned(),
                    "i64" => Some(eg.value_to_base::<i64>(*v).to_string()),
                    "String" => Some(serde_json::to_string(
                        &eg.value_to_base::<egglog::sort::S>(*v).to_string(),
                    )?),
                    _ => return Err("unsupported closure literal sort".into()),
                };
                if let Some(s) = s {
                    args.push(s);
                } else {
                    ready = false;
                    break;
                }
            }
            if !ready {
                later.push((op, row, hidden));
                continue;
            }
            let value = eg.get_canonical_value(*row.last().unwrap(), &math);
            classes.insert(value);
            let term = format!(
                "({op}{})",
                args.iter().map(|s| format!(" {s}")).collect::<String>()
            );
            if let Some(n) = known.get(&value) {
                actions.push(format!("(union {n} {term})"));
            } else {
                let n = format!("adaptive_local_{}", known.len());
                actions.push(format!("(let {n} {term})"));
                known.insert(value, n);
            }
        }
        if later.len() == before {
            return Err("unconstructible closure cycle".into());
        }
        pending = later;
    }
    let mut slot_ids = BTreeMap::new();
    let mut slots = vec![];
    let mut template = vec![];
    for (op, row, _) in body_rows {
        let f = eg.get_function(&op).unwrap();
        let mut columns = vec![];
        for (v, ty) in row
            .iter()
            .zip(f.schema().input.iter().chain([&f.schema().output]))
        {
            let v = eg.get_canonical_value(*v, ty);
            let key = (ty.name().to_string(), v);
            let next = slots.len();
            let slot = if let Some(id) = slot_ids.get(&key) {
                *id
            } else {
                let term = match ty.name() {
                    "Math" => known.get(&v).ok_or("unbound body value")?.clone(),
                    "i64" => eg.value_to_base::<i64>(v).to_string(),
                    "String" => {
                        serde_json::to_string(&eg.value_to_base::<egglog::sort::S>(v).to_string())?
                    }
                    _ => return Err("unsupported binding sort".into()),
                };
                slots.push((term, ty.name().to_string()));
                slot_ids.insert(key, next);
                next
            };
            columns.push(slot);
        }
        template.push((op, columns));
    }
    Ok(Macro {
        rule: id,
        actions: actions.join("\n"),
        rows: count,
        classes: known.len(),
        proof_matches,
        slots,
        template,
    })
}
fn rule_program(id:usize,r:&Rule,m:Option<&RipSatCell>,plans:&[transitions::Compiled],machine:Option<&cell_machine::Machine>)->(String,String) {
    let set=format!("adaptive_{}_{}",if m.is_some(){"baked"}else{"source"},id);
    let mut declarations=String::new();
    let mut actions=m.map(|m|m.actions.clone()).unwrap_or_else(||format!("(union adaptive_root {})",r.rhs));
    if let Some(m)=m {
        let relation=format!("adaptive_instance_{id}");
        declarations=format!("(relation {relation} ({}))\n",m.slots.iter().map(|s|s.1.as_str()).collect::<Vec<_>>().join(" "));
        actions+=&format!("\n({relation} {})",m.slots.iter().map(|s|s.0.as_str()).collect::<Vec<_>>().join(" "));
    }
    for p in plans.iter().filter(|p|p.spec.source_rule==id) {actions+=&format!("\n{}",p.projections);}
    if let Some(machine)=machine {if let Some(registrations)=machine.actions.get(&id) {actions+=&format!("\n{registrations}");}}
    let lhs=plans.iter().find(|p|p.spec.consumer_rule==id).map(|p|p.residual.clone()).unwrap_or_else(||format!("(= adaptive_root {})",r.lhs));
    let program=format!("{declarations}(ruleset {set}) (rule ({lhs}) ({actions}) :ruleset {set} :name \"{set}\")\n");
    (set,program)
}
fn install(eg:&mut EGraph,id:usize,r:&Rule,m:Option<&RipSatCell>,plans:&[transitions::Compiled],machine:Option<&cell_machine::Machine>)->Result<String> {
    let (set,program)=rule_program(id,r,m,plans,machine);eg.parse_and_run_program(None,&program)?;Ok(set)
}
// Instance rows are emitted by macro actions. No saturated-body query/rematching:
// each row is the binding matrix required by native storage validation.
fn pack_instances(
    eg: &mut EGraph,
    bake: &Bake,
    min_reuse: usize,
    audit: &mut Option<super::enclosure_audit::Audit>,
) -> Result<Value> {
    if !std::env::var("EGG_LAYOUT_BATCH_CELLS").is_ok_and(|v|v=="0") {
        return pack_cell_wave(eg,bake,min_reuse,audit);
    }
    let mut instances = 0;
    let mut templates = vec![];
    for (id, m) in &bake.macros {
        let name = format!("adaptive_instance_{id}");
        if eg.get_function(&name).is_none() {
            continue;
        }
        let bindings = eg.drain_notification_relation(&name)?;
        if let Some(a) = audit.as_mut() {
            a.observe(eg, m, &bindings)?;
        }
        instances += bindings.len();
        if bindings.len() < min_reuse {
            templates.push(json!({"rule":id,"instances":bindings.len(),"packed":false,"reason":"insufficient distinct instances in this batch"}));
            continue;
        }
        let before = eg
            .virtual_fact_stats()
            .iter()
            .map(|(_, s)| s.virtual_rows)
            .sum::<usize>();
        let template = eg.virtual_fact_template(
            m.template
                .iter()
                .map(|(function, columns)| egglog::VirtualFactRow {
                    function: function.clone(),
                    columns: columns.clone(),
                })
                .collect(),
        )?;
        let packed = eg.virtualize_existing_facts(&template, &bindings)?;
        let after = eg
            .virtual_fact_stats()
            .iter()
            .map(|(_, s)| s.virtual_rows)
            .sum::<usize>();
        templates.push(json!({"rule":id,"instances":bindings.len(),"packed":true,"distinct_native_rows":packed.distinct_native_rows,"new_virtual_rows_net":after.saturating_sub(before)}));
    }
    Ok(
        json!({"min_reuse":min_reuse,"templates":templates,"processed_instance_packets":instances,"virtual_rows":eg.virtual_fact_stats().iter().map(|(_,s)|s.virtual_rows).sum::<usize>()}),
    )
}
fn pack_cell_wave(eg:&mut EGraph,bake:&Bake,min_reuse:usize,audit:&mut Option<super::enclosure_audit::Audit>)->Result<Value> {
    let t=Instant::now();
    let names=bake.macros.keys().map(|id|format!("adaptive_instance_{id}")).filter(|n|eg.get_function(n).is_some()).collect::<Vec<_>>();
    let mut notifications=eg.drain_notification_relations(&names)?;
    let mut batches=vec![];let mut templates=vec![];let mut instances=0;
    for (id,m) in &bake.macros {
        let name=format!("adaptive_instance_{id}");if eg.get_function(&name).is_none(){continue;}
        let bindings=notifications.remove(&name).unwrap_or_default();
        if let Some(a)=audit.as_mut(){a.observe(eg,m,&bindings)?;}
        instances+=bindings.len();
        let admitted=bindings.len()>=min_reuse;
        templates.push(json!({"rule":id,"instances":bindings.len(),"packed":admitted}));
        if admitted {
            let template=eg.virtual_fact_template(m.template.iter().map(|(f,c)|egglog::VirtualFactRow{function:f.clone(),columns:c.clone()}).collect())?;
            batches.push((template,bindings));
        }
    }
    let drain_seconds=t.elapsed().as_secs_f64();
    let refs=batches.iter().map(|(t,b)|(t,b.as_slice())).collect::<Vec<_>>();
    if std::env::var("EGG_LAYOUT_COMPACT_CELLS").is_ok_and(|v| v == "1") {
        let r = eg.compact_cell_facts(&refs)?;
        return Ok(json!({"compact":true,"cell_batches":r.cell_batches,"cell_instances":r.cell_instances,"owned_rows":r.owned_rows,"existing_cell_rows":r.existing_cell_rows,"storage":crate::reuse_graph::native_storage::compact_report(eg)}));
    }
    let r=eg.virtualize_unshared_facts_batch(&refs)?;
    let st=Instant::now();let virtual_rows=eg.virtual_fact_stats().iter().map(|(_,s)|s.virtual_rows).sum::<usize>();
    Ok(json!({"drain_seconds":drain_seconds,"validation_seconds":r.validation_seconds,"encoding_seconds":r.encoding_seconds,"statistics_seconds":st.elapsed().as_secs_f64(),"batched":true,"templates":templates,"processed_instance_packets":instances,"shared_rows_reused":r.shared_rows_reused,"requested_rows":r.requested_rows,"distinct_native_rows":r.distinct_native_rows,"new_virtual_rows_net":r.new_virtual_rows,"encoded_tables":r.affected_tables,"virtual_rows":virtual_rows}))
}

fn save_bake(out: &Path, name: &str, bake: &Bake, rules: &[Rule]) -> Result<usize> {
    let bytes = serde_json::to_vec_pretty(bake)?;
    std::fs::write(out.join(format!("{name}.json")), &bytes)?;
    let mut text = format!(
        "; Executable saturated constructor closures; native boundary matching remains enabled.\n; @stats {}\n",
        json!({"schema":bake.schema,"bodies":bake.macros.len(),"unfinished":bake.unfinished,"json_bytes":bytes.len()})
    );
    // A standalone readable library, with no input instances or execution schedule.
    for line in &bake.environment {
        if line.starts_with("(datatype ") {
            text += line;
            text.push('\n');
        }
    }
    for (id, m) in &bake.macros {
        text += &format!(
            "; @body {}\n(ruleset adaptive_baked_{id})\n(rule ((= adaptive_root {})) ({}) :ruleset adaptive_baked_{id} :name \"adaptive_baked_{id}\")\n",
            json!({"rule":id,"enodes":m.rows,"eclasses":m.classes,"proof_matches":m.proof_matches}),
            rules[*id].lhs,
            m.actions
        );
    }
    std::fs::write(out.join(format!("{name}.egg")), text)?;
    Ok(bytes.len())
}
fn counts(eg: &EGraph) -> Value {
    let mut cls = BTreeSet::new();
    let mut nodes = 0;
    for op in eg.get_function_names() {
        let f = eg.get_function(&op).unwrap();
        if (op.starts_with("adaptive_cell_view_") && !f.is_constructor()) || f.is_let_binding() || f.schema().output.name() != "Math" {
            continue;
        }
        eg.function_for_each(&op, |r| {
            nodes += 1;
            cls.insert(eg.get_canonical_value(*r.vals.last().unwrap(), &f.schema().output));
        })
        .unwrap();
    }
    let rss = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|x| x * 1024);
    json!({"enodes":nodes,"eclasses":cls.len(),"rss_bytes":rss})
}
/// Modes: native, adaptive (learn on matched source rules), frozen (loaded macros only).
/// Each round is a native simultaneous combined ruleset; swapping a RHS can
/// advance more source inference steps. Finite round counts are NOT equal work.
pub fn run(source: &str, input: Option<&Path>, out: &Path, mode: &str) -> Result<Value> {
    let min_reuse = std::env::var("EGG_LAYOUT_MIN_TEMPLATE_REUSE")
        .ok()
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(2);
    if min_reuse == 0 {
        return Err("minimum template reuse must be positive".into());
    }
    let transition_specs=std::env::var("EGG_LAYOUT_CELL_TRANSITIONS").ok().map(|p|->Result<Vec<transitions::Spec>> {Ok(serde_json::from_slice(&std::fs::read(p)?)?)}).transpose()?;
    run_impl(
        source,
        input,
        out,
        mode,
        min_reuse,
        std::env::var("EGG_LAYOUT_AUDIT_ENCLOSURES").is_ok_and(|s| s == "1")
            || std::env::var("EGG_LAYOUT_SKIP_INTERNAL_RHS").is_ok_and(|s| s == "1"),
        std::env::var("EGG_LAYOUT_SKIP_INTERNAL_RHS").is_ok_and(|s| s == "1"),
        std::env::var("EGG_LAYOUT_PRUNE_INTERNAL_JOIN").is_ok_and(|s| s == "1"),
        std::env::var("EGG_LAYOUT_CELL_EVOLUTION").is_ok_and(|s| s == "1"),
        transition_specs.as_deref(),
        std::env::var("EGG_LAYOUT_CELL_MACHINE").is_ok_and(|v|v=="1"),
    )
}
/// Explicit policy for reproducible ablations; 1 restores unrestricted admission.
pub fn run_with_min_reuse(
    source: &str,
    input: Option<&Path>,
    out: &Path,
    mode: &str,
    min_reuse: usize,
) -> Result<Value> {
    run_impl(source, input, out, mode, min_reuse, false, false, false, false, None, false)
}
pub fn run_audit(
    source: &str,
    input: Option<&Path>,
    out: &Path,
    mode: &str,
    min_reuse: usize,
) -> Result<Value> {
    run_impl(source, input, out, mode, min_reuse, true, false, false, false, None, false)
}
pub fn run_audit_with_rhs_skip(
    source: &str,
    input: Option<&Path>,
    out: &Path,
    mode: &str,
    min_reuse: usize,
) -> Result<Value> {
    run_impl(source, input, out, mode, min_reuse, true, true, false, false, None, false)
}
/// Root-slice pruning before native Join, without diagnostic tracing.
pub fn run_with_join_pruning(source: &str, input: Option<&Path>, out: &Path,
    mode: &str, min_reuse: usize) -> Result<Value> {
    run_impl(source, input, out, mode, min_reuse, false, false, true, false, None, false)
}
/// Observe boundary-to-cell transitions without changing source execution.
pub fn run_with_evolution(source: &str, input: Option<&Path>, out: &Path,
    mode: &str, min_reuse: usize) -> Result<Value> {
    run_impl(source, input, out, mode, min_reuse, false, false, false, true, None, false)
}
pub fn run_with_transitions(source:&str, library:&Path, out:&Path, specs:&[transitions::Spec], min_reuse:usize)->Result<Value> {
    run_impl(source,Some(library),out,"frozen",min_reuse,false,false,false,false,Some(specs),false)
}
pub fn run_cell_machine(source:&str,library:&Path,out:&Path,specs:&[transitions::Spec],min_reuse:usize)->Result<Value> {
    run_impl(source,Some(library),out,"frozen",min_reuse,false,false,false,false,Some(specs),true)
}
fn run_impl(
    source: &str,
    input: Option<&Path>,
    out: &Path,
    mode: &str,
    min_reuse: usize,
    audit_enabled: bool,
    skip_rhs: bool,
    prune_join: bool,
    evolution_enabled: bool,
    transition_specs: Option<&[transitions::Spec]>,
    machine_enabled: bool,
) -> Result<Value> {
    if (transition_specs.is_some() || machine_enabled) && (mode!="frozen" || input.is_none() || audit_enabled || skip_rhs || prune_join || evolution_enabled) {return Err("residual transitions require a frozen library and untraced execution".into());}
    if evolution_enabled && (skip_rhs || prune_join) {return Err("cell evolution observes unpruned execution; disable join/RHS skipping".into());}
    if min_reuse == 0 {
        return Err("minimum template reuse must be positive".into());
    }
    if !["native", "adaptive", "frozen"].contains(&mode) {
        return Err("unknown adaptive mode".into());
    }
    if out.exists() {
        return Err("output exists".into());
    }
    std::fs::create_dir_all(out)?;
    let started = Instant::now();
    let mut parser = EGraph::default();
    let commands = parser.parse_program(None, source)?;
    let mut environment = vec![];
    let mut rules = vec![];
    for c in &commands {
        match c {
        Command::Datatype { .. }=>environment.push(c.to_string()),
        Command::Rewrite(set,r,sub)=>{if !set.is_empty() || !r.conditions.is_empty() || *sub {return Err("conditional/subsuming/named rewrites use native runner; not positive macro runtime".into());}
            environment.push(c.to_string());rules.push(Rule{lhs:r.lhs.clone(),rhs:r.rhs.clone()});},
        Command::Action(egglog::ast::Action::Expr(..)|egglog::ast::Action::Let(..)|egglog::ast::Action::Union(..))|Command::RunSchedule(..)|Command::Check(..)|Command::PrintSize(..)|Command::PrintOverallStatistics(..)=>{},
        _=>return Err("adaptive runtime supports Math datatype, positive rewrites, positive actions and finite run schedules".into()),
    }
    }
    let mut bake = if let Some(p) = input {
        serde_json::from_slice::<Bake>(&std::fs::read(p)?)?
    } else {
        Bake {
            schema: "adaptive-ripsat/v2".into(),
            environment: environment.clone(),
            ..Default::default()
        }
    };
    if bake.schema != "adaptive-ripsat/v2" || bake.environment != environment {
        return Err("bake rule environment mismatch".into());
    }
    let mut eg = EGraph::default();
    for c in &commands {
        if matches!(c, Command::Datatype { .. }) {
            eg.run_program(vec![c.clone()])?;
        }
    }
    // Validate positive constructors, all variables, and reserved macro identifiers before running.
    for r in &rules {
        let mut names = BTreeMap::new();
        vars(&eg, &r.lhs, "Math", &mut names)?;
        vars(&eg, &r.rhs, "Math", &mut names)?;
        if names.keys().any(|n| n.starts_with("adaptive_")) {
            return Err("reserved adaptive_ variable".into());
        }
    }
    let plans=transitions::compile(&mut eg,&rules,&bake,transition_specs.unwrap_or(&[]))?;
    let machine=if machine_enabled {Some(cell_machine::compile(&mut eg,&bake,&rules,&plans)?)}else{None};
    if let Some(machine)=&machine {std::fs::write(out.join("cell-machine.json"),serde_json::to_vec_pretty(&machine.proof)?)?;}
    if !plans.is_empty() || machine.is_some() {
        std::fs::write(out.join("cell-transitions.json"),serde_json::to_vec_pretty(&plans.iter().map(|p|&p.proof).collect::<Vec<_>>())?)?;
        let mut generated=commands.iter().filter(|c|matches!(c,Command::Datatype{..})).map(ToString::to_string).collect::<Vec<_>>().join("\n");
        generated+="\n";
        for p in &plans {generated+=&p.definitions;}
        if let Some(machine)=&machine {generated+=&machine.definitions;}
        for (id,r) in rules.iter().enumerate() {generated+=&rule_program(id,r,bake.macros.get(&id),&plans,machine.as_ref()).1;}
        std::fs::write(out.join("cell-transitions.egg"),generated)?;
    }
    let mut selected = vec![];
    for (id, r) in rules.iter().enumerate() {
        selected.push(install(
            &mut eg,
            id,
            r,
            if mode == "native" {
                None
            } else {
                bake.macros.get(&id)
            },
            &plans,
            machine.as_ref(),
        )?);
    }
    let mut audit = (audit_enabled || prune_join).then(super::enclosure_audit::Audit::default);
    let trace = (audit_enabled || evolution_enabled).then(egglog::TraceSession::with_dependencies);
    let mut evolution = evolution_enabled.then(evolution::Evolution::default);
    #[cfg(feature="native-read-profile")]
    eg.reset_join_read_profile(std::env::var("EGG_LAYOUT_JOIN_READ_PROFILE").is_ok_and(|v|v=="1"));
    let lean=std::env::var("EGG_LAYOUT_RUNTIME_SUMMARY").is_ok_and(|v|v=="1");
    let setup_seconds=started.elapsed().as_secs_f64();
    let mut input_seconds=0.;let mut packing_total=0.;
    let mut samples = vec![];
    let mut round = 0;
    let mut total = 0;
    let mut learning_seconds = 0.;
    let mut execution_seconds = 0.;
    for c in commands {
        match c {
            Command::Datatype { .. } | Command::Rewrite(..) => {}
            Command::RunSchedule(GenericSchedule::Repeat(_, n, inner)) => {
                if !matches!(inner.as_ref(),GenericSchedule::Run(_,r) if r.ruleset.is_empty() && r.until.is_none())
                {
                    return Err("simple default (run N) required".into());
                }
                for _ in 0..n {
                    round += 1;
                    let set = format!("adaptive_round_{round}");
                    eg.parse_and_run_program(
                        None,
                        &format!("(unstable-combined-ruleset {set} {})", selected.join(" ")),
                    )?;
                    let t = Instant::now();
                    let prepare_start=Instant::now();
                    let transition_work=if plans.is_empty() {Value::Null} else {transitions::prepare(&mut eg,&plans)?};
                    let prepare_seconds=prepare_start.elapsed().as_secs_f64();
                    let union_before = eg.native_union_count();
                    if let Some(e)=evolution.as_mut() {e.begin(&eg);}
                    let completed_before=eg.completed_cell_counts();
                    let promised_before=eg.promised_cell_prefixes();
                    let queued_before=eg.queued_cell_prefixes();
                    let (report, mut audit_report) = if let Some(machine)=&machine {
                        (if std::env::var("EGG_LAYOUT_CELL_PROMISES").is_ok_and(|v|v=="0") {eg.step_rules_with_completed_cells(&set,machine.completion.clone())?} else {if std::env::var("EGG_LAYOUT_CELL_ORBITS").is_ok_and(|v|v=="0") {eg.step_rules_with_cell_promises(&set,machine.completion.clone())?} else {eg.step_rules_with_cell_machine(&set,machine.completion.clone(),machine.symmetries.clone())?}},Value::Null)
                    } else if prune_join {
                        let a = audit.as_mut().unwrap();
                        a.begin_join(&eg);
                        let entries = rules.iter().enumerate().map(|(id, r)|
                            (selected[id].clone(), match &r.lhs { Expr::Call(_,op,_) => op.clone(), _ => String::new() }, a.closed_roots(&eg, &r.lhs)))
                            .filter(|(_,_,roots)| !roots.is_empty()).collect::<Vec<_>>();
                        let certified = entries.iter().map(|(_,_,v)| v.len()).sum::<usize>();
                        let certificate_seconds = t.elapsed().as_secs_f64();
                        let report = eg.step_rules_excluding_proven_roots(&set, entries)?;
                        (report, json!({"certified_rule_roots":certified,"certificate_seconds":certificate_seconds}))
                    } else if let Some(trace) = trace.as_ref() {
                        if let Some(a)=audit.as_mut() {
                            a.begin(&eg);
                            if skip_rhs {trace.set_proven_redundant_rhs_filter(Some(a.rhs_filter(&eg)));}
                        }
                        let report = eg.step_rules_with_trace(&set, trace)?;
                        let batch=trace.drain_completed();
                        if let Some(e)=evolution.as_mut() {e.finish(&eg,trace,&batch,&bake.macros,round,eg.native_union_count().saturating_sub(union_before));}
                        let stats=if let Some(a)=audit.as_mut() {a.finish(&eg,trace,batch)} else {Value::Null};
                        (report, stats)
                    } else {
                        (eg.step_rules(&set)?, Value::Null)
                    };
                    if !audit_report.is_null() {
                        audit_report["native_union_merges_total"] =
                            json!(eg.native_union_count().saturating_sub(union_before));
                    }
                    execution_seconds += t.elapsed().as_secs_f64();
                    let matches = report.num_matches_per_rule.values().sum::<usize>();
                    total += matches;
                    let macro_matches = report
                        .num_matches_per_rule
                        .iter()
                        .filter(|(n, _)| n.starts_with("adaptive_baked_"))
                        .map(|(_, n)| *n)
                        .sum::<usize>();
                    let source_matches = matches - macro_matches;
                    let t = Instant::now();
                    let packing = if mode != "native" {
                        pack_instances(&mut eg, &bake, min_reuse, &mut audit)?
                    } else {
                        json!({"processed_instance_packets":0,"virtual_rows":0})
                    };
                    let packing_seconds = t.elapsed().as_secs_f64();packing_total+=packing_seconds;
                    let mut admitted = vec![];
                    let mut rejected = vec![];
                    let t = Instant::now();
                    if mode == "adaptive" {
                        for (id, r) in rules.iter().enumerate() {
                            if bake.macros.contains_key(&id)
                                || bake.unfinished.contains_key(&id)
                                || report
                                    .num_matches_per_rule
                                    .get(selected[id].as_str())
                                    .copied()
                                    .unwrap_or(0)
                                    == 0
                            {
                                continue;
                            }
                            match learn(&environment, r, id) {
                                Ok(m) => {
                                    selected[id] = install(&mut eg, id, r, Some(&m), &plans,machine.as_ref())?;
                                    admitted.push(id);
                                    bake.macros.insert(id, m);
                                }
                                Err(e) => {
                                    let msg = e.to_string();
                                    rejected.push(json!({"rule":id,"reason":msg}));
                                    bake.unfinished.insert(id, msg);
                                }
                            }
                        }
                    }
                    learning_seconds += t.elapsed().as_secs_f64();
                    let library_bytes =
                        if lean {0} else {save_bake(out, &format!("bake_lib_round{round}"), &bake, &rules)?};
                    samples.push(json!({"cell_states":machine.as_ref().map(|m|m.counts(&eg)),"queued_prefixes":eg.queued_cell_prefixes().saturating_sub(queued_before),"promised_prefixes":eg.promised_cell_prefixes().saturating_sub(promised_before),"completed_prefixes":eg.completed_cell_counts().0.saturating_sub(completed_before.0),"unresolved_completion_plans":eg.completed_cell_counts().1.saturating_sub(completed_before.1),"cell_transition":transition_work,"transition_prepare_seconds":prepare_seconds,"round":round,"enclosure_audit":audit_report,"matches":matches,"macro_matches":macro_matches,"source_matches":source_matches,"cumulative_matches":total,"graph":if lean {Value::Null}else{counts(&eg)},"packing":packing,"packing_seconds":packing_seconds,"admitted":admitted,"rejected":rejected,"library_bytes":library_bytes,"bodies":bake.macros.len(),"execution_seconds":execution_seconds,"learning_seconds":learning_seconds}));
                    if !lean {std::fs::write(out.join("progress.json"),serde_json::to_vec_pretty(&samples)?)?;}
                    if report.can_stop {
                        break;
                    }
                }
            }
            Command::RunSchedule(..) => return Err("simple (run N) required".into()),
            other => {
                let t=Instant::now();eg.run_program(vec![other])?;input_seconds+=t.elapsed().as_secs_f64();
            }
        }
    }
    save_bake(out, "bake_lib", &bake, &rules)?;
    if std::env::var("EGG_LAYOUT_AUDIT_VERIFY").is_ok_and(|s| s == "1") {
        std::fs::write(
            out.join("verification-graph.json"),
            serde_json::to_vec(&super::enclosure_audit::export_math(&eg))?,
        )?;
    }
    let evolution_report=if let Some(e)=evolution.as_ref() {
        let mut full=e.report();full["rule_environment"]=json!(environment);
        let file=std::io::BufWriter::new(std::fs::File::create(out.join("cell-evolution.json"))?);
        serde_json::to_writer_pretty(file,&full)?;
        Some(full.as_object().unwrap().iter().filter(|(k,_)|!["patterns","families","rounds","rule_environment"].contains(&k.as_str())).map(|(k,v)|(k.clone(),v.clone())).collect::<serde_json::Map<_,_>>())
    } else {None};
    let report = json!({"summary_only":lean,"setup_seconds":setup_seconds,"input_seconds":input_seconds,"packing_seconds":packing_total,"cell_machine_enabled":machine.is_some(),"cell_states":machine.as_ref().map(|m|m.counts(&eg)),"cell_transitions_enabled":!plans.is_empty(),"join_reads":transitions::read_profile(&eg),"cell_evolution":evolution_report,"mode":mode,"rhs_skip_enabled":skip_rhs,"join_pruning_enabled":prune_join,"min_template_reuse":min_reuse,"seconds":started.elapsed().as_secs_f64(),"execution_seconds":execution_seconds,"learning_seconds":learning_seconds,"matches":total,"samples":samples,"final":counts(&eg),"storage":crate::reuse_graph::native_storage::storage_report(&eg),"scope":"actual native tier0; source RHS replaced by certified symbolic saturated closure, one native trigger query per invocation; original source rule disabled for replaced RHS; boundary matching retained; no claim all internal joins suppressed"});
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}
