//! One native run per workload/variant. No tracing, analysis, graph rendering,
//! engine edits, repeated averages, or hidden exclusion of rule installation.
use egg_layout::{
    pipeline::Result,
    saturated_rule_composition::{
        self as state, Comparison, Row, SaturatedRuleComposition, Vertex,
    },
};
use egglog::{
    ArcSort, EGraph, Value,
    ast::{Action, Command},
};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::Instant,
};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.
}
// Only consecutive expression assertions. Checks, let, set, union, subsume,
// schedules and declarations are barriers. These fixtures' grouped expressions
// are constructor-only; arbitrary expressions require a separate eligibility check.
fn batch(commands: &[Command], enabled: bool) -> (String, String, usize) {
    if !enabled {
        return (
            String::new(),
            commands
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
            0,
        );
    }
    let mut defs = String::new();
    let mut body = String::new();
    let mut actions = vec![];
    let mut groups = 0;
    let flush = |actions: &mut Vec<String>,
                 defs: &mut String,
                 body: &mut String,
                 groups: &mut usize| {
        if actions.is_empty() {
            return;
        }
        if actions.len() == 1 {
            body.push_str(&actions[0]);
            body.push('\n');
        } else {
            defs.push_str(&format!("(ruleset ReplayBatch{groups})\n(rule () ({}) :ruleset ReplayBatch{groups} :name \"replay-batch-{groups}\")\n",actions.join("\n")));
            body.push_str(&format!("(run ReplayBatch{groups} 1)\n"));
            *groups += 1;
        }
        actions.clear();
    };
    for c in commands {
        if let Command::Action(Action::Expr(..)) = c {
            actions.push(c.to_string())
        } else {
            flush(&mut actions, &mut defs, &mut body, &mut groups);
            body.push_str(&c.to_string());
            body.push('\n');
        }
    }
    flush(&mut actions, &mut defs, &mut body, &mut groups);
    (defs, body, groups)
}
fn snapshot(eg: &EGraph) -> Result<SaturatedRuleComposition> {
    let mut ids = BTreeMap::<(String, Value), usize>::new();
    let mut values = vec![];
    let mut rows = BTreeSet::new();
    let mut hidden = BTreeSet::new();
    let mut vertex = |v: Value, s: &ArcSort| {
        let v = eg.get_canonical_value(v, s);
        let next = ids.len();
        *ids.entry((s.name().into(), v)).or_insert_with(|| {
            let literal = match s.name() {
                "i64" => Some(eg.value_to_base::<i64>(v).to_string()),
                "String" => Some(
                    serde_json::to_string(&eg.value_to_base::<egglog::sort::S>(v).to_string())
                        .unwrap(),
                ),
                "Unit" => Some("()".into()),
                _ => None,
            };
            values.push(Vertex {
                sort: s.name().into(),
                literal,
            });
            next
        })
    };
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        if f.is_let_binding() {
            continue;
        }
        let schema = f.schema();
        eg.function_for_each(&name, |r| {
            let row = Row {
                op: name.clone(),
                args: r.vals[..schema.input.len()]
                    .iter()
                    .zip(&schema.input)
                    .map(|(v, s)| vertex(*v, s))
                    .collect(),
                result: vertex(r.vals[schema.input.len()], &schema.output),
            };
            if r.subsumed {
                hidden.insert(row.clone());
            }
            rows.insert(row);
        })?;
    }
    let rows: Vec<_> = rows.into_iter().collect();
    let subsumed_rows = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| hidden.contains(r).then_some(i))
        .collect();
    Ok(SaturatedRuleComposition {
        version: 2,
        local_ids: vec![],
        scope: json!({}),
        values,
        rows,
        ports: BTreeMap::new(),
        subsumed_rows,
    })
}
fn sample(
    base: &EGraph,
    commands: &[Command],
    enabled: bool,
    folder: &Path,
) -> Result<(Json, SaturatedRuleComposition)> {
    let tick = Instant::now();
    let mut eg = base.clone();
    let clone_ms = ms(tick);
    let tick = Instant::now();
    let (defs, body, groups) = batch(commands, enabled);
    let generate_ms = ms(tick);
    fs::create_dir_all(folder)?;
    fs::write(folder.join("batch-rules.egg"), &defs)?;
    fs::write(folder.join("actions.egg"), &body)?;
    let tick = Instant::now();
    let definitions = eg.parse_program(None, &defs)?;
    let actions = eg.parse_program(None, &body)?;
    let parse_ms = ms(tick);
    let tick = Instant::now();
    eg.run_program(definitions)?;
    let install_ms = ms(tick);
    let tick = Instant::now();
    eg.run_program(actions)?;
    let execute_ms = ms(tick);
    // Verification/export is outside timed execution for both variants.
    let state = snapshot(&eg)?;
    let tables: BTreeMap<_, _> = eg
        .get_function_names()
        .into_iter()
        .filter(|n| !eg.get_function(n).unwrap().is_let_binding())
        .map(|n| {
            let count = eg.get_size(&n);
            (n, count)
        })
        .collect();
    let report = json!({"clone_ms":clone_ms,"generate_ms":generate_ms,"parse_ms":parse_ms,"install_ms":install_ms,"execute_ms":execute_ms,"total_ms":clone_ms+generate_ms+parse_ms+install_ms+execute_ms,
        "fact_actions":commands.iter().filter(|c|matches!(c,Command::Action(Action::Expr(..)))).count(),"batch_rules":groups,"rows":state.rows.len(),"eclasses":state.values.iter().filter(|v|v.sort=="T").count(),"tables":tables,
        "matches":eg.get_overall_run_report().num_matches_per_rule.values().sum::<usize>(),
        "note":"direct execute includes per-command compilation; batch rule compilation is in install_ms; counts are logical matches, not committed writes"});
    Ok((report, state))
}
fn workload(name: &str, text: &str, out: &Path) -> Result<Json> {
    let mut parser = EGraph::default();
    let parsed = parser.parse_program(None, text)?;
    let split = parsed
        .iter()
        .position(|c| {
            matches!(
                c,
                Command::Action(..) | Command::Check(..) | Command::Fail(..)
            )
        })
        .ok_or("no input actions")?;
    let tick = Instant::now();
    let mut base = EGraph::default();
    base.run_program(parsed[..split].to_vec())?;
    let setup_ms = ms(tick);
    let folder = out.join(name);
    fs::create_dir_all(&folder)?;
    fs::write(folder.join("source.egg"), text)?;
    let (direct, ds) = sample(&base, &parsed[split..], false, &folder.join("direct"))?;
    let (batched, bs) = sample(&base, &parsed[split..], true, &folder.join("batched"))?;
    if !matches!(state::compare(&ds, &bs)?, Comparison::Equivalent { .. }) {
        return Err(format!("{name}: batching changed the state").into());
    }
    let ratio = direct["total_ms"].as_f64().unwrap() / batched["total_ms"].as_f64().unwrap();
    Ok(
        json!({"name":name,"common_environment_setup_ms":setup_ms,"direct":direct,"batched":batched,"complete_states_equal":true,"total_speedup":ratio}),
    )
}
fn ordering_counterexample() -> Result<Json> {
    let env = "(datatype T (N i64))";
    let original = "(N 0) (fail (check (N 1))) (N 1)";
    let mut direct = EGraph::default();
    direct.parse_and_run_program(None, env)?;
    let commands = direct.parse_program(None, original)?;
    direct.run_program(commands.clone())?;
    let (defs, body, _) = batch(&commands, true);
    let mut staged = EGraph::default();
    staged.parse_and_run_program(None, &format!("{env}\n{defs}\n{body}"))?;
    let mut unsafe_batch = EGraph::default();
    let err=unsafe_batch.parse_and_run_program(None,&format!("{env} (ruleset load) (rule () ((N 0) (N 1)) :ruleset load) (run load 1) (fail (check (N 1)))")).expect_err("lifting later facts must break the early absence check");
    // Not every top-level action is legal inside a rule RHS. Record the native
    // acceptance/error for a mutable function read rather than assume equivalence.
    let env = "(datatype T (N i64) (Seen i64)) (function Cost (T) i64 :merge (min old new)) (set (Cost (N 0)) 7)";
    let actions = "(set (Cost (N 0)) 5) (Seen (Cost (N 0)))";
    let mut direct = EGraph::default();
    direct.parse_and_run_program(None, &format!("{env} {actions}"))?;
    let mut batch = EGraph::default();
    let batch_result = batch.parse_and_run_program(
        None,
        &format!("{env} (ruleset load) (rule () ({actions}) :ruleset load) (run load 1)"),
    );
    let batch_error = batch_result.as_ref().err().map(ToString::to_string);
    let states_equal = if batch_result.is_ok() {
        Some(matches!(
            state::compare(&snapshot(&direct)?, &snapshot(&batch)?)?,
            Comparison::Equivalent { .. }
        ))
    } else {
        None
    };
    let seen = |eg: &EGraph| -> Result<Vec<i64>> {
        let mut values = vec![];
        eg.function_for_each("Seen", |r| values.push(eg.value_to_base::<i64>(r.vals[0])))?;
        Ok(values)
    };
    Ok(
        json!({"phase_preserving_passed":true,"lift_all_to_start_rejected":err.to_string(),"set_then_lookup":{"complete_states_equal":states_equal,"direct_seen":seen(&direct)?,"batch_error":batch_error,"batched_seen":if batch_result.is_ok(){Some(seen(&batch)?)}else{None}}}),
    )
}
fn main() -> Result {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 2 {
        return Err("usage: cargo run --release --example replay_fact_batch -- OUT".into());
    }
    EGraph::set_num_threads(1);
    // Initialize native infrastructure once, outside both timed paths. No workload is repeated.
    let mut warm = EGraph::default();
    warm.parse_and_run_program(None, "(datatype Warm (W)) (W)")?;
    let out = Path::new(&args[1]);
    fs::create_dir_all(out)?;
    let mini = include_str!("../experiments/replay_fact_batching/mini_replay.egg");
    let mut bulk = "(datatype T (Num i64) (Add T T))\n".to_owned();
    for i in 0..1024 {
        bulk.push_str(&format!("(Add (Num {}) (Num {}))\n", i * 2, i * 2 + 1));
    }
    let rows = vec![
        workload("real_replay", mini, out)?,
        workload("bulk_1024", &bulk, out)?,
    ];
    let report = json!({"scope":"standalone batching experiment on the actual local native egglog runtime; production analyze unchanged; no tracing or graph export; one run per variant; environment setup shared and reported separately; file output, full-state verification and compilation of this Rust executable excluded from timings","threads":1,"workloads":rows,"ordering":ordering_counterexample()?});
    fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
