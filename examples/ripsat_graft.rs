//! Differential native-engine experiment; not automatic production reuse.
use egg_layout::{
    pipeline::Result,
    saturated_rule_composition::{Comparison, SaturatedRuleComposition as State, compare},
    visual_rule,
};
use egglog::{CommandOutput, EGraph, TraceSession, ast::Command};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::{Duration, Instant},
};
#[path = "../experiments/eggcc_cell_probe/snapshot.rs"]
mod snapshot;
#[derive(Clone, Deserialize, Serialize)]
struct Graft {
    id: String,
    steps: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
struct Family {
    id: String,
    environment: String,
    seed: String,
    schedule: String,
    grafts: Vec<Graft>,
}
fn parse(eg: &mut EGraph, s: &str) -> Result<Vec<Command>> {
    Ok(visual_rule::surface_program(eg.parse_program(None, s)?))
}
fn state(eg: &EGraph) -> Result<State> {
    snapshot::snapshot_filtered(
        eg,
        &BTreeSet::new(),
        "graft-differential/v1",
        &BTreeSet::new(),
    )
}
fn equivalent(a: &State, b: &State) -> Result<bool> {
    Ok(matches!(compare(a, b)?, Comparison::Equivalent { .. }))
}
fn size(eg: &EGraph, names: &[String]) -> usize {
    names.iter().map(|n| eg.get_size(n)).sum()
}
fn saturate(eg: &mut EGraph, schedule: &[Command], names: &[String], ms: u64) -> Json {
    let start = Instant::now();
    let trace = TraceSession::with_effects();
    let mut passes = 0;
    let mut matches = 0;
    let mut inserted = 0;
    let (status, error) = loop {
        let n = size(eg, names);
        if start.elapsed() >= Duration::from_millis(ms) || n >= 200_000 {
            break ("unfinished", None);
        }
        trace.set_execution_limits(
            Some(Duration::from_millis(ms).saturating_sub(start.elapsed())),
            Some(200_000 - n),
        );
        let result = eg.run_program_with_trace(schedule.to_vec(), &trace);
        let stop = trace.execution_stop_reason();
        trace.clear_execution_limits();
        passes += 1;
        matches += trace.take_effect_match_count();
        inserted += trace.effect_summary().0.values().sum::<usize>();
        trace.drain_completed();
        match result {
            Err(e) => break ("error", Some(e.to_string())),
            Ok(outputs) => {
                if stop.is_some() {
                    break ("unfinished", None);
                }
                if !outputs
                    .iter()
                    .any(|o| matches!(o,CommandOutput::RunSchedule(r) if r.updated))
                {
                    break ("fixed_point", None);
                }
            }
        }
    };
    json!({"status":status,"error":error,"seconds":start.elapsed().as_secs_f64(),"passes":passes,"logical_matches":matches,"inserted_receipts":inserted,"rows":size(eg,names)})
}
fn storage(engines: &[&EGraph]) -> Json {
    let mut allocations = BTreeMap::new();
    let mut logical = 0;
    for eg in engines {
        for (_, id, _, bytes) in eg.shared_row_storage() {
            logical += bytes;
            allocations.entry(id).or_insert(bytes);
        }
    }
    json!({"shared_backing_logical_bytes":logical,"shared_backing_unique_bytes":allocations.values().sum::<usize>(),"allocations":allocations.len(),"scope":"only frozen/shared row backing allocations; excludes owned/modified rows, indexes, compiled rules, metadata and allocator overhead"})
}
fn run(families: &[Family], mode: &str, ms: u64) -> Result<Json> {
    if !["rebuild", "snapshot", "fused"].contains(&mode) {
        return Err("mode must be rebuild, snapshot or fused".into());
    }
    let mut results = vec![];
    for f in families {
        let begin = Instant::now();
        let mut base = EGraph::default();
        let env = parse(&mut base, &f.environment)?;
        base.run_program(env)?;
        let seed = parse(&mut base, &f.seed)?;
        let schedule = parse(&mut base, &f.schedule)?;
        let grafts = f
            .grafts
            .iter()
            .map(|g| {
                g.steps
                    .iter()
                    .map(|s| parse(&mut base, s))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        let names = base.get_function_names();
        let setup_seconds = begin.elapsed().as_secs_f64();
        let mut base = Some(base);
        let mut parent = None;
        let mut bake = Json::Null;
        if mode == "snapshot" {
            let begin = Instant::now();
            // The baked parent replaces the scratch environment in this batch.
            // Do not charge snapshot reuse for an unused third engine.
            let mut eg = base.take().unwrap();
            eg.run_program(seed.clone())?;
            bake = saturate(&mut eg, &schedule, &names, ms);
            bake["preparation_seconds"] = json!(begin.elapsed().as_secs_f64());
            if bake["status"] == "fixed_point" {
                parent = Some(eg);
            }
        }
        let parent_before = parent.as_ref().map(state).transpose()?;
        let mut cases = vec![];
        for (g, steps) in f.grafts.iter().zip(grafts) {
            if mode == "snapshot" && parent.is_none() {
                cases.push(json!({"id":g.id,"status":"parent_unfinished_or_error","parent":bake}));
                continue;
            }
            let begin = Instant::now();
            let mut eg = if let Some(parent) = &mut parent {
                parent.fork_shared()
            } else {
                base.as_ref().unwrap().clone()
            };
            let fork_seconds = begin.elapsed().as_secs_f64();
            let mut parent_run = Json::Null;
            if mode != "snapshot" {
                eg.run_program(seed.clone())?;
                if mode == "rebuild" {
                    parent_run = saturate(&mut eg, &schedule, &names, ms);
                }
            }
            let before_graft_seconds = begin.elapsed().as_secs_f64();
            if mode == "rebuild" && parent_run["status"] != "fixed_point" {
                cases.push(json!({"id":g.id,"status":"parent_unfinished_or_error","parent":parent_run,"before_graft_seconds":before_graft_seconds}));
                continue;
            }
            let shared_before = if let Some(p) = &parent {
                storage(&[p, &eg])
            } else {
                storage(&[&eg])
            };
            let mut outcomes = vec![];
            let mut execution_seconds = before_graft_seconds;
            for commands in steps {
                let tick = Instant::now();
                let injection = eg.run_program(commands);
                let injection_seconds = tick.elapsed().as_secs_f64();
                let mut outcome = match injection {
                    Ok(_) => saturate(&mut eg, &schedule, &names, ms),
                    Err(e) => json!({"status":"error","error":e.to_string(),"seconds":0.0}),
                };
                outcome["phase"] = json!(if outcome.get("passes").is_some() {
                    "saturation"
                } else {
                    "graft"
                });
                outcome["injection_seconds"] = json!(injection_seconds);
                execution_seconds += injection_seconds + outcome["seconds"].as_f64().unwrap_or(0.0);
                // Never equate truncated states, nor serialize a growing graph just for timing.
                if outcome["status"] != "unfinished" {
                    match state(&eg) {
                        Ok(s) => outcome["snapshot"] = serde_json::to_value(s)?,
                        Err(e) => outcome["snapshot_error"] = json!(e.to_string()),
                    }
                }
                let done = outcome["status"] == "fixed_point";
                outcomes.push(outcome);
                if !done {
                    break;
                }
            }
            let parent_unchanged = if let (Some(p), Some(before)) = (&parent, &parent_before) {
                Some(equivalent(before, &state(p)?)?)
            } else {
                None
            };
            if parent_unchanged == Some(false) {
                return Err("child modified frozen parent".into());
            }
            let shared_after = if let Some(p) = &parent {
                storage(&[p, &eg])
            } else {
                storage(&[&eg])
            };
            cases.push(json!({"id":g.id,"status":outcomes.last().map(|o|o["status"].clone()),"steps":outcomes,"parent_run":parent_run,"fork_seconds":fork_seconds,"before_graft_seconds":before_graft_seconds,"execution_seconds":execution_seconds,"parent_unchanged":parent_unchanged,"storage_before":shared_before,"storage_after":shared_after}));
        }
        results.push(
            json!({"family":f.id,"setup_seconds":setup_seconds,"parent_bake":bake,"cases":cases}),
        );
    }
    Ok(
        json!({"contract":families,"mode":mode,"limits":{"milliseconds_per_closure":ms,"facts":200000},"families":results,"scope":"native-engine differential experiment; parsing/serialization/comparison excluded from execution timings; no target-state cache or automatic pipeline admission"}),
    )
}
fn declarations(c: &Command) -> bool {
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
fn code(xs: &[Command]) -> String {
    xs.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
fn prepare(analysis: &Path, id: usize, out: &Path) -> Result {
    let report: Json = serde_json::from_slice(&std::fs::read(analysis.join("analysis.json"))?)?;
    let job = report["saturated_rule_composition"]["jobs"]
        .as_array()
        .ok_or("jobs missing")?
        .iter()
        .find(|j| j["candidate_id"] == id)
        .ok_or("cell missing")?;
    let origin = &job["origin"];
    if origin["parameters"].as_array().map(Vec::len) != Some(1)
        || origin["parameters"][0]["sort"] != "Expr"
    {
        return Err("this corpus generator expects one Expr boundary slot".into());
    }
    let mut parser = EGraph::default();
    let source = std::fs::read_to_string(analysis.join("source.egg"))?;
    let original = parse(&mut parser, &source)?;
    let schedules: Vec<_> = original
        .into_iter()
        .filter(|c| matches!(c, Command::RunSchedule(..)))
        .collect();
    if schedules.len() != 2 || !schedules[0].to_string().contains("initialization") {
        return Err("expected separate input and optimizer schedules".into());
    }
    let entry = std::fs::read_to_string(analysis.join(format!(
        "saturated-rule-composition/cells/CSUnit-{id:06}/entry.egg"
    )))?;
    let commands = parse(&mut parser, &entry)?;
    let environment: Vec<_> = commands
        .iter()
        .filter(|c| declarations(c))
        .cloned()
        .collect();
    let seed: Vec<_> = commands.into_iter().filter(|c| !declarations(c)).collect();
    let mut initialized = EGraph::default();
    initialized.run_program(environment.clone())?;
    initialized.run_program(vec![schedules[0].clone()])?;
    let sort = initialized
        .get_sort_by_name("Expr")
        .ok_or("Expr missing")?
        .clone();
    let mut values = vec![];
    initialized.function_for_each("Get", |row| values.push(row.vals[0]))?;
    let mut terms = BTreeSet::new();
    for value in values {
        terms.insert(initialized.extract_value_to_string(&sort, value)?.0);
    }
    let input = format!(
        "({} 0)",
        origin["parameter_markers"]["Expr"]
            .as_str()
            .ok_or("marker missing")?
    );
    let mut families: Vec<Family> =
        serde_json::from_str(include_str!("../experiments/ripsat_graft/contracts.json"))?;
    families.push(Family {
        id: format!("eggcc-cell-{id}"),
        environment: code(&environment),
        seed: code(&seed),
        schedule: code(&schedules[1..]),
        grafts: terms
            .into_iter()
            .enumerate()
            .map(|(i, t)| Graft {
                id: format!("get-input-{i}"),
                steps: vec![format!("(union {input} {t})")],
            })
            .collect(),
    });
    std::fs::write(out, serde_json::to_vec_pretty(&families)?)?;
    Ok(())
}
fn comparison(a: &Json, b: &Json) -> Result<Json> {
    if a["contract"] != b["contract"] || a["limits"] != b["limits"] {
        return Err("different workload or limits".into());
    }
    if a["families"].as_array().map(Vec::len) != b["families"].as_array().map(Vec::len) {
        return Err("family count differs".into());
    }
    let mut rows = vec![];
    for (fa, fb) in a["families"]
        .as_array()
        .ok_or("families missing")?
        .iter()
        .zip(b["families"].as_array().ok_or("families missing")?)
    {
        if fa["family"] != fb["family"] {
            return Err("family mismatch".into());
        }
        if fa["cases"].as_array().map(Vec::len) != fb["cases"].as_array().map(Vec::len) {
            return Err("graft count differs".into());
        }
        for (ca, cb) in fa["cases"]
            .as_array()
            .unwrap()
            .iter()
            .zip(fb["cases"].as_array().unwrap())
        {
            if ca["id"] != cb["id"] {
                return Err("graft mismatch".into());
            }
            let mut equal = true;
            let mut inconclusive = false;
            let mut details = vec![];
            match (ca["steps"].as_array(), cb["steps"].as_array()) {
                (Some(sa), Some(sb)) => {
                    if sa.len() != sb.len()
                        && ca["status"] != "unfinished"
                        && cb["status"] != "unfinished"
                    {
                        equal = false;
                    }
                    for (x, y) in sa.iter().zip(sb) {
                        if x["status"] == "unfinished" || y["status"] == "unfinished" {
                            inconclusive = true;
                            continue;
                        }
                        let same_status = x["status"] == y["status"] && x["phase"] == y["phase"];
                        let same_error = x["error"] == y["error"];
                        let snapshot_equal = match (x.get("snapshot"), y.get("snapshot")) {
                            (Some(x), Some(y)) => Some(equivalent(
                                &serde_json::from_value(x.clone())?,
                                &serde_json::from_value(y.clone())?,
                            )?),
                            _ => None,
                        };
                        if snapshot_equal.is_none() {
                            inconclusive = true;
                        }
                        equal &= same_status && same_error && snapshot_equal != Some(false);
                        details.push(json!({"status_equal":same_status,"error_equal":same_error,"snapshot_equal":snapshot_equal}));
                    }
                }
                _ => inconclusive = true,
            }
            rows.push(json!({"family":fa["family"],"case":ca["id"],"result":if !equal{"different"}else if inconclusive{"inconclusive"}else{"equal"},"details":details}));
        }
    }
    Ok(json!({"left":a["mode"],"right":b["mode"],"cases":rows}))
}
fn main() -> Result {
    let a: Vec<_> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("prepare") if a.len() == 5 => {
            prepare(Path::new(&a[2]), a[3].parse()?, Path::new(&a[4]))
        }
        Some("run") if a.len() == 6 => {
            let f: Vec<Family> = serde_json::from_slice(&std::fs::read(&a[2])?)?;
            let r = run(&f, &a[3], a[5].parse()?)?;
            std::fs::write(&a[4], serde_json::to_vec_pretty(&r)?)?;
            Ok(())
        }
        Some("compare") if a.len() == 5 => {
            let x = serde_json::from_slice(&std::fs::read(&a[2])?)?;
            let y = serde_json::from_slice(&std::fs::read(&a[3])?)?;
            std::fs::write(&a[4], serde_json::to_vec_pretty(&comparison(&x, &y)?)?)?;
            Ok(())
        }
        _ => Err(
            "prepare ANALYSIS CELL_ID MANIFEST | run MANIFEST MODE REPORT MS | compare A B REPORT"
                .into(),
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_parent_preserves_order_aliases_visibility_and_failure() {
        let mut f: Vec<Family> =
            serde_json::from_str(include_str!("../experiments/ripsat_graft/contracts.json"))
                .unwrap();
        f.retain(|f| f.id != "growth");
        let cold = run(&f, "rebuild", 1000).unwrap();
        let warm = run(&f, "snapshot", 1000).unwrap();
        for report in [&cold, &warm] {
            let errors = report["families"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["family"] == "error")
                .unwrap();
            assert_eq!(errors["cases"][0]["status"], "error");
            assert_eq!(
                errors["cases"][1]["status"], "fixed_point",
                "failed child must not poison another fork"
            );
        }
        let report = comparison(&cold, &warm).unwrap();
        assert!(
            report["cases"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["result"] == "equal"),
            "{report}"
        );
        let fused = run(&f, "fused", 1000).unwrap();
        let diff = comparison(&cold, &fused).unwrap();
        for name in ["mutable-table", "visibility"] {
            assert!(
                diff["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["family"] == name && c["result"] == "different"),
                "{diff}"
            );
        }
    }
}
