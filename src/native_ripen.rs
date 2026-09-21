//! Ripen an explicit local cell with the native engine; feed its witnessed history
//! and whole-cell fixed-point state back into Tier1. No private matcher/scheduler.
use super::*;
use crate::coarse_smooth::{RipenFeedback, RipenOrigin};

#[path = "native_ripen_entry.rs"]
mod entry;
pub use entry::from_use;
pub(super) use entry::{prepare, prepare_members};

fn supported_action(a: &Action) -> bool {
    matches!(
        a,
        Action::Let(..) | Action::Expr(..) | Action::Union(..) | Action::Set(..)
    )
}

pub fn run(source: &Path, out: &Path, max_rounds: usize) -> Result<Json> {
    run_with_origin(source, out, max_rounds, None, true, false)
}
pub(super) fn run_with_origin(
    source: &Path,
    out: &Path,
    max_rounds: usize,
    origin: Option<RipenOrigin>,
    artifacts: bool,
    symbolic_boundary: bool,
) -> Result<Json> {
    if max_rounds == 0 {
        return Err("ripen max-rounds must be positive".into());
    }
    if out.exists() {
        return Err("ripen output already exists".into());
    }
    let source = source.canonicalize()?;
    let text = std::fs::read_to_string(&source)?;
    let mut eg = EGraph::default();
    let commands = crate::visual_rule::surface_program(
        eg.parse_program(Some(source.display().to_string()), &text)?,
    );
    let mut setup = vec![];
    let mut checks = vec![];
    let mut rules = vec![];
    let mut names = BTreeSet::new();
    let mut rulesets = BTreeSet::new();
    let mut datatype = None;
    for command in commands {
        if matches!(command, Command::Rewrite(_, _, true)) {
            return Err("ripen rejects subsuming rewrites".into());
        }
        let mut command = crate::visual_rule::normalize(command, rules.len());
        if let Command::Rule { rule } = &mut command {
            if rule.name.is_empty() {
                rule.name = format!("R{}", rules.len());
            }
        }
        match &command {
            Command::Datatype { name, .. } => {
                if datatype.replace((name.clone(), command.to_string())).is_some() {
                    return Err("ripen currently requires one self-contained datatype".into());
                }
            }
            Command::Rule { rule } => {
                if !rule.head.0.iter().all(supported_action) { return Err("ripen rejects unsupported delete/subsume actions".into()); }
                if !names.insert(rule.name.clone()) { return Err("ripen rule names must be unique".into()); }
                rulesets.insert(rule.ruleset.clone());
                let calls = crate::visual_rule::positions(rule).into_iter().map(|(span,path,_)| {
                    let expression=crate::visual_rule::owned_expression_at(rule,&path).unwrap().clone();
                    (Arc::from(span),(path,expression))
                }).collect();
                rules.push(RuleInfo{rule:rule.clone(),calls});
            }
            Command::AddRuleset(..) | Command::Relation {..} | Command::Function{..} => {}
            Command::Action(a) if supported_action(a) => {}
            Command::Check(..) => { checks.push(command); continue; }
            _ => return Err(format!("unsupported ripen command: {command}; use an explicit entry file without run/include/function/deletion commands").into()),
        }
        setup.push(command);
    }
    let (datatype_name, _) = datatype.ok_or("ripen requires a datatype")?;
    let datatype = setup
        .iter()
        .filter(|c| {
            matches!(
                c,
                Command::Datatype { .. } | Command::Relation { .. } | Command::Function { .. }
            )
        })
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let rulesets: Vec<_> = rulesets.into_iter().collect();
    eg.run_program(setup.clone())?;
    let mut port_values = vec![];
    for cmd in &setup {
        if let Command::Action(Action::Let(_, name, _)) = cmd {
            let (sort, value) = eg.eval_expr(&Expr::Var(sp(), name.clone()))?;
            port_values.push((name.clone(), sort, value));
        }
    }
    let trace = TraceSession::with_dependencies();
    let mut c = Captured {
        preview_source: text.clone(),
        boundaries: vec![],
        layers: Default::default(),
        datatype,
        datatype_name,
        rules,
        records: vec![],
        pool: Pool::default(),
        events: 0,
        rounds: Some(0),
        trace_seconds: 0.,
        trace_peak: 0,
        trace_batches: 0,
        rejected: 0,
    };
    let mut producers = BTreeMap::new();
    let mut exporter = layer_view::Exporter::default();
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("entry.egg"), &text)?;
    let started = Instant::now();
    let outcome = (|| -> Result<Json> {
        let mut schedule = String::new();
        let mut feedback = None;
        for round in 1..=max_rounds {
            let mut updated = false;
            for ruleset in &rulesets {
                updated |= eg.step_rules_with_trace(ruleset, &trace)?.updated;
                schedule += &format!("(run {} 1)\n", ruleset);
                collect(&eg, &trace, &mut c, &mut producers)?;
            }
            let state = if !updated {
                "Closed"
            } else if round == max_rounds {
                "Suspended"
            } else {
                "Growing"
            };
            let f=RipenFeedback{origin:origin.clone(),state:state.into(),round,max_rounds,rulesets:rulesets.clone(),updated,
                excluded_matches:c.rejected,scope:"whole isolated cell under the declared rulesets and fixed entry; not each sub-Use, no cross-instance proof or automatic tier0 substitution".into()};
            c.boundaries.push(CaptureBoundary {
                kind: "ripen-round".into(),
                round: Some(round),
                end: c.records.len(),
                ripen: Some(f.clone()),
            });
            c.rounds = Some(round);
            if artifacts {
                exporter.capture(&mut c, out)?;
            } else {
                update_layers(&mut c)?;
            }
            feedback = Some(f);
            if !updated {
                break;
            }
        }
        c.trace_seconds = started.elapsed().as_secs_f64();
        let feedback = feedback.unwrap();
        let closed = feedback.state == "Closed";
        if closed {
            eg.run_program(checks.clone())?;
        }
        let mut replay = setup
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        replay.push('\n');
        replay.push_str(&schedule);
        if closed {
            replay.push_str(
                &checks
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        std::fs::write(out.join("ripened.egg"), replay)?;
        if artifacts {
            history::save(&out.join("history.json"), &source, &c)?;
        }
        let state_export = if closed {
            match export_state(
                setup
                    .iter()
                    .find(|c| matches!(c, Command::Datatype { .. }))
                    .unwrap(),
                &eg,
                &c,
                &port_values,
                symbolic_boundary || origin.as_ref().is_some_and(|o| o.symbolic_boundary),
            ) {
                Ok(state) => {
                    std::fs::write(
                        out.join("closed-state.json"),
                        serde_json::to_vec_pretty(&state)?,
                    )?;
                    json!({"status":"exported","file":"closed-state.json","values":state.values.len(),"facts":state.rows.len()})
                }
                Err(e) => json!({"status":"unavailable","reason":e.to_string()}),
            }
        } else {
            json!({"status":"not_closed"})
        };
        // Tier1 is the actual importer/Use builder, populated during every sweep.
        let report = json!({"ripen":feedback,"checks":if closed{"passed"}else{"deferred"},
            "checks_count":checks.len(),"rules":c.rules.iter().map(|r|r.rule.to_string()).collect::<Vec<_>>(),
            "source":source,"source_text":text,"events":c.events,"imported_applies":c.records.len(),
            "closed_state":state_export,"tables":table_sizes(&eg, &c.datatype)?,"tier1":c.layers.report(),"round_manifest":if artifacts {Some("rounds/manifest.json")}else{None},
            "history":if artifacts {Some("history.json")}else{None},"seconds":c.trace_seconds,"fractal_summaries_used":0});
        std::fs::write(out.join("ripen.json"), serde_json::to_vec_pretty(&report)?)?;
        Ok(report)
    })();
    if let Err(e) = &outcome {
        std::fs::write(
            out.join("ripen.json"),
            serde_json::to_vec_pretty(&json!({"state":"Failed","error":e.to_string()}))?,
        )?;
    }
    outcome
}

fn table_sizes(eg: &EGraph, datatype: &str) -> Result<BTreeMap<String, usize>> {
    let mut parser = EGraph::default();
    let cmds = parser.parse_program(None, datatype)?;
    let mut sizes = BTreeMap::new();
    for cmd in &cmds {
        let names: Vec<String> = match cmd {
            Command::Datatype { variants, .. } => variants.iter().map(|v| v.name.clone()).collect(),
            Command::Relation { name, .. } => vec![name.clone()],
            Command::Function { name, .. } => vec![name.clone()],
            _ => vec![],
        };
        for name in names {
            let mut count = 0;
            eg.function_for_each(&name, |_| count += 1)?;
            sizes.insert(name, count);
        }
    }
    Ok(sizes)
}

fn export_state(
    datatype: &Command,
    eg: &EGraph,
    c: &Captured,
    ports: &[(String, egglog::ArcSort, Value)],
    symbolic: bool,
) -> Result<crate::closed_state::ClosedState> {
    use crate::closed_state::{ClosedState, Row, Vertex};
    let Command::Datatype { name, variants, .. } = datatype else {
        return Err("expected datatype".into());
    };
    if name.contains('-')
        || variants
            .iter()
            .flat_map(|v| &v.types)
            .any(|t| t != name && !matches!(t.as_str(), "i64" | "String" | "bool"))
        || ports.iter().any(|(_, s, _)| s.name() != name)
    {
        return Err("closed-state export currently supports one equality datatype, i64/String/bool fields and equality-sort named ports".into());
    }
    fn constructor_expr(e: &Expr, ops: &BTreeSet<&str>) -> bool {
        match e {
            Expr::Call(_, op, args) => {
                ops.contains(op.as_str()) && args.iter().all(|e| constructor_expr(e, ops))
            }
            _ => true,
        }
    }
    let mut parser = EGraph::default();
    let declarations = parser.parse_program(None, &c.datatype)?;
    let mut tables = vec![];
    for d in &declarations {
        match d {
            Command::Relation { name, inputs, .. } => {
                tables.push((name.clone(), inputs.clone(), "Unit".to_string()))
            }
            Command::Function {
                name: op,
                schema,
                merge,
                ..
            } => {
                if merge.is_some() {
                    return Err("function merge executes natively, but ClosedState sharing does not yet certify merge semantics".into());
                }
                tables.push((op.clone(), schema.input.clone(), schema.output.clone()));
            }
            _ => {}
        }
    }
    if tables
        .iter()
        .flat_map(|(_, inputs, output)| inputs.iter().chain(std::iter::once(output)))
        .any(|t| t != name && !matches!(t.as_str(), "i64" | "String" | "bool" | "Unit"))
    {
        return Err("unsupported table field sort in ClosedState export".into());
    }
    let ops: BTreeSet<_> = variants
        .iter()
        .map(|v| v.name.as_str())
        .chain(tables.iter().map(|t| t.0.as_str()))
        .collect();
    for r in &c.rules {
        let body = r.rule.body.iter().all(|f| match f {
            Fact::Eq(_, a, b) => constructor_expr(a, &ops) && constructor_expr(b, &ops),
            Fact::Fact(e) => constructor_expr(e, &ops),
        });
        let head = r.rule.head.0.iter().all(|a| match a {
            Action::Union(_, a, b) => constructor_expr(a, &ops) && constructor_expr(b, &ops),
            Action::Let(_, _, e) | Action::Expr(_, e) => constructor_expr(e, &ops),
            Action::Set(_, op, args, value) => {
                ops.contains(op.as_str())
                    && args.iter().all(|e| constructor_expr(e, &ops))
                    && constructor_expr(value, &ops)
            }
            _ => false,
        });
        if !body || !head {
            return Err("ClosedState sharing currently certifies positive constructor/equality rules only; primitive guards/actions require an explicit semantics contract".into());
        }
    }
    let serialized = eg.serialize(egglog::SerializeConfig::default());
    if !serialized.discarded_functions.is_empty() || !serialized.truncated_functions.is_empty() {
        return Err("truncated engine serialization".into());
    }
    let literals: BTreeMap<_, _> = serialized
        .egraph
        .nodes
        .iter()
        .filter_map(|(id, n)| {
            eg.from_node_id(id)
                .is_primitive()
                .then_some((n.eclass.to_string(), n.op.clone()))
        })
        .collect();
    let mut ids = BTreeMap::<String, usize>::new();
    let mut values = vec![];
    let mut vertex = |ty: &str, v: Value| -> Result<usize> {
        let sort = eg.get_arcsort_by(|s| s.name() == ty);
        let key = eg.value_to_class_id(&sort, v).to_string();
        if let Some(i) = ids.get(&key) {
            return Ok(*i);
        }
        let literal = if ty == name {
            None
        } else if ty == "Unit" {
            Some("()".into())
        } else {
            Some(
                literals
                    .get(&key)
                    .cloned()
                    .ok_or("missing scalar literal")?,
            )
        };
        let i = values.len();
        ids.insert(key, i);
        values.push(Vertex {
            sort: ty.into(),
            literal,
        });
        Ok(i)
    };
    let mut rows = BTreeSet::new();
    let mut sorted = variants.clone();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for v in &sorted {
        let mut raw = vec![];
        eg.function_for_each(&v.name, |row| raw.push((row.vals.to_vec(), row.subsumed)))?;
        for (row, subsumed) in raw {
            if subsumed || row.len() != v.types.len() + 1 {
                return Err("unexpected subsumed row or arity".into());
            }
            let args = v
                .types
                .iter()
                .zip(&row)
                .map(|(t, x)| vertex(t, *x))
                .collect::<Result<Vec<_>>>()?;
            rows.insert(Row {
                op: v.name.clone(),
                args,
                result: vertex(name, *row.last().unwrap())?,
            });
        }
    }
    for (op, inputs, output) in &tables {
        let mut raw = vec![];
        eg.function_for_each(op, |r| raw.push((r.vals.to_vec(), r.subsumed)))?;
        for (row, subsumed) in raw {
            if subsumed || row.len() != inputs.len() + 1 {
                return Err("unexpected table row".into());
            }
            let args = inputs
                .iter()
                .zip(&row)
                .map(|(t, v)| vertex(t, *v))
                .collect::<Result<Vec<_>>>()?;
            rows.insert(Row {
                op: op.clone(),
                args,
                result: vertex(output, *row.last().unwrap())?,
            });
        }
    }
    let mut named = BTreeMap::new();
    for (p, sort, value) in ports {
        named.insert(p.clone(), vertex(sort.name(), *value)?);
    }
    let mut rules: Vec<_> = c
        .rules
        .iter()
        .map(|r| {
            let mut r = r.rule.clone();
            r.name.clear();
            r.ruleset.clear();
            r.naive = false;
            r.to_string()
        })
        .collect();
    rules.sort();
    rules.dedup();
    let mut local_ids = vec![String::new(); values.len()];
    for (key, i) in ids {
        local_ids[i] = key;
    }
    let mut state = ClosedState {
        version: 1,
        local_ids,
        scope: json!({"datatype":name,"constructors":sorted.iter().map(|v|json!({"name":v.name,"types":v.types,"cost":v.cost.map(|x|x.to_string()),"unextractable":v.unextractable})).collect::<Vec<_>>(),"rules":rules,"symbolic_boundary":symbolic,"semantics":"positive-constructor-equality/v1","ports":"fixed named globals; otherwise all facts observable"}),
        values,
        rows: rows.into_iter().collect(),
        ports: named,
    };
    if !tables.is_empty() {
        state.scope["tables"] = json!(
            declarations
                .iter()
                .filter(|d| matches!(d, Command::Relation { .. } | Command::Function { .. }))
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
        state.scope["semantics"] = json!("positive-constructor-table-equality/v1");
    }
    Ok(state)
}
