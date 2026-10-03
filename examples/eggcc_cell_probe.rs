//! Diagnostic only: native local fixed points under eggcc's actual optimizer schedule.
//! Does not register cells or certify a symbolic library for arbitrary contexts.
use egg_layout::{
    pipeline::Result,
    saturated_rule_composition::{
        Comparison, Row, SaturatedRuleComposition as State, Vertex, compare,
    },
    visual_rule,
};
use egglog::{CommandOutput, EGraph, TraceSession, ast::Command};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};
fn declaration(c: &Command) -> bool {
    matches!(
        c,
        Command::Datatype { .. }
            | Command::Datatypes { .. }
            | Command::Sort { .. }
            | Command::Constructor { .. }
            | Command::Function { .. }
            | Command::Relation { .. }
            | Command::AddRuleset(..)
            | Command::Rule { .. }
    )
}
fn tables(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            Command::Constructor { name, .. }
            | Command::Function { name, .. }
            | Command::Relation { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}
fn size(eg: &EGraph, names: &[String]) -> usize {
    names.iter().map(|n| eg.get_size(n)).sum()
}
fn counts(eg: &EGraph, names: &[String]) -> BTreeMap<String, usize> {
    names
        .iter()
        .filter_map(|n| {
            let k = eg.get_size(n);
            (k > 0).then(|| (n.clone(), k))
        })
        .collect()
}
fn snapshot(eg: &EGraph, markers: &BTreeSet<String>, scope: &str) -> Result<State> {
    let serialized = eg.serialize(egglog::SerializeConfig::default());
    if !serialized.is_complete() {
        return Err("incomplete native serialization".into());
    }
    let g = serialized.egraph;
    let mut raw = vec![];
    let mut boundaries = vec![];
    let mut used = BTreeSet::new();
    for n in g.nodes.values() {
        if markers.contains(&n.op) {
            boundaries.push(n.eclass.clone());
            used.insert(n.eclass.clone());
            continue;
        }
        let ty = g.class_data[&n.eclass]
            .typ
            .as_deref()
            .ok_or("missing sort")?;
        let sort = eg.get_sort_by_name(ty).ok_or("missing native sort")?;
        // Native serialization expands Set Expr using unordered membership.
        if sort.is_container_sort() {
            if n.op != "set-of" {
                return Err(format!("probe does not compare container {ty}/{}", n.op).into());
            }
            raw.push((format!("@set:{ty}"), vec![], n.eclass.clone(), false));
            for child in &n.children {
                let c = g.nodes[child].eclass.clone();
                raw.push((format!("@member:{ty}"), vec![c], n.eclass.clone(), false));
            }
        } else if eg.get_function(&n.op).is_some() {
            raw.push((
                n.op.clone(),
                n.children
                    .iter()
                    .map(|c| g.nodes[c].eclass.clone())
                    .collect(),
                n.eclass.clone(),
                n.subsumed,
            ));
        }
    }
    for (_, args, result, _) in &raw {
        used.extend(args.iter().cloned());
        used.insert(result.clone());
    }
    let ids: BTreeMap<_, _> = used
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, c)| (c, i))
        .collect();
    let mut values = vec![];
    for c in &used {
        let ty = g.class_data[c].typ.clone().ok_or("missing sort")?;
        let sort = eg.get_sort_by_name(&ty).unwrap();
        let literal = if sort.is_eq_sort() || sort.is_container_sort() {
            None
        } else {
            Some(
                g.nodes
                    .values()
                    .find(|n| &n.eclass == c && eg.get_function(&n.op).is_none())
                    .ok_or("missing primitive representation")?
                    .op
                    .clone(),
            )
        };
        values.push(Vertex { sort: ty, literal });
    }
    let mut rows: BTreeMap<Row, bool> = BTreeMap::new();
    for (op, args, result, hidden) in raw {
        let row = Row {
            op,
            args: args.iter().map(|c| ids[c]).collect(),
            result: ids[&result],
        };
        rows.entry(row)
            .and_modify(|h| *h &= hidden)
            .or_insert(hidden);
    }
    // Keep the number and aliases of boundary slots, while permitting renaming.
    for c in boundaries {
        let slot = values.len();
        values.push(Vertex {
            sort: format!("@input:{}", values[ids[&c]].sort),
            literal: None,
        });
        rows.insert(
            Row {
                op: "@binding".into(),
                args: vec![ids[&c]],
                result: slot,
            },
            false,
        );
    }
    let hidden = rows
        .values()
        .enumerate()
        .filter_map(|(i, h)| h.then_some(i))
        .collect();
    Ok(State {
        version: 2,
        local_ids: vec![],
        scope: json!({"probe_environment":scope,"semantics":"observed snapshot isomorphism only; literals retained; input slots may be renamed; no general merge proof"}),
        values,
        rows: rows.into_keys().collect(),
        subsumed_rows: hidden,
        ports: BTreeMap::new(),
    })
}
fn intern(s: State, groups: &mut Vec<State>) -> Result<usize> {
    for (i, g) in groups.iter().enumerate() {
        if matches!(compare(&s, g)?, Comparison::Equivalent { .. }) {
            return Ok(i);
        }
    }
    let i = groups.len();
    groups.push(s);
    Ok(i)
}
fn saturate(eg: &mut EGraph, schedule: &[Command], names: &[String], ms: u64) -> Result<Json> {
    let trace = TraceSession::with_effects();
    let started = Instant::now();
    let mut passes = 0;
    let mut matches = 0;
    let mut inserted = 0;
    let mut union = false;
    loop {
        let facts = size(eg, names);
        if facts >= 200_000 || started.elapsed() >= Duration::from_millis(ms) {
            return Ok(
                json!({"status":"unfinished","passes":passes,"matches":matches,"inserted_receipts":inserted,"union":union,"seconds":started.elapsed().as_secs_f64(),"stop":"probe observation limit"}),
            );
        }
        trace.set_execution_limits(
            Some(Duration::from_millis(ms).saturating_sub(started.elapsed())),
            Some(200_000 - facts),
        );
        let result = eg.run_program_with_trace(schedule.to_vec(), &trace);
        let stop = trace.execution_stop_reason();
        trace.clear_execution_limits();
        matches += trace.take_effect_match_count();
        let (counts, u, _) = trace.effect_summary();
        inserted += counts.values().sum::<usize>();
        union |= u;
        trace.drain_completed();
        passes += 1;
        let outputs = match result {
            Ok(x) => x,
            Err(e) => {
                return Ok(
                    json!({"status":"error","reason":e.to_string(),"passes":passes,"matches":matches}),
                );
            }
        };
        if let Some(stop) = stop {
            return Ok(
                json!({"status":"unfinished","stop":stop,"passes":passes,"matches":matches,"inserted_receipts":inserted,"seconds":started.elapsed().as_secs_f64()}),
            );
        }
        let changed = outputs
            .iter()
            .any(|o| matches!(o,CommandOutput::RunSchedule(r) if r.updated));
        if !changed {
            return Ok(
                json!({"status":"fixed_point","passes":passes,"matches":matches,"inserted_receipts":inserted,"union":union,"seconds":started.elapsed().as_secs_f64()}),
            );
        }
    }
}
// Attribution ablation only. Never used to certify an original-environment cell.
fn without_swap_if(schedule: egglog::ast::Schedule) -> egglog::ast::Schedule {
    use egglog::ast::GenericSchedule::*;
    match schedule {
        Run(span, c) if c.ruleset == "swap-if" => Sequence(span, vec![]),
        Saturate(span, s) => Saturate(span, Box::new(without_swap_if(*s))),
        Repeat(span, n, s) => Repeat(span, n, Box::new(without_swap_if(*s))),
        Sequence(span, ss) => Sequence(span, ss.into_iter().map(without_swap_if).collect()),
        s => s,
    }
}
fn main() -> Result {
    let args: Vec<_> = std::env::args().collect();
    let analysis = PathBuf::from(
        args.get(1)
            .ok_or("ANALYSIS_DIR OUTPUT_DIR [milliseconds]")?,
    );
    let out = PathBuf::from(&args[2]);
    let ms = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(50);
    std::fs::create_dir(&out)?;
    let report: Json = serde_json::from_slice(&std::fs::read(analysis.join("analysis.json"))?)?;
    let history: Json = serde_json::from_slice(&std::fs::read(analysis.join("history.json"))?)?;
    let mut parser = EGraph::default();
    let source = std::fs::read_to_string(analysis.join("source.egg"))?;
    let original = visual_rule::surface_program(parser.parse_program(None, &source)?);
    let schedules: Vec<_> = original
        .iter()
        .filter(|c| matches!(c, Command::RunSchedule(..)))
        .cloned()
        .collect();
    if schedules.len() != 2
        || !schedules[0].to_string().contains("initialization")
        || schedules[1].to_string().contains("initialization")
    {
        return Err("expected separate initialization and optimizer schedules".into());
    }
    let ablation: Vec<_> = schedules[1..]
        .iter()
        .map(|c| {
            let Command::RunSchedule(s) = c else {
                unreachable!()
            };
            Command::RunSchedule(without_swap_if(s.clone()))
        })
        .collect();
    let mut initializer = BTreeSet::new();
    let mut rule_codes = vec![];
    for (i, r) in history["rules"].as_array().unwrap().iter().enumerate() {
        let commands = parser.parse_program(None, r["code"].as_str().unwrap())?;
        let Command::Rule { rule } = &commands[0] else {
            return Err("not a rule".into());
        };
        if rule.ruleset == "initialization" {
            initializer.insert(i);
        }
        rule_codes.push(rule.to_string());
    }
    let prepared: Vec<_> = report["saturated_rule_composition"]["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|j| j.get("origin").is_some())
        .collect();
    let selected: Vec<_> = prepared
        .iter()
        .filter(|j| {
            !j["origin"]["members"].as_array().unwrap().iter().any(|i| {
                initializer.contains(
                    &(history["records"][i.as_u64().unwrap() as usize]["rule"]
                        .as_u64()
                        .unwrap() as usize),
                )
            })
        })
        .copied()
        .collect();
    let first = selected
        .first()
        .ok_or("no prepared non-initialization cases")?;
    let file = analysis
        .join("saturated-rule-composition/cells")
        .join(format!(
            "CSUnit-{:06}",
            first["candidate_id"].as_u64().unwrap()
        ))
        .join("entry.egg");
    let commands = parser.parse_program(None, &std::fs::read_to_string(file)?)?;
    let environment: Vec<_> = commands
        .iter()
        .filter(|c| declaration(c))
        .cloned()
        .collect();
    let env_code = environment
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let names = tables(&environment);
    let mut base = EGraph::default();
    base.run_program(environment)?;
    let markers: BTreeSet<_> = first["origin"]["parameter_markers"]
        .as_object()
        .unwrap()
        .values()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    let mut empty = base.clone();
    let control = saturate(&mut empty, &schedules[1..], &names, ms)?;
    let empty_counts = counts(&empty, &names);
    let mut initialized = base.clone();
    initialized.run_program(vec![schedules[0].clone()])?;
    let initialized_counts = counts(&initialized, &names);
    let mut groups = vec![];
    let mut initial_groups = vec![];
    let mut rows = vec![];
    let mut graft_case: Option<(EGraph, Vec<Command>, u64, String)> = None;
    for (index, j) in selected.iter().enumerate() {
        let id = j["candidate_id"].as_u64().unwrap();
        let members = j["origin"]["members"].as_array().unwrap();
        let family: Vec<_> = members
            .iter()
            .map(|v| {
                history["records"][v.as_u64().unwrap() as usize]["rule"]
                    .as_u64()
                    .unwrap()
            })
            .collect();
        let file = analysis
            .join("saturated-rule-composition/cells")
            .join(format!("CSUnit-{id:06}"))
            .join("entry.egg");
        let commands = parser.parse_program(None, &std::fs::read_to_string(file)?)?;
        if commands
            .iter()
            .filter(|c| declaration(c))
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            != env_code
        {
            return Err("case changed rule environment".into());
        }
        let actions: Vec<_> = commands.into_iter().filter(|c| !declaration(c)).collect();
        let mut eg = base.clone();
        let seed = eg.run_program(actions.clone());
        let mut row = json!({"id":id,"family":family,"interface_slots":j["origin"]["parameters"].as_array().unwrap().len(),"members":members});
        if let Err(e) = seed {
            row["result"] = json!({"status":"seed_error","reason":e.to_string()});
            rows.push(row);
            continue;
        }
        row["initial_tables"] = json!(counts(&eg, &names));
        row["initial_rows"] = json!(size(&eg, &names));
        row["initial_group"] = json!(intern(
            snapshot(&eg, &markers, "eggcc-optimizer")?,
            &mut initial_groups
        )?);
        let result = saturate(&mut eg, &schedules[1..], &names, ms)?;
        row["final_rows"] = json!(size(&eg, &names));
        row["final_tables"] = json!(counts(&eg, &names));
        if result["status"] == "fixed_point" {
            row["final_group"] = json!(intern(
                snapshot(&eg, &markers, "eggcc-optimizer")?,
                &mut groups
            )?);
        }
        if graft_case.is_none()
            && result["status"] == "fixed_point"
            && family.len() == 1
            && j["origin"]["parameters"].as_array().unwrap().len() == 1
            && j["origin"]["parameters"][0]["sort"] == "Expr"
        {
            let parsed = parser.parse_program(None, &rule_codes[family[0] as usize])?;
            if let Command::Rule { rule } = &parsed[0] {
                if matches!(rule.body.as_slice(),[egglog::ast::Fact::Fact(egglog::ast::Expr::Call(_,op,_))] if op=="Get")
                {
                    graft_case = Some((
                        eg.clone(),
                        actions,
                        id,
                        format!(
                            "({} 0)",
                            j["origin"]["parameter_markers"]["Expr"].as_str().unwrap()
                        ),
                    ));
                }
            }
        }
        if result["status"] == "unfinished" {
            let mut retry = base.clone();
            // This counterfactual disables one schedule component, not its rules.
            let code = parser.parse_program(
                None,
                &std::fs::read_to_string(
                    analysis
                        .join("saturated-rule-composition/cells")
                        .join(format!("CSUnit-{id:06}"))
                        .join("entry.egg"),
                )?,
            )?;
            retry.run_program(code.into_iter().filter(|c| !declaration(c)).collect())?;
            row["ablation_without_swap_if"] = saturate(&mut retry, &ablation, &names, ms)?;
            row["ablation_rows"] = json!(size(&retry, &names));
        }
        row["result"] = result;
        rows.push(row);
        eprintln!(
            "[probe] {}/{} id={} {}",
            index + 1,
            selected.len(),
            id,
            rows.last().unwrap()["result"]["status"]
        );
    }
    for (i, g) in groups.iter().enumerate() {
        std::fs::write(
            out.join(format!("group-{i}.json")),
            serde_json::to_vec_pretty(g)?,
        )?;
    }
    let mut grafts = vec![];
    if let Some((cell, actions, id, input)) = graft_case {
        let expr_sort = initialized.get_sort_by_name("Expr").unwrap().clone();
        let mut reads = vec![];
        initialized.function_for_each("Get", |r| reads.push(r.vals[0]))?;
        let mut terms = BTreeSet::new();
        for value in reads {
            if let Ok((term, _)) = initialized.extract_value_to_string(&expr_sort, value) {
                terms.insert(term);
            }
        }
        for term in terms {
            let code = format!("(union {input} {term})");
            let injection = parser.parse_program(None, &code)?;
            let mut warm = cell.clone();
            let before = size(&warm, &names);
            warm.run_program(injection.clone())?;
            let injected = size(&warm, &names);
            let warm_result = saturate(&mut warm, &schedules[1..], &names, ms)?;
            let mut fresh = base.clone();
            fresh.run_program(actions.clone())?;
            fresh.run_program(injection)?;
            let fresh_result = saturate(&mut fresh, &schedules[1..], &names, ms)?;
            let equal = if warm_result["status"] == "fixed_point"
                && fresh_result["status"] == "fixed_point"
            {
                Some(matches!(
                    compare(
                        &snapshot(&warm, &markers, "eggcc-optimizer")?,
                        &snapshot(&fresh, &markers, "eggcc-optimizer")?
                    )?,
                    Comparison::Equivalent { .. }
                ))
            } else {
                None
            };
            grafts.push(json!({"cell_id":id,"concrete_source":"Get operand extracted from actual eggcc input-initialization graph","binding":term,"before_rows":before,"after_injection_rows":injected,"after_ripen_rows":size(&warm,&names),"warm":warm_result,"fresh":fresh_result,"same_final_snapshot":equal}));
        }
    }
    let summary = json!({"scope":"standalone native-engine probe; preserved concrete scalar payloads, symbolic equality-sort boundary slots from validated entries; exact finite snapshot comparison, not production cell reuse or universal symbolic equivalence","milliseconds":ms,"fact_limit":200000,"prepared_jobs":prepared.len(),"excluded_with_input_initializer":prepared.len()-selected.len(),"selected_jobs":selected.len(),"input_initializer_rules":initializer,"background_control":control,"background_tables":empty_counts,"input_initializer_tables":initialized_counts,"initial_groups":initial_groups.len(),"final_groups":groups.len(),"rows":rows,"grafts":grafts,"rules":rule_codes});
    std::fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    println!("selected={} final_groups={}", selected.len(), groups.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn graph(facts: &str) -> State {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None,&format!("(sort E) (sort S (Set E)) (constructor A () E) (constructor B () E) (constructor Box (S) E) (constructor Num (i64) E) {facts}")).unwrap();
        snapshot(&eg, &BTreeSet::new(), "test").unwrap()
    }
    #[test]
    fn set_members_are_unordered_but_not_erased() {
        assert!(matches!(
            compare(
                &graph("(Box (set-of (A) (B)))"),
                &graph("(Box (set-of (B) (A)))")
            )
            .unwrap(),
            Comparison::Equivalent { .. }
        ));
        assert!(matches!(
            compare(
                &graph("(Box (set-of (A) (B)))"),
                &graph("(Box (set-of (A)))")
            )
            .unwrap(),
            Comparison::Different { .. }
        ));
    }
    #[test]
    fn boundary_aliases_are_not_erased() {
        let build = |facts: &str| {
            let mut eg = EGraph::default();
            eg.parse_and_run_program(None, &format!("(datatype E (P i64) (Pair E E)) {facts}"))
                .unwrap();
            snapshot(&eg, &BTreeSet::from(["P".into()]), "test").unwrap()
        };
        assert!(matches!(
            compare(
                &build("(Pair (P 0) (P 1))"),
                &build("(P 1) (Pair (P 0) (P 0))")
            )
            .unwrap(),
            Comparison::Different { .. }
        ));
    }
    #[test]
    fn primitive_values_are_not_erased() {
        assert!(matches!(
            compare(&graph("(Num 7)"), &graph("(Num 8)")).unwrap(),
            Comparison::Different { .. }
        ));
    }
}
