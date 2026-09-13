//! Observational discovery of A -> m B -> A DAG units. No future facts invented.
use super::*;
#[derive(Clone)]
struct Branch {
    child: usize,
    target: Option<usize>,
    boundary: bool,
}
struct Unit {
    slots: BTreeMap<String, Branch>,
    ambiguous: bool,
}
fn refinement(r: &Record) -> String {
    let mut ids = BTreeMap::new();
    let mut alias = |v: usize| {
        let next = ids.len();
        *ids.entry(v).or_insert(next)
    };
    let input: Vec<_> = r.wanted.iter().map(|v| alias(*v)).collect();
    let output: Vec<_> = r.values.iter().map(|v| alias(*v)).collect();
    let produced: Vec<_> = r.produced.iter().map(|v| alias(*v)).collect();
    let unions: Vec<_> = r
        .unions
        .iter()
        .map(|(a, b)| (alias(*a), alias(*b)))
        .collect();
    json!({"input_aliases":input,"output_aliases":output,"produced_aliases":produced,"union_aliases":unions}).to_string()
}
fn signature(c: &Captured, x: &Extensions, r: &Record) -> String {
    let e = &x.keys[r.extension];
    json!({"rule":c.rules[r.rule].rule.to_string(),"inputs":e.inputs,"routes":e.routes,"refinement":refinement(r)}).to_string()
}
fn refined(r: &Record) -> Expr {
    call(
        "RefinedExtension",
        vec![
            call("ImportedExtension", vec![num(r.extension as u64)]),
            string(&refinement(r)),
        ],
    )
}

