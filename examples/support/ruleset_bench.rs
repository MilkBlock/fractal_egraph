//! Benchmark one original schedule packet from a real, live replay checkpoint.
//! Reuses rule_repl's scheduler; neither parse/source rewriting nor graph export
//! is charged to the timed execution. Diagnostics start from the same clone.
use egglog::{CommandOutput, EGraph, Value, ast::Command};
use egglog_reports::RunReport;
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, time::Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type TableRows = BTreeMap<Vec<Value>, (Value, bool)>;
type Snapshot = BTreeMap<String, TableRows>;

fn snapshot(eg: &EGraph) -> Result<Snapshot> {
    let mut out = BTreeMap::new();
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        let mut rows = BTreeMap::new();
        eg.function_for_each(&name, |r| {
            let v: Vec<_> = r
                .vals
                .iter()
                .zip(f.schema().input.iter().chain([&f.schema().output]))
                .map(|(v, s)| eg.get_canonical_value(*v, s))
                .collect();
            rows.insert(v[..v.len() - 1].to_vec(), (*v.last().unwrap(), r.subsumed));
        })?;
        out.insert(name, rows);
    }
    Ok(out)
}
fn structure(eg: &EGraph, snap: &Snapshot) -> Json {
    let mut classes = BTreeMap::<String, BTreeMap<Value, usize>>::new();
    let mut tables = BTreeMap::new();
    for (name, rows) in snap {
        let f = eg.get_function(name).unwrap();
        let visible = rows.values().filter(|(_, hidden)| !hidden).count();
        tables.insert(name, json!({"stored_rows":rows.len(),"visible_rows":visible,"constructor":f.is_constructor(),"output_sort":f.schema().output.name()}));
        // Keep output sorts separate. Flattened relations may have constructor
        // encodings (e.g. ___PotentialExtractedExprSort); do not pool their
        // trivial singleton classes with Expr/Term density.
        if f.is_constructor() && !f.is_let_binding() && !f.schema().output.name().starts_with('@') {
            for (v, hidden) in rows.values() {
                if !hidden {
                    *classes
                        .entry(f.schema().output.name().into())
                        .or_default()
                        .entry(*v)
                        .or_default() += 1;
                }
            }
        }
    }
    let sorts: BTreeMap<_,_> = classes.into_iter().map(|(s,c)| {
        let nodes:usize = c.values().sum(); let singleton=c.values().filter(|n| **n==1).count();
        (s,json!({"visible_constructor_enodes":nodes,"eclasses":c.len(),"singleton_eclasses":singleton,"enodes_per_eclass":nodes as f64/c.len().max(1) as f64}))
    }).collect();
    json!({"table_rows":eg.num_tuples(),"constructor_sorts":sorts,"tables":tables})
}
fn reports(messages: Vec<CommandOutput>) -> RunReport {
    let mut out = RunReport::default();
    for m in messages {
        if let CommandOutput::RunSchedule(r) = m {
            out.union(r);
        }
    }
    out
}
fn summary(r: &RunReport) -> Json {
    let mut rules:Vec<_>=r.search_and_apply_time_per_rule.iter().map(|(name,t)|json!({"rule":name,"matches":r.num_matches_per_rule.get(name).copied().unwrap_or(0),"search_and_apply_seconds":t.as_secs_f64()})).collect();
    rules.sort_by(|a, b| {
        b["search_and_apply_seconds"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["search_and_apply_seconds"].as_f64().unwrap())
    });
    json!({"iterations":r.iterations.len(),"can_stop":r.can_stop,"matches":r.num_matches_per_rule.values().sum::<usize>(),
        "search_and_apply_seconds":r.search_and_apply_time_per_ruleset.values().map(|t|t.as_secs_f64()).sum::<f64>(),
        "insert_merge_seconds":r.merge_time_per_ruleset.values().map(|t|t.as_secs_f64()).sum::<f64>(),
        "rebuild_seconds":r.rebuild_time_per_ruleset.values().map(|t|t.as_secs_f64()).sum::<f64>(),
        "rules":rules,"rulesets":r.search_and_apply_time_per_ruleset.iter().map(|(n,t)|(n.to_string(),t.as_secs_f64())).collect::<BTreeMap<_,_>>()})
}
fn stages(r: &RunReport) -> Json {
    let mut by_rule = BTreeMap::<String, (usize, usize, usize)>::new();
    for iteration in &r.iterations {
        for (rule, reports) in &iteration.rule_set_report.rule_reports {
            for report in reports {
                if let Some(plan) = &report.plan {
                    for (_, stats, _) in &plan.stages {
                        if let Some(s) = stats {
                            let row = by_rule.entry(rule.to_string()).or_default();
                            row.0 += 1;
                            row.1 += s.num_candidates;
                            row.2 += s.num_succeeded;
                        }
                    }
                }
            }
        }
    }
    if by_rule.is_empty() {
        return Json::Null; // Missing native stage counters are not zero work.
    }
    json!(by_rule.into_iter().map(|(name,(stages,candidates,successes))|(name,json!({"stage_reports":stages,"stage_candidates":candidates,"stage_successes":successes}))).collect::<BTreeMap<_,_>>())
}
fn eligibility(eg: &EGraph, r: &RunReport) -> Result<Json> {
    let mut copy = eg.clone();
    let mut rows = vec![];
    for (name, time) in &r.search_and_apply_time_per_rule {
        let mut reason = "not a direct rule".to_string();
        let mut accepted = false;
        let mut atoms = None;
        for set in r.search_and_apply_time_per_ruleset.keys() {
            match copy.prepare_cell_action(set, name) {
                Ok(sink) => {
                    accepted = true;
                    atoms = Some(sink.atoms.len());
                    reason="native sink accepts syntax; baked template coverage has NOT been established".into();
                    break;
                }
                Err(e) => {
                    let msg = e.to_string();
                    if !msg.contains("unknown assisted rule") {
                        reason = msg;
                        break;
                    }
                }
            }
        }
        rows.push(json!({"rule":name,"logical_matches":r.num_matches_per_rule.get(name).copied().unwrap_or(0),"search_and_apply_seconds":time.as_secs_f64(),"sink_supported":accepted,"atoms":atoms,"reason":reason}));
    }
    rows.sort_by(|a, b| {
        b["search_and_apply_seconds"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["search_and_apply_seconds"].as_f64().unwrap())
    });
    Ok(json!({"resident_executed":false,"baked_cell_coverage":null,"rules":rows}))
}

