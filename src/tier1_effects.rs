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
pub fn execute(source: &str) -> Result<Value, String> {execute_mode(source,false)}
pub fn execute_mode(source: &str,compact:bool) -> Result<Value, String> {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, source)
        .map_err(|e| e.to_string())?;
    eg.parse_and_run_program(None, "(run-schedule (saturate (run tier1)))")
        .map_err(|e| e.to_string())?;
    eg.parse_and_run_program(
        None,
        "(run-schedule (saturate (run tier1_equivalences))) (run-schedule (saturate (run tier1)))",
    )
    .map_err(|e| e.to_string())?;
    export_mode(&eg,compact)
}
/// Generated input contains one complete command per line. Batch parser calls
/// without retaining the entire textual import program.
pub fn run_command_stream(eg: &mut EGraph, reader: impl std::io::BufRead) -> Result<(), String> {
    let mut batch=String::new();
    for line in reader.lines() {
        batch.push_str(&line.map_err(|e|e.to_string())?);batch.push('\n');
        if batch.len()>=65536 {
            eg.parse_and_run_program(None,&batch).map_err(|e|e.to_string())?;batch.clear();
        }
    }
    if !batch.is_empty(){eg.parse_and_run_program(None,&batch).map_err(|e|e.to_string())?;}
    Ok(())
}
/// Consume a generated command stream without an intermediate .egg file.
pub fn execute_stream(reader: impl std::io::BufRead, compact: bool) -> Result<Value, String> {
    let mut eg = EGraph::default();
    let start=std::time::Instant::now();
    run_command_stream(&mut eg,reader)?;
    eprintln!("[native-import] input-and-checks {:.6}s",start.elapsed().as_secs_f64());
    let start=std::time::Instant::now();
    eg.parse_and_run_program(None,"(run-schedule (saturate (run tier1))) (run-schedule (saturate (run tier1_equivalences))) (run-schedule (saturate (run tier1)))").map_err(|e|e.to_string())?;
    eprintln!("[native-import] final-saturation {:.6}s",start.elapsed().as_secs_f64());
    let start=std::time::Instant::now();
    let result=export_mode(&eg,compact);
    eprintln!("[native-import] export {:.6}s",start.elapsed().as_secs_f64());
    result
}
fn validate_independence(eg: &EGraph) -> Result<(), String> {
    if let Some(empty) = eg.lookup_function("Empty", &[]) {
        let mut invalid = false;
        eg.function_for_each("Occurrence", |r| {
            invalid |= r.vals[1] == empty;
        })
        .map_err(|e| e.to_string())?;
        if invalid {
            return Err("Empty is a context unit, not an executed rule occurrence".into());
        }
    }

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
pub fn export(eg: &EGraph) -> Result<Value, String> {export_mode(eg,false)}
fn export_mode(eg: &EGraph,compact:bool) -> Result<Value, String> {
    validate_independence(eg)?;
    let cid = |sort: &str, v| {
        eg.value_to_class_id(eg.get_sort_by_name(sort).unwrap(), v)
            .to_string()
    };
    // The graph is immutable throughout export. Share one cost computation
    // instead of rebuilding an extractor for every occurrence and template.
    let mut roots=vec!["RuleId","RelativeBinding","PartialRelativeBinding","Args"];
    if !compact {roots.push("Effect");}
    let extractor=egglog::extract::Extractor::compute_costs_from_rootsorts(
        Some(roots.into_iter().map(|s|eg.get_sort_by_name(s).unwrap().clone()).collect()),
        eg,egglog::extract::TreeAdditiveCostModel::default());
    let mut dag=egglog::TermDag::default();
    let mut render=|sort:&str,value| {
        let (_,term)=extractor.extract_best_with_sort(eg,&mut dag,value,eg.get_sort_by_name(sort).unwrap().clone()).unwrap();
        dag.to_string(term)
    };
    let mut templates = BTreeMap::<String, Value>::new();
    for name in ["Empty", "SmoothRuleComposition", "CoarseRuleComposition"] {
        eg.function_for_each(name, |row| {
            let id = cid("Comb", *row.vals.last().unwrap());
            if name=="Empty"{templates.insert(id.clone(),json!({"id":id,"kind":"Empty","rule":"","relative_binding":"","parents":[]}));return;}
            let rule_text = render("RuleId",row.vals[1]);
            let rule: String = serde_json::from_str(
                rule_text
                    .strip_prefix("(Rule ")
                    .unwrap()
                    .strip_suffix(')')
                    .unwrap(),
            )
            .unwrap();
            let ports=render(if name=="SmoothRuleComposition" {"RelativeBinding"} else {"PartialRelativeBinding"},row.vals[2]);
            let form=json!({"kind":name,"rule":rule,"relative_binding":ports});
            if let Some(existing)=templates.get_mut(&id){existing["equivalent_forms"].as_array_mut().unwrap().push(form);}else{
                templates.insert(id.clone(),json!({"id":id,"kind":name,"rule":rule,"relative_binding":ports,"parents":[],"equivalent_forms":[form]}));
            }
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
    eg.function_for_each("Occurrence",|r|{let id=cid("Instance",r.vals[2]);instances.insert(id.clone(),json!({"id":id,"event":eg.value_to_base::<i64>(r.vals[0]),"template":cid("Comb",r.vals[1]),"bindings":[],"effects":[],"effect_count":0}));}).map_err(|e|e.to_string())?;
    eg.function_for_each("Binding", |r| {
        instances.get_mut(&cid("Instance", r.vals[0])).unwrap()["bindings"]
            .as_array_mut()
            .unwrap()
            .push(json!(
                render("Args",r.vals[1])
            ));
    })
    .map_err(|e| e.to_string())?;
    eg.function_for_each("Provides",|r|{
        let instance=instances.get_mut(&cid("Instance",r.vals[0])).unwrap();
        instance["effect_count"]=json!(instance["effect_count"].as_u64().unwrap()+1);
        if !compact{instance["effects"].as_array_mut().unwrap().push(json!(render("Effect",r.vals[1])));}
    }).map_err(|e|e.to_string())?;
    let nodes=if compact { template_nodes(eg)? } else {
    let serialized = eg.serialize(egglog::SerializeConfig {
        max_functions: None,
        max_calls_per_function: None,
        include_temporary_functions: true,
        root_eclasses: vec![],
    });
    assert!(serialized.is_complete());
    let nodes=serialized.egraph.nodes.iter().map(|(id,n)|(id.to_string(),json!({"op":n.op,"children":n.children.iter().map(ToString::to_string).collect::<Vec<_>>(),"eclass":n.eclass.to_string(),"subsumed":n.subsumed}))).collect::<BTreeMap<_,_>>();
    nodes
    };
    Ok(
        json!({"relation_sizes":(["LocalResolved","PartialResolved","LocalArgs","PartialArgs","Binding","Provides","ArgsEqual"].into_iter().map(|name|(name,eg.get_size(name))).collect::<BTreeMap<_,_>>()),"templates":templates.into_values().collect::<Vec<_>>(),"instances":instances.into_values().collect::<Vec<_>>(),"native_egraph":{"nodes":nodes},"serialization_complete":!compact,"graph_scope":if compact{"template constructors only; instance effects counted"}else{"full native graph"},"scope":"native tier-1 templates plus occurrence-scoped evidence; supplied witnesses, not an automatic tier-0 importer or compression result"}),
    )
}

fn template_nodes(eg:&EGraph)->Result<BTreeMap<String,Value>,String>{
    let mut nodes=BTreeMap::<String,Value>::new();let mut reps=BTreeMap::new();
    for name in ["Empty","SmoothRuleComposition","CoarseRuleComposition","Rule","NoParents","MoreParents","ParentPort","Make","RNil","RCons","Local","External","MakePartial","PNil","PCons"]{
        let schema=eg.get_function(name).unwrap().schema();
        eg.function_for_each(name,|r|{
            let class=eg.value_to_class_id(&schema.output,*r.vals.last().unwrap()).to_string();
            let id=format!("{name}:{:?}",r.vals);let mut children=vec![];
            for (sort,v) in schema.input.iter().zip(r.vals.iter()){
                let child=eg.value_to_class_id(sort,*v).to_string();
                if !sort.is_eq_sort(){
                    let op=eg.extract_value_to_string(sort,*v).unwrap().0;
                    nodes.entry(child.clone()).or_insert(json!({"op":op,"eclass":child,"children":[],"subsumed":false}));reps.entry(child.clone()).or_insert(child.clone());
                }
                children.push(child);
            }
            reps.entry(class.clone()).or_insert(id.clone());
            nodes.insert(id,json!({"op":name,"eclass":class,"children":children,"subsumed":r.subsumed}));
        }).map_err(|e|e.to_string())?;
    }
    for n in nodes.values_mut(){for c in n["children"].as_array_mut().unwrap(){*c=json!(reps.get(c.as_str().unwrap()).ok_or("missing template child")?);}}
    Ok(nodes)
}