pub(super) fn build(
    c: &Captured,
    x: &Extensions,
    eg: &mut EGraph,
    root: &Path,
    linear: &Json,
) -> Result<Json> {
    load(eg, &root.join("rules/recursive_patterns.egg"), root)?;
    eg.parse_and_run_program(
        None,
        "(function ImportedRecursivePattern (i64) Extension :no-merge)",
    )?;
    let eligible = |r: &Record| !r.coarse && r.parents.len() == 1 && x.known[r.extension];
    let signatures: Vec<_> = c.records.iter().map(|r| signature(c, x, r)).collect();
    let mut children = vec![vec![]; c.records.len()];
    for (i, r) in c.records.iter().enumerate() {
        for p in &r.parents {
            children[*p].push(i);
        }
    }
    let mut groups = BTreeSet::<(String, usize)>::new();
    let mut entry_roots = BTreeMap::<String, Vec<usize>>::new();
    for (i, r) in c.records.iter().enumerate().filter(|(_, r)| eligible(r)) {
        entry_roots
            .entry(signatures[i].clone())
            .or_default()
            .push(i);
        for b in &children[i] {
            if eligible(&c.records[*b]) && c.records[*b].rule != r.rule {
                groups.insert((signatures[i].clone(), c.records[*b].rule));
            }
        }
    }
    let mut batch = vec![];
    for r in &c.records {
        emit(
            eg,
            &mut batch,
            fact(
                "StepExtension",
                vec![
                    iref(r.id),
                    call("ImportedExtension", vec![num(r.extension as u64)]),
                ],
            ),
        )?;
    }
    let mut patterns = vec![];
    let mut unsupported = vec![];
    let mut expected_members = vec![];
    let mut expected_returns = vec![];
    let mut expected_views = vec![];
    for (entry_sig, child_rule) in groups {
        let entries = &entry_roots[&entry_sig];
        let mut units = BTreeMap::<usize, Unit>::new();
        let mut votes = BTreeMap::<Vec<String>, Vec<usize>>::new();
        for a in entries {
            let mut slots = BTreeMap::new();
            let mut ambiguous = false;
            for b in children[*a]
                .iter()
                .filter(|b| eligible(&c.records[**b]) && c.records[**b].rule == child_rule)
            {
                let returns: Vec<_> = children[*b]
                    .iter()
                    .filter(|v| eligible(&c.records[**v]) && signatures[**v] == entry_sig)
                    .copied()
                    .collect();
                let all_returns = children[*b]
                    .iter()
                    .filter(|v| c.records[**v].rule == c.records[*a].rule)
                    .count();
                let boundary = all_returns > 1 || returns.len() != all_returns;
                let branch = Branch {
                    child: *b,
                    target: if returns.len() == 1 {
                        Some(returns[0])
                    } else {
                        None
                    },
                    boundary,
                };
                if slots.insert(signatures[*b].clone(), branch).is_some() {
                    ambiguous = true;
                }
            }
            // Learn only from units with actual returning A applies at all ports.
            if !ambiguous
                && !slots.is_empty()
                && slots.values().all(|b| b.target.is_some() && !b.boundary)
            {
                votes
                    .entry(slots.keys().cloned().collect())
                    .or_default()
                    .push(*a);
            }
            units.insert(*a, Unit { slots, ambiguous });
        }
        let supported: Vec<_> = votes
            .iter()
            .filter(|(_, roots)| roots.len() >= 2)
            .map(|(ports, roots)| (ports.clone(), roots.clone()))
            .collect();
        if supported.is_empty() {
            unsupported.push(json!({"entry_signature":serde_json::from_str::<Json>(&entry_sig)?,"branch_rule":c.rules[child_rule].rule.name,"observed_entries":entries.len(),"reason":"fewer than two matching units with a unique closed return at every observed port"}));
        }
        for (ports, training) in &supported {
            let id = patterns.len();
            let pref = || call("ImportedRecursivePattern", vec![num(id as u64)]);
            let representative = &units[&training[0]];
            let port_expr = ports.iter().enumerate().rev().fold(
                call("NoRecursivePorts", vec![]),
                |tail, (slot, key)| {
                    call(
                        "RecursivePort",
                        vec![
                            num(slot as u64),
                            refined(&c.records[representative.slots[key].child]),
                            tail,
                        ],
                    )
                },
            );
            emit(
                eg,
                &mut batch,
                set(
                    "ImportedRecursivePattern",
                    id as u64,
                    call(
                        "RecursivePattern",
                        vec![refined(&c.records[training[0]]), port_expr],
                    ),
                ),
            )?;
            let included: BTreeSet<_> = units
                .iter()
                .filter(|(_, u)| !u.ambiguous && u.slots.keys().all(|k| ports.contains(k)))
                .map(|(i, _)| *i)
                .collect();
            let incoming: BTreeSet<_> = included
                .iter()
                .flat_map(|i| units[i].slots.values().filter_map(|b| b.target))
                .filter(|i| included.contains(i))
                .collect();
            for a in &included {
                emit(
                    eg,
                    &mut batch,
                    fact("RecursiveUnit", vec![iref(c.records[*a].id), pref()]),
                )?;
                for (slot, key) in ports.iter().enumerate() {
                    if let Some(b) = units[a].slots.get(key) {
                        emit(
                            eg,
                            &mut batch,
                            fact(
                                "UnitMember",
                                vec![
                                    iref(c.records[*a].id),
                                    pref(),
                                    num(slot as u64),
                                    iref(c.records[b.child].id),
                                ],
                            ),
                        )?;
                        expected_members.push((*a, id, slot, b.child));
                        if let Some(next) = b.target.filter(|v| included.contains(v)) {
                            emit(
                                eg,
                                &mut batch,
                                fact(
                                    "UnitReturn",
                                    vec![
                                        iref(c.records[*a].id),
                                        pref(),
                                        num(slot as u64),
                                        iref(c.records[next].id),
                                    ],
                                ),
                            )?;
                            expected_returns.push((*a, id, slot, next));
                        }
                    }
                }
            }
            let mut instances = vec![];
            for start in included.difference(&incoming) {
                // Local indices are assigned by structural port order; actual shared
                // targets reuse one index, never duplicated into a fictitious tree.
                let mut nodes = vec![*start];
                let mut local = BTreeMap::from([(*start, 0usize)]);
                let mut incoming_count = BTreeMap::new();
                let mut cursor = 0;
                while cursor < nodes.len() {
                    let a = nodes[cursor];
                    for key in ports {
                        if let Some(target) = units[&a]
                            .slots
                            .get(key)
                            .and_then(|b| b.target)
                            .filter(|v| included.contains(v))
                        {
                            *incoming_count.entry(target).or_insert(0usize) += 1;
                            if !local.contains_key(&target) {
                                let n = nodes.len();
                                local.insert(target, n);
                                nodes.push(target);
                            }
                        }
                    }
                    cursor += 1;
                }
                let mut uniform = BTreeMap::<usize, Option<usize>>::new();
                let mut topo = nodes.clone();
                topo.sort_unstable();
                for a in topo.iter().rev() {
                    let child_depths: Option<Vec<_>> = ports
                        .iter()
                        .map(|key| match units[a].slots.get(key) {
                            None => None,
                            Some(b) if b.boundary => None,
                            Some(b) => match b.target {
                                None => Some(0),
                                Some(t) if included.contains(&t) => uniform[&t],
                                _ => None,
                            },
                        })
                        .collect();
                    let depth = child_depths
                        .filter(|d| d.iter().all(|k| *k == d[0]))
                        .map(|d| d[0] + 1);
                    uniform.insert(*a, depth);
                }
                let depth = uniform[start].filter(|_| incoming_count.values().all(|n| *n == 1));
                if let Some(n) = depth {
                    let mut count = 0usize;
                    let mut width = 1usize;
                    for _ in 0..n {
                        count = count.checked_add(width).ok_or("extent size overflow")?;
                        width = width
                            .checked_mul(ports.len())
                            .ok_or("extent width overflow")?;
                    }
                    if count != nodes.len() {
                        return Err("uniform extent lost shared/extra units".into());
                    }
                }
                let mut node_expr = call("NoExpansionNodes", vec![]);
                let mut observed = vec![];
                let mut members = BTreeSet::new();
                for a in nodes.iter().rev() {
                    let position = local[a];
                    members.insert(*a);
                    emit(
                        eg,
                        &mut batch,
                        fact(
                            "UnitAddress",
                            vec![
                                iref(c.records[*start].id),
                                pref(),
                                num(position as u64),
                                iref(c.records[*a].id),
                            ],
                        ),
                    )?;
                    let mut edges = call("NoExpansionEdges", vec![]);
                    let mut states = vec![];
                    for (slot, key) in ports.iter().enumerate().rev() {
                        let (op, target, child) = match units[a].slots.get(key) {
                            None => ("OpenPort", None, None),
                            Some(b) => {
                                members.insert(b.child);
                                if b.boundary {
                                    ("BoundaryPort", None, Some(b.child))
                                } else if let Some(t) = b.target {
                                    if let Some(local) = local.get(&t) {
                                        ("ExpandTo", Some(*local), Some(b.child))
                                    } else {
                                        ("BoundaryPort", None, Some(b.child))
                                    }
                                } else {
                                    ("ReturnFrontier", None, Some(b.child))
                                }
                            }
                        };
                        let mut args = vec![num(slot as u64)];
                        if let Some(t) = target {
                            args.push(num(t as u64));
                        }
                        args.push(edges);
                        edges = call(op, args);
                        states.push(json!({"port":slot,"state":op,"target":target,"observed_child":child.map(|i|c.records[i].id)}));
                    }
                    states.reverse();
                    let local_children: BTreeSet<_> =
                        units[a].slots.values().map(|b| b.child).collect();
                    let outside: Vec<_> = children[*a]
                        .iter()
                        .filter(|v| !local_children.contains(v))
                        .map(|v| c.records[*v].id)
                        .collect();
                    observed.push(
                        json!({"local":position,"entry_event":c.records[*a].id,"ports":states,"outside_consumers":outside}),
                    );
                    node_expr = call(
                        "ExpansionNode",
                        vec![num(position as u64), edges, node_expr],
                    );
                }
                observed.reverse();
                let extent = depth
                    .map(|n| call("Depth", vec![num(n as u64)]))
                    .unwrap_or_else(|| call("SparseExtent", vec![node_expr]));
                let r = &c.records[*start];
                let expression = call(
                    "FractalComb",
                    vec![
                        extent.clone(),
                        pref(),
                        cref(c.records[r.parents[0]].id),
                        fractal::binding(c, r),
                    ],
                );
                emit(
                    eg,
                    &mut batch,
                    fact("ObservedExtent", vec![iref(r.id), pref(), extent.clone()]),
                )?;
                emit(
                    eg,
                    &mut batch,
                    fact("RecursiveView", vec![iref(r.id), expression.clone()]),
                )?;
                expected_views.push(*start);
                let facts: Vec<_> = members.iter().map(|i| witness(c, &c.records[*i])).collect();
                instances.push(json!({"entry_event":r.id,"start_context_event":c.records[r.parents[0]].id,"extent_kind":if depth.is_some(){"Depth"}else{"SparseExtent"},"depth":depth,"extent":extent.to_string(),"fractal_comb":expression.to_string(),"observed_units":nodes.len(),"observed_apply_events":members.len(),"nodes":observed,"fact_witnesses":facts}));
            }
            patterns.push(json!({"id":id,"kind":"recursive_dag","native_pattern_id":id,"entry_rule":c.rules[c.records[training[0]].rule].rule.name,"branch_rule":c.rules[child_rule].rule.name,"entry_binding":serde_json::from_str::<Json>(&entry_sig)?,"ports":ports.iter().enumerate().map(|(i,p)|json!({"port":i,"binding_transfer":serde_json::from_str::<Json>(p).unwrap(),"return_transfer":"same entry binding interface; F_port = entry_transfer composed with port_transfer"})).collect::<Vec<_>>(),"observed_arity":ports.len(),"learning_witnesses":training.iter().take(2).map(|i|c.records[*i].id).collect::<Vec<_>>(),"additional_returning_witnesses":training.len()-2,"ambiguous_units":units.iter().filter(|(_,u)|u.ambiguous).map(|(i,_)|c.records[*i].id).collect::<Vec<_>>(),"instances":instances}));
        }
    }
    flush(eg, &mut batch)?;
    eg.parse_and_run_program(None, "(run-schedule (saturate (run recursive-patterns)))")?;
    let native_pattern = |id| {
        eg.lookup_function("ImportedRecursivePattern", &[eg.base_to_value(id as i64)])
            .unwrap()
    };
    for (a, id, slot, b) in &expected_members {
        if eg
            .lookup_function(
                "VerifiedMember",
                &[
                    c.records[*a].instance.unwrap(),
                    native_pattern(*id),
                    eg.base_to_value(*slot as i64),
                    c.records[*b].instance.unwrap(),
                ],
            )
            .is_none()
        {
            return Err("recursive member certificate failed".into());
        }
    }
    for (a, id, slot, b) in &expected_returns {
        if eg
            .lookup_function(
                "VerifiedReturn",
                &[
                    c.records[*a].instance.unwrap(),
                    native_pattern(*id),
                    eg.base_to_value(*slot as i64),
                    c.records[*b].instance.unwrap(),
                ],
            )
            .is_none()
        {
            return Err("recursive return certificate failed".into());
        }
    }
    if eg.get_size("VerifiedRecursiveView") != expected_views.len() {
        return Err("recursive view certificate failed".into());
    }
    let mut member_lookup = BTreeMap::new();
    eg.function_for_each("RecursiveMember", |r| {
        member_lookup.insert((r.vals[0], r.vals[1], r.vals[2], r.vals[3]), r.vals[4]);
    })?;
    let mut valid = true;
    eg.function_for_each("RecursiveOutput", |r| {
        let m = member_lookup[&(r.vals[0], r.vals[1], r.vals[2], r.vals[3])];
        valid &= eg
            .lookup_function("OutputAt", &[m, r.vals[4], r.vals[5]])
            .is_some();
    })?;
    eg.function_for_each("RecursiveEffect", |r| {
        let m = member_lookup[&(r.vals[0], r.vals[1], r.vals[2], r.vals[3])];
        valid &= eg.lookup_function("Produced", &[m, r.vals[4]]).is_some();
    })?;
    if !valid {
        return Err("recursive fact address does not match original occurrence".into());
    }
    let native_pattern_count = patterns.len();
    patterns.extend(linear_patterns(c, x, linear, patterns.len()));
    let catalog = catalog::organize(patterns)?;
    let mut report = json!({"scope":"Finite observed recursive templates. Shared definitions preserve binding/effect contracts; dominance is only certified for observed instances with the same entry. No semantic dominance union or arbitrary-depth proof.","native_pattern_count":native_pattern_count,"verified_members":expected_members.len(),"verified_returns":expected_returns.len(),"verified_views":expected_views.len(),"output_addresses":eg.get_size("RecursiveOutput"),"effect_addresses":eg.get_size("RecursiveEffect"),"native_shared_step_templates":eg.get_size("RefinedExtension"),"unsupported_groups":unsupported});
    for (key, value) in catalog.as_object().unwrap() {
        report[key] = value.clone();
    }
    Ok(report)
}

