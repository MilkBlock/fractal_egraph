//! Incremental history evidence, grouped only by completed exact Cell certificates.
//! Counts are not all-path counts, allocated bytes, or measured runtime savings.
use super::*;

#[derive(Default)]
struct Evidence {
    binding: Option<String>,
    fields: usize,
    external: BTreeSet<usize>,
}
#[derive(Default)]
struct EventEvidence {
    facts: BTreeSet<usize>,
    effects: BTreeSet<usize>,
    union_events: BTreeSet<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alternative_proofs_do_not_multiply_bindings_and_unfinished_is_unrated() {
        let mut census=Census::default();
        let mut store=cs::Store::default();
        for (unit,key,facts) in [(0,"same",vec![1,2]),(1,"same",vec![1,2]),(2,"other",vec![2,3]),(3,"unfinished",vec![4])] {
            census.entries.insert(unit,Evidence { binding:Some(key.into()),fields:1,..Default::default() });
            census.events.insert(unit,EventEvidence {facts:facts.iter().copied().collect(),effects:facts.into_iter().collect(),..Default::default()});
            store.units.push(cs::Unit {coarse_layer:None,coarse:vec![unit],smooth:vec![],source_composition:None,depth:0});
        }
        let registry=json!({"units":[{"unit":0,"eclass":0},{"unit":1,"eclass":0},{"unit":2,"eclass":0},{"unit":3,"eclass":1}]});
        let catalog=json!({"catalog":{"triggers":[
            {"binding_origin":{"candidate_id":0},"saturated_rule_composition":0},
            {"binding_origin":{"candidate_id":1},"saturated_rule_composition":0},
            {"binding_origin":{"candidate_id":2},"saturated_rule_composition":0}]},
            "states":[{"rows":[{"op":"A","args":[0],"result":1}]}]});
        let r=census.report(&registry,Some(&catalog),&store);
        let cell=&r["cells"][0];
        assert_eq!(cell["registered_constructions"],3);
        assert_eq!(cell["interface_instances"],2);
        assert_eq!(cell["historical_fact_coverage"],3);
        assert_eq!(cell["overlapping_fact_references"],1);
        assert_eq!(r["unrated_entries"],1);
        assert_eq!(r["cells"].as_array().unwrap().len(),1);
        assert_eq!(census.report(&registry,None,&store)["cells"],json!([]));
    }
}
#[derive(Default)]
pub(super) struct Census { entries: BTreeMap<usize, Evidence>, events: BTreeMap<usize,EventEvidence> }

impl Census {
    pub fn register(&mut self, unit: usize, c: &Captured, members: &[usize], origin: &Json) {
        if self.entries.contains_key(&unit) { return }
        let mut e = Evidence::default();
        let mut supplied=BTreeSet::<usize>::new();
        for &i in members {
            let r = &c.records[i];
            e.external.extend(r.required.iter().filter(|f| !supplied.contains(*f)).copied());
            supplied.extend(r.produced.iter().copied());
            self.events.entry(i).or_insert_with(|| EventEvidence {
                facts:r.required.iter().chain(&r.produced).copied().collect(),
                effects:r.produced.iter().copied().collect(),
                union_events:if r.unions.is_empty(){BTreeSet::new()}else{BTreeSet::from([r.id])},
            });
        }
        let mut binding = vec![];
        let mut complete = origin["parameters"].is_array() && origin["reuse_interface_terms"]["upper"].is_array();
        for (i, p) in origin["parameters"].as_array().into_iter().flatten().enumerate() {
            complete &= p["token"].is_u64();
            binding.push(json!(["input",i,p["sort"],p["token"]]));
        }
        for (i, term) in origin["reuse_interface_terms"]["upper"].as_array().into_iter().flatten().enumerate() {
            let values: Vec<_> = origin["symbolic_values"].as_array().into_iter().flatten()
                .filter(|v| v["term"] == *term).map(|v| json!([v["sort"],v["token"]])).collect();
            complete &= !values.is_empty();
            binding.push(json!(["output",i,values]));
        }
        e.fields = binding.len();
        if complete { e.binding = Some(json!([binding,origin["analysis_program"]["instance_inputs"]]).to_string()); }
        self.entries.insert(unit,e);
    }

