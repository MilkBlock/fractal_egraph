//! Native ripen is a resumable task: one sweep per poll, no automatic stop budget.
use super::*;
#[path = "native_ripen_entry.rs"]
mod entry;
pub(super) use entry::PreparationCache;
pub(super) fn measured_engine() -> EGraph {
    EGraph::default()
}
fn supported_action(a: &Action) -> bool {
    matches!(
        a,
        Action::Let(..)
            | Action::Expr(..)
            | Action::Union(..)
            | Action::Set(..)
            | Action::Change(_, egglog::ast::Change::Subsume, _, _)
    )
}
pub(super) struct Session {
    pub eg: EGraph,
    pub before: EGraph,
    pub weak_queries: BTreeMap<(usize, bool), weak_dominate::CachedQuery>,
    pub has_effects: bool,
    pub initial_facts: usize,
    pub effect_components: BTreeMap<String, u64>,
    pub union_effect: bool,
    pub initial: Option<crate::saturated_rule_composition::SaturatedRuleComposition>,
    pub trigger: Option<crate::saturated_rule_composition::SaturatedRuleComposition>,
    pub final_state: Option<crate::saturated_rule_composition::SaturatedRuleComposition>,
    source: std::path::PathBuf,
    out: std::path::PathBuf,
    text: String,
    setup: Vec<Command>,
    checks: Vec<Command>,
    c: Captured,
    ports: Vec<(String, egglog::ArcSort, Value)>,
    trace: TraceSession,
    producers: BTreeMap<u64, Producer>,
    rulesets: Vec<String>,
    round: usize,
    schedule: String,
    pub report: Json,
    pub done: bool,
}
impl Session {
    pub fn new(source: &Path, out: &Path) -> Result<Self> {
        Self::new_impl(source, out, None)
    }
    pub(super) fn new_cached(source: &Path, out: &Path, cache: &PreparationCache) -> Result<Self> {
        Self::new_impl(source, out, cache.environment.as_ref())
    }
    fn new_impl(
        source: &Path,
        out: &Path,
        environment: Option<&entry::Environment>,
    ) -> Result<Self> {
        let source = source.canonicalize()?;
        let text = std::fs::read_to_string(&source)?;
        let mut eg = measured_engine();
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
                if !rule.head.0.iter().all(supported_action) { return Err("ripen rejects delete and other unsupported actions".into()); }
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
        if let Some(environment) = environment {
            let code = setup
                .iter()
                .filter(|c| !matches!(c, Command::Action(_)))
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            if code != environment.code {
                return Err("cached ripen rule environment mismatch".into());
            }
            eg = environment.base.clone();
            rules = environment.rules.clone(); // source spans must match the compiled rules
            eg.run_program(
                setup
                    .iter()
                    .filter(|c| matches!(c, Command::Action(_)))
                    .cloned()
                    .collect(),
            )?;
        } else {
            eg.run_program(setup.clone())?;
        }
        let mut port_values = vec![];
        for cmd in &setup {
            if let Command::Action(Action::Let(_, name, _)) = cmd {
                let (sort, value) = eg.eval_expr(&Expr::Var(sp(), name.clone()))?;
                port_values.push((name.clone(), sort, value));
            }
        }
        let trace = TraceSession::with_dependencies();
        let c = Captured {
            changes: vec![],
            last_scope: 0,
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

        std::fs::create_dir_all(out)?;
        std::fs::write(out.join("entry.egg"), &text)?;
        write_native_graph(&eg, out, "native-egraph-initial")?;
        let initial = export_state(
            setup
                .iter()
                .find(|c| matches!(c, Command::Datatype { .. }))
                .unwrap(),
            &eg,
            &c,
            &port_values,
            true,
            out,
            false,
        )
        .ok();
        if let Some(state) = &initial {
            crate::pipeline::write_json(&out.join("initial-state.json"), state)?;
        }
        let trigger = initial.clone().or_else(|| {
            export_state(
                setup
                    .iter()
                    .find(|c| matches!(c, Command::Datatype { .. }))
                    .unwrap(),
                &eg,
                &c,
                &port_values,
                true,
                out,
                true,
            )
            .ok()
        });
        if let Some(state) = &trigger {
            crate::pipeline::write_json(&out.join("trigger-state.json"), state)?;
        }
        let initial_facts = table_sizes(&eg, &c.datatype)?.values().sum();
        let before = eg.clone();
        Ok(Self {
            eg,
            before,
            weak_queries: BTreeMap::new(),
            has_effects: false,
            initial_facts,
            effect_components: BTreeMap::new(),
            union_effect: false,
            initial,
            trigger,
            final_state: None,
            source,
            out: out.to_path_buf(),
            text,
            setup,
            checks,
            c,
            ports: port_values,
            trace,
            producers: BTreeMap::new(),
            rulesets,
            round: 0,
            schedule: String::new(),
            report: json!({"ripen":{"state":"Running","round":0}}),
            done: false,
        })
    }
    pub(super) fn snapshot_unfinished(&self) -> Result<Json> {
        let state = export_state(
            self.setup
                .iter()
                .find(|c| matches!(c, Command::Datatype { .. }))
                .unwrap(),
            &self.eg,
            &self.c,
            &self.ports,
            true,
            &self.out,
            true,
        )?;
        let path = self.out.join("unfinished-state.json");
        crate::pipeline::write_json(&path, &state)?;
        Ok(json!({"status":"observed_only","path":path,"facts":state.rows.len()}))
    }
    pub fn advance(&mut self) -> Result<()> {
        if self.done {
            return Ok(());
        }
        self.round += 1;
        let mut updated = false;
        for ruleset in &self.rulesets {
            updated |= self.eg.step_rules_with_trace(ruleset, &self.trace)?.updated;
            self.schedule += &format!("(run {ruleset} 1)\n");
            let table_names: BTreeMap<_, _> = self.trace.table_names().into_iter().collect();
            for write in self
                .trace
                .write_events()
                .iter()
                .filter(|w| w.outcome == WriteOutcome::Inserted && w.rebuild_of.is_none())
            {
                if let Some(name) = table_names.get(&write.table) {
                    *self.effect_components.entry(name.to_string()).or_default() += 1;
                }
            }
            self.union_effect |= self
                .trace
                .union_events()
                .iter()
                .any(|u| u.displaced.is_some());
            self.has_effects |= self
                .trace
                .write_events()
                .iter()
                .any(|w| matches!(w.outcome, WriteOutcome::Inserted | WriteOutcome::Updated))
                || self
                    .trace
                    .union_events()
                    .iter()
                    .any(|u| u.displaced.is_some());
            collect(&self.eg, &self.trace, &mut self.c, &mut self.producers)?;
        }
        let f = crate::coarse_smooth::RipenFeedback {
            origin: None,
            state: if updated {
                "Running".into()
            } else {
                "Saturated".into()
            },
            round: self.round,
            rulesets: self.rulesets.clone(),
            updated,
            excluded_matches: self.c.rejected,
            scope: "whole isolated cell".into(),
        };
        self.c.boundaries.push(CaptureBoundary {
            ripen: Some(f.clone()),
            kind: "ripen-round".into(),
            round: Some(self.round),
            end: self.c.records.len(),
        });
        update_layers(&mut self.c)?;
        self.done = !updated;
        let mut exported = json!({"status":"running"});
        if self.done {
            self.eg.run_program(self.checks.clone())?;
            match export_state(
                self.setup
                    .iter()
                    .find(|c| matches!(c, Command::Datatype { .. }))
                    .unwrap(),
                &self.eg,
                &self.c,
                &self.ports,
                true,
                &self.out,
                false,
            ) {
                Ok(state) => {
                    std::fs::write(
                        self.out.join("saturated-rule-composition.json"),
                        serde_json::to_vec_pretty(&state)?,
                    )?;
                    exported = json!({"status":"exported","values":state.values.len(),"facts":state.rows.len()});
                    self.final_state = Some(state);
                }
                Err(e) => exported = json!({"status":"unavailable","reason":e.to_string()}),
            }
        }
        self.report = json!({"ripen":f,"source":self.source,"source_text":self.text,"checks":if self.done{"passed"}else{"deferred"},"checks_count":self.checks.len(),"imported_applies":self.c.records.len(),"events":self.c.events,"saturated_rule_composition":exported,"native_egraph":if self.final_state.is_some(){Some("native-egraph.dot")}else{None},"tier1":self.c.layers.report(),"tables":table_sizes(&self.eg,&self.c.datatype)?});
        crate::pipeline::write_json(&self.out.join("ripen.json"), &self.report)?;
        if self.done {
            let replay = self
                .setup
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
                + &self.schedule
                + &self
                    .checks
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n");
            std::fs::write(self.out.join("ripened.egg"), replay)?;
        }
        Ok(())
    }
}
pub fn run(source: &Path, out: &Path) -> Result<Json> {
    if out.exists() {
        return Err("ripen output already exists".into());
    }
    let mut task = Session::new(source, out)?;
    while !task.done {
        task.advance()?;
        eprintln!("[ripen] {}", task.report["ripen"]);
    }
    Ok(task.report)
}
fn table_sizes(eg: &EGraph, datatype: &str) -> Result<BTreeMap<String, usize>> {
    let mut parser = measured_engine();
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

/// Export the replayed local graph together with visibility and fixed ports.
///
/// Opaque boundary values remain opaque. This is why the exported object can be
/// compared across triggers without pretending that it recovered the complete
/// original e-graph.
/// Serialize one native EGraph exactly as the kernel sees it, no reconstruction.
fn write_native_graph(eg: &EGraph, out: &Path, stem: &str) -> Result<()> {
    let serialized = eg.serialize(egglog::SerializeConfig::default());
    if !serialized.discarded_functions.is_empty() || !serialized.truncated_functions.is_empty() {
        return Err("truncated engine serialization".into());
    }
    serialized
        .egraph
        .to_dot_file(out.join(format!("{stem}.dot")))?;
    serialized
        .egraph
        .to_svg_file(out.join(format!("{stem}.svg")))?;
    Ok(())
}

fn export_state(
    datatype: &Command,
    eg: &EGraph,
    c: &Captured,
    ports: &[(String, egglog::ArcSort, Value)],
    symbolic: bool,
    out: &Path,
    observation_only: bool,
) -> Result<crate::saturated_rule_composition::SaturatedRuleComposition> {
    use crate::saturated_rule_composition::{Row, SaturatedRuleComposition, Vertex};
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
        return Err("saturated-rule-composition export currently supports one equality datatype, i64/String/bool fields and equality-sort named ports".into());
    }
    fn constructor_expr(e: &Expr, ops: &BTreeSet<&str>) -> bool {
        match e {
            Expr::Call(_, op, args) => {
                ops.contains(op.as_str()) && args.iter().all(|e| constructor_expr(e, ops))
            }
            _ => true,
        }
    }
    let mut parser = measured_engine();
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
                if merge.as_ref().is_some_and(|e| !matches!(e,Expr::Call(_,op,args) if matches!(op.as_str(),"max"|"min") && args.len()==2 && matches!((&args[0],&args[1]),(Expr::Var(_,a),Expr::Var(_,b)) if (a=="old"&&b=="new")||(a=="new"&&b=="old")))) {
                    return Err("SaturatedRuleComposition sharing supports only min/max lattice merges; other merges remain uncertified".into());
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
        return Err("unsupported table field sort in SaturatedRuleComposition export".into());
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
            Action::Change(_, egglog::ast::Change::Subsume, op, args) => {
                ops.contains(op.as_str()) && args.iter().all(|e| constructor_expr(e, &ops))
            }
            Action::Set(_, op, args, value) => {
                ops.contains(op.as_str())
                    && args.iter().all(|e| constructor_expr(e, &ops))
                    && constructor_expr(value, &ops)
            }
            _ => false,
        });
        if !observation_only && (!body || !head) {
            return Err("SaturatedRuleComposition sharing currently certifies positive constructor/equality rules only; primitive guards/actions require an explicit semantics contract".into());
        }
    }
    let serialized = eg.serialize(egglog::SerializeConfig::default());
    if !serialized.discarded_functions.is_empty() || !serialized.truncated_functions.is_empty() {
        return Err("truncated engine serialization".into());
    }
    // Keep the graph produced by this exact native ripen EGraph. Downstream
    // supplementary figures copy these files; they do not reconstruct a generic
    // Node/Row carrier, so labels and e-class topology stay faithful to egglog.
    // The root crate enables egglog's graphviz feature, so these calls use
    // egraph-serialize's native DOT/SVG implementation directly.
    serialized
        .egraph
        .to_dot_file(out.join("native-egraph.dot"))?;
    serialized
        .egraph
        .to_svg_file(out.join("native-egraph.svg"))?;
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
    let mut hidden = BTreeSet::new();
    let mut sorted = variants.clone();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for v in &sorted {
        let mut raw = vec![];
        eg.function_for_each(&v.name, |row| raw.push((row.vals.to_vec(), row.subsumed)))?;
        for (row, subsumed) in raw {
            if row.len() != v.types.len() + 1 {
                return Err("unexpected subsumed row or arity".into());
            }
            let args = v
                .types
                .iter()
                .zip(&row)
                .map(|(t, x)| vertex(t, *x))
                .collect::<Result<Vec<_>>>()?;
            let fact = Row {
                op: v.name.clone(),
                args,
                result: vertex(name, *row.last().unwrap())?,
            };
            if rows.contains(&fact) && hidden.contains(&fact) != subsumed {
                return Err("ambiguous canonical row visibility".into());
            }
            if subsumed {
                hidden.insert(fact.clone());
            }
            rows.insert(fact);
        }
    }
    for (op, inputs, output) in &tables {
        let mut raw = vec![];
        eg.function_for_each(op, |r| raw.push((r.vals.to_vec(), r.subsumed)))?;
        for (row, subsumed) in raw {
            if row.len() != inputs.len() + 1 {
                return Err("unexpected table row".into());
            }
            let args = inputs
                .iter()
                .zip(&row)
                .map(|(t, v)| vertex(t, *v))
                .collect::<Result<Vec<_>>>()?;
            let fact = Row {
                op: op.clone(),
                args,
                result: vertex(output, *row.last().unwrap())?,
            };
            if rows.contains(&fact) && hidden.contains(&fact) != subsumed {
                return Err("ambiguous canonical row visibility".into());
            }
            if subsumed {
                hidden.insert(fact.clone());
            }
            rows.insert(fact);
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
    let mut state = SaturatedRuleComposition {
        version: 2,
        local_ids,
        scope: json!({"datatype":name,"constructors":sorted.iter().map(|v|json!({"name":v.name,"types":v.types,"cost":v.cost.map(|x|x.to_string()),"unextractable":v.unextractable})).collect::<Vec<_>>(),"rules":rules,"symbolic_boundary":symbolic,"semantics":if observation_only{"observed-constructor-graph/v1"}else{"positive-constructor-equality/v1"},"observation_only":observation_only,"ports":"fixed named globals; otherwise all facts observable"}),
        values,
        subsumed_rows: rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| hidden.contains(r).then_some(i))
            .collect(),
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
    if c.rules.iter().any(|r| {
        r.rule
            .head
            .0
            .iter()
            .any(|a| matches!(a, Action::Change(_, egglog::ast::Change::Subsume, _, _)))
    }) {
        state.scope["semantics"] = json!("constructor-table-visibility/v2");
    }
    if declarations
        .iter()
        .any(|d| matches!(d, Command::Function { merge: Some(_), .. }))
    {
        state.scope["merge_contract"] = json!("min/max lattice; exact retained table rows");
    }
    if c.rules.iter().any(|r| {
        r.rule
            .head
            .0
            .iter()
            .any(|a| matches!(a, Action::Change(..) | Action::Set(..)))
    }) {
        // Visibility and mutable tables make scheduling observable. Do not use
        // the order-insensitive theory normalization of the pure fragment.
        state.scope["operational_schedule"] = json!({"registered_rules":c.rules.iter().map(|r|r.rule.to_string()).collect::<Vec<_>>(),"sweep":"sorted ruleset names, native rule order"});
    }
    if observation_only {
        state.scope["semantics"] = json!("observed-constructor-graph/v1");
    } else {
        state
            .scope
            .as_object_mut()
            .unwrap()
            .remove("observation_only");
    }
    Ok(state)
}
