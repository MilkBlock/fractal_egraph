//! Shared rule-comb templates and occurrence-scoped evidence, executed in egglog.
use egglog::EGraph;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Canonicalize external names by first occurrence, retaining sort and aliasing.
/// Concrete values are deliberately not accepted as part of this template API.
pub fn external_ports(ports: &[(String, String)]) -> Result<String, String> {
    let mut slots = BTreeMap::<String, (usize, String)>::new();
    let mut encoded = vec![];
    for (name, sort) in ports {
        let next = slots.len();
        let (index, old_sort) = slots.entry(name.clone()).or_insert((next, sort.clone()));
        if old_sort != sort {
            return Err("external port has inconsistent sorts".into());
        }
        encoded.push(format!(
            "(External {} {})",
            index,
            serde_json::to_string(sort).unwrap()
        ));
    }
    Ok(encoded
        .into_iter()
        .rev()
        .fold("(PNil)".into(), |tail, p| format!("(PCons {p} {tail})")))
}
pub fn execute(source: &str) -> Result<Value, String> {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, source)
        .map_err(|e| e.to_string())?;
    eg.parse_and_run_program(None, "(run-schedule (saturate (run tier1)))")
        .map_err(|e| e.to_string())?;
    export(&eg)
}
fn validate_independence(eg: &EGraph) -> Result<(), String> {
    for name in ["ParentAt", "OutputAt", "ExternalAt"] {
        let mut values = BTreeMap::new();
        let mut conflict = false;
        eg.function_for_each(name, |r| {
            let key = format!("{:?}/{:?}", r.vals[0], r.vals[1]);
            let val = format!("{:?}", r.vals[2]);
            if values
                .insert(key, val.clone())
                .is_some_and(|old| old != val)
            {
                conflict = true;
            }
        })
        .map_err(|e| e.to_string())?;
        if conflict {
            return Err(format!(
                "conflicting {name} witnesses for one occurrence port"
            ));
        }
    }

    let mut parents = BTreeMap::<String, Vec<String>>::new();
    eg.function_for_each("ParentAt", |r| {
        parents
            .entry(format!("{:?}", r.vals[0]))
            .or_default()
            .push(format!("{:?}", r.vals[2]));
    })
    .map_err(|e| e.to_string())?;
    let mut proofs = vec![];
    eg.function_for_each("Independent", |r| {
        proofs.push((format!("{:?}", r.vals[0]), format!("{:?}", r.vals[1])))
    })
    .map_err(|e| e.to_string())?;
    for (provider, removed) in proofs {
        let mut todo = vec![provider];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(p) = todo.pop() {
            if p == removed {
                return Err("independence certificate depends on removed occurrence".into());
            }
            if seen.insert(p.clone()) {
                todo.extend(parents.get(&p).into_iter().flatten().cloned());
            }
        }
    }
    Ok(())
}
pub fn export(eg: &EGraph) -> Result<Value, String> {
    validate_independence(eg)?;
    let cid = |sort: &str, v| {
        eg.value_to_class_id(eg.get_sort_by_name(sort).unwrap(), v)
            .to_string()
    };
    let mut templates = BTreeMap::<String, Value>::new();
    for name in ["Basic", "SmoothComb", "CoarseComb"] {
        eg.function_for_each(name, |row| {
            let id = cid("Comb", *row.vals.last().unwrap());
            let rule_text = eg
                .extract_value_to_string(
                    eg.get_sort_by_name("String").unwrap(),
                    row.vals[if name == "Basic" { 0 } else { 1 }],
                )
                .unwrap()
                .0;
            let rule: String = serde_json::from_str(&rule_text).unwrap();
            let ports = if name == "Basic" {
                String::new()
            } else {
                eg.extract_value_to_string(eg.get_sort_by_name("Ports").unwrap(), row.vals[2])
                    .unwrap()
                    .0
            };
            templates.insert(
                id.clone(),
                json!({"id":id,"kind":name,"rule":rule,"relative_binding":ports,"parents":[]}),
            );
        })
        .map_err(|e| e.to_string())?;
    }
    eg.function_for_each("ParentTemplate",|row|{let id=cid("Comb",row.vals[0]);templates.get_mut(&id).unwrap()["parents"].as_array_mut().unwrap().push(json!({"slot":eg.value_to_base::<i64>(row.vals[1]),"template":cid("Comb",row.vals[2])}));}).map_err(|e|e.to_string())?;
    for t in templates.values_mut() {
        t["parents"]
            .as_array_mut()
            .unwrap()
            .sort_by_key(|p| p["slot"].as_i64().unwrap());
    }
    let mut instances = BTreeMap::<String, Value>::new();
    eg.function_for_each("Occurrence",|r|{let id=cid("Instance",r.vals[2]);instances.insert(id.clone(),json!({"id":id,"event":eg.value_to_base::<i64>(r.vals[0]),"template":cid("Comb",r.vals[1]),"bindings":[],"effects":[]}));}).map_err(|e|e.to_string())?;
    eg.function_for_each("Binding", |r| {
        instances.get_mut(&cid("Instance", r.vals[0])).unwrap()["bindings"]
            .as_array_mut()
            .unwrap()
            .push(json!(
                eg.extract_value_to_string(eg.get_sort_by_name("Args").unwrap(), r.vals[1])
                    .unwrap()
                    .0
            ));
    })
    .map_err(|e| e.to_string())?;
    eg.function_for_each("Provides", |r| {
        instances.get_mut(&cid("Instance", r.vals[0])).unwrap()["effects"]
            .as_array_mut()
            .unwrap()
            .push(json!(
                eg.extract_value_to_string(eg.get_sort_by_name("Effect").unwrap(), r.vals[1])
                    .unwrap()
                    .0
            ));
    })
    .map_err(|e| e.to_string())?;
    let serialized = eg.serialize(egglog::SerializeConfig {
        max_functions: None,
        max_calls_per_function: None,
        include_temporary_functions: true,
        root_eclasses: vec![],
    });
    assert!(serialized.is_complete());
    let nodes=serialized.egraph.nodes.iter().map(|(id,n)|(id.to_string(),json!({"op":n.op,"children":n.children.iter().map(ToString::to_string).collect::<Vec<_>>(),"eclass":n.eclass.to_string(),"subsumed":n.subsumed}))).collect::<BTreeMap<_,_>>();
    Ok(
        json!({"templates":templates.into_values().collect::<Vec<_>>(),"instances":instances.into_values().collect::<Vec<_>>(),"native_egraph":{"nodes":nodes},"serialization_complete":true,"scope":"native tier-1 templates plus occurrence-scoped evidence; supplied witnesses, not an automatic tier-0 importer or compression result"}),
    )
}
