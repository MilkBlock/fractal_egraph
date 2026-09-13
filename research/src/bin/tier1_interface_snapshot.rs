//! Read symbolic port schemas and parent links back from the native tier-1 egraph.
use egglog::{EGraph, Value};
use serde_json::json;
use std::collections::BTreeMap;
fn string(eg: &EGraph, v: Value) -> String {
    eg.value_to_base::<egglog::sort::S>(v).as_str().to_owned()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = "experiments/tier1_extract/";
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        &std::fs::read_to_string(format!("{dir}interfaces.egg"))?,
    )?;
    let mut classes = BTreeMap::new();
    eg.function_for_each("OriginalClass", |r| {
        assert!(classes.insert(r.vals[0], string(&eg, r.vals[1])).is_none());
    })?;
    let mut instances = BTreeMap::new();
    eg.function_for_each("Occurrence",|r|{instances.insert(r.vals[2],json!({"event":eg.value_to_base::<i64>(r.vals[0]),"template":classes[&r.vals[1]],"parents":[]}));})?;
    let mut parents = BTreeMap::<Value, BTreeMap<i64, i64>>::new();
    eg.function_for_each("ParentAt", |r| {
        let event = instances[&r.vals[2]]["event"].as_i64().unwrap();
        assert!(
            parents
                .entry(r.vals[0])
                .or_default()
                .insert(eg.value_to_base::<i64>(r.vals[1]), event)
                .is_none()
        );
    })?;
    eg.function_for_each("InterfaceLayout", |r| {
        let layout: serde_json::Value = serde_json::from_str(&string(&eg, r.vals[1])).unwrap();
        let i = instances.get_mut(&r.vals[0]).unwrap();
        assert!(i.get("inputs").is_none());
        i["inputs"] = layout["inputs"].clone();
        i["outputs"] = layout["outputs"].clone();
    })?;
    for (key, i) in &mut instances {
        assert!(i.get("inputs").is_some());
        if let Some(p) = parents.get(key) {
            assert!(p.keys().copied().eq(0..p.len() as i64));
            i["parents"] = json!(p.values().collect::<Vec<_>>());
        }
    }
    let mut rules = BTreeMap::new();
    eg.function_for_each("SourceRuleAST", |r| {
        let rule = eg
            .extract_value_to_string(eg.get_sort_by_name("RuleId").unwrap(), r.vals[0])
            .unwrap()
            .0;
        let name: String = serde_json::from_str(
            rule.strip_prefix("(Rule ")
                .unwrap()
                .strip_suffix(')')
                .unwrap(),
        )
        .unwrap();
        let ast: serde_json::Value = serde_json::from_str(&string(&eg, r.vals[1])).unwrap();
        assert!(rules.insert(name, ast).is_none());
    })?;
    let mut fingerprint = Vec::new();
    eg.function_for_each("SnapshotFingerprint", |r| {
        fingerprint.push(string(&eg, r.vals[0]))
    })?;
    assert_eq!(fingerprint.len(), 1);
    let result = json!({"native_sha256":fingerprint[0],"scope":"Read from native InterfaceLayout, ParentAt, Occurrence and SourceRuleAST tables; every parent link passes native LinkedParent checks. OriginalClass maps reloaded classes to the saved extraction.","occurrences":instances.into_values().collect::<Vec<_>>(),"rules":rules,"parent_edges":parents.values().map(|p|p.len()).sum::<usize>()});
    std::fs::write(
        format!("{dir}native_interfaces.json"),
        serde_json::to_string_pretty(&result)? + "\n",
    )?;
    Ok(())
}
