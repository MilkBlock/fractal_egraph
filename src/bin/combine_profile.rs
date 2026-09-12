//! Rank witnessed rule-dependency shapes; does not claim compiled macro usage.
use egglog::{EGraph, RuleActionOutcome, TraceSession, WriteOutcome, ast::Command};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default)]
struct Count {
    occurrences: usize,
    productive: usize,
    scopes: BTreeMap<usize, usize>,
    continued: BTreeSet<u64>,
    next_motifs: BTreeMap<String, usize>,
    examples: Vec<u64>,
    next: BTreeMap<String, usize>,
}
fn profile(path: &str) -> Json {
    profile_rounds(path, None)
}
fn profile_rounds(path: &str, max_rounds: Option<usize>) -> Json {
    let source = std::fs::read_to_string(path).unwrap();
    let mut reference = EGraph::default();
    reference
        .parse_and_run_program(Some(path.into()), &source)
        .unwrap();
    let mut eg = EGraph::default();
    let mut commands = eg.parse_program(Some(path.into()), &source).unwrap();
    let mut catalogue = BTreeMap::new();
    let mut source_positions = BTreeMap::new();
    let mut index = 0;
    for command in &mut commands {
        *command = egg_layout::visual_rule::normalize(command.clone(), index);
        if let Command::Rule { rule } = command {
            let original = rule.to_string();
            if rule.name.is_empty() {
                rule.name = format!("R{index}");
            }
            source_positions.insert(rule.name.clone(), egg_layout::visual_rule::positions(rule));
            catalogue.insert(rule.name.clone(), original);
            index += 1;
        }
    }
    if let Some(limit) = max_rounds {
        for command in &mut commands {
            if let Command::RunSchedule(egglog::ast::GenericSchedule::Repeat(_, n, inner)) = command
            {
                assert!(
                    matches!(**inner, egglog::ast::GenericSchedule::Run(..)),
                    "sampling supports simple repeat(run) only"
                );
                *n = (*n).min(limit);
            }
        }
    }
    let trace = TraceSession::with_dependencies();
    let mut event_round = BTreeMap::new();
    let mut round = 0usize;
    for command in commands {
        if let Command::RunSchedule(egglog::ast::GenericSchedule::Repeat(_, n, inner)) = &command {
            if matches!(**inner, egglog::ast::GenericSchedule::Run(..)) {
                for _ in 0..*n {
                    round += 1;
                    let before: BTreeSet<_> = trace.matches().iter().map(|m|m.event_id).collect();
                    eg.run_program_with_trace(vec![Command::RunSchedule((**inner).clone())], &trace).unwrap();
                    for m in trace.matches() {if !before.contains(&m.event_id){event_round.insert(m.event_id,round);}}
                }
                continue;
            }
        }
        eg.run_program_with_trace(vec![command], &trace).unwrap();
    }
    let matches: BTreeMap<_, _> = trace
        .matches()
        .into_iter()
        .map(|m| (m.event_id, m))
        .collect();
    let writes: BTreeMap<_, _> = trace
        .write_events()
        .into_iter()
        .map(|w| (w.event_id, w))
        .collect();
    let survived: BTreeSet<_> = trace
        .action_outcomes()
        .iter()
        .filter(|a| a.outcome == RuleActionOutcome::Survived)
        .map(|a| a.match_event_id)
        .collect();
    let resets = trace.scope_resets();
    let scope = |event: u64| resets.iter().filter(|&&id| id < event).count();
    let mut productive = BTreeSet::new();
    let mut rule_stats = BTreeMap::new();
    for (name, definition) in &catalogue {
        let ids: BTreeSet<_> = matches
            .values()
            .filter(|m| m.rule.as_ref() == name)
            .map(|m| m.event_id)
            .collect();
        let inserted: Vec<_> = writes
            .values()
            .filter(|w| {
                ids.contains(&w.match_event_id)
                    && w.rebuild_of.is_none()
                    && w.outcome == WriteOutcome::Inserted
            })
            .collect();
        let changed_unions: Vec<_> = trace
            .union_events()
            .into_iter()
            .filter(|u| ids.contains(&u.match_event_id) && u.displaced.is_some())
            .collect();
        productive.extend(inserted.iter().map(|w| w.match_event_id));
        productive.extend(changed_unions.iter().map(|u| u.match_event_id));
        rule_stats.insert(name.clone(),json!({"definition":definition,"matches":ids.len(),"survived":ids.intersection(&survived).count(),"direct_inserted_rows":inserted.len(),"committed_unions":changed_unions.len(),"complete_witnesses":ids.iter().filter(|m|matches[m].physical_witness_complete).count()}));
    }
    let mut reads = BTreeMap::<u64, Vec<_>>::new();
    for read in trace.row_reads() {
        reads.entry(read.match_event_id).or_default().push(read);
    }
    let mut motifs = BTreeMap::<String, Count>::new();
    let mut instance_motif = BTreeMap::new();
    let mut edges = BTreeSet::new();
    let mut pairs = BTreeMap::<String, Count>::new();
    let mut coarse_shapes = BTreeSet::new();
    let mut unknown_reads = 0;
    let mut evidence = Vec::new();
    for (&consumer, rs) in &reads {
        if !survived.contains(&consumer) {
            continue;
        }
        let target = matches[&consumer].rule.to_string();
        let mut ordinals = BTreeMap::<String, usize>::new();
        let mut parents = BTreeMap::new();
        let mut signature = Vec::new();
        let mut coarse_signature = Vec::new();
        let mut support = Vec::new();
        for r in rs {
            let table = r.table_name.as_deref().unwrap_or("?").to_string();
            let ordinal = ordinals.entry(table.clone()).or_default();
            let slot = *ordinal;
            *ordinal += 1;
            let (Some(producer), Some(write)) =
                (r.producer_match_event_id, r.producer_write_event_id)
            else {
                unknown_reads += 1;
                continue;
            };
            let Some(w) = writes.get(&write) else {
                continue;
            };
            if w.actual != r.row
                || w.table != r.table
                || w.outcome != WriteOutcome::Inserted
                || w.event_id >= r.event_id
                || scope(producer) != scope(consumer)
            {
                continue;
            }
            let Some(pm) = matches.get(&producer) else {
                continue;
            };
            if !survived.contains(&producer) {
                continue;
            }
            let parent_index = parents.len();
            let parent = *parents.entry(producer).or_insert(parent_index);
            let kind = if w.rebuild_of.is_some() {
                "rebuild"
            } else {
                "row"
            };
            coarse_signature.push(format!("{}@p{parent}:{table}#{slot}", pm.rule));
            let locate = |rule: &str, span: Option<&str>| -> Vec<String> {
                source_positions
                    .get(rule)
                    .into_iter()
                    .flatten()
                    .filter(|(id, _, op)| Some(id.as_str()) == span && op == &table)
                    .map(|(_, path, _)| path.clone())
                    .collect()
            };
            let producer_sites = locate(pm.rule.as_ref(), w.source_span.as_deref());
            let consumer_sites = locate(matches[&consumer].rule.as_ref(), r.source_span.as_deref());
            signature.push(format!(
                "{}@p{parent}:{table}#{slot}{{{}=>{}}}",
                pm.rule,
                producer_sites.join("|"),
                consumer_sites.join("|")
            ));
            support.push(json!({"producer_source_span":w.source_span,"consumer_source_span":r.source_span,"producer_sites":producer_sites,"consumer_sites":consumer_sites,"producer":producer,"consumer":consumer,"read":r.event_id,"write":write,"table":table,"slot":slot,"kind":kind,"key_values":r.key.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"row_values":r.row.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"producer_bindings":pm.bindings.iter().filter_map(|b|b.name.as_ref().map(|n|(n.to_string(),format!("{:?}",b.value)))).collect::<BTreeMap<_,_>>(),"rebuild_of":w.rebuild_of,"union_dependencies":w.union_dependencies}));
            edges.insert((producer, consumer));
            let key = format!("{} -> {} via {}#{slot}/{kind}", pm.rule, target, table);
            let c = pairs.entry(key).or_default();
            c.occurrences += 1;
            c.productive += usize::from(productive.contains(&consumer));
            *c.scopes.entry(scope(consumer)).or_default() += 1;
            if c.examples.len() < 3 {
                c.examples.push(consumer);
            }
        }
        if signature.is_empty() {
            continue;
        }
        let key = format!("[{}] -> {target}", signature.join(" + "));
        coarse_shapes.insert(format!("[{}] -> {target}", coarse_signature.join(" + ")));
        let c = motifs.entry(key.clone()).or_default();
        c.occurrences += 1;
        c.productive += usize::from(productive.contains(&consumer));
        *c.scopes.entry(scope(consumer)).or_default() += 1;
        if c.examples.len() < 3 {
            c.examples.push(consumer);
        }
        instance_motif.insert(consumer, key.clone());
        evidence.push(json!({"consumer":consumer,"scope":scope(consumer),"consumer_bindings":matches[&consumer].bindings.iter().filter_map(|b|b.name.as_ref().map(|n|(n.to_string(),format!("{:?}",b.value)))).collect::<BTreeMap<_,_>>(),"motif":key,"productive":productive.contains(&consumer),"complete_witness":matches[&consumer].physical_witness_complete,"boundary_read_count":rs.len()-support.len(),"support":support}));
    }
    for &(p, c) in &edges {
        if let Some(key) = instance_motif.get(&p) {
            let count = motifs.get_mut(key).unwrap();
            count.continued.insert(p);
            if let Some(next) = instance_motif.get(&c) {
                *count.next_motifs.entry(next.clone()).or_default() += 1;
            }

            *motifs
                .get_mut(key)
                .unwrap()
                .next
                .entry(matches[&c].rule.to_string())
                .or_default() += 1;
        }
    }
    let ranked = |map: BTreeMap<String, Count>| {
        let mut rows: Vec<_> = map.into_iter().collect();
        rows.sort_by(|a, b| b.1.occurrences.cmp(&a.1.occurrences).then(a.0.cmp(&b.0)));
        rows.into_iter().enumerate().map(|(i,(shape,c))|json!({"rank":i+1,"shape":shape,"observed_occurrences":c.occurrences,"productive_consumer_occurrences":c.productive,"scopes":c.scopes,"example_matches":c.examples,"continued_instances":c.continued.len(),"next_motif_counts":c.next_motifs,"next_rule_counts":c.next})).collect::<Vec<_>>()
    };
    json!({"events":matches.values().map(|m|json!({"id":m.event_id,"rule":m.rule,"scope":scope(m.event_id),"round":event_round.get(&m.event_id),"productive":productive.contains(&m.event_id),"bindings":m.bindings.iter().filter_map(|b|b.name.as_ref().map(|n|(n.to_string(),format!("{:?}",b.value)))).collect::<BTreeMap<_,_>>()})).collect::<Vec<_>>(),"round_tracking":"explicit repeat(run) boundaries only; other schedule forms have null rounds","coarse_motif_classes":coarse_shapes.len(),"source_mapping":"compiler source spans mapped to normalized AST positions; motif identity includes endpoint positions","profile_round_limit":max_rounds,"source":path,"scope":"historical committed row-dependency motifs, not generated or executed shortcut rules","native_checks":"original native program completed; normalized profiled program completed with explicit profile_round_limit when supplied","rule_labels":rule_stats,"pair_rankings":ranked(pairs),"motif_rankings":ranked(motifs),"witnesses":evidence,"surviving_reads_without_same_session_producer":unknown_reads,"executed_compiled_macros":0,"selection_policy":"none: ranking is observational, no activation policy","limitations":["one execution per scope, not independent benchmark repetitions","motifs retain producer-instance aliasing and same-table read ordinals; not full typed binding-isomorphism classes","boundary inputs and union support remain explicit; not closed rewrite certificates","historical support remains counted even if row origins are invalidated later","union-only causes are shown as rebuild support, not ranked as Inserted producers"]})
}
fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or("egglog/tests/web-demo/cyk.egg".into());
    let limit = std::env::args()
        .nth(3)
        .map(|s| s.parse().expect("round limit"));
    let result = profile_rounds(&path, limit);
    let text = serde_json::to_string_pretty(&result).unwrap();
    if let Some(out) = std::env::args().nth(2) {
        std::fs::write(out, text).unwrap();
    } else {
        println!("{text}");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_has_recursive_motifs_and_preserves_usage_meaning() {
        let p = profile("egglog/tests/web-demo/cyk.egg");
        assert_eq!(p["executed_compiled_macros"], 0);
        assert!(!p["motif_rankings"].as_array().unwrap().is_empty());
        assert!(
            p["pair_rankings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["shape"].as_str().unwrap().starts_with("R1 -> R1"))
        );
        let total: usize = p["motif_rankings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["observed_occurrences"].as_u64().unwrap() as usize)
            .sum();
        assert_eq!(total, p["witnesses"].as_array().unwrap().len());
    }
}