fn witness(c: &Captured, r: &Record) -> Json {
    let token =
        |v: &usize| json!({"sort":c.pool.values[*v].sort,"identity":c.pool.values[*v].label()});
    json!({"event":r.id,"rule":c.rules[r.rule].rule.name,"parents":r.parents.iter().map(|p|c.records[*p].id).collect::<Vec<_>>(),"binding":r.wanted.iter().map(token).collect::<Vec<_>>(),"outputs":r.values.iter().map(token).collect::<Vec<_>>(),"row_effects":r.produced.iter().map(token).collect::<Vec<_>>(),"union_effects":r.unions.iter().map(|(a,b)|json!([token(a),token(b)])).collect::<Vec<_>>()})
}
fn linear_patterns(c: &Captured, x: &Extensions, linear: &Json, first_id: usize) -> Vec<Json> {
    let by_id: BTreeMap<_, _> = c
        .records
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id, i))
        .collect();
    let histories = linear["fact_history"].as_array().unwrap();
    let paths: Vec<Vec<u64>> = histories
        .iter()
        .map(|h| {
            std::iter::once(h["trigger_event"].as_u64().unwrap())
                .chain(
                    h["members"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|m| m["event"].as_u64().unwrap()),
                )
                .collect()
        })
        .collect();
    let mut grouped = BTreeMap::<usize, Vec<&Vec<u64>>>::new();
    for p in &paths {
        if !paths.iter().any(|q| q.len() > p.len() && q.starts_with(p)) {
            grouped
                .entry(c.records[by_id[&p[1]]].extension)
                .or_default()
                .push(p);
        }
    }
    grouped.into_iter().enumerate().map(|(n,(ext,paths))|{
        let first=&c.records[by_id[&paths[0][1]]];let key=&x.keys[ext];
        let entry=json!({"rule":c.rules[first.rule].rule.to_string(),"inputs":key.inputs,"routes":key.routes,"refinement":null,"alias_policy":"preserved in occurrence witnesses; not assumed invariant"});
        let instances:Vec<_>=paths.iter().map(|path|{
            let first=&c.records[by_id[&path[1]]];let depth=path.len()-1;
            let extent=call("Depth",vec![num(depth as u64)]);
            let expr=call("FractalComb",vec![extent.clone(),call("ImportedExtension",vec![num(ext as u64)]),cref(path[0]),fractal::binding(c,first)]);
            json!({"entry_event":path[1],"start_context_event":path[0],"extent_kind":"Depth","depth":depth,"extent":extent.to_string(),"fractal_comb":expr.to_string(),"observed_units":depth,"observed_apply_events":depth,"nodes":path[1..].iter().enumerate().map(|(j,id)|json!({"local":j,"entry_event":id,"ports":[],"next":if j+1<depth{Some(j+1)}else{None}})).collect::<Vec<_>>(),"fact_witnesses":path[1..].iter().map(|id|witness(c,&c.records[by_id[id]])).collect::<Vec<_>>()})
        }).collect();
        json!({"id":first_id+n,"kind":"linear_extension","native_pattern_id":null,"entry_rule":c.rules[first.rule].rule.name,"branch_rule":null,"entry_binding":entry,"ports":[],"observed_arity":1,"learning_witnesses":paths.iter().map(|p|p[1]).collect::<Vec<_>>(),"additional_returning_witnesses":0,"ambiguous_units":[],"instances":instances})
    }).collect()
}
