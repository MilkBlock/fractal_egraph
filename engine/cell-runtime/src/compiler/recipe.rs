use crate::program::{Result, array, string};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};

fn without(j: &J, fields: &[&str]) -> J {
    let mut out = j.clone();
    for f in fields {
        out.as_object_mut().unwrap().remove(*f);
    }
    out
}
fn visit(j: &J, f: &mut impl FnMut(&J)) {
    f(j);
    if let Some(a) = j.as_array() {
        for v in a {
            visit(v, f)
        }
    }
    if let Some(o) = j.as_object() {
        for v in o.values() {
            visit(v, f)
        }
    }
}
pub(super) fn requirements(core: &J) -> J {
    let mut ops = BTreeSet::new();
    let mut controls = BTreeSet::new();
    let mut primitives = BTreeMap::new();
    visit(core, &mut |v| {
        if let Some(op) = v["op"].as_str() {
            ops.insert(op.to_owned());
        }
        for k in [
            "run",
            "repeat",
            "saturate",
            "sequence",
            "until_run",
            "assert",
        ] {
            if v.get(k).is_some() {
                controls.insert(k);
            }
        }
        if let (Some(n), Some(inputs), Some(output)) = (
            v["primitive"].as_str(),
            v["inputs"].as_array(),
            v["output"].as_str(),
        ) {
            *primitives
                .entry((
                    n.to_owned(),
                    inputs
                        .iter()
                        .map(|v| v.as_str().unwrap().to_owned())
                        .collect::<Vec<_>>(),
                    output.to_owned(),
                ))
                .or_insert(0) += 1;
        }
    });
    let sorts = core["tables"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["schema"].as_array().unwrap())
        .map(|s| &s["name"])
        .map(|s| s.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    json!({"primitives":primitives.into_iter().map(|((n,i,o),count)|json!({"name":n,"inputs":i,"output":o,"occurrences":count})).collect::<Vec<_>>(),"effects":ops,"control":controls,"sorts":sorts})
}
struct Scope {
    slots: Vec<J>,
    names: BTreeMap<String, usize>,
}
impl Scope {
    fn operand(&mut self, a: &J, bind: bool) -> Result<J> {
        if a.get("literal").is_some() {
            return Ok(a.clone());
        }
        let n = string(&a["var"])?;
        if !self.names.contains_key(n) {
            if !bind {
                return Err(format!("unbound action operand {n}"));
            }
            self.names.insert(n.into(), self.slots.len());
            self.slots
                .push(json!({"name":n,"sort":a["sort"],"definition":"query"}));
        }
        let id = self.names[n];
        if self.slots[id]["sort"] != a["sort"] {
            return Err(format!("sort mismatch for {n}"));
        }
        Ok(json!({"slot":id}))
    }
}
fn rule(r: &J, tables: &BTreeMap<&str, &J>) -> Result<J> {
    let mut scope = Scope {
        slots: vec![],
        names: BTreeMap::new(),
    };
    let mut query = vec![];
    let mut rows = vec![];
    let mut prim = vec![];
    let mut effects = vec![];
    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    let mut unions = BTreeSet::new();
    let mut ops = BTreeSet::new();
    for (i, a) in array(&r["body"])?.iter().enumerate() {
        let mut q = a.clone();
        let args = array(&a["args"])?
            .iter()
            .map(|v| scope.operand(v, true))
            .collect::<Result<Vec<_>>>()?;
        if let Some(n) = a["call"]["table"].as_str() {
            reads.insert(n);
            rows.push(json!({"atom":i,"table":n,"columns":args,"key_columns":(0..args.len()-1).collect::<Vec<_>>(),"witness":"complete visible row"}));
        } else {
            prim.push(i)
        }
        q["args"] = json!(args);
        query.push(q);
    }
    for (i, e) in array(&r["head"])?.iter().enumerate() {
        let mut out = e.clone();
        let op = string(&e["op"])?;
        let table = e["call"]["table"].as_str();
        if let Some(aa) = e["args"].as_array() {
            out["args"] = json!(
                aa.iter()
                    .map(|v| scope.operand(v, false))
                    .collect::<Result<Vec<_>>>()?
            );
        }
        for k in ["arg", "value"] {
            if let Some(v) = e.get(k) {
                out[k] = scope.operand(v, false)?;
            }
        }
        if matches!(op, "copy" | "let") {
            let n = string(&e["dst"])?;
            let id = scope.slots.len();
            scope.names.insert(n.into(), id);
            scope
                .slots
                .push(json!({"name":n,"sort":e["sort"],"definition":i}));
            out["dst"] = json!(id);
        }
        let kind = if op == "let" {
            match table {
                None => "compute",
                Some(n) if tables[n]["kind"] == "constructor" => "construct",
                Some(_) => "read",
            }
        } else {
            op
        };
        if let Some(n) = table {
            if kind == "read" {
                reads.insert(n);
            } else {
                writes.insert(n);
            }
            if op == "set" && tables[n]["kind"] == "constructor" {
                unions.insert(
                    tables[n]["schema"].as_array().unwrap().last().unwrap()["name"]
                        .as_str()
                        .unwrap(),
                );
            }
        }
        if op == "union" {
            unions.insert(string(&e["args"][0]["sort"]["name"])?);
        }
        ops.insert(kind);
        out["kind"] = json!(kind);
        effects.push(out);
    }
    let mut todo = writes.iter().copied().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    fn merge_refs<'a>(j: &'a J, out: &mut Vec<&'a str>) {
        if let Some(n) = j["call"]["table"].as_str() {
            out.push(n);
        }
        if let Some(xs) = j["args"].as_array() {
            for a in xs {
                merge_refs(a, out)
            }
        }
    }
    while let Some(n) = todo.pop() {
        if !seen.insert(n) {
            continue;
        }
        let mut refs = vec![];
        merge_refs(&tables[n]["merge_expr"], &mut refs);
        for name in refs {
            if tables[name]["kind"] == "constructor" {
                if writes.insert(name) {
                    todo.push(name)
                }
            } else {
                reads.insert(name);
            }
        }
    }
    fn touches(s: &J, u: &BTreeSet<&str>) -> bool {
        u.contains(s["name"].as_str().unwrap_or(""))
            || s.get("element").is_some_and(|e| touches(e, u))
    }
    let invalid = tables
        .iter()
        .filter(|(_, v)| {
            v["schema"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| touches(s, &unions))
        })
        .map(|(n, _)| n)
        .collect::<Vec<_>>();
    Ok(
        json!({"metadata":without(r,&["body","head"]),"slots":scope.slots,"query":query,"effects":effects,
        "interface":{"rows":rows,"primitive_atoms":prim,"guaranteed_atoms":[],"residual_atoms":(0..query.len()).collect::<Vec<_>>(),"correlation":"one complete binding row; slot equality and literals remain constraints"},
        "footprint":{"read_tables":reads,"write_tables":writes,"union_sorts":unions,"equality_invalidates":invalid,"operations":ops,"precision":"conservative direct/merge footprint; global rebuild may have further effects"}}),
    )
}
pub(super) fn build(core: &J) -> Result<J> {
    if core["schema"] != "egglog-device-core/v1" {
        return Err("expected native typed core".into());
    }
    let tables = array(&core["tables"])?
        .iter()
        .map(|t| Ok((string(&t["name"])?, t)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut ids = BTreeMap::new();
    let mut rules = vec![];
    let mut sets = vec![];
    for group in array(&core["rulesets"])? {
        let mut uses = vec![];
        for r in array(&group["rules"])? {
            let normalized = rule(r, &tables)?;
            let key = crate::checksum::artifact_digest(&normalized);
            let id = *ids.entry(key).or_insert(rules.len());
            if id == rules.len() {
                rules.push(normalized);
            }
            uses.push(id);
        }
        let mut group = without(group, &["rules"]);
        group["rule_ids"] = json!(uses);
        sets.push(group);
    }
    let mut readers = BTreeMap::<String, Vec<J>>::new();
    let mut pubs = BTreeMap::<String, Vec<J>>::new();
    let mut ops = BTreeSet::new();
    for (rid, r) in rules.iter().enumerate() {
        for row in array(&r["interface"]["rows"])? {
            let i = row["atom"].as_u64().unwrap() as usize;
            readers.entry(string(&row["table"])?.into()).or_default().push(json!({"rule":rid,"atom":i,"columns":row["columns"],"residual_atoms":(0..array(&r["query"])?.len()).filter(|j|*j!=i).collect::<Vec<_>>()}));
        }
    }
    for (rid, r) in rules.iter().enumerate() {
        for (i, e) in array(&r["effects"])?.iter().enumerate() {
            ops.insert(string(&e["kind"])?);
            if let Some(t) = e["call"]["table"].as_str()
                && matches!(e["kind"].as_str(), Some("construct" | "set"))
                && readers.contains_key(t)
            {
                pubs.entry(t.into())
                    .or_default()
                    .push(json!({"rule":rid,"effect":i}));
            }
        }
    }
    let routes=pubs.into_iter().map(|(t,p)|json!({"table":t,"publishers":p,"consumers":readers[&t],"payload":"actual post-merge canonical visible row; not the proposed set value","activation_requires":["same-row column constraints","residual query","original stage gate"],"runtime_subscription_installed":false})).collect::<Vec<_>>();
    let mut result = json!({"schema":"eggfuse-recipe/v1","scope":"original_program","source_core_sha256":crate::checksum::artifact_digest(&without(core,&["initial"])),"core_attributes":without(core,&["initial","tables","rulesets","schedule"]),"tables":core["tables"],"rulesets":sets,"rules":rules,"control":core["schedule"],"event_routes":routes,"authorization":{"live_substitution_authorized":false,"guarantee":"exact query/effect/control program; optimization needs a separately validated regional certificate"},"capabilities":{"operations":ops,"generic_queries_retained":true,"optimized_region":false}});
    result["sha256"] = json!(crate::checksum::artifact_digest(&result));
    Ok(result)
}
