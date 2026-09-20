//! Ripen an explicit local cell with the native engine; feed its witnessed history
//! and whole-cell fixed-point state back into Tier1. No private matcher/scheduler.
use super::*;
use crate::coarse_smooth::{RipenFeedback, RipenOrigin};

#[path = "native_ripen_entry.rs"]
mod entry;
pub use entry::from_use;

fn monotone(a: &Action) -> bool {
    matches!(a, Action::Let(..) | Action::Expr(..) | Action::Union(..))
}

pub fn run(source: &Path, out: &Path, max_rounds: usize) -> Result<Json> {
    run_with_origin(source, out, max_rounds, None)
}
fn run_with_origin(
    source: &Path,
    out: &Path,
    max_rounds: usize,
    origin: Option<RipenOrigin>,
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
    let commands = eg.parse_program(Some(source.display().to_string()), &text)?;
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
                if !rule.head.0.iter().all(monotone) { return Err("ripen rejects non-monotone/set actions".into()); }
                if !names.insert(rule.name.clone()) { return Err("ripen rule names must be unique".into()); }
                rulesets.insert(rule.ruleset.clone());
                let calls = crate::visual_rule::positions(rule).into_iter().map(|(span,path,_)| {
                    let expression=crate::visual_rule::expression_at(rule,&path).unwrap().clone();
                    (Arc::from(span),(path,expression))
                }).collect();
                rules.push(RuleInfo{rule:rule.clone(),calls});
            }
            Command::AddRuleset(..) => {}
            Command::Action(a) if monotone(a) => {}
            Command::Check(..) => { checks.push(command); continue; }
            _ => return Err(format!("unsupported ripen command: {command}; use an explicit entry file without run/include/function/deletion commands").into()),
        }
        setup.push(command);
    }
    let (datatype_name, datatype) = datatype.ok_or("ripen requires a datatype")?;
    let rulesets: Vec<_> = rulesets.into_iter().collect();
    eg.run_program(setup.clone())?;
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
            exporter.capture(&mut c, out)?;
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
        history::save(&out.join("history.json"), &source, &c)?;
        // Tier1 is the actual importer/Use builder, populated during every sweep.
        let report = json!({"ripen":feedback,"checks":if closed{"passed"}else{"deferred"},
            "checks_count":checks.len(),"rules":c.rules.iter().map(|r|r.rule.to_string()).collect::<Vec<_>>(),
            "source":source,"source_text":text,"events":c.events,"imported_applies":c.records.len(),
            "tables":table_sizes(&eg, &c.datatype)?,"tier1":c.layers.report(),"round_manifest":"rounds/manifest.json",
            "history":"history.json","seconds":c.trace_seconds,"fractal_summaries_used":0});
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
    let Command::Datatype { variants, .. } = &cmds[0] else {
        return Err("expected datatype".into());
    };
    let mut sizes = BTreeMap::new();
    for variant in variants {
        let mut count = 0;
        eg.function_for_each(&variant.name, |_| count += 1)?;
        sizes.insert(variant.name.clone(), count);
    }
    Ok(sizes)
}
