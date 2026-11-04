//! Summarize one completed profile, optionally evaluating an independently trained catalog.
use eggfuse_cell_runtime::{compiler::bake_spine_blocks, program::Program};
use serde::Deserialize;
use serde_json::{Value as J, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
};
type Edge = [usize; 4];
#[derive(Deserialize)]
struct Node {
    parent: usize,
    edge: Edge,
    visits: u64,
    leaves: u64,
    same_fact: u64,
}
#[derive(Deserialize)]
struct Profile {
    schema: String,
    program_sha256: String,
    prefixes: Vec<Node>,
    #[serde(rename = "static")]
    topology: J,
}
fn dist(counts: impl IntoIterator<Item = u64>) -> J {
    let mut v: Vec<_> = counts.into_iter().filter(|&v| v != 0).collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    let total: u64 = v.iter().sum();
    let mut prefix = vec![0];
    for c in &v {
        prefix.push(prefix.last().unwrap() + c);
    }
    let ranks = [50, 80, 90, 95, 99].map(|p| {
        (
            p.to_string(),
            prefix.partition_point(|s| u128::from(*s) * 100 < u128::from(total) * p),
        )
    });
    let top = [1usize, 10, 16, 32, 64, 100].map(|k| (k.to_string(), prefix[k.min(v.len())]));
    json!({"distinct":v.len(),"occurrences":total,"singletons":v.iter().filter(|&&x|x==1).count(),
        "top_visits":BTreeMap::from(top),"keys_for_percent":BTreeMap::from(ranks)})
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let files: Vec<_> = env::args().skip(1).collect();
    if !matches!(files.len(), 2 | 4) {
        return Err("expected TARGET_PROGRAM TARGET_PROFILE [TRAIN_PROGRAM TRAIN_PROFILE]".into());
    }
    let read = |p: &str| -> Result<J, Box<dyn std::error::Error>> {
        Ok(serde_json::from_slice(&fs::read(p)?)?)
    };
    let target = Program::parse(read(&files[0])?)?;
    let q: Profile = serde_json::from_value(read(&files[1])?)?;
    if q.schema != "eggfuse-spine-shapes/v1" || q.program_sha256 != target.artifact["sha256"] {
        return Err("profile mismatch".into());
    }
    let mut edges = BTreeMap::new();
    let mut pairs = BTreeMap::new();
    let mut entries = BTreeMap::new();
    let mut different_sets = BTreeMap::new();
    let mut totals_by_length = BTreeMap::<usize, u64>::new();
    let mut paths = vec![(0, 1)]; // Rule-path id and rule count, including the root rule.
    let mut rule_paths = BTreeMap::new();
    let mut leaves = BTreeMap::new();
    let mut rule_prefixes = BTreeMap::new();
    let mut same_fact = 0;
    let mut ngrams = BTreeMap::new();
    let meta = |r: usize| &target.artifact["recipe"]["rules"][r]["metadata"];
    for node in &q.prefixes {
        let [p, o, r, a] = node.edge;
        let &(previous, depth) = paths.get(node.parent).ok_or("prefix parent out of range")?;
        if node.parent != 0 {
            let old = q.prefixes[node.parent - 1].edge;
            if old[2] != p {
                return Err("discontinuous lineage".into());
            }
            *ngrams.entry((old, node.edge)).or_insert(0) += node.visits;
        }
        *edges.entry(node.edge).or_insert(0) += node.visits;
        *pairs.entry((p, r)).or_insert(0) += node.visits;
        *entries.entry((r, a)).or_insert(0) += node.visits;
        if meta(p)["source_set"] != meta(r)["source_set"] {
            *different_sets.entry([p, o, r, a]).or_insert(0) += node.visits;
        }
        let next = rule_paths.len() + 1;
        let id = *rule_paths.entry((previous, p, r)).or_insert(next);
        paths.push((id, depth + 1));
        *rule_prefixes.entry(id).or_insert(0) += node.visits;
        *leaves.entry(id).or_insert(0) += node.leaves;
        *totals_by_length.entry(depth + 1).or_default() += node.visits;
        same_fact += node.same_fact;
    }
    let mut ranked: Vec<_> = edges.iter().collect();
    ranked.sort_by_key(|(e, n)| (std::cmp::Reverse(**n), **e));
    let mut sender = BTreeMap::<usize, (usize, u64, usize)>::new();
    for (&e, &n) in &edges {
        let x = sender.entry(e[0]).or_default();
        x.0 += 1;
        x.1 += n;
        x.2 += usize::from(n == 1);
    }
    let dominant=sender.iter().max_by_key(|(_,x)|x.0).map(|(p,(distinct,uses,singletons))|json!({
        "producer":p,"ruleset":meta(*p)["source_set"],"distinct_edges":distinct,"visits":uses,"singletons":singletons}));
    let mut result = json!({"program_sha256":q.program_sha256,"static":q.topology,"same_physical_fact":same_fact,"rule_pairs":dist(pairs.values().copied()),
        "instruction_edges":dist(edges.values().copied()),"consumer_entries":dist(entries.values().copied()),
        "different_ruleset_edges":dist(different_sets.values().copied()),"two_transition_windows":dist(ngrams.values().copied()),
        "full_prefixes":dist(q.prefixes.iter().map(|x|x.visits)),"leaf_paths":dist(q.prefixes.iter().map(|x|x.leaves)),
        "rule_prefixes":dist(rule_prefixes.into_values()),"rule_leaf_paths":dist(leaves.into_values()),
        "visits_by_rule_depth":totals_by_length,"most_diverse_producer":dominant,
        "top_edges":ranked.into_iter().take(10).map(|(e,n)|json!({"edge":e,"visits":n,"producer_set":meta(e[0])["source_set"],"consumer_set":meta(e[2])["source_set"]})).collect::<Vec<_>>()});
    if files.len() == 4 {
        let train = Program::parse(read(&files[2])?)?;
        let profile = read(&files[3])?;
        let mut curve = vec![];
        for limit in [4, 8, 16, 32, 64, 256] {
            let plan = bake_spine_blocks(&train, &profile, &target, limit)?;
            let selected = plan["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["consumer"].as_u64().unwrap() as usize,
                        e["pivot"].as_u64().unwrap() as usize,
                    )
                })
                .collect::<BTreeSet<_>>();
            let mass = |hist: &BTreeMap<Edge, u64>| {
                hist.iter()
                    .filter(|(e, _)| selected.contains(&(e[2], e[3])))
                    .map(|(_, v)| v)
                    .sum::<u64>()
            };
            curve.push(json!({"budget":limit,"blocks":selected.len(),"resumptions":mass(&edges),"different_ruleset":mass(&different_sets)}));
        }
        result["transfer_curve"] = json!(curve);
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
