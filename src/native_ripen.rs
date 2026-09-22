//! Ripen an explicit local cell with the native engine; feed its witnessed history
//! and whole-cell fixed-point state back into Tier1. No private matcher/scheduler.
use super::*;
use crate::coarse_smooth::{RipenFeedback, RipenOrigin};

#[path = "native_ripen_entry.rs"]
mod entry;
#[path = "native_ripen_packets.rs"]
mod packets;
pub use entry::from_use;
pub(super) use entry::{prepare, prepare_members};

thread_local! {static ENGINE_CREATION: std::cell::Cell<(usize,f64)> = const {std::cell::Cell::new((0,0.))};}
pub(super) fn measured_engine()->EGraph {
    let start=Instant::now();let eg=EGraph::default();
    ENGINE_CREATION.with(|c|{let(n,t)=c.get();c.set((n+1,t+start.elapsed().as_secs_f64()));});eg
}

pub(super) fn engine_creation_snapshot()->(usize,f64){ENGINE_CREATION.with(|c|c.get())}

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

/// Replay one explicit local entry with the native engine.
///
/// Saturation here means a fixed point for this isolated entry, ruleset, and
/// round budget. It does not mean that the surrounding tier0 execution or every
/// concrete embedding has been saturated.
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
    run_observed(source, out, max_rounds, origin, artifacts, symbolic_boundary, None)
}

/// Measure cross-cell intermediate convergence without discarding native history.
pub fn probe(sources: &[std::path::PathBuf], out: &Path, rounds: usize) -> Result<Json> {
    probe_mode(sources, out, rounds, true)
}

