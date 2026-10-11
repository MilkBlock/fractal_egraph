//! Whole-constructor-state differential check of generated spine programs.
//! Inspection never evaluates constructor expressions; it reads native tables.
use egg_layout::{
    pipeline::Result,
    saturated_rule_composition::{self as state, Row, SaturatedRuleComposition, Vertex},
};
use egglog::{ArcSort, EGraph, Value};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

#[derive(Deserialize)]
struct Manifest {
    constructors: Vec<String>,
    cases: Vec<Case>,
    #[serde(default)]
    library: String,
    #[serde(default)]
    dump_states: bool,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    reference: String,
    reference_path: Option<String>,
    compressed: String,
    restore: String,
    expected_summary: Option<String>,
}

fn snapshot(eg: &EGraph, names: &[String]) -> Result<SaturatedRuleComposition> {
    let mut ids = BTreeMap::<(String, Value), usize>::new();
    let mut values = vec![];
    let mut rows = BTreeSet::new();
    let mut hidden = BTreeSet::new();
    let mut vertex = |v: Value, s: &ArcSort| -> usize {
        let v = eg.get_canonical_value(v, s);
        let next = ids.len();
        *ids.entry((s.name().into(), v)).or_insert_with(|| {
            let literal = match s.name() {
                "i64" => Some(eg.value_to_base::<i64>(v).to_string()),
                "String" => Some(
                    serde_json::to_string(&eg.value_to_base::<egglog::sort::S>(v).to_string())
                        .unwrap(),
                ),
                "bool" => Some(eg.value_to_base::<bool>(v).to_string()),
                "f64" => Some(eg.value_to_base::<egglog::sort::F>(v).to_string()),
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
    for op in names {
        let Some(f) = eg.get_function(op) else {
            continue;
        };
        let schema = f.schema();
        eg.function_for_each(op, |r| {
            let row = Row {
                op: op.clone(),
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
        scope: json!({"contract":"spine-positive-native-validation/v1"}),
        values,
        rows,
        ports: BTreeMap::new(),
        subsumed_rows,
    })
}
fn count(eg: &EGraph, name: &str) -> usize {
    let mut n = 0;
    if eg.get_function(name).is_some() {
        eg.function_for_each(name, |_| n += 1).unwrap();
    }
    n
}
fn classes(state: &SaturatedRuleComposition) -> usize {
    state
        .rows
        .iter()
        .map(|r| r.result)
        .collect::<BTreeSet<_>>()
        .len()
}
fn table_rows(state: &SaturatedRuleComposition) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for row in &state.rows {
        *counts.entry(row.op.clone()).or_default() += 1;
    }
    counts
}
fn matches(eg: &EGraph) -> BTreeMap<String, usize> {
    eg.get_overall_run_report()
        .num_matches_per_rule
        .iter()
        .map(|(rule, count)| (rule.to_string(), *count))
        .collect()
}
fn execute_spine(eg: &mut EGraph, source: &str) -> Result {
    use egglog::ast::{Command, GenericSchedule as S};
    fn admitted(s: &egglog::ast::Schedule) -> bool {
        match s {
            S::Run(_, cfg) => matches!(cfg.ruleset.as_str(), "Spine-Run" | "Spine-Export"),
            S::Repeat(_, _, s) | S::Saturate(_, s) => admitted(s),
            S::Sequence(_, items) => items.iter().all(admitted),
        }
    }
    let commands = eg.parse_program(None, source)?;
    if commands
        .iter()
        .any(|c| matches!(c,Command::RunSchedule(s) if !admitted(s)))
    {
        return Err("compressed path may not execute the original ruleset".into());
    }
    eg.run_program(commands)?;
    Ok(())
}
fn main() -> Result {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: spine_validate MANIFEST.json REPORT.json".into());
    }
    let manifest: Manifest = serde_json::from_str(&fs::read_to_string(&args[1])?)?;
    let mut prepared = EGraph::default();
    prepared.parse_and_run_program(None, &manifest.library)?;
    let mut report = vec![];
    for c in manifest.cases {
        let mut original = EGraph::default();
        if let Some(path) = &c.reference_path {
            original.parse_and_run_program(Some(c.name.clone()), &fs::read_to_string(path)?)?;
        }
        original.parse_and_run_program(Some(c.name.clone()), &c.reference)?;
        let expected = snapshot(&original, &manifest.constructors)?;
        let mut compressed = prepared.clone();
        execute_spine(&mut compressed, &c.compressed)?;
        let before = snapshot(&compressed, &manifest.constructors)?;
        if !before.rows.is_empty() {
            return Err(format!(
                "{}: original constructors materialized during spine execution",
                c.name
            )
            .into());
        }
        if let Some(name) = &c.expected_summary {
            if count(&compressed, name) == 0 {
                return Err(format!("{}: missing expected summary {name}", c.name).into());
            }
        }
        let spine_names: Vec<_> = manifest
            .constructors
            .iter()
            .map(|n| format!("Spine-{n}"))
            .collect();
        let spine = snapshot(&compressed, &spine_names)?;
        let summary_tables: BTreeMap<_, _> = compressed
            .get_function_names()
            .iter()
            .filter(|n| n.starts_with("Spine-Summary-"))
            .map(|n| (n.to_string(), count(&compressed, n)))
            .collect();
        let summary_rows: usize = summary_tables.values().sum();
        // Capture before export: cached expansion matches must not be charged
        // to compressed inference. These are logical matches, not mutations.
        let compressed_matches = matches(&compressed);
        let native_rows_before_export = compressed.num_tuples();
        execute_spine(&mut compressed, &c.restore)?;
        let actual = snapshot(&compressed, &manifest.constructors)?;
        let comparison = state::compare(&expected, &actual)?;
        if !matches!(comparison, state::Comparison::Equivalent { .. }) {
            return Err(format!(
                "{}: full closure mismatch: {:?}",
                c.name,
                serde_json::to_value(&comparison)?
            )
            .into());
        }
        let mut result = json!({"case":c.name,"status":"passed","original_rules_executed":false,
            "physical_original_rows_before_export":before.rows.len(),"spine_constructor_rows":spine.rows.len(),
            "summary_rows":summary_rows,"summary_tables":summary_tables,
            "native_total_rows_before_export":native_rows_before_export,"original_native_total_rows":original.num_tuples(),
            "original_constructor_rows":expected.rows.len(),"restored_constructor_rows":actual.rows.len(),
            "original_constructor_eclasses":classes(&expected),"spine_constructor_eclasses":classes(&spine),
            "restored_constructor_eclasses":classes(&actual),
            "original_tables":table_rows(&expected),"spine_tables":table_rows(&spine),
            "original_logical_matches":matches(&original),"compressed_logical_matches_before_export":compressed_matches,
            "comparison":comparison});
        if manifest.dump_states {
            result["states"] = json!({"original":expected,"spine":spine,"restored":actual});
        }
        report.push(result);
    }
    fs::write(
        &args[2],
        serde_json::to_string_pretty(
            &json!({"native_runtime":true,"scope":"complete constructor rows and equality partition after explicit native closure restoration, not a runtime speedup benchmark","cases":report}),
        )? + "\n",
    )?;
    println!("validated {} native spine cases", report.len());
    Ok(())
}
