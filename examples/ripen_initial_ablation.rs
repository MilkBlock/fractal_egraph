//! Replay every completed physical cell, comparing initial vs saturated partitions.
use egg_layout::{
    native_analyze::ripen,
    pipeline::Result,
    saturated_rule_composition::{Comparison, SaturatedRuleComposition, compare, read},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
fn intern(
    s: SaturatedRuleComposition,
    states: &mut Vec<SaturatedRuleComposition>,
) -> Result<usize> {
    for (i, t) in states.iter().enumerate() {
        if matches!(compare(&s, t)?, Comparison::Equivalent { .. }) {
            return Ok(i);
        }
    }
    let id = states.len();
    states.push(s);
    Ok(id)
}
fn main() -> Result {
    let args: Vec<_> = std::env::args().collect();
    let q: Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let out = PathBuf::from(&args[2]);
    std::fs::create_dir(&out)?;
    let mut initial = vec![];
    let mut final_states = vec![];
    let mut rows = vec![];
    for j in q["jobs"].as_array().unwrap().iter().filter(|j| {
        j["state"] == "Saturated"
            && j.get("fully_equivalent_to").is_none()
            && j["export"]["status"] == "exported"
    }) {
        let id = j["candidate_id"].as_u64().unwrap();
        let folder = out.join(format!("cell-{id}"));
        let original = PathBuf::from(j["folder"].as_str().unwrap());
        ripen::run(&original.join("entry.egg"), &folder)?;
        let before = read(&folder.join("initial-state.json"))?;
        let after = read(&folder.join("saturated-rule-composition.json"))?;
        if !matches!(
            compare(
                &after,
                &read(&original.join("saturated-rule-composition.json"))?
            )?,
            Comparison::Equivalent { .. }
        ) {
            return Err("replayed final state differs".into());
        }
        let counts = json!({"initial_facts":before.rows.len(),"final_facts":after.rows.len()});
        rows.push(json!({"candidate_id":id,"initial_group":intern(before,&mut initial)?,"final_group":intern(after,&mut final_states)?,"counts":counts}));
    }
    let mut groups = BTreeMap::<usize, BTreeSet<usize>>::new();
    for r in &rows {
        groups
            .entry(r["final_group"].as_u64().unwrap() as usize)
            .or_default()
            .insert(r["initial_group"].as_u64().unwrap() as usize);
    }
    let result = json!({"physical_completed_entries":rows.len(),"initial_structural_groups":initial.len(),"final_structural_groups":final_states.len(),"final_groups_merging_distinct_initial_states":groups.values().filter(|s|s.len()>1).count(),"groups":groups,"rows":rows});
    std::fs::write(
        out.join("summary.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    Ok(())
}
