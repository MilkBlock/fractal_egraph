//! Bounded discovery of a growing side family along an observed fractal spine.
//! Coordinates come from birth/parent edges, never timestamps or sibling order.
use super::*;
#[derive(Clone)]
struct Observation {
    event: u64,
    level: usize,
    parents: Vec<u64>,
    recipe: String,
    boundary: bool,
    witness: Json,
}
fn assess(levels: usize, observations: &[Observation]) -> Json {
    let training = 3;
    let events: BTreeSet<_> = observations.iter().map(|o| o.event).collect();
    let event_levels: BTreeMap<_, _> = observations.iter().map(|o| (o.event, o.level)).collect();
    let mut coordinates = BTreeMap::<u64, usize>::new();
    let mut rows = vec![];
    let mut exceptions = vec![];
    for d in 0..levels {
        let layer: Vec<_> = observations.iter().filter(|o| o.level == d).collect();
        let births = layer
            .iter()
            .filter(|o| o.parents.is_empty() && !o.boundary)
            .count();
        for o in layer {
            let position = if o.boundary {
                None
            } else if o.parents.is_empty() && births == 1 {
                Some(d)
            } else if o.parents.len() == 1 {
                let p = o.parents[0];
                event_levels
                    .get(&p)
                    .filter(|level| **level + 1 == d)
                    .and_then(|_| coordinates.get(&p).copied())
            } else {
                None
            };
            if let Some(j) = position {
                coordinates.insert(o.event, j);
                rows.push(json!({"event":o.event,"d":d,"j":j,"mode":if o.parents.is_empty(){"birth"}else{"inherit"},"recipe":o.recipe,"witness":o.witness}));
            } else {
                exceptions.push(json!({"event":o.event,"d":d,"reason":if o.boundary || o.parents.iter().any(|p|!events.contains(p)){"external or unresolved boundary"}else if o.parents.is_empty(){"ambiguous births: no coordinate chosen from execution order"}else{"not a single previous-layer predecessor"},"witness":o.witness}));
            }
        }
    }
    rows.sort_by_key(|r| (r["d"].as_u64(), r["j"].as_u64(), r["event"].as_u64()));
    let mut templates = BTreeMap::<String, usize>::new();
    for row in &rows {
        if row["d"].as_u64().unwrap() < training as u64 {
            let key = format!(
                "{}:{}",
                row["mode"].as_str().unwrap(),
                row["recipe"].as_str().unwrap()
            );
            let next = templates.len();
            templates.entry(key).or_insert(next);
        }
    }
    let mut layer_results = vec![];
    let mut shape_ok = true;
    let mut recipe_ok = true;
    for d in 0..levels {
        let layer: Vec<_> = rows.iter().filter(|r| r["d"] == d).collect();
        let positions: BTreeSet<_> = layer
            .iter()
            .map(|r| r["j"].as_u64().unwrap() as usize)
            .collect();
        let full = positions == (0..=d).collect() && layer.len() == d + 1;
        let boundary_only = positions == BTreeSet::from([d]) && layer.len() == 1;
        let unseen: Vec<_> = layer
            .iter()
            .filter(|r| {
                !templates.contains_key(&format!(
                    "{}:{}",
                    r["mode"].as_str().unwrap(),
                    r["recipe"].as_str().unwrap()
                ))
            })
            .map(|r| r["event"].clone())
            .collect();
        shape_ok &= full;
        recipe_ok &= unseen.is_empty();
        layer_results.push(json!({"d":d,"observed":observations.iter().filter(|o|o.level==d).count(),"mapped":layer.len(),"positions":positions,"growing_domain_matches":full,"boundary_only":boundary_only,"unseen_recipe_events":unseen,"split":if d<training{"training"}else{"held_out"}}));
    }
    // A training fit is not enough: at least one held-out level is required.
    let status = if levels <= training {
        "insufficient_held_out_levels"
    } else if !shape_ok {
        "observed_domain_mismatch"
    } else if !recipe_ok {
        "unseen_binding_or_effect_recipe"
    } else if !exceptions.is_empty() {
        "held_out_fit_with_boundary"
    } else {
        "held_out_fit"
    };
    for row in &mut rows {
        let key = format!(
            "{}:{}",
            row["mode"].as_str().unwrap(),
            row["recipe"].as_str().unwrap()
        );
        row["template"] = templates
            .get(&key)
            .copied()
            .map(Json::from)
            .unwrap_or(Json::Null);
    }
    let mut template_views:Vec<_>=templates.iter().map(|(key,id)| {
        let (mode,recipe)=key.split_once(':').unwrap();
        json!({"id":id,"mode":mode,"binding_effect_recipe":serde_json::from_str::<Json>(recipe).unwrap_or_else(|_|json!(recipe))})
    }).collect();
    template_views.sort_by_key(|r| r["id"].as_u64());
    json!({"status":status,"candidate":{"domain":"0 <= j <= d","birth":"j = d","inherit":"(d-1,j) -> (d,j)","training_levels":training},"observed_events":observations.len(),"mapped_events":rows.len(),"templates":template_views,"layers":layer_results,"instances":rows,"exceptions":exceptions,"proof_status":"finite observation only; neither completeness of the frontier nor an inductive invariant is proved"})
}

