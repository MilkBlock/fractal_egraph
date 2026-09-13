//! Exact sharing and an observed-instance coverage partial order. No semantic unions.
use super::*;
fn encoded_len<T: serde::Serialize>(value: &T) -> Result<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Counter(0);
    serde_json::to_writer(&mut out, value)?;
    Ok(out.0)
}
fn intern(value: Json, keys: &mut BTreeMap<String, usize>, definitions: &mut Vec<Json>) -> usize {
    let key = value.to_string();
    if let Some(id) = keys.get(&key) {
        return *id;
    }
    let id = definitions.len();
    definitions.push(json!({"id":id,"definition":value}));
    keys.insert(key, id);
    id
}
#[derive(Clone)]
struct Footprint {
    entry: u64,
    events: BTreeSet<u64>,
    boundary: BTreeSet<u64>,
}
struct Info {
    id: u64,
    instances: Vec<Footprint>,
    events: BTreeSet<u64>,
    bytes: usize,
}
fn dominates(a: &Info, b: &Info) -> bool {
    a.events.len() > b.events.len()
        && a.events.is_superset(&b.events)
        && !b.instances.is_empty()
        && b.instances.iter().all(|small| {
            a.instances.iter().any(|large| {
                large.entry == small.entry
                    && large.events.is_superset(&small.events)
                    && large.boundary.is_subset(&small.boundary)
            })
        })
}
pub(super) fn organize(raw: Vec<Json>) -> Result<Json> {
    let before = encoded_len(&raw)?;
    let mut steps = vec![];
    let mut step_keys = BTreeMap::new();
    let mut extents = vec![];
    let mut extent_keys = BTreeMap::new();
    let mut witnesses = BTreeMap::<String, Json>::new();
    let mut pattern_keys = BTreeMap::<String, usize>::new();
    let mut patterns = Vec::<Json>::new();
    let mut repeated_witness_records = 0usize;
    for mut p in raw {
        let source_id = p["id"].as_u64().ok_or("missing pattern id")?;
        let entry = p
            .as_object_mut()
            .unwrap()
            .remove("entry_binding")
            .ok_or("missing entry definition")?;
        p["entry_template"] = json!(intern(entry, &mut step_keys, &mut steps));
        for port in p["ports"].as_array_mut().ok_or("missing ports")? {
            let definition = port
                .as_object_mut()
                .unwrap()
                .remove("binding_transfer")
                .ok_or("missing port definition")?;
            port["step_template"] = json!(intern(definition, &mut step_keys, &mut steps));
        }
        let key = json!([p["kind"], p["entry_template"], p["ports"]]).to_string();
        let definition_bytes = {
            let mut referenced = BTreeSet::from([p["entry_template"].as_u64().unwrap() as usize]);
            for port in p["ports"].as_array().unwrap() {
                referenced.insert(port["step_template"].as_u64().unwrap() as usize);
            }
            key.len()
                + referenced
                    .iter()
                    .map(|id| serde_json::to_vec(&steps[*id]["definition"]).unwrap().len())
                    .sum::<usize>()
        };
        for i in p["instances"].as_array_mut().ok_or("missing instances")? {
            let extent = i
                .as_object_mut()
                .unwrap()
                .remove("extent")
                .ok_or("missing extent")?;
            i["extent_template"] = json!(intern(extent, &mut extent_keys, &mut extents));
            let records = i
                .as_object_mut()
                .unwrap()
                .remove("fact_witnesses")
                .ok_or("missing fact witnesses")?;
            let mut refs = vec![];
            let Json::Array(records) = records else {
                return Err("invalid witnesses".into());
            };
            for w in records {
                let event = w["event"].as_u64().ok_or("missing witness event")?;
                let k = event.to_string();
                if let Some(old) = witnesses.get(&k) {
                    if old != &w {
                        return Err("conflicting witness payload for the same apply event".into());
                    }
                    repeated_witness_records += 1;
                } else {
                    witnesses.insert(k, w);
                }
                refs.push(event);
            }
            i["event_refs"] = json!(refs);
        }
        p["source_pattern_ids"] = json!([source_id]);
        p["definition_bytes"] = json!(definition_bytes);
        p["source_reports"] = json!([{"pattern_id":source_id,"learning_witnesses":p["learning_witnesses"],"additional_returning_witnesses":p["additional_returning_witnesses"]}]);
        if let Some(index) = pattern_keys.get(&key) {
            let old = &mut patterns[*index];
            old["source_pattern_ids"]
                .as_array_mut()
                .unwrap()
                .push(json!(source_id));
            old["source_reports"]
                .as_array_mut()
                .unwrap()
                .extend(p["source_reports"].as_array().unwrap().iter().cloned());
            for instance in p["instances"].as_array().unwrap() {
                let same = old["instances"].as_array().unwrap().iter().find(|other| {
                    other["entry_event"] == instance["entry_event"]
                        && other["extent_template"] == instance["extent_template"]
                });
                if let Some(other) = same {
                    let mut a = other.clone();
                    let mut b = instance.clone();
                    a.as_object_mut().unwrap().remove("fractal_comb");
                    b.as_object_mut().unwrap().remove("fractal_comb");
                    if a != b {
                        return Err("conflicting instances for an exact shared template".into());
                    }
                } else {
                    old["instances"]
                        .as_array_mut()
                        .unwrap()
                        .push(instance.clone());
                }
            }
        } else {
            pattern_keys.insert(key, patterns.len());
            patterns.push(p);
        }
    }
    let mut uses = vec![BTreeSet::<u64>::new(); steps.len()];
    for p in &patterns {
        let entry = p["entry_template"].as_u64().unwrap() as usize;
        for i in p["instances"].as_array().unwrap() {
            for node in i["nodes"].as_array().unwrap() {
                uses[entry].insert(node["entry_event"].as_u64().unwrap());
                for port in node["ports"].as_array().unwrap() {
                    if let Some(event) = port["observed_child"].as_u64() {
                        let slot = port["port"].as_u64().unwrap();
                        if let Some(def) = p["ports"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|d| d["port"] == slot)
                        {
                            uses[def["step_template"].as_u64().unwrap() as usize].insert(event);
                        }
                    }
                }
            }
        }
    }
    for (i, events) in uses.iter().enumerate() {
        steps[i]["observed_apply_uses"] = json!(events.len());
    }
    let infos: Vec<_> = patterns
        .iter()
        .map(|p| {
            let instances: Vec<_> = p["instances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| {
                    let events: BTreeSet<_> = i["event_refs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap())
                        .collect();
                    let boundary = events
                        .iter()
                        .flat_map(|e| {
                            witnesses[&e.to_string()]["parents"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| v.as_u64().unwrap())
                        })
                        .filter(|p| !events.contains(p))
                        .collect();
                    Footprint {
                        entry: i["entry_event"].as_u64().unwrap(),
                        events,
                        boundary,
                    }
                })
                .collect();
            Info {
                id: p["id"].as_u64().unwrap(),
                events: instances
                    .iter()
                    .flat_map(|i| i.events.iter().copied())
                    .collect(),
                instances,
                bytes: p["definition_bytes"].as_u64().unwrap() as usize,
            }
        })
        .collect();
    let mut edges = vec![];
    let mut overlaps = vec![];
    let mut incoming = vec![BTreeSet::new(); infos.len()];
    for (a, large) in infos.iter().enumerate() {
        for (b, small) in infos.iter().enumerate() {
            if a == b {
                continue;
            }
            if dominates(large, small) {
                incoming[b].insert(a);
                edges.push(json!({"dominant":large.id,"dominated":small.id,"kind":"observed_instance_dominance","certificate":"Every dominated instance has the same observed entry, all its apply events preserved, and no extra external parent dependencies.","matched_entries":small.instances.iter().map(|i|i.entry).collect::<Vec<_>>(),"semantic_dominance":"unproved"}));
            } else if a < b && !dominates(small, large) && !large.events.is_disjoint(&small.events)
            {
                overlaps.push(json!({"a":large.id,"b":small.id,"shared_apply_events":large.events.intersection(&small.events).count(),"relation":"overlap_without_observed_dominance"}));
            }
        }
    }
    let mut remaining: BTreeSet<_> = (0..infos.len()).collect();
    let mut covered = BTreeSet::new();
    let mut ordered = vec![];
    while !remaining.is_empty() {
        let ready: Vec<_> = remaining
            .iter()
            .copied()
            .filter(|i| incoming[*i].iter().all(|p| !remaining.contains(p)))
            .collect();
        let best = ready
            .into_iter()
            .max_by(|a, b| {
                let ma = infos[*a].events.difference(&covered).count() as u128;
                let mb = infos[*b].events.difference(&covered).count() as u128;
                (ma * infos[*b].bytes.max(1) as u128)
                    .cmp(&(mb * infos[*a].bytes.max(1) as u128))
                    .then(infos[*a].events.len().cmp(&infos[*b].events.len()))
                    .then(infos[*b].id.cmp(&infos[*a].id))
            })
            .ok_or("cycle in observed coverage dominance")?;
        let marginal = infos[best].events.difference(&covered).count();
        covered.extend(&infos[best].events);
        remaining.remove(&best);
        let mut p = patterns[best].take();
        p["ranking"] = json!({"rank":ordered.len()+1,"unique_apply_events":infos[best].events.len(),"new_apply_events":marginal,"definition_bytes":infos[best].bytes,"new_events_per_kib":marginal as f64*1024.0/infos[best].bytes.max(1) as f64,"observed_instances":infos[best].instances.len(),"semantic_dominance":"unproved"});
        ordered.push(p);
    }
    let core = json!({"patterns":ordered,"step_templates":steps,"extent_templates":extents,"event_witnesses":witnesses});
    let after = encoded_len(&core)?;
    let mut result = core;
    result["pattern_count"] = json!(infos.len());
    result["observed_dominance"] = json!(edges);
    result["overlaps"] = json!(overlaps);
    result["sharing"] = json!({"unique_step_templates":result["step_templates"].as_array().unwrap().len(),"unique_extents":result["extent_templates"].as_array().unwrap().len(),"unique_event_witnesses":result["event_witnesses"].as_object().unwrap().len(),"repeated_witness_records_removed":repeated_witness_records,"raw_catalog_json_bytes":before,"shared_catalog_json_bytes":after,"scope":"Catalog core JSON only (definitions, instances, witness registry and ranking fields); excludes outer diagnostics/dominance summaries. Not runtime memory or tier0 enode compression"});
    result["ranking_scope"] = json!(
        "Observed dominance first; then greedy marginal unique apply coverage per definition byte. Incomparable patterns retained. Exact definitions shared; no non-identical templates unioned."
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn witness(event: u64, parent: u64) -> Json {
        json!({"event":event,"parents":[parent],"binding":[1],"outputs":[event],"row_effects":[event],"union_effects":[]})
    }
    fn pattern(id: u64, definition: &str, events: &[(u64, u64)], entry: u64) -> Json {
        json!({"id":id,"entry_rule":definition,"branch_rule":"b","entry_binding":{"literal":definition},"ports":[{"port":0,"binding_transfer":{"op":"b"},"return_transfer":"same"}],"observed_arity":1,"learning_witnesses":[],"additional_returning_witnesses":0,"instances":[{"entry_event":entry,"extent":"(Depth 2)","fractal_comb":format!("pattern_{id}"),"nodes":[{"entry_event":entry,"ports":[]}],"fact_witnesses":events.iter().map(|(e,p)|witness(*e,*p)).collect::<Vec<_>>()}]})
    }
    #[test]
    fn exact_templates_share_but_literals_and_parameters_do_not_collapse() {
        let a = pattern(0, "literal:1", &[(1, 0), (2, 1)], 1);
        let duplicate = pattern(1, "literal:1", &[(1, 0), (2, 1)], 1);
        let parameter = pattern(2, "parameter:c", &[(1, 0), (2, 1)], 1);
        let r = organize(vec![a, duplicate, parameter]).unwrap();
        assert_eq!(r["pattern_count"], 2);
        assert_eq!(r["sharing"]["unique_event_witnesses"], 2);
        assert_eq!(r["sharing"]["unique_step_templates"], 3);
        assert_eq!(r["sharing"]["unique_extents"], 1);
        assert!(
            r["patterns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["source_pattern_ids"] == json!([0, 1]))
        );
        assert!(r["observed_dominance"].as_array().unwrap().is_empty());
    }
    #[test]
    fn dominance_requires_same_entry_and_no_extra_boundary() {
        let a = pattern(0, "large", &[(1, 0), (2, 1), (3, 2)], 1);
        let b = pattern(1, "small", &[(1, 0), (2, 1)], 1);
        let shifted = pattern(2, "shifted", &[(2, 1), (3, 2)], 2);
        let r = organize(vec![b, a, shifted]).unwrap();
        let edges = r["observed_dominance"].as_array().unwrap();
        assert!(
            edges
                .iter()
                .any(|e| e["dominant"] == 0 && e["dominated"] == 1)
        );
        assert!(!edges.iter().any(|e| e["dominated"] == 2));
        let order: Vec<_> = r["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_u64().unwrap())
            .collect();
        assert!(order.iter().position(|p| *p == 0) < order.iter().position(|p| *p == 1));
        assert_eq!(
            r["patterns"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["ranking"]["new_apply_events"].as_u64().unwrap())
                .sum::<u64>(),
            3
        );
        // A superset that additionally reads an outside event is not certified.
        let external = pattern(3, "external", &[(1, 0), (2, 1), (4, 99)], 1);
        let r = organize(vec![external, pattern(1, "small", &[(1, 0), (2, 1)], 1)]).unwrap();
        assert!(r["observed_dominance"].as_array().unwrap().is_empty());
    }
    #[test]
    fn shared_witness_payloads_roundtrip_and_conflicts_are_rejected() {
        let raw = pattern(0, "a", &[(1, 0), (2, 1)], 1);
        let r = organize(vec![raw.clone()]).unwrap();
        let p = &r["patterns"][0];
        assert_eq!(
            r["step_templates"][p["entry_template"].as_u64().unwrap() as usize]["definition"],
            raw["entry_binding"]
        );
        let i = &p["instances"][0];
        let restored: Vec<_> = i["event_refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| r["event_witnesses"][id.to_string()].clone())
            .collect();
        assert_eq!(json!(restored), raw["instances"][0]["fact_witnesses"]);
        assert_eq!(
            r["extent_templates"][i["extent_template"].as_u64().unwrap() as usize]["definition"],
            raw["instances"][0]["extent"]
        );
        let mut bad = raw.clone();
        bad["id"] = json!(1);
        bad["instances"][0]["fact_witnesses"][0]["outputs"] = json!([999]);
        assert!(organize(vec![raw, bad]).is_err());
    }
}
