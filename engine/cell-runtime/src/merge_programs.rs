//! Checked ordered merge bytecode and fixed, stage-gated consumer programs.
//! The source AST is reconstructed independently of Python, including all reads.
use crate::program::{Arg, Call, Op, Program, Result, arg, array, call};
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Merge {
    pub operations: Vec<Op>,
    pub result: Arg,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Programs {
    pub merges: BTreeMap<usize, Rc<Merge>>,
    pub entries: BTreeMap<(usize, usize), Rc<Vec<usize>>>,
}

fn lower(
    e: &J,
    p: &Program,
    types: &BTreeMap<String, J>,
    sorts: &mut Vec<J>,
    ops: &mut Vec<J>,
    constructed: &mut BTreeSet<String>,
) -> Result<J> {
    if let Some(v) = e["var"].as_str() {
        let i = match v {
            "old" => 0,
            "new" => 1,
            _ => return Err("merge parameter".into()),
        };
        if !e["sort"].is_null() && e["sort"] != sorts[i] {
            return Err("merge parameter type".into());
        }
        return Ok(json!({"slot":i}));
    }
    if e.get("literal").is_some() {
        return Ok(e.clone());
    }
    let args = array(&e["args"])?
        .iter()
        .map(|a| lower(a, p, types, sorts, ops, constructed))
        .collect::<Result<Vec<_>>>()?;
    let atype = |a: &J| {
        if let Some(i) = a["slot"].as_u64() {
            sorts[i as usize].clone()
        } else {
            a["sort"].clone()
        }
    };
    let (out, kind) = if let Some(name) = e["call"]["table"].as_str() {
        let t = p
            .tables
            .iter()
            .find(|t| t.name == name)
            .ok_or("merge table")?;
        if args.len() + 1 != t.schema.len()
            || args.iter().zip(&t.schema).any(|(a, s)| atype(a) != *s)
        {
            return Err("merge argument type".into());
        }
        let kind = if t.virtual_fact {
            constructed.insert(name.into());
            "construct"
        } else {
            "read"
        };
        (t.schema.last().unwrap().clone(), kind)
    } else {
        if args
            .iter()
            .map(|a| atype(a)["name"].clone())
            .collect::<Vec<_>>()
            != *array(&e["call"]["inputs"])?
        {
            return Err("merge primitive inputs".into());
        }
        (
            types
                .get(
                    e["call"]["output"]
                        .as_str()
                        .ok_or("merge primitive output")?,
                )
                .ok_or("unknown merge type")?
                .clone(),
            "compute",
        )
    };
    if !e["sort"].is_null() && e["sort"] != out {
        return Err("merge output type".into());
    }
    let dst = sorts.len();
    sorts.push(out);
    ops.push(json!({"dst":dst,"call":e["call"],"args":args,"kind":kind}));
    Ok(json!({"slot":dst}))
}

pub fn compile(p: &Program) -> Result<J> {
    let mut types = BTreeMap::new();
    for t in &p.tables {
        for s in &t.schema {
            types.insert(s["name"].as_str().unwrap().to_owned(), s.clone());
        }
    }
    for t in &p.templates {
        for s in &t.sorts {
            types.insert(s["name"].as_str().unwrap().to_owned(), s.clone());
        }
    }
    for n in ["i64", "bool", "Unit", "String", "f64"] {
        types.entry(n.into()).or_insert(json!({"name":n,"kind":n}));
    }
    let mut programs = vec![];
    let mut constructed = BTreeSet::new();
    for t in &p.tables {
        if t.constructor || t.merge.is_null() {
            continue;
        }
        let mut sorts = vec![t.schema.last().unwrap().clone(); 2];
        let mut ops = vec![];
        let result = lower(&t.merge, p, &types, &mut sorts, &mut ops, &mut constructed)?;
        let result_type = if let Some(i) = result["slot"].as_u64() {
            &sorts[i as usize]
        } else {
            &result["sort"]
        };
        if result_type != t.schema.last().unwrap() {
            return Err("merge result type".into());
        }
        programs.push(json!({"table":t.name,"slots":sorts,"operations":ops,"result":result,"equal_inputs":if t.merge["call"]["table"].is_string(){"return_old"}else{"evaluate"}}));
    }
    let mut relays = vec![];
    loop {
        let mut fresh = false;
        for (rid, r) in p.rules.iter().enumerate() {
            if !r
                .query
                .iter()
                .any(|a| matches!(a.call,Call::Table(t) if constructed.contains(&p.tables[t].name)))
            {
                continue;
            }
            let mut targets = vec![];
            let mut valid = true;
            for e in &*p.templates[r.template].ops {
                if matches!(e, Op::Copy(..)) {
                    continue;
                }
                if let Op::Set(t, _, Arg::Literal(crate::value::Value::Int(0))) = e {
                    let t = &p.tables[*t];
                    if !t.constructor
                        && t.schema.last().unwrap()["kind"] == "Unit"
                        && (t.merge.is_null() || t.merge == json!({"var":"old"}))
                    {
                        targets.push(t.name.clone());
                        continue;
                    }
                }
                valid = false;
                break;
            }
            if valid {
                for name in targets {
                    if constructed.insert(name.clone()) {
                        fresh = true;
                        relays.push(json!({"rule":rid,"table":name}));
                    }
                }
            }
        }
        if !fresh {
            break;
        }
    }
    let mut entries = vec![];
    let mut unsupported = vec![];
    for (rid, r) in p.rules.iter().enumerate() {
        for (anchor, a) in r.query.iter().enumerate() {
            if !matches!(a.call,Call::Table(t) if constructed.contains(&p.tables[t].name)) {
                continue;
            }
            let mut known = a
                .args
                .iter()
                .filter_map(|a| if let Arg::Slot(s) = a { Some(*s) } else { None })
                .collect::<BTreeSet<_>>();
            let mut remaining = (0..r.query.len())
                .filter(|i| *i != anchor)
                .collect::<BTreeSet<_>>();
            let mut order = vec![];
            loop {
                let next=remaining.iter().filter_map(|i|{
                    let q=&r.query[*i];let full=q.args[..q.args.len()-1].iter().all(|a|!matches!(a,Arg::Slot(s) if !known.contains(s)));
                    let inverse=matches!(q.call,Call::Table(t) if p.tables[t].constructor && p.tables[t].schema.last().unwrap()["kind"]=="eq") && !matches!(q.args.last(),Some(Arg::Slot(s)) if !known.contains(s));
                    (full||inverse).then_some((if matches!(q.call,Call::Primitive(..)){0}else if full{1}else{2},*i))
                }).min();
                let Some((_, i)) = next else { break };
                remaining.remove(&i);
                order.push(i);
                known.extend(
                    r.query[i]
                        .args
                        .iter()
                        .filter_map(|a| if let Arg::Slot(s) = a { Some(*s) } else { None }),
                );
            }
            if remaining.is_empty() {
                entries.push(json!({"rule":rid,"anchor":anchor,"atoms":order}));
            } else {
                unsupported.push(json!({"rule":rid,"anchor":anchor,"residual":remaining}));
            }
        }
    }
    Ok(
        json!({"schema":"eggfuse-merge-programs/v1","recipe_sha256":p.artifact["recipe"]["sha256"],"programs":programs,"entries":entries,"unsupported":unsupported,"relays":relays,"control":p.control,
        "contract":{"old_value":"read by the existing ordered set/repair commit","operands":"typed local slots; exact integers; full constructor alternatives","publication":"original Run StagePatch and repair boundaries","invalidation":"read current canonical visible ports; late rows use original delta entries","effects":"original complete RHS; no winner-only reduction","terminal_cell_substitution":false}}),
    )
}

impl Programs {
    pub fn parse(p: &Program, raw: &J) -> Result<Self> {
        if raw.is_null() {
            return Ok(Self::default());
        }
        if *raw != compile(p)? {
            return Err("merge execution recipe differs from typed source/control".into());
        }
        let names = p
            .tables
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name.clone(), i))
            .collect();
        let mut result = Self::default();
        for m in array(&raw["programs"])? {
            let t = p
                .tables
                .iter()
                .position(|t| Some(t.name.as_str()) == m["table"].as_str())
                .unwrap();
            let operations = array(&m["operations"])?
                .iter()
                .map(|o| {
                    Ok(Op::Let(
                        o["dst"].as_u64().unwrap() as usize,
                        call(&o["call"], &names)?,
                        array(&o["args"])?
                            .iter()
                            .map(arg)
                            .collect::<Result<Vec<_>>>()?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            result.merges.insert(
                t,
                Rc::new(Merge {
                    operations,
                    result: arg(&m["result"])?,
                }),
            );
        }
        for e in array(&raw["entries"])? {
            result.entries.insert(
                (
                    e["rule"].as_u64().unwrap() as usize,
                    e["anchor"].as_u64().unwrap() as usize,
                ),
                Rc::new(
                    array(&e["atoms"])?
                        .iter()
                        .map(|x| x.as_u64().unwrap() as usize)
                        .collect(),
                ),
            );
        }
        Ok(result)
    }
}