fn semantic_role(c: &Captured, p: usize, j: usize) -> Option<String> {
    let r = &c.records[p];
    let info = &c.rules[r.rule];
    match &r.outputs[j] {
        Output::Column(span, _) | Output::Row(span) => {
            let (_, Expr::Call(_, op, _)) = info.calls.get(span)? else {
                return None;
            };
            let n = info
                .calls
                .values()
                .filter(|(path, e)| {
                    path.starts_with("head/") && matches!(e,Expr::Call(_,name,_) if name==op)
                })
                .count();
            if n == 1 {
                Some(match &r.outputs[j] {
                    Output::Column(_, col) => format!("{op}:column:{col}"),
                    _ => format!("{op}:row"),
                })
            } else {
                role(c, p, j)
            }
        }
        _ => role(c, p, j),
    }
}

pub(super) fn analyze(c: &Captured, fractal: &Json) -> Json {
    let histories = fractal["fact_history"].as_array().unwrap();
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
    let maximal: Vec<_> = paths
        .iter()
        .filter(|p| !paths.iter().any(|q| q.len() > p.len() && q.starts_with(p)))
        .collect();
    let spine_count = maximal.len();
    let mut reports = vec![];
    let mut unclassified = vec![];
    for spine in maximal {
        let positions: BTreeMap<_, _> = spine.iter().enumerate().map(|(d, id)| (*id, d)).collect();
        let mut groups = BTreeMap::<String, Vec<usize>>::new();
        for (i, r) in c.records.iter().enumerate() {
            if positions.contains_key(&r.id)
                || !r
                    .parents
                    .iter()
                    .any(|p| positions.contains_key(&c.records[*p].id))
            {
                continue;
            }
            // One output-table family; multi-output rules retain the whole set.
            let family: BTreeSet<_> = r
                .produced
                .iter()
                .map(|v| c.pool.values[*v].sort.to_string())
                .collect();
            if family.is_empty() {
                unclassified.push(json!({"spine":spine,"event":r.id,"reason":"no direct inserted row family; deduplication, union-only and other outcomes are not modeled"}));
                continue;
            }
            groups.entry(format!("{family:?}")).or_default().push(i);
        }
        for (family, indices) in groups {
            let members: BTreeSet<_> = indices.iter().copied().collect();
            let anchored: BTreeMap<_, _> = indices
                .iter()
                .map(|i| {
                    (
                        *i,
                        c.records[*i]
                            .parents
                            .iter()
                            .filter_map(|p| positions.get(&c.records[*p].id))
                            .copied()
                            .max()
                            .unwrap(),
                    )
                })
                .collect();
            let observations:Vec<_>=indices.iter().map(|i|{
                let r=&c.records[*i];let d=anchored[i];let mut boundary=false;
                let parents:Vec<_>=r.parents.iter().filter(|p|!positions.contains_key(&c.records[**p].id)).map(|p|{if !members.contains(p){boundary=true;} c.records[*p].id}).collect();
                let mut aliases=BTreeMap::new();
                let mut alias=|v:usize|{let n=aliases.len();*aliases.entry(v).or_insert(n)};
                let inputs:Vec<_>=r.wanted.iter().map(|v|alias(*v)).collect();
                let outputs:Vec<_>=r.values.iter().map(|v|alias(*v)).collect();
                let effects:Vec<_>=r.produced.iter().map(|v|alias(*v)).collect();
                let unions:Vec<_>=r.unions.iter().map(|(a,b)|(alias(*a),alias(*b))).collect();
                // Named values may be available through several equal-valued
                // outputs. Prefer the branch's constructor interface consistently.
                // Physical read witnesses keep their actual producer route.
                let mut normalized_sources=vec![];
                let routes:Vec<_>=r.ports.iter().enumerate().map(|(slot,port)| {
                    let chosen=if matches!(r.inputs[slot],Input::Var(_)) {
                        r.parents.iter().flat_map(|p|c.records[*p].values.iter().enumerate().filter(move |(_,v)|**v==r.wanted[slot]).map(move |(j,_)|(*p,j)))
                            .min_by_key(|(p,j)|(if members.contains(p){0}else if positions.contains_key(&c.records[*p].id){1}else{2},if matches!(c.records[*p].outputs[*j],Output::Column(..)){0}else{1},semantic_role(c,*p,*j)))
                    }else {match port {Port::Parent(k,j)=>Some((r.parents[*k],*j)),_=>None}};
                    if let Some((p,j))=chosen {
                        let pr=&c.records[p];
                        let address=if let Some(pd)=positions.get(&pr.id){format!("spine[{}]",*pd as i64-d as i64)}else if let Some(pd)=anchored.get(&p){format!("branch[{},same-j]",*pd as i64-d as i64)}else{boundary=true;"outside-parent".into()};
                        let source_role=semantic_role(c,p,j).unwrap_or_else(||{boundary=true;"unmapped".into()});
                        normalized_sources.push(json!([pr.id,j]));
                        format!("{address}:{source_role}:{}",c.pool.values[r.wanted[slot]].sort)
                    }else{
                        boundary=true;normalized_sources.push(Json::Null);
                        match port {Port::External(k)=>format!("outside:{k}"),_=>"unmapped".into()}
                    }
                }).collect();
                let recipe=json!({"rule":c.rules[r.rule].rule.name,"source_rule":c.rules[r.rule].rule.to_string(),"input_roles":format!("{:?}",r.inputs.iter().map(|x|match x{Input::Var(n)=>format!("var:{n}"),Input::Read(s)=>c.rules[r.rule].calls.get(s).map(|x|x.0.clone()).unwrap_or_else(||"unmapped".into())}).collect::<Vec<_>>()),"routes":routes,"input_aliases":inputs,"output_aliases":outputs,"produced_aliases":effects,"union_aliases":unions}).to_string();
                let witness=json!({"original_routes":format!("{:?}",r.ports),"normalized_sources":normalized_sources,"rule":c.rules[r.rule].rule.name,"parents":r.parents.iter().map(|p|c.records[*p].id).collect::<Vec<_>>(),"inputs":r.wanted.iter().map(|v|c.pool.values[*v].label()).collect::<Vec<_>>(),"outputs":r.values.iter().map(|v|c.pool.values[*v].label()).collect::<Vec<_>>(),"row_effects":r.produced.iter().map(|v|c.pool.values[*v].label()).collect::<Vec<_>>(),"union_effects":r.unions.iter().map(|(a,b)|(c.pool.values[*a].label(),c.pool.values[*b].label())).collect::<Vec<_>>()});
                Observation{event:r.id,level:d,parents,recipe,boundary,witness}
            }).collect();
            let mut report = assess(spine.len(), &observations);
            report["spine"] = json!(spine);
            report["family"] = json!(family);
            reports.push(report);
        }
    }
    let fits = reports
        .iter()
        .filter(|r| r["status"].as_str().unwrap().starts_with("held_out_fit"))
        .count();
    json!({"scope":"Offline bounded analysis of actual direct consumers of maximal witnessed spines. Coordinates use predecessor edges; no new tier-0 facts or reordering rules are created.","maximal_spines":spine_count,"unclassified":unclassified,"candidate_families":reports.len(),"held_out_fits":fits,"reports":reports})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn triangle(levels: usize) -> Vec<Observation> {
        (0..levels)
            .flat_map(|d| {
                (0..=d).map(move |j| Observation {
                    event: (d * 100 + j) as u64,
                    level: d,
                    parents: if j == d {
                        vec![]
                    } else {
                        vec![((d - 1) * 100 + j) as u64]
                    },
                    recipe: if j == d { "birth" } else { "inherit" }.into(),
                    boundary: false,
                    witness: json!({}),
                })
            })
            .collect()
    }
    #[test]
    fn predecessor_coordinates_ignore_sibling_execution_order() {
        let a = triangle(6);
        let mut b = a.clone();
        b.reverse();
        let x = assess(6, &a);
        let y = assess(6, &b);
        assert_eq!(x, y);
        assert_eq!(x["status"], "held_out_fit");
        assert_eq!(x["mapped_events"], 21);
        assert_eq!(
            assess(3, &triangle(3))["status"],
            "insufficient_held_out_levels"
        );
    }
    #[test]
    fn equal_counts_do_not_hide_wrong_edges_or_bindings() {
        let mut a = triangle(5);
        a.iter_mut().find(|o| o.event == 400).unwrap().parents = vec![301];
        assert_eq!(assess(5, &a)["status"], "observed_domain_mismatch");
        let mut b = triangle(5);
        b.last_mut().unwrap().recipe = "changed binding/union alias".into();
        assert_eq!(assess(5, &b)["status"], "unseen_binding_or_effect_recipe");
    }
    #[test]
    fn missing_or_ambiguous_positions_are_not_invented() {
        let mut a = triangle(5);
        a.retain(|o| o.event != 400);
        let report = assess(5, &a);
        assert_eq!(report["status"], "observed_domain_mismatch");
        assert_eq!(report["mapped_events"], 14);
        let mut b = triangle(5);
        let mut extra = b[0].clone();
        extra.event = 999;
        b.push(extra);
        let report = assess(5, &b);
        assert!(!report["exceptions"].as_array().unwrap().is_empty());
        assert!(
            !report["instances"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["event"] == 999)
        );
    }
}
