//! Independently checked multi-rule programs over original Run boundaries.
use crate::program::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Cursor {
    pub program: usize,
    pub entry: u32,
    pub steps: u32,
    pub marker_entry: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instruction {
    pub atom: usize,
    pub columns: Option<Vec<Arg>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub id: usize,
    pub program: usize,
    pub producer: usize,
    pub consumer: usize,
    pub instructions: Vec<Instruction>,
    pub reuse: BTreeMap<usize, Vec<Arg>>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Compositions {
    pub edges: HashMap<(usize, usize, usize, usize), Rc<Edge>>,
    pub rule_program: BTreeMap<usize, usize>,
    pub marker_entries: BTreeSet<usize>,
    pub programs: usize,
}
fn num(j: &J, k: &str) -> Result<usize> {
    j[k].as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| format!("composition {k}"))
}
fn norm(a: &Arg, parents: &BTreeMap<Arg, Arg>) -> Arg {
    let mut a = a.clone();
    while let Some(b) = parents.get(&a) {
        a = b.clone();
    }
    a
}
impl Compositions {
    pub fn parse(p: &Program, raw: &J) -> Result<Self> {
        if raw.is_null() {
            return Ok(Self::default());
        }
        let mut payload = raw.clone();
        let sha = payload
            .as_object_mut()
            .ok_or("composition object")?
            .remove("sha256");
        if sha != Some(crate::checksum::artifact_digest(&payload).into())
            || raw["schema"] != "eggfuse-compositions/v1"
            || raw["recipe_sha256"] != p.artifact["recipe"]["sha256"]
            || raw["control"] != p.artifact["recipe"]["control"]
            || raw["evidence"]["recipe_sha256"] != raw["recipe_sha256"]
        {
            return Err("composition source/control/integrity differs".into());
        }
        let marker_names = p
            .views
            .markers
            .iter()
            .map(|t| p.tables[*t].name.clone())
            .collect::<BTreeSet<_>>();
        let raw_names = array(&raw["marker_tables"])?
            .iter()
            .map(|j| string(j).map(str::to_owned))
            .collect::<Result<BTreeSet<_>>>()?;
        if marker_names != raw_names {
            return Err("composition marker ABI differs".into());
        }
        let mut observed = BTreeSet::new();
        for pair in array(&raw["evidence"]["pairs"])? {
            let a = array(pair)?;
            if a.len() != 2 {
                return Err("composition observation arity".into());
            }
            let ids = a
                .iter()
                .map(|v| {
                    v.as_u64()
                        .map(|n| n as usize)
                        .filter(|i| *i < p.rules.len())
                        .ok_or("composition observed rule".into())
                })
                .collect::<Result<Vec<_>>>()?;
            observed.insert((ids[0], ids[1]));
        }
        let removed = p
            .templates
            .iter()
            .flat_map(|t| t.ops.iter())
            .filter_map(|op| {
                if let Op::Hide(t, ..) = op {
                    Some(*t)
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        let immutable = |t: usize| {
            !removed.contains(&t)
                && ((p.tables[t].constructor && p.tables[t].merge.is_null())
                    || p.views.markers.contains(&t))
        };
        let mut out = Self::default();
        for program in array(&raw["programs"])? {
            let id = num(program, "id")?;
            if id != out.programs {
                return Err("composition program order".into());
            }
            out.programs += 1;
            for step in array(&program["steps"])? {
                let r = num(step, "rule")?;
                let rule = p.rules.get(r).ok_or("composition step rule")?;
                let original = &p.artifact["recipe"]["rules"][r];
                if original["metadata"]["source_set"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("cell-")
                {
                    return Err("helper cannot count as multi-rule step".into());
                }
                let effects = (0..p.templates[rule.template].ops.len()).collect::<Vec<_>>();
                let guards = rule
                    .query
                    .iter()
                    .enumerate()
                    .filter_map(|(i, a)| matches!(a.call, Call::Primitive(..)).then_some(i))
                    .collect::<Vec<_>>();
                let equalities=array(&original["effects"])?.iter().enumerate().filter_map(|(i,e)|{
                    if e["op"]=="union" {Some(json!({"op":i,"kind":"union","args":e["args"],"activation":"after-source-Run-repair"}))}
                    else if e["op"]=="set" && p.tables.iter().any(|t|t.constructor && e["call"]["table"]==t.name) {
                        Some(json!({"op":i,"kind":"constructor-result","table":e["call"]["table"],"args":e["args"],"result":e["value"],"activation":"after-source-Run-repair"}))
                    }else{None}
                }).collect::<Vec<_>>();
                if step["effects"] != json!(effects)
                    || step["guards"] != json!(guards)
                    || step["equalities"] != json!(equalities)
                    || out.rule_program.insert(r, id).is_some()
                {
                    return Err("composition dropped/reordered effect or condition".into());
                }
            }
            let mut expected_entries = vec![];
            for (&r, &owner) in &out.rule_program {
                if owner != id {
                    continue;
                }
                for (atom, a) in p.rules[r].query.iter().enumerate() {
                    if let Call::Table(t) = a.call
                        && p.views.markers.contains(&t)
                    {
                        expected_entries.push(json!({"rule":r,"atom":atom,"marker":p.tables[t].name,"arguments":p.artifact["recipe"]["rules"][r]["query"][atom]["args"]}));
                        out.marker_entries.insert(r);
                    }
                }
            }
            if program["marker_entries"] != json!(expected_entries) {
                return Err("composition marker projection differs".into());
            }
        }
        for (id, e) in array(&raw["edges"])?.iter().enumerate() {
            let producer = num(e, "producer")?;
            let consumer = num(e, "consumer")?;
            let effect = num(e, "effect")?;
            let anchor = num(e, "anchor")?;
            let program = num(e, "program")?;
            if producer == consumer
                || !observed.contains(&(producer, consumer))
                || out.rule_program.get(&producer) != Some(&program)
                || out.rule_program.get(&consumer) != Some(&program)
            {
                return Err("unobserved composition edge".into());
            }
            let src = &p.templates[p.rules[producer].template];
            let dst = &p.rules[consumer];
            let mut aliases = BTreeMap::new();
            for op in src.ops.iter() {
                if let Op::Union(a, b) = op {
                    let a = norm(&crate::multistage::normalize(src, a), &aliases);
                    let b = norm(&crate::multistage::normalize(src, b), &aliases);
                    if a != b {
                        aliases.insert(a.clone().max(b.clone()), a.min(b));
                    }
                }
            }
            let source = |kind: &str, index: usize| -> Result<(usize, Vec<Arg>)> {
                let (t, c) = crate::multistage::source(p, producer, kind, index)?;
                if !immutable(t) {
                    return Err("composition depends on mutable/removable source".into());
                }
                Ok((t, c.iter().map(|a| norm(a, &aliases)).collect()))
            };
            let (table, columns) = source("effect", effect)?;
            let a = dst.query.get(anchor).ok_or("composition anchor")?;
            let mut mapping = BTreeMap::new();
            crate::multistage::extend(a, table, &columns, &mut mapping)?;
            let mut covered = BTreeSet::from([anchor]);
            let mut known = a
                .args
                .iter()
                .filter_map(|a| if let Arg::Slot(s) = a { Some(*s) } else { None })
                .collect::<BTreeSet<_>>();
            let mut instructions = vec![];
            for i in array(&e["instructions"])? {
                let atom = num(i, "atom")?;
                let a = dst.query.get(atom).ok_or("composition instruction atom")?;
                if !covered.insert(atom)
                    || a.args[..a.args.len() - 1]
                        .iter()
                        .any(|a| matches!(a,Arg::Slot(s) if !known.contains(s)))
                {
                    return Err("composition has non-functional or duplicate query".into());
                }
                let columns = match string(&i["kind"])? {
                    "query" => None,
                    "known" => {
                        let (t, c) = source(string(&i["source_kind"])?, num(i, "source_index")?)?;
                        crate::multistage::extend(a, t, &c, &mut mapping)?;
                        Some(c)
                    }
                    _ => return Err("composition instruction kind".into()),
                };
                known.extend(
                    a.args
                        .iter()
                        .filter_map(|a| if let Arg::Slot(s) = a { Some(*s) } else { None }),
                );
                instructions.push(Instruction { atom, columns });
            }
            if covered.len() != dst.query.len() {
                return Err("composition dropped a query condition".into());
            }
            let mut declarations = BTreeMap::new();
            for reuse in array(&e["reuse"])? {
                if declarations.insert(num(reuse, "op")?, reuse).is_some() {
                    return Err("duplicate composition reuse".into());
                }
            }
            let mut reuse = BTreeMap::new();
            for (op, effect) in p.templates[dst.template].ops.iter().enumerate() {
                if let Op::Copy(s, a) = effect {
                    if let Some(v) = match a {
                        Arg::Literal(_) => Some(a.clone()),
                        Arg::Slot(s) => mapping.get(s).cloned(),
                    } {
                        mapping.insert(*s, v);
                    }
                }
                if let Some(spec) = declarations.remove(&op) {
                    let Op::Let(s, Call::Table(t), args) = effect else {
                        return Err("reuse is not a construction".into());
                    };
                    if !p.tables[*t].constructor {
                        return Err("reuse of function update".into());
                    }
                    let (st, cols) =
                        source(string(&spec["source_kind"])?, num(spec, "source_index")?)?;
                    let key = args
                        .iter()
                        .map(|a| match a {
                            Arg::Literal(_) => Ok(a.clone()),
                            Arg::Slot(s) => {
                                mapping.get(s).cloned().ok_or("unbound reuse key".into())
                            }
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if st != *t || cols[..cols.len() - 1] != key {
                        return Err("composition reuse key differs".into());
                    }
                    mapping.insert(*s, cols.last().unwrap().clone());
                    reuse.insert(op, cols);
                }
            }
            if !declarations.is_empty() {
                return Err("reuse operation out of range".into());
            }
            let edge = Rc::new(Edge {
                id,
                program,
                producer,
                consumer,
                instructions,
                reuse,
            });
            if out
                .edges
                .insert((p.rules[producer].template, effect, consumer, anchor), edge)
                .is_some()
            {
                return Err("duplicate composition edge".into());
            }
        }
        Ok(out)
    }
}
