//! Run SlottedCanon over a saved native CoarseLayer/SmoothLayer snapshot.
//! This is structural Graft-attachment analysis, not a new native execution.
use egg_layout::slotted_canon::{GraftCandidate, SlotDescriptor, SlottedCanon};
use serde_json::{Value, json};
use std::{collections::BTreeSet, env, fs};

fn operator(rule: &str) -> String {
    let marker = "(= __viz_root (";
    let marker2 = "(= root (";
    let start = rule
        .find(marker)
        .map(|i| i + marker.len())
        .or_else(|| rule.find(marker2).map(|i| i + marker2.len()));
    start
        .and_then(|i| rule[i..].split_whitespace().next())
        .unwrap_or("unknown")
        .to_string()
}
fn role_class(role: &str, op: &str, index: usize) -> String {
    if role.starts_with("var:") {
        if matches!(op, "Add" | "Mul") && index < 2 {
            "var:commutative".into()
        } else {
            format!("var:pos{index}")
        }
    } else if role.starts_with("read:") {
        format!("read:depth{}", role.matches("/args/").count())
    } else {
        role.into()
    }
}
fn main() {
    let path = env::args()
        .nth(1)
        .expect("usage: slotted_canon_math LAYERS.json");
    let layers: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let combs = layers["combs"].as_array().unwrap();
    let occurrences = layers["occurrences"].as_array().unwrap();
    let mut canon = SlottedCanon::new();
    let mut candidates = 0usize;
    for (occ_id, occurrence) in occurrences.iter().enumerate() {
        let comb = &combs[occurrence["comb"].as_u64().unwrap() as usize];
        let rule = comb["rule"].as_str().unwrap_or("unknown");
        let op = operator(rule);
        let roles = comb["input_roles"].as_array().unwrap();
        let classes: Vec<_> = roles
            .iter()
            .enumerate()
            .map(|(i, r)| role_class(r.as_str().unwrap_or("unknown"), &op, i))
            .collect();
        let slots: Vec<_> = classes
            .iter()
            .map(|class| SlotDescriptor {
                sort: "Math".into(),
                role: class.clone(),
                upper_degree: usize::from(class.starts_with("var:")),
                lower_degree: usize::from(class.starts_with("read:")),
                alias_class: 0,
            })
            .collect();
        let residual_ops: Vec<String> = comb["effects"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(ToString::to_string)
            .collect();
        for target in 0..slots.len() {
            let candidate = GraftCandidate {
                rule: rule.into(),
                slots: slots.clone(),
                targets: [target].into_iter().collect(),
                upper: [target].into_iter().collect(),
                lower: BTreeSet::new(),
                binding: vec![],
                residual_ops: residual_ops.clone(),
                witness: Some(egg_layout::slotted_canon::GraftWitness {
                    event: occ_id as u64,
                    instance: occurrence["comb"].as_u64().unwrap_or(0) as usize,
                }),
            };
            canon.insert(candidate);
            candidates += 1;
        }
    }
    let report = canon.report();
    println!("{}", serde_json::to_string_pretty(&json!({
        "source_scope":"saved actual native math_mini3 bounded layers; no new execution",
        "layers":{"combs":combs.len(),"occurrences":occurrences.len(),"coarse_layers":layers["coarse_layers"].as_array().map_or(0,Vec::len),"smooth_layers":layers["smooth_layers"].as_array().map_or(0,Vec::len)},
        "candidate_graft_attachments":candidates,
        "slotted_canon":report,
        "top_orbits":canon.orbits().iter().enumerate().filter(|(_,o)|o.members.len()>1).take(20).map(|(i,o)|json!({"orbit":i,"size":o.members.len(),"rule":o.signature.rule,"target_classes":o.signature.target_classes,"slot_classes":o.signature.slot_classes,"binding":o.signature.binding})).collect::<Vec<_>>(),
        "interpretation":"Candidates are possible Graft attachment positions derived from observed CoarseLayer Apply input roles. They are not claims that every candidate committed a runtime Graft."
    })).unwrap());
}
