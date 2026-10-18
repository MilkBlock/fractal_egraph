//! Import witnessed closure-port interactions into the shared matrix program.
//! Discovery belongs to analyze. This importer validates the whole LHS and
//! certificate transport; it never enumerates possible Cell pairs.
use crate::{Result, program::*};
use egglog::{
    EGraph,
    ast::{Command, Expr, Fact},
};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn n(v: &Json) -> Result<usize> {
    Ok(v.as_u64()
        .ok_or("missing integer in analyze evidence")?
        .try_into()?)
}
fn arr(v: &Json) -> Result<&Vec<Json>> {
    Ok(v.as_array().ok_or("missing array in analyze evidence")?)
}
fn nums(v: &Json) -> Result<Vec<usize>> {
    arr(v)?.iter().map(n).collect()
}
fn text(v: &Json) -> Result<&str> {
    Ok(v.as_str().ok_or("missing string in analyze evidence")?)
}
fn partition<T: PartialEq>(v: &[T]) -> Vec<usize> {
    (0..v.len())
        .map(|i| v.iter().position(|x| *x == v[i]).unwrap())
        .collect()
}

pub fn load(dir: &Path) -> Result<Program> {
    let graph = crate::json(&dir.join("cell-egraph.json"))?;
    let history = crate::json(&dir.join("history.json"))?;
    let ports = &graph["repetition_families"]["port_progressions"];
    let mut states = BTreeMap::new();
    for i in arr(&ports["instances"])? {
        let id = n(&i["catalog_state"])?;
        if !states.contains_key(&id) {
            states.insert(
                id,
                crate::json(&dir.join(format!("catalog/states/state-{id:04}.json")))?,
            );
        }
    }
    let mut p = import(ports, &history, &states)?;
    p.evidence["analysis_directory"] = json!(dir);
    Ok(p)
}

// Validate the reported complete-body injection, including visibility, literal
// identity and alias consistency. Same root alone is not sufficient.
fn retained(source: &Json, target: &Json, proof: &Json, markers: &[Json]) -> Result<()> {
    if source["scope"] != target["scope"]
        || source["scope"]["semantics"] != "positive-constructor-equality/v1"
    {
        return Err(
            "closure environments differ or lack positive-constructor certification".into(),
        );
    }
    let sv = arr(&source["values"])?;
    let tv = arr(&target["values"])?;
    let mut map = BTreeMap::new();
    for pair in arr(&proof["value_map"])? {
        let xs = nums(pair)?;
        if xs.len() != 2 || map.insert(xs[0], xs[1]).is_some() {
            return Err("ambiguous body map".into());
        }
        let a = sv.get(xs[0]).ok_or("missing source value")?;
        let b = tv.get(xs[1]).ok_or("missing target value")?;
        if a["sort"] != b["sort"] || a["literal"] != b["literal"] {
            return Err("typed literal transport changed".into());
        }
    }
    let hidden = source
        .get("subsumed_rows")
        .map(nums)
        .transpose()?
        .unwrap_or_default();
    let th = target
        .get("subsumed_rows")
        .map(nums)
        .transpose()?
        .unwrap_or_default();
    let expected: BTreeSet<_> = arr(&source["rows"])?
        .iter()
        .enumerate()
        .filter(|(i, r)| !hidden.contains(i) && !markers.contains(&r["op"]))
        .map(|(i, _)| i)
        .collect();
    if nums(&proof["checked_visible_rows"])?
        .into_iter()
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err("incomplete retained-body certificate".into());
    }
    for i in expected {
        let r = &source["rows"][i];
        let args = nums(&r["args"])?
            .iter()
            .map(|v| map.get(v).copied().ok_or("unmapped body argument"))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let result = *map.get(&n(&r["result"])?).ok_or("unmapped body result")?;
        if !arr(&target["rows"])?.iter().enumerate().any(|(j, t)| {
            !th.contains(&j)
                && t["op"] == r["op"]
                && t["args"] == json!(args)
                && t["result"] == result
        }) {
            return Err("transported parent row absent from target".into());
        }
    }
    Ok(())
}

