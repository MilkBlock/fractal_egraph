//! Compile native typed egglog to the standalone Cell CPU instruction format.

#[cfg(feature = "spine-blocks")]
mod spine_blocks;
#[cfg(feature = "spine-blocks")]
pub use spine_blocks::bake_spine_blocks;
#[cfg(feature="two-run-blocks")]
pub(crate) use spine_blocks::signature as rule_signature;
mod recipe;
use crate::{
    multistage::{extend, source},
    program::*,
};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};

fn points(
    p: &Program,
    r: &Rule,
    mut left: BTreeSet<usize>,
    mut known: BTreeSet<usize>,
) -> Vec<usize> {
    let mut order = vec![];
    loop {
        let ready=left.iter().filter_map(|&i|{let a=&r.query[i];let (out,key)=a.args.split_last()?;
            let bound=|v:&Arg|match v{Arg::Literal(_)=>true,Arg::Slot(s)=>known.contains(s)};
            let priority=if key.iter().all(bound){usize::from(matches!(a.call,Call::Table(_)))}else if matches!(a.call,Call::Table(t) if p.tables[t].schema.last().unwrap()["kind"]=="eq") && matches!(out,Arg::Slot(s) if known.contains(s)){2}else{return None};Some((priority,i))}).min();
        let Some((_, i)) = ready else { break };
        left.remove(&i);
        order.push(i);
        known.extend(
            r.query[i]
                .args
                .iter()
                .filter_map(|a| if let Arg::Slot(s) = a { Some(*s) } else { None }),
        );
    }
    order
}
fn multistage(p: &Program) -> J {
    let mut readers = vec![vec![]; p.tables.len()];
    let (mut entries, mut finishes, mut routes, mut loaders) = (vec![], vec![], vec![], vec![]);
    for (rid, r) in p.rules.iter().enumerate() {
        let all = (0..r.query.len()).collect::<BTreeSet<_>>();
        let mut rest = all.clone();
        let mut slots = BTreeSet::new();
        for (i, a) in r.query.iter().enumerate() {
            if let Call::Table(t) = a.call {
                readers[t].push((rid, i));
                rest.remove(&i);
                let known = a
                    .args
                    .iter()
                    .filter_map(|v| if let Arg::Slot(s) = v { Some(*s) } else { None })
                    .collect::<BTreeSet<_>>();
                slots.extend(&known);
                let mut tail = all.clone();
                tail.remove(&i);
                let atoms = points(p, r, tail, known);
                if !atoms.is_empty() {
                    entries.push(json!({"rule":rid,"anchor":i,"atoms":atoms}));
                }
            }
        }
        finishes.push(points(p, r, rest, slots));
    }
    for (producer, r) in p.rules.iter().enumerate() {
        if !r.query.iter().any(|a| matches!(a.call, Call::Table(_))) {
            loaders.push(producer);
            continue;
        }
        let local = [
            ("query", r.query.len()),
            ("effect", p.templates[r.template].ops.len()),
        ]
        .into_iter()
        .flat_map(|(kind, len)| {
            (0..len).filter_map(move |i| {
                source(p, producer, kind, i)
                    .ok()
                    .map(|(t, cols)| (kind, i, t, cols))
            })
        })
        .collect::<Vec<_>>();
        for (_, effect, t, cols) in local.iter().filter(|a| a.0 == "effect") {
            for &(consumer, anchor) in &readers[*t] {
                let q = &p.rules[consumer].query;
                let mut mapping = BTreeMap::new();
                if extend(&q[anchor], *t, cols, &mut mapping).is_err() {
                    continue;
                }
                let mut remaining = (0..q.len())
                    .filter(|i| *i != anchor)
                    .collect::<BTreeSet<_>>();
                let mut steps = vec![];
                loop {
                    let n = remaining.len();
                    for i in remaining.clone() {
                        let a = &q[i];
                        if !a.args.iter().any(|v| {
                            matches!(v, Arg::Literal(_))
                                || matches!(v,Arg::Slot(s) if mapping.contains_key(s))
                        }) {
                            continue;
                        }
                        let mut matches = BTreeMap::new();
                        for (kind, idx, table, row) in &local {
                            let mut m = mapping.clone();
                            if extend(a, *table, row, &mut m).is_ok() {
                                matches.entry(row).or_insert((kind, idx, m));
                                if matches.len() > 1 {
                                    break;
                                }
                            }
                        }
                        if matches.len() == 1 {
                            let (columns, (kind, idx, m)) = matches.into_iter().next().unwrap();
                            mapping = m;
                            let mut step = json!({"atom":i,"source_kind":kind,"source_index":idx});
                            if *kind == "effect" {
                                step["assignments"] =
                                    json!(crate::multistage::assignments(&q[anchor], a, columns));
                            }
                            steps.push(step);
                            remaining.remove(&i);
                        }
                    }
                    if remaining.len() == n {
                        break;
                    }
                }
                if !steps.is_empty()
                    || q.iter()
                        .filter(|a| matches!(a.call, Call::Table(_)))
                        .count()
                        == 1
                {
                    routes.push(json!({"producer":producer,"effect":effect,"consumer":consumer,"anchor":anchor,"steps":steps,"residual":remaining}));
                }
            }
        }
    }
    let ir = &p.artifact["recipe"];
    let sets = ir["rulesets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| json!({"name":s["name"],"rule_ids":s["rule_ids"]}))
        .collect::<Vec<_>>();
    json!({"schema":"eggfuse-multistage/v1","recipe_sha256":ir["sha256"],"control":ir["control"],"rule_sets":sets,"routes":routes,"entries":entries,"finishes":finishes,"stage_entries":crate::stage_spine::compile_entries(p),"data_loaders":loaders,
        "contract":{"activation":"committed producer fact; current visible exhaustive projection","publication":"original Run occurrence, after all entry bindings are gathered","late_input":"ordinary delta route remains authoritative","alias_alternatives":"residual matching whenever a projection is not exhaustive"}})
}

pub fn compile(core: J, markers: &BTreeSet<String>) -> Result<J> {
    let ir = recipe::build(&core)?;
    let mut templates = vec![];
    for (rid, r) in array(&ir["rules"])?.iter().enumerate() {
        let reads = array(&r["interface"]["rows"])?;
        templates.push(json!({"rule":rid,"fact_operations":array(&r["effects"])?.iter().enumerate().filter_map(|(i,e)|matches!(e["kind"].as_str(),Some("construct"|"set")).then_some(i)).collect::<Vec<_>>(),"direct_entry":reads.len()==1,"entry_table":if reads.len()==1{reads[0]["table"].clone()}else{J::Null},"primitive_guards":r["interface"]["primitive_atoms"]}));
    }
    let mut j = json!({"schema":"eggfuse-cell-runtime/v1","initial":core["initial"],"requirements":recipe::requirements(&core),"source_core":core,"recipe":ir,"templates":templates,"contract":{"storage":"shared effect recipes plus instance slots and fact-position masks","native_runtime_dependency":false,"native_free_join":false,"completeness":"exact residual virtual-fact matching; no learned exclusion"}});
    let p = Program::load(j, true)?;
    let multi = multistage(&p);
    let contexts = crate::local_context::derive(&p, &multi["stage_entries"]);
    let spine = multi["stage_entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["rule"].as_u64().unwrap() as usize)
        .collect();
    let (known_results, known_json) = crate::known_results::Plans::derive(&p)?;
    #[cfg(not(feature = "rhs-locals"))]
    let _ = known_results;
    let plans = [
        (
            "view_consumer_blocks",
            json!(crate::view_plan::derive(&p, &spine)),
        ),
        ("single_writes", json!(&p.writes)),
        ("neighbor_entries", json!(&p.neighbor_plans)),
        ("row_kernels", json!(&p.kernels)),
        ("physical_plan", json!(&p.physical)),
        (
            "merge_contracts",
            json!({"schema":"eggfuse-merge-contracts/v1","tables":p.tables.iter().map(|t|crate::merge_contract::table(&p,t)).collect::<Result<Vec<_>>>()?}),
        ),
        ("known_results", known_json),
        ("entry_schedule", crate::entry_schedule::Plan::derive(&p)?.1),
        ("merge_programs", crate::merge_programs::compile(&p)?),
        ("multi_stage", multi),
        ("local_contexts", json!(contexts)),
        (
            "context_cuts",
            json!(crate::local_context::layout(
                &crate::local_context::derive_cuts(&p)
            )),
        ),
        ("marker_tables", json!(markers)),
    ];
    let entry = crate::entry_join::Plan::derive(&p);
    #[cfg(feature = "structural-depth")]
    let structural = json!(crate::depth_plan::Plan::derive(&p));
    #[cfg(feature = "rhs-locals")]
    let local_blocks = json!(crate::local_rhs::derive(&p, &known_results));
    #[cfg(feature="origin-demand")] let demand=json!(p.origin_required);
    #[cfg(feature = "deferred-incidence")]
    let deferred = json!(p.deferred_incidence);
    #[cfg(feature = "port-frames")]
    let ports=json!(crate::graph::port_frames::derive(&p,&known_results));
    j = p.artifact;
    #[cfg(feature = "port-frames")]
    {j["port_frames"]=ports;}
    #[cfg(feature = "deferred-incidence")]
    { j["deferred_incidence"] = deferred; }
    #[cfg(feature="origin-demand")] {j["origin_demand"]=demand;}
    #[cfg(feature = "rhs-locals")]
    {
        j["local_rhs_blocks"] = local_blocks;
    }
    #[cfg(feature = "structural-depth")]
    {
        j["structural_ports"] = structural;
    }
    j["free_join_entry"] = json!(entry);
    for (k, v) in plans {
        j[k] = v;
    }
    j["sha256"] = json!(crate::checksum::artifact_digest(&j));
    let checked=Program::parse(j.clone())?;
    #[cfg(feature="two-run-blocks")]
    {
        j["multi_run_blocks"]=checked.compile_multi_run();
        j.as_object_mut().unwrap().remove("sha256");
        j["sha256"]=json!(crate::checksum::artifact_digest(&j));
        Program::parse(j.clone())?;
    }
    #[cfg(not(feature="two-run-blocks"))]
    let _=checked;
    Ok(j)
}