pub fn probe_mode(sources: &[std::path::PathBuf], out: &Path, rounds: usize, enabled: bool) -> Result<Json> {
    if out.exists() { return Err("probe output already exists".into()); }
    std::fs::create_dir_all(out)?;
    let mut index = crate::ripen_convergence::Index::new(4096, 10000).with_library_path(out.join("continuations.json"));
    let mut reports = vec![];
    for (i, source) in sources.iter().enumerate() {
        let cell_started=Instant::now();
        let folder=out.join(format!("cell-{i}"));
        let mut report=run_observed(source, &folder, rounds, None, false, false, if enabled { Some(&mut index) } else { None })?;
        report["cell_seconds"]=json!(cell_started.elapsed().as_secs_f64());
        std::fs::write(folder.join("ripen.json"),serde_json::to_vec_pretty(&report)?)?;
        reports.push(report);
    }
    let mut opportunities = vec![];
    for (i,r) in reports.iter().enumerate() {
        if let Some(events) = r["convergence"]["events"].as_array() {
            if let Some(hit) = events.iter().find(|e| e["status"] == "verified") {
                let round = hit["round"].as_u64().unwrap();
                let total = r["ripen"]["round"].as_u64().unwrap();
                opportunities.push(json!({"cell":i,"first_hit":hit,"remaining_observed_rounds":total.saturating_sub(round)}));
            }
        }
    }
    index.persist_library()?;
    let catalog=if std::env::var_os("EGG_LAYOUT_PROFILE_CATALOG").is_some() {
        let dirs=(0..reports.len()).filter(|i|reports[*i]["saturated_rule_composition"]["status"]=="exported").map(|i|out.join(format!("cell-{i}"))).collect::<Vec<_>>();
        let start=Instant::now();
        let r=crate::saturated_rule_composition::catalog(&dirs,&out.join("catalog"),10000)?;
        Some(json!({"wall_seconds":start.elapsed().as_secs_f64(),"timings":r["timings"],"states":r["saturated_rule_compositions"],"comparisons":r["comparisons"].as_array().map(Vec::len)}))
    }else{None};
    let report = json!({"catalog_profile":catalog,"enabled":enabled,"index":index.report(),"opportunities":opportunities,"cells":reports});
    std::fs::write(out.join("convergence.json"), serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}

pub(super) fn run_observed(
    source: &Path, out: &Path, max_rounds: usize, origin: Option<RipenOrigin>,
    artifacts: bool, symbolic_boundary: bool,
    mut convergence: Option<&mut crate::ripen_convergence::Index>,
) -> Result<Json> {
    if max_rounds == 0 {
        return Err("ripen max-rounds must be positive".into());
    }
    if out.exists() {
        return Err("ripen output already exists".into());
    }
    let creation_baseline=engine_creation_snapshot();
    let source = source.canonicalize()?;
    let text = std::fs::read_to_string(&source)?;
    let create_started=Instant::now();
    let mut eg = measured_engine();
    let create_seconds=create_started.elapsed().as_secs_f64();
    let parse_started=Instant::now();
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
    let parse_seconds=parse_started.elapsed().as_secs_f64();
    let setup_started=Instant::now();
    eg.run_program(setup.clone())?;
    let setup_seconds=setup_started.elapsed().as_secs_f64();
    let mut port_values = vec![];
    for cmd in &setup {
        if let Command::Action(Action::Let(_, name, _)) = cmd {
            let (sort, value) = eg.eval_expr(&Expr::Var(sp(), name.clone()))?;
            port_values.push((name.clone(), sort, value));
        }
    }
    let trace = TraceSession::with_dependencies();
    let mut c = Captured {
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
    let mut producers = BTreeMap::new();
    let mut exporter = layer_view::Exporter::default();
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("entry.egg"), &text)?;
    let started = Instant::now();
    let outcome = (|| -> Result<Json> {
        let mut native_seconds=0.;
        let mut collect_seconds=0.;
        let mut tier1_seconds=0.;
        let mut schedule = String::new();
        let mut feedback = None;
        let mut convergence_events = vec![];
        let mut fingerprint = crate::ripen_convergence::Fingerprint::default();
        let mut convergence_seconds = 0.;
        // Whole isolated cells have no pending staged injections. Compare at sweep boundaries.
        let contract = serde_json::to_string(&json!({"rulesets":rulesets,"registered_rules":c.rules.iter().map(|r|r.rule.to_string()).collect::<Vec<_>>(),"pending_injections":[],"boundary":"completed sweep or initial setup"}))?;
        let mut tracker = None;
        let mut packet_updates = 0usize;
        let mut packet_boundaries=vec![json!({"round":0,"end":0})];
        let mut packet_seconds = 0.;
        let mut snapshot_exports = 0usize;
        let mut audit_exports = 0usize;
        let mut fallback_reasons = vec![];
        let reuse_enabled = !artifacts && std::env::var_os("EGG_LAYOUT_RIPEN_OBSERVE_ONLY").is_none();
        let snapshot_mode = std::env::var_os("EGG_LAYOUT_RIPEN_SNAPSHOT_PROBE").is_some();
        if convergence.is_some() && !snapshot_mode {
            snapshot_exports += 1;
            match export_state(setup.iter().find(|c|matches!(c,Command::Datatype{..})).unwrap(), &eg, &c, &port_values, symbolic_boundary) {
                Ok(s) => match packets::Tracker::new(&eg,&c,&s,&port_values) {
                    Ok(t) => tracker=Some(t), Err(e)=>fallback_reasons.push(e.to_string())
                }, Err(e)=>fallback_reasons.push(e.to_string())
            }
        }
        let mut observe = |round:usize,eg:&EGraph,c:&Captured,tracker:&Option<packets::Tracker>| {
            if let Some(index)=convergence.as_deref_mut() {
                let start=Instant::now();
                if let Some(t)=tracker {
                    let hit=index.observe_packets(&out.display().to_string(),round,&t.graph,&contract);
                    let reuse=if reuse_enabled {index.continuation(&hit,max_rounds.saturating_sub(round))}else{None};
                    convergence_events.push(hit);
                    convergence_seconds+=start.elapsed().as_secs_f64();
                    return reuse;
                } else {
                    snapshot_exports+=1;
                    match export_state(setup.iter().find(|c|matches!(c,Command::Datatype{..})).unwrap(),eg,c,&port_values,symbolic_boundary){
                        Ok(s)=>convergence_events.push(index.observe(&out.display().to_string(),round,s,&contract,&mut fingerprint)),
                        Err(e)=>convergence_events.push(json!({"status":"unsupported","reason":e.to_string()}))
                    }
                }
                convergence_seconds+=start.elapsed().as_secs_f64();
            }
            None
        };
        let mut reuse = observe(0,&eg,&c,&tracker);
        for round in 1..=max_rounds {
            if reuse.is_some() {break;}
            let mut updated = false;
            for ruleset in &rulesets {
                let stage=Instant::now();
                updated |= eg.step_rules_with_trace(ruleset, &trace)?.updated;
                native_seconds+=stage.elapsed().as_secs_f64();
                schedule += &format!("(run {} 1)\n", ruleset);
                if let Some(t)=tracker.as_mut() {
                    let start=Instant::now();
                    match t.apply(&eg,&trace) {Ok(n)=>packet_updates+=n,Err(e)=>{fallback_reasons.push(e.to_string());tracker=None;}}
                    // Packet processing time is reported separately from snapshot observations.
                    packet_seconds += start.elapsed().as_secs_f64();
                }
                let stage=Instant::now();
                collect(&eg, &trace, &mut c, &mut producers)?;
                collect_seconds+=stage.elapsed().as_secs_f64();
            }
            if std::env::var_os("EGG_LAYOUT_RIPEN_PACKET_AUDIT").is_some() {
                if let Some(t)=&tracker {
                    audit_exports+=1;
                    let actual=export_state(setup.iter().find(|c|matches!(c,Command::Datatype{..})).unwrap(),&eg,&c,&port_values,symbolic_boundary)?;
                    let comparison=crate::saturated_rule_composition::compare(&t.graph.snapshot(),&actual,10000)?;
                    if !matches!(comparison,crate::saturated_rule_composition::Comparison::Equivalent{..}) {
                        return Err(format!("packet state diverged at round {round}: {comparison:?}").into());
                    }
                }
            }
            packet_boundaries.push(json!({"round":round,"end":tracker.as_ref().map(|t|t.packets.len()).unwrap_or(0)}));
            reuse = observe(round, &eg, &c, &tracker);
            let state = if !updated {
                "Saturated"
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
            let stage=Instant::now();
            if artifacts {
                exporter.capture(&mut c, out)?;
            } else {
                update_layers(&mut c)?;
            }
            tier1_seconds+=stage.elapsed().as_secs_f64();
            feedback = Some(f);
            if !updated {
                break;
            }
        }
        drop(observe);
        let native_rounds=c.rounds.unwrap_or(0);
        let mut shared_state=None;
        let mut shared_continuation=None;
        let mut skipped=0;
        if let Some(reuse)=reuse {
            skipped=reuse.remaining;
            let donor=reuse.completion;
            let prior_round=reuse.hit["prior_round"].as_u64().unwrap() as usize;
            let donor_start=donor.report["execution_boundaries"].as_array().and_then(|bs|bs.iter().filter(|b|b["round"].as_u64().unwrap_or(0)<=prior_round as u64).last()).and_then(|b|b["end"].as_u64()).unwrap_or(0);
            shared_continuation=Some(json!({"kind":"shared_ripen_continuation","library":convergence.as_ref().and_then(|i|i.library_file()),"hit":reuse.hit,"donor_interface":reuse.interface,"current_interface":tracker.as_ref().map(|t|t.graph.snapshot()),"evidence_id":donor.evidence_id,"packet_suffix":reuse.packet_suffix,"donor_record_start":donor_start,"donor_record_end":donor.report["imported_applies"],"semantics":"shared library evidence; packet variable IDs are scoped by evidence_id; hit.value_map maps current to donor intermediate values"}));
            eg=donor.engine.clone();
            shared_state=Some(donor.state.clone());
            for _ in 0..skipped {for rs in &rulesets {schedule+=&format!("(run {} 1)\n",rs);}}
            let f=RipenFeedback{origin:origin.clone(),state:"Saturated".into(),round:native_rounds+skipped,max_rounds,rulesets:rulesets.clone(),updated:false,excluded_matches:c.rejected,scope:"whole isolated positive cell; saturation reused through verified intermediate isomorphism; skipped applications are shared donor evidence".into()};
            c.boundaries.push(CaptureBoundary{kind:"shared-continuation".into(),round:Some(f.round),end:c.records.len(),ripen:Some(f.clone())});
            c.rounds=Some(f.round);
            update_layers(&mut c)?;
            feedback=Some(f);
        }
        c.trace_seconds = started.elapsed().as_secs_f64();
        let feedback = feedback.unwrap();
        let saturated = feedback.state == "Saturated";
        let checks_started=Instant::now();
        if saturated {
            eg.run_program(checks.clone())?;
        }
        let checks_seconds=checks_started.elapsed().as_secs_f64();
        let mut replay = setup
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        replay.push('\n');
        replay.push_str(&schedule);
        if saturated {
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
        let export_started=Instant::now();
        let mut completed_state=None;
        let state_export = if saturated {
            let exported=if let Some(s)=shared_state {Ok(s)}else{export_state(
                setup
                    .iter()
                    .find(|c| matches!(c, Command::Datatype { .. }))
                    .unwrap(),
                &eg,
                &c,
                &port_values,
                symbolic_boundary || origin.as_ref().is_some_and(|o| o.symbolic_boundary),
            )};
            match exported {
                Ok(state) => {
                    completed_state=Some(state.clone());
                    std::fs::write(
                        out.join("saturated-rule-composition.json"),
                        serde_json::to_vec_pretty(&state)?,
                    )?;
                    json!({"status":"exported","file":"saturated-rule-composition.json","values":state.values.len(),"facts":state.rows.len()})
                }
                Err(e) => json!({"status":"unavailable","reason":e.to_string()}),
            }
        } else {
            json!({"status":"not_saturated"})
        };
        let export_seconds=export_started.elapsed().as_secs_f64();
        let tables_started=Instant::now();
        let tables=table_sizes(&eg,&c.datatype)?;
        let tables_seconds=tables_started.elapsed().as_secs_f64();
        let current=engine_creation_snapshot();
        let creations=(current.0-creation_baseline.0,current.1-creation_baseline.1);
        // Tier1 is the actual importer/Use builder, populated during every sweep.
        let mut tier1=c.layers.report();
        tier1["shared_continuation"]=json!(shared_continuation);
        let report = json!({"timings":{"initial_engine_seconds":create_seconds,"parse_normalize_seconds":parse_seconds,"setup_seconds":setup_seconds,"native_saturation_seconds":native_seconds,"trace_import_seconds":collect_seconds,"tier1_seconds":tier1_seconds,"checks_seconds":checks_seconds,"state_export_seconds":export_seconds,"table_stats_seconds":tables_seconds,"engine_creations":creations.0,"all_engine_creation_seconds":creations.1},"packet_boundaries":packet_boundaries,"execution_boundaries":c.boundaries,"native_rounds":native_rounds,"ripen":feedback,"checks":if saturated{"passed"}else{"deferred"},
            "checks_count":checks.len(),"rules":c.rules.iter().map(|r|r.rule.to_string()).collect::<Vec<_>>(),
            "source":source,"source_text":text,"events":c.events,"imported_applies":c.records.len(),
            "saturated_rule_composition":state_export,"tables":tables,"tier1":tier1,"round_manifest":if artifacts {Some("rounds/manifest.json")}else{None},
            "history":if artifacts {Some("history.json")}else{None},"seconds":c.trace_seconds,"fractal_summaries_used":0,"convergence":{"events":convergence_events,"seconds":convergence_seconds,"actual_rounds_skipped":skipped,"packet_updates":packet_updates,"packet_seconds":packet_seconds,"snapshot_exports":snapshot_exports,"audit_exports":audit_exports,"fallback_reasons":fallback_reasons}});
        std::fs::write(out.join("ripen.json"), serde_json::to_vec_pretty(&report)?)?;
        // Do not cache reused results recursively: donor evidence stays one hop.
        if reuse_enabled && skipped==0 && saturated && tracker.is_some() {
            if let (Some(index),Some(state))=(convergence.as_deref_mut(),completed_state) {
                let t=tracker.as_mut().unwrap();
                let space=t.graph.binding_space();
                index.finish_packets(&out.display().to_string(),&eg,state,report.clone(),std::mem::take(&mut t.packets),space);
            }
        }
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
fn export_state(
    datatype: &Command,
    eg: &EGraph,
    c: &Captured,
    ports: &[(String, egglog::ArcSort, Value)],
    symbolic: bool,
) -> Result<crate::saturated_rule_composition::SaturatedRuleComposition> {
    use crate::saturated_rule_composition::{SaturatedRuleComposition, Row, Vertex};
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
        if !body || !head {
            return Err("SaturatedRuleComposition sharing currently certifies positive constructor/equality rules only; primitive guards/actions require an explicit semantics contract".into());
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
        scope: json!({"datatype":name,"constructors":sorted.iter().map(|v|json!({"name":v.name,"types":v.types,"cost":v.cost.map(|x|x.to_string()),"unextractable":v.unextractable})).collect::<Vec<_>>(),"rules":rules,"symbolic_boundary":symbolic,"semantics":"positive-constructor-equality/v1","ports":"fixed named globals; otherwise all facts observable"}),
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
    Ok(state)
}