    fn coverage(&self, units: impl Iterator<Item=usize>, store: &cs::Store) -> EventEvidence {
        let mut todo:Vec<_>=units.collect();let mut visited=BTreeSet::new();let mut records=BTreeSet::new();
        while let Some(unit)=todo.pop() {
            if !visited.insert(unit) {continue}
            let u=&store.units[unit];records.extend(u.coarse.iter().copied());
            if let Some(comp)=u.source_composition {todo.extend(store.compositions[comp].parts.iter().copied());}
        }
        let mut out=EventEvidence::default();
        for record in records {
            if let Some(e)=self.events.get(&record) {
                out.facts.extend(&e.facts);out.effects.extend(&e.effects);out.union_events.extend(&e.union_events);
            }
        }
        out
    }
    pub fn report(&self, registry: &Json, catalog: Option<&Json>, store: &cs::Store) -> Json {
        let classes: BTreeMap<_,_> = registry["units"].as_array().into_iter().flatten()
            .filter_map(|u| Some((u["unit"].as_u64()? as usize,u["eclass"].as_u64()?))).collect();
        let mut certified = BTreeMap::new();
        let mut markers = BTreeSet::new();
        if let Some(cat) = catalog {
            for t in cat["catalog"]["triggers"].as_array().into_iter().flatten() {
                if let (Some(u),Some(f)) = (t["binding_origin"]["candidate_id"].as_u64(),t["saturated_rule_composition"].as_u64()) {
                    certified.insert(u as usize,f as usize);
                }
                let o=&t["binding_origin"];
                if let Some(m)=o["parameter_marker"].as_str() { markers.insert(m.to_owned()); }
                for m in o["parameter_markers"].as_object().into_iter().flat_map(|m|m.values()) {
                    if let Some(m)=m.as_str() { markers.insert(m.to_owned()); }
                }
            }
        }
        let mut groups = BTreeMap::<u64,Vec<usize>>::new();
        for &u in certified.keys() {
            if self.entries.contains_key(&u) {
                if let Some(&class)=classes.get(&u) { groups.entry(class).or_default().push(u); }
            }
        }
        let mut result=vec![];
        let mut windows_by_unit=BTreeMap::<usize,Vec<&cs::Window>>::new();
        for w in &store.windows { windows_by_unit.entry(w.unit).or_default().push(w); }
        for (class,units) in groups {
            let mut instances=BTreeMap::<&str,(Vec<usize>,usize)>::new();
            let mut external=BTreeSet::<usize>::new();
            let evidence=self.coverage(units.iter().copied(),store);
            let mut unknown=0;
            for u in &units {
                let e=&self.entries[u];
                external.extend(&e.external);
                if let Some(key)=e.binding.as_deref() {
                    let item=instances.entry(key).or_default(); item.0.push(*u); item.1=e.fields;
                } else { unknown+=1; }
            }
            let f=certified[&units[0]];
            let body=catalog.map(|c|&c["states"][f]);
            let rows:Vec<_>=body.into_iter().flat_map(|b|b["rows"].as_array().into_iter().flatten())
                .filter(|r|!markers.contains(r["op"].as_str().unwrap_or(""))).collect();
            let body_slots:usize=rows.iter().map(|r|r["args"].as_array().map_or(0,Vec::len)+1).sum();
            let binding_slots:usize=instances.values().map(|(_,n)|n+1).sum(); // one template reference
            let n=instances.len();
            let mut sum_occurrence_facts:usize=0;let mut known_facts=BTreeSet::<usize>::new();
            for (us,_) in instances.values() {
                let e=self.coverage(us.iter().copied(),store);sum_occurrence_facts+=e.facts.len();known_facts.extend(e.facts);
            }
            let views:Vec<_>=units.iter().flat_map(|u|windows_by_unit.get(u).into_iter().flatten().copied()).collect();
            let window_units:BTreeSet<_>=views.iter().filter(|w|w.new_entry).map(|w|w.unit).collect();
            result.push(json!({"eclass":class,"catalog_state":f,"units":units,
                "registered_constructions":units.len(),"interface_instances":n,"unknown_interface_entries":unknown,
                "historical_fact_coverage":evidence.facts.len(),"historical_effect_fact_coverage":evidence.effects.len(),"historical_union_events":evidence.union_events.len(),
                "external_fact_coverage":external.len(),"overlapping_fact_references":sum_occurrence_facts.saturating_sub(known_facts.len()),
                "window_readings":views.len(),"new_window_entries":window_units.len(),
                "window_sources":views.iter().map(|w|w.container).collect::<BTreeSet<_>>(),
                "closure_rows":rows.len(),"closure_value_slots":body_slots,"instance_reference_slots":binding_slots,
                "isolated_description_gain":if unknown==0 {Some(n as i64*body_slots as i64-body_slots as i64-binding_slots as i64)}else{None}}));
        }
        result.sort_by(|a,b| b["interface_instances"].as_u64().cmp(&a["interface_instances"].as_u64())
            .then_with(||b["historical_effect_fact_coverage"].as_u64().cmp(&a["historical_effect_fact_coverage"].as_u64()))
            .then_with(||a["eclass"].as_u64().cmp(&b["eclass"].as_u64())));
        json!({"schema":"cell-reuse-value/v1","cells":result,
            "unrated_entries":self.entries.len().saturating_sub(certified.len()),
            "frequency_scope":"distinct ordered typed input/output token signatures in this history; versions retained, symmetries and later union aliases not quotiented; not physical live-instance counts",
            "construction_scope":"registered replay entries, not all DAG paths or match counts",
            "coverage_scope":"versioned historical fact certificates; overlapping Cell coverage must not be summed as e-node coverage",
            "gain_scope":"isolated description model: n*closure_value_slots - closure_value_slots - sum(interface_fields+1); assumes separate bodies, excludes overlap/hashcons/indices/retired markers; NOT measured memory or speed savings",
            "ranking":"interface_instances descending, then distinct observed effect facts; display only, no candidates pruned"})
    }
}
