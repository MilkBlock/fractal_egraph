//! Compile native typed egglog to the standalone Cell CPU instruction format.
mod compositions;
#[cfg(feature = "spine-blocks")]
mod spine_blocks;
#[cfg(feature = "spine-blocks")]
pub use spine_blocks::bake_spine_blocks;
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
    json!({"schema":"eggfuse-multistage/v1","recipe_sha256":ir["sha256"],"control":ir["control"],"rule_sets":sets,"routes":routes,"entries":entries,"finishes":finishes,"stage_entries":crate::stage_spine::compile_entries(p),"data_loaders":loaders,"union_entries":crate::union_graft::compile(p),
        "contract":{"activation":"committed producer fact; current visible exhaustive projection","publication":"original Run occurrence, after all entry bindings are gathered","late_input":"ordinary delta route remains authoritative","alias_alternatives":"residual matching whenever a projection is not exhaustive"}})
}
fn views(p: &Program, markers: &BTreeSet<String>) -> Result<J> {
    let mut out = vec![];
    for (rid, r) in p.rules.iter().enumerate() {
        let q = &p.artifact["recipe"]["rules"][rid];
        let ops = &p.templates[r.template].ops;
        let eligible = |t: usize| p.tables[t].virtual_fact || markers.contains(&p.tables[t].name);
        let (mut carriers, mut slots, mut seen, mut rows) =
            (vec![], vec![], BTreeSet::new(), vec![]);
        for (i, a) in r.query.iter().enumerate() {
            if let Call::Table(t) = a.call
                && eligible(t)
            {
                carriers.push(
                    json!({"atom":i,"table":p.tables[t].name,"columns":q["query"][i]["args"]}),
                );
                for (col, v) in a.args.iter().enumerate() {
                    if let Arg::Slot(s) = v
                        && seen.insert(*s)
                    {
                        slots.push(json!({"slot":s,"atom":i,"column":col}));
                    }
                }
            }
        }
        for (op, e) in ops.iter().enumerate() {
            match e {
                Op::Let(dst, Call::Table(t), a) if eligible(*t) => {
                    let mut v = a.clone();
                    v.push(Arg::Slot(*dst));
                    rows.push((op, v))
                }
                Op::Set(t, a, v) if eligible(*t) => {
                    let mut a = a.clone();
                    a.push(v.clone());
                    rows.push((op, a))
                }
                _ => (),
            }
        }
        for (i, c) in carriers.iter().enumerate() {
            rows.push((
                ops.len() + i,
                r.query[c["atom"].as_u64().unwrap() as usize].args.clone(),
            ));
        }
        let mut exports = BTreeMap::<usize, Vec<J>>::new();
        for (op, a) in rows {
            for (col, v) in a.into_iter().enumerate() {
                if let Arg::Slot(s) = v {
                    exports
                        .entry(s)
                        .or_default()
                        .push(json!({"op":op,"column":col}));
                }
            }
        }
        let marker = |e: &Op| matches!(e,Op::Set(t,_,_) if markers.contains(&p.tables[*t].name));
        let only = ops.iter().any(marker)
            && ops.iter().all(|e| {
                marker(e) || matches!(e, Op::Copy(..) | Op::Let(_, Call::Primitive(..), _))
            });
        out.push(json!({"rule":rid,"storage":if only{"reference-view"}else{"owned-recipe"},"carriers":carriers,"slots":slots,"continuation_slots":exports.into_iter().map(|(s,c)|json!({"slot":s,"sources":c})).collect::<Vec<_>>()}));
    }
    Ok(
        json!({"schema":"eggfuse-reference-views/v1","recipe_sha256":p.artifact["recipe"]["sha256"],"marker_tables":markers,"rules":out,"contract":{"projection":"immutable matched row coordinates; mutable ports keep copied values","containment":"all owned live facts and retained parameter references must match target carriers","publication":"relocate only after Run RHS and repair; preserve fact epoch/born/hidden","retirement":"redirect references before removing empty source frame; no logical subsume","fallback":"retain source storage when any obligation is not covered"}}),
    )
}
pub fn compile(core: J, markers: &BTreeSet<String>, evidence: Option<&J>) -> Result<J> {
    compile_with_entry(core, markers, evidence, false)
}
pub fn compile_with_entry(
    core: J,
    markers: &BTreeSet<String>,
    evidence: Option<&J>,
    emit_entry: bool,
) -> Result<J> {
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
    let plans = [
        ("single_writes",json!(&p.writes)),
        ("row_kernels",json!(&p.kernels)),
        ("physical_plan", json!(&p.physical)),
        (
            "merge_contracts",
            json!({"schema":"eggfuse-merge-contracts/v1","tables":p.tables.iter().map(|t|crate::merge_contract::table(&p,t)).collect::<Result<Vec<_>>>()?}),
        ),
        ("known_results", crate::known_results::Plans::derive(&p)?.1),
        ("entry_schedule", crate::entry_schedule::Plan::derive(&p)?.1),
        ("merge_programs", crate::merge_programs::compile(&p)?),
        ("multi_stage", multi),
        ("local_contexts", json!(contexts)),
        ("reference_views", views(&p, markers)?),
        ("congruence", crate::congruence::compile(&p)?),
    ];
    let entry = emit_entry
        .then(|| serde_json::to_value(crate::entry_join::Plan::derive(&p)))
        .transpose()
        .map_err(|e| e.to_string())?;
    j = p.artifact;
    if let Some(plan) = entry {
        j["free_join_entry"] = plan;
    }
    for (k, v) in plans {
        j[k] = v;
    }
    if let Some(e) = evidence {
        let p = Program::parse(j)?;
        let plan = compositions::build(&p, e)?;
        j = p.artifact;
        j["compositions"] = plan;
    }
    j["sha256"] = json!(crate::checksum::artifact_digest(&j));
    Program::parse(j.clone())?;
    Ok(j)
}
