//! Select frequently applied consumer entries and lower them against the target recipe.
use crate::{program::*, spine_blocks::Block};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
// Keep rule-local variable names and debug labels out of the cross-recipe key.
// Distinct matching target rules are rejected rather than arbitrarily picked.
fn signature(rule: &J) -> String {
    let slots = rule["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| json!([s["sort"], s["definition"]]))
        .collect::<Vec<_>>();
    crate::checksum::artifact_digest(&json!([
        rule["metadata"]["source_set"],
        rule["metadata"]["seminaive"],
        slots,
        rule["query"],
        rule["effects"]
    ]))
}
pub fn bake_spine_blocks(
    training: &Program,
    profile: &J,
    target: &Program,
    limit: usize,
) -> Result<J> {
    if profile["schema"] != "eggfuse-spine-shapes/v1"
        || profile["program_sha256"] != training.artifact["sha256"]
    {
        return Err("spine profile belongs to a different training program".into());
    }
    let mut frequencies = BTreeMap::<(usize, usize), u64>::new();
    for prefix in profile["prefixes"]
        .as_array()
        .ok_or("missing spine prefixes")?
    {
        let edge: [usize; 4] =
            serde_json::from_value(prefix["edge"].clone()).map_err(|e| e.to_string())?;
        let count = prefix["visits"].as_u64().ok_or("invalid prefix visits")?;
        let v = frequencies.entry((edge[2], edge[3])).or_default();
        *v = v.checked_add(count).ok_or("spine visits overflow")?;
    }
    let mut ranked = frequencies
        .into_iter()
        .filter(|((r, a), _)| Block::lower(training, *r, *a).is_some())
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(entry, count)| (std::cmp::Reverse(*count), *entry));
    let mut ids = BTreeMap::<String, Vec<usize>>::new();
    for (i, r) in target.artifact["recipe"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        ids.entry(signature(r)).or_default().push(i);
    }
    let mut entries = vec![];
    let mut seen = BTreeSet::new();
    for ((r, pivot), n) in ranked.into_iter().take(limit) {
        let sig = signature(&training.artifact["recipe"]["rules"][r]);
        let Some(&[consumer]) = ids.get(&sig).map(Vec::as_slice) else {
            continue;
        };
        if !seen.insert((consumer, pivot)) {
            continue;
        }
        if let Some(block) = Block::lower(target, consumer, pivot) {
            entries.push(json!({"consumer":consumer,"pivot":pivot,"rhs_template":target.rules[consumer].template,"block":block,
                "training_rule":r,"training_visits":n}));
        }
    }
    Ok(
        json!({"schema":"eggfuse-spine-blocks/v1","recipe_sha256":target.artifact["recipe"]["sha256"],
        "training_program_sha256":training.artifact["sha256"],"limit":limit,"entries":entries}),
    )
}