pub fn benchmark(base: &EGraph, commands: &[Command], label: &str) -> Result<Json> {
    let before = snapshot(base)?;
    let before_structure = structure(base, &before);
    let t = Instant::now();
    let mut timed = base.clone();
    let clone_seconds = t.elapsed().as_secs_f64();
    let commands_owned = commands.to_vec();
    let old = egglog::set_probe_counts_enabled(false);
    let counters_before = egglog::probe_counts();
    let unions_before = timed.native_union_count();
    let tick = Instant::now();
    let result = timed.run_program(commands_owned);
    let wall = tick.elapsed().as_secs_f64();
    egglog::set_probe_counts_enabled(old);
    let control = reports(result?);
    if egglog::probe_counts() != counters_before {
        return Err("timed replay unexpectedly collected probe counters".into());
    }
    let after = snapshot(&timed)?;
    let after_structure = structure(&timed, &after);
    // This is observable boundary change, not inserted writes / rule effect counts.
    let visible_before = before
        .values()
        .flat_map(|r| r.values())
        .filter(|(_, h)| !h)
        .count();
    let visible_after = after
        .values()
        .flat_map(|r| r.values())
        .filter(|(_, h)| !h)
        .count();
    let mut diagnostic = base.clone();
    diagnostic.enable_stage_info_reporting();
    #[cfg(feature = "native-read-profile")]
    diagnostic.reset_join_read_profile(true);
    let old = egglog::set_probe_counts_enabled(true);
    let counts_before: BTreeMap<_, _> = egglog::probe_counts().into_iter().collect();
    let result = diagnostic.run_program(commands.to_vec());
    egglog::set_probe_counts_enabled(old);
    let diag = reports(result?);
    let counters: BTreeMap<_, _> = egglog::probe_counts()
        .into_iter()
        .map(|(k, v)| (k, v - counts_before[k]))
        .collect();
    let diag_snapshot = snapshot(&diagnostic)?;
    if after != diag_snapshot {
        return Err(
            "timed and diagnostic replay have different final rows, equivalences or visibility"
                .into(),
        );
    }
    if control.num_matches_per_rule != diag.num_matches_per_rule {
        return Err("diagnostics changed per-rule match counts".into());
    }
    if before != snapshot(base)? {
        return Err("benchmark modified parent checkpoint".into());
    }
    #[cfg(not(feature = "native-read-profile"))]
    let reads = Json::Null;
    #[cfg(feature = "native-read-profile")]
    let reads=json!(diagnostic.join_read_profile().into_iter().map(|(name,p)|(name,p.map(|p|json!({"scan_calls":p.scan_calls,"scan_rows":p.scan_rows,"refine_rows":p.refine_rows,"key_lookups":p.key_lookups})))).collect::<Vec<_>>());
    Ok(
        json!({"scope":"one original native schedule packet from a production-derived checkpoint; one timed replay plus separate diagnostics; same fork, not pristine or GPU acceleration",
        "label":label,"schedule":commands.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "wall_seconds":wall,"clone_seconds_excluded":clone_seconds,"native":summary(&control),
        "before":before_structure,"after":after_structure,
        "visible_rows_net_change":visible_after as i64-visible_before as i64,
        "successful_union_displacements":timed.native_union_count()-unions_before,
        "diagnostic":{"probe_counters":counters,"stage_work_by_rule":stages(&diag),"table_reads":reads},
        "resident_support":eligibility(base,&control)?,
        "checks":{"exact_final_rows_equivalence_and_visibility":true,"same_rule_match_counts":true,"parent_unchanged":true,"timing_probe_counter_increments":0},
        "limits":"No extraction in this isolated packet. Search/apply includes RHS and is not pure Join time. Candidate visits repeat across stages, not a single rejection funnel. Net rows and union displacements are distinct; neither measures per-match new enodes."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_replay_preserves_parent_and_separates_counters() {
        EGraph::set_num_threads(1);
        let mut e = EGraph::default();
        e.parse_and_run_program(None,"(datatype E (N i64)) (relation P (i64 i64)) (ruleset grow) (N 0) (P 0 1) (P 1 2) (P 2 3) (P 3 4) (P 4 5) (P 5 6) (rule ((N n) (P n m)) ((N m)) :ruleset grow)").unwrap();
        let cmds = e
            .parse_program(None, "(run-schedule (saturate grow))")
            .unwrap();
        let r = benchmark(&e, &cmds, "test").unwrap();
        assert_eq!(r["after"]["tables"]["N"]["stored_rows"], 7);
        assert_eq!(r["native"]["matches"], 6);
        assert_eq!(e.get_size("N"), 1);
        assert!(
            r["diagnostic"]["probe_counters"]["probes"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
}
