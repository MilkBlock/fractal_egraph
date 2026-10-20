//! SOURCE.egg OUTPUT.json. Compile inventory only; no main inference or GPU run.
use egglog::{
    EGraph,
    ast::{Command, GenericSchedule, Schedule},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
fn sets(s: &Schedule, out: &mut BTreeSet<String>) {
    match s {
        GenericSchedule::Run(_, r) => {
            out.insert(r.ruleset.clone());
        }
        GenericSchedule::Sequence(_, xs) => {
            for x in xs {
                sets(x, out)
            }
        }
        GenericSchedule::Repeat(_, _, x) | GenericSchedule::Saturate(_, x) => sets(x, out),
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 2 {
        return Err("SOURCE.egg OUTPUT.json".into());
    }
    EGraph::set_num_threads(1);
    let mut eg = EGraph::default();
    let cs = eg.parse_program(Some(a[0].clone()), &std::fs::read_to_string(&a[0])?)?;
    let last = cs
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("missing schedule")?;
    let mut scheduled = BTreeSet::new();
    let Command::RunSchedule(s) = &cs[last] else {
        unreachable!()
    };
    sets(s, &mut scheduled);
    eg.run_program(cs[..last].to_vec())?;
    let mut members = BTreeSet::new();
    for set in &scheduled {
        members.extend(eg.cell_ruleset_members(set)?);
    }
    let mut rows = vec![];
    let mut errors = BTreeMap::<String, usize>::new();
    for (set, name) in &members {
        let features = eg.cell_rule_features(set, name)?;
        let result = eg.prepare_gpu_cell_action(set, name);
        let error = result.as_ref().err().map(ToString::to_string);
        if let Some(e) = &error {
            *errors.entry(e.clone()).or_default() += 1;
        }
        rows.push(json!({"ruleset":set,"rule":name,"features":features,"supported":error.is_none(),"error":error,
            "table_atoms":result.as_ref().ok().map(|s|s.atoms.len()),"device_slots":result.as_ref().ok().map(|s|s.device_slots()),
            "container_columns":result.as_ref().ok().map(|s|s.atoms.iter().flat_map(|a|a.sorts.iter()).filter(|n|eg.get_sort_by_name(n).unwrap().is_container_sort()).collect::<BTreeSet<_>>())}));
    }
    let report = json!({"scheduled_rulesets":scheduled,"rules":rows,"rule_count":members.len(),"errors":errors,
        "scope":"Actual typed LHS admission over scheduled original Pass-3 rules, including combined-set members; not Cell coverage or a GPU execution proof."});
    if let Some(p) = Path::new(&a[1]).parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&a[1], serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}",
        json!({"rules":members.len(),"errors":report["errors"]})
    );
    Ok(())
}