/// Compile only analyze's witnessed complete-overlap port contracts. Source
/// parsing here is validation of those witnesses, never discovery of families.
pub fn import(ports: &Json, history: &Json, states: &BTreeMap<usize, Json>) -> Result<Program> {
    if ports["schema"] != "closure-port-progressions/v1" || history["complete"] != true {
        return Err("need complete analyze history and closure-port-progressions/v1".into());
    }
    let instances = arr(&ports["instances"])?;
    let mut templates = vec![];
    let mut template_ids = BTreeMap::new();
    let mut cells = vec![];
    let mut bindings = vec![];
    let mut instance_ids = BTreeMap::new();
    let mut identities = BTreeMap::new();
    for i in instances {
        let id = n(&i["id"])?;
        if instance_ids.insert(id, cells.len()).is_some() {
            return Err("duplicate instance".into());
        }
        let state = n(&i["catalog_state"])?;
        let body = states.get(&state).ok_or("missing certified closure")?;
        let template = *template_ids.entry(state).or_insert_with(|| {
            let id = templates.len();
            templates.push(json!({"catalog_state":state,"closure":body}));
            id
        });
        let mut values = vec![];
        for (j, port) in arr(&i["ports"])?.iter().enumerate() {
            if n(&port["port"])? != j {
                return Err("unordered ports".into());
            }
            let key = (text(&port["sort"])?.to_owned(), n(&port["token"])?);
            if history["tokens"][key.1]["sort"] != key.0
                || body["values"][n(&port["closure_value"])?]["sort"] != key.0
            {
                return Err("port sort/token mismatch".into());
            }
            let marker = text(&port["marker"])?;
            let marker_index = port["marker_index"]
                .as_i64()
                .ok_or("missing parameter index")?
                .to_string();
            let hidden = body
                .get("subsumed_rows")
                .map(nums)
                .transpose()?
                .unwrap_or_default();
            if !arr(&body["rows"])?.iter().enumerate().any(|(ri, row)| {
                !hidden.contains(&ri)
                    && row["op"] == marker
                    && row["result"] == port["closure_value"]
                    && row["args"].as_array().is_some_and(|args| {
                        args.len() == 1
                            && args[0].as_u64().is_some_and(|v| {
                                body["values"][v as usize]["sort"] == "i64"
                                    && body["values"][v as usize]["literal"] == marker_index
                            })
                    })
            }) {
                return Err("port missing from certified parameter interface".into());
            }
            let next = u32::try_from(identities.len())?;
            values.push(*identities.entry(key).or_insert(next));
        }
        let symbolic = arr(&i["ports"])?
            .iter()
            .map(|p| Ok((text(&p["sort"])?, n(&p["closure_value"])?)))
            .collect::<Result<Vec<_>>>()?;
        if partition(&symbolic) != partition(&values) {
            return Err("historical aliases differ from the certified Cell frame".into());
        }
        cells.push(Cell {
            template,
            binding: Some(values.clone()),
            partition: partition(&values),
        });
        bindings.push(values);
    }
    let slots = cells
        .iter()
        .map(|c| c.partition.len())
        .max()
        .ok_or("no certified instances")?;
    let mut p = Program {
        version: 1,
        bits: (32 - (identities.len().saturating_sub(1) as u32).leading_zeros()).max(1) as usize,
        slots,
        nodes: (0..slots)
            .map(|slot| Node {
                op: Op::Port,
                args: vec![],
                slot,
            })
            .collect(),
        cells,
        templates,
        conditions: vec![],
        channels: vec![],
        stages: vec![],
        updates: vec![],
        evidence: json!({"source":"analyze committed producer-read witnesses and exact completed closure certificates","scope":"observed connections only; not a whole-runtime matcher or a proof of unbounded recurrence","identities":identities.iter().map(|((s,t),v)|json!({"sort":s,"token":t,"coordinate":v})).collect::<Vec<_>>(),"channels":[],"excluded":[]}),
    };
    let mut parser = EGraph::default();
    let mut registered = BTreeSet::new();
    let mut events = vec![];
    // Validate each family atomically. An unsupported guard never disappears.
    for family in arr(&ports["families"])? {
        let result = (|| -> Result<Vec<(usize, Channel, Json)>> {
            let c = &family["contract"];
            let source = states
                .get(&n(&c["source_closure"])?)
                .ok_or("missing source closure")?;
            let target = states
                .get(&n(&c["target_closure"])?)
                .ok_or("missing target closure")?;
            let maps = arr(&c["target_port_maps"])?
                .iter()
                .map(nums)
                .collect::<Result<Vec<_>>>()?;
            if maps.len() != 2 || arr(&family["source_body_maps"])?.len() != 2 {
                return Err("need two complete source maps".into());
            }
            for side in 0..2 {
                let proof = &family["source_body_maps"][side];
                if nums(&proof["port_map"])? != maps[side] {
                    return Err("body/port maps disagree".into());
                }
                retained(
                    source,
                    target,
                    proof,
                    arr(&c["source_parameter_markers"][side])?,
                )?;
                let value_map: BTreeMap<_, _> = arr(&proof["value_map"])?
                    .iter()
                    .map(|x| Ok((n(&x[0])?, n(&x[1])?)))
                    .collect::<Result<_>>()?;
                let sp = nums(&c["source_port_values"][side])?;
                let tp = nums(&c["target_port_values"])?;
                if sp.len() != maps[side].len()
                    || sp
                        .iter()
                        .zip(&maps[side])
                        .any(|(a, b)| tp.get(*b) != value_map.get(a))
                {
                    return Err("body map disagrees with port transport".into());
                }
            }
            let mut out = vec![];
            for w in arr(&family["witnesses"])? {
                let record_id = n(&w["record"])?;
                let record = &history["records"][record_id];
                if record["id"] != w["event"]
                    || arr(&record["produced"])?.is_empty() && arr(&record["unions"])?.is_empty()
                {
                    return Err("witness lacks a committed output".into());
                }
                let hr = &history["rules"][n(&record["rule"])?];
                let commands = parser.parse_program(None, text(&hr["code"])?)?;
                let Some(Command::Rule { rule }) = commands.first() else {
                    return Err("witness is not an analyzed rule".into());
                };
                if commands.len() != 1 || rule.body.len() != 2 || rule.name != text(&w["rule"])? {
                    return Err(
                        "full LHS has additional conditions or mismatched rule identity".into(),
                    );
                }
                let source_ids = [n(&w["from"])?, n(&w["to"])?];
                let sources = source_ids
                    .iter()
                    .map(|i| {
                        instance_ids
                            .get(i)
                            .copied()
                            .ok_or("missing source instance")
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let dst = *instance_ids
                    .get(&n(&w["target"])?)
                    .ok_or("missing target instance")?;
                if sources.contains(&dst) {
                    return Err("cyclic/identical target needs explicit event versions".into());
                }
                let mut by_var = BTreeMap::<String, Vec<Wire>>::new();
                let mut port_names = BTreeMap::<Wire, String>::new();
                let mut results = vec![];
                let mut bodies = BTreeSet::new();
                for side in 0..2 {
                    let input = n(&w["read_slots"][side])?;
                    let span = &record["inputs"][input]["Read"];
                    let fact = n(&w["versioned_facts"][side])?;
                    if record["wanted"][input] != fact {
                        return Err("read witness version mismatch".into());
                    }
                    let pr = n(&w["producer_records"][side])?;
                    if pr >= record_id
                        || !nums(&history["records"][pr]["produced"])?.contains(&fact)
                    {
                        return Err("producer did not commit the read fact".into());
                    }
                    let call = arr(&hr["calls"])?
                        .iter()
                        .find(|x| x[0] == *span)
                        .ok_or("read source location missing")?;
                    let path = text(&call[1])?.split('/').collect::<Vec<_>>();
                    if path.len() != 4 || path[0] != "body" || path[2..] != ["expr", "1"] {
                        return Err("unsupported nested LHS evidence".into());
                    }
                    let bi: usize = path[1].parse()?;
                    if !bodies.insert(bi) {
                        return Err("duplicate LHS evidence".into());
                    }
                    let Some(Fact::Eq(_, Expr::Var(_, result), Expr::Call(_, op, args))) =
                        rule.body.get(bi)
                    else {
                        return Err(
                            "only two constructor reads supported by this port contract".into()
                        );
                    };
                    results.push(result.clone());
                    let columns = nums(&c["read_columns"][side])?;
                    let src = &instances[sources[side]];
                    if n(&src["catalog_state"])? != n(&c["source_closure"])?
                        || columns.len() != args.len()
                    {
                        return Err("source frame mismatch".into());
                    }
                    if nums(&c["source_port_values"][side])?
                        != arr(&src["ports"])?
                            .iter()
                            .map(|x| n(&x["closure_value"]))
                            .collect::<Result<Vec<_>>>()?
                    {
                        return Err("source port frame changed".into());
                    }
                    let mut row_args = vec![];
                    for (arg, &slot) in args.iter().zip(&columns) {
                        let Expr::Var(_, name) = arg else {
                            return Err(
                                "computed/literal LHS needs a typed parameter contract".into()
                            );
                        };
                        let port = arr(&src["ports"])?.get(slot).ok_or("read column absent")?;
                        row_args.push(n(&port["closure_value"])?);
                        let varslot = arr(&record["inputs"])?
                            .iter()
                            .position(|x| x["Var"] == *name)
                            .ok_or("missing historical variable")?;
                        if record["wanted"][varslot] != port["token"] {
                            return Err("read binding disagrees with port evidence".into());
                        }
                        let wire = Wire(sources[side], slot);
                        if port_names
                            .insert(wire, name.clone())
                            .is_some_and(|old| old != *name)
                        {
                            return Err("incidental input alias requires specialization".into());
                        }
                        by_var.entry(name.clone()).or_default().push(wire);
                    }
                    let hidden = source
                        .get("subsumed_rows")
                        .map(nums)
                        .transpose()?
                        .unwrap_or_default();
                    if !arr(&source["rows"])?.iter().enumerate().any(|(j, r)| {
                        !hidden.contains(&j) && r["op"] == *op && r["args"] == json!(row_args)
                    }) {
                        return Err("certified source does not provide the read".into());
                    }
                }
                if results[0] == results[1] || results.iter().any(|x| by_var.contains_key(x)) {
                    return Err(
                        "result-value equality needs an exported internal coordinate".into(),
                    );
                }
                let dst_inst = &instances[dst];
                if n(&dst_inst["catalog_state"])? != n(&c["target_closure"])?
                    || nums(&c["target_port_values"])?
                        != arr(&dst_inst["ports"])?
                            .iter()
                            .map(|x| n(&x["closure_value"]))
                            .collect::<Result<Vec<_>>>()?
                {
                    return Err("target frame mismatch".into());
                }
                let mut gathers = vec![None; p.cells[dst].partition.len()];
                for side in 0..2 {
                    for (slot, &to) in maps[side].iter().enumerate() {
                        let wire = Wire(sources[side], slot);
                        let dest = gathers.get_mut(to).ok_or("target map outside frame")?;
                        if let Some(old) = dest {
                            if port_names.get(old).is_none()
                                || port_names.get(old) != port_names.get(&wire)
                            {
                                return Err(
                                    "incidental cross-Cell alias requires specialization".into()
                                );
                            }
                        }
                        if bindings[sources[side]].get(slot) != bindings[dst].get(to) {
                            return Err("target binding differs from history".into());
                        }
                        *dest = Some(wire);
                    }
                }
                let gathers = gathers
                    .into_iter()
                    .collect::<Option<Vec<_>>>()
                    .ok_or("target has unresolved inputs")?;
                let pairs = by_var
                    .values()
                    .flat_map(|ws| ws[1..].iter().map(|w| (ws[0], *w)))
                    .collect::<Vec<_>>();
                let evidence = json!({"family":family["id"],"witness":w,"logical_equalities":pairs,"source_body_maps":family["source_body_maps"],"source_instances":source_ids,"target_instance":w["target"]});
                // Conditions are added only once the entire family validates.
                let channel = Channel {
                    label: format!("analyze:{}:{}", text(&w["rule"])?, record_id),
                    sources,
                    alternatives: vec![],
                    target: Some(Target {
                        cell: dst,
                        bindings: gathers,
                        specializations: vec![],
                    }),
                };
                out.push((record_id, channel, evidence));
            }
            Ok(out)
        })();
        match result {
            Ok(ws) => events.extend(ws),
            Err(e) => p.evidence["excluded"]
                .as_array_mut()
                .unwrap()
                .push(json!({"family":family["id"],"reason":e.to_string()})),
        }
    }
    events.sort_by_key(|(r, _, _)| *r);
    for (record, mut ch, ev) in events {
        let target = ch.target.as_ref().unwrap().cell;
        if !registered.insert(target) {
            return Err(
                "multiple observed producers of one target need explicit instance versions".into(),
            );
        }
        let mut reqs = vec![];
        for pair in arr(&ev["logical_equalities"])? {
            let left: Wire = serde_json::from_value(pair[0].clone())?;
            let right: Wire = serde_json::from_value(pair[1].clone())?;
            let i = p.conditions.len();
            p.conditions.push(Condition { left, right });
            reqs.push(Requirement {
                condition: i,
                depth: 0,
            });
        }
        ch.alternatives = vec![reqs];
        p.cells[target].binding = None;
        p.evidence["channels"]
            .as_array_mut()
            .unwrap()
            .push(json!({"record":record,"evidence":ev}));
        p.channels.push(ch);
    }
    if p.channels.is_empty() {
        return Err(format!(
            "no complete supported analyze port transitions: {}",
            p.evidence["excluded"]
        )
        .into());
    }
    let mut available: BTreeSet<_> = p
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.binding.is_some())
        .map(|(i, _)| i)
        .collect();
    let mut remaining: BTreeSet<_> = (0..p.channels.len()).collect();
    while !remaining.is_empty() {
        let stage = remaining
            .iter()
            .copied()
            .filter(|i| p.channels[*i].sources.iter().all(|s| available.contains(s)))
            .collect::<Vec<_>>();
        if stage.is_empty() {
            return Err("cyclic witnessed graph needs event versioning".into());
        }
        for i in &stage {
            remaining.remove(i);
            available.insert(p.channels[*i].target.as_ref().unwrap().cell);
        }
        p.stages.push(stage);
    }
    p.clone().check()?;
    Ok(p)
}
