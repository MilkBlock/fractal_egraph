//! Compile witnessed rule handoffs. The runtime independently checks each edge.
use super::*;
use crate::multistage::normalize;
fn remap(a: &Arg, slots: &BTreeMap<usize, Arg>) -> Option<Arg> {
    match a {
        Arg::Literal(_) => Some(a.clone()),
        Arg::Slot(i) => slots.get(i).cloned(),
    }
}
pub(super) fn build(p: &Program, evidence: &J) -> Result<J> {
    let ir = &p.artifact["recipe"];
    if evidence["recipe_sha256"] != ir["sha256"] {
        return Err("composition evidence belongs to another recipe".into());
    }
    let mut pairs = BTreeSet::new();
    for pair in array(&evidence["pairs"])? {
        let a = array(pair)?;
        if a.len() != 2 {
            return Err("composition observation arity".into());
        }
        let id = |v: &J| {
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|&i| i < p.rules.len())
                .ok_or("composition rule out of range")
        };
        pairs.insert((id(&a[0])?, id(&a[1])?));
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
    let mut edges = vec![];
    let mut rejected = BTreeMap::new();
    let mut adj = BTreeMap::<usize, BTreeSet<usize>>::new();
    for (producer, consumer) in pairs {
        if producer == consumer
            || [producer, consumer].iter().any(|&id| {
                ir["rules"][id]["metadata"]["source_set"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("cell-")
            })
        {
            *rejected.entry("auxiliary_or_same_rule").or_insert(0) += 1;
            continue;
        }
        let src = &p.rules[producer];
        let dst = &p.rules[consumer];
        let t = &p.templates[src.template];
        let mut aliases = BTreeMap::<Arg, Arg>::new();
        let root = |mut v: Arg, parents: &BTreeMap<Arg, Arg>| {
            while let Some(a) = parents.get(&v) {
                v = a.clone()
            }
            v
        };
        for e in t.ops.iter() {
            if let Op::Union(l, r) = e {
                let (l, r) = (
                    root(normalize(t, l), &aliases),
                    root(normalize(t, r), &aliases),
                );
                if l != r {
                    aliases.insert(l.clone().max(r.clone()), l.min(r));
                }
            }
        }
        let mut local = vec![];
        for (kind, n) in [("query", src.query.len()), ("effect", t.ops.len())] {
            for i in 0..n {
                if let Ok((table, cols)) = source(p, producer, kind, i)
                    && !removed.contains(&table)
                    && ((p.tables[table].constructor && p.tables[table].merge.is_null())
                        || p.views.markers.contains(&table))
                {
                    local.push((
                        kind,
                        i,
                        table,
                        cols.into_iter()
                            .map(|a| root(a, &aliases))
                            .collect::<Vec<_>>(),
                    ));
                }
            }
        }
        let before = edges.len();
        for (_, effect, table, row) in local.iter().filter(|l| l.0 == "effect") {
            for (anchor, a) in dst.query.iter().enumerate() {
                let mut map = BTreeMap::new();
                if extend(a, *table, row, &mut map).is_err() {
                    continue;
                }
                let mut known = map.keys().copied().collect::<BTreeSet<_>>();
                let mut todo = (0..dst.query.len())
                    .filter(|&i| i != anchor)
                    .collect::<BTreeSet<_>>();
                let mut instructions = vec![];
                while let Some(i) = todo.iter().copied().find(|&i| {
                    dst.query[i].args.split_last().is_some_and(|(_, keys)| {
                        keys.iter().all(|a| {
                            matches!(a, Arg::Literal(_))
                                || matches!(a,Arg::Slot(s) if known.contains(s))
                        })
                    })
                }) {
                    let atom = &dst.query[i];
                    let mut instruction = json!({"atom":i,"kind":"query"});
                    if atom.args[..atom.args.len() - 1]
                        .iter()
                        .all(|a| remap(a, &map).is_some())
                    {
                        for (k, j, t, row) in &local {
                            let mut m = map.clone();
                            if extend(atom, *t, row, &mut m).is_ok() {
                                map = m;
                                instruction = json!({"atom":i,"kind":"known","source_kind":k,"source_index":j});
                                break;
                            }
                        }
                    }
                    instructions.push(instruction);
                    todo.remove(&i);
                    known.extend(
                        atom.args
                            .iter()
                            .filter_map(|a| if let Arg::Slot(i) = a { Some(*i) } else { None }),
                    );
                }
                if !todo.is_empty() {
                    *rejected
                        .entry("unbound_inverse_or_disconnected")
                        .or_insert(0) += 1;
                    continue;
                }
                let mut reuse = vec![];
                for (op, e) in p.templates[dst.template].ops.iter().enumerate() {
                    match e {
                        Op::Copy(s, a) => {
                            if let Some(v) = remap(a, &map) {
                                map.insert(*s, v);
                            }
                        }
                        Op::Let(s, Call::Table(t), args) if p.tables[*t].constructor => {
                            if let Some(key) = args
                                .iter()
                                .map(|a| remap(a, &map))
                                .collect::<Option<Vec<_>>>()
                                && let Some((k, j, _, row)) =
                                    local.iter().find(|(_, _, table, row)| {
                                        table == t && row[..row.len() - 1] == key
                                    })
                            {
                                reuse.push(json!({"op":op,"source_kind":k,"source_index":j}));
                                map.insert(*s, row.last().unwrap().clone());
                            }
                        }
                        _ => (),
                    }
                }
                edges.push(json!({"producer":producer,"effect":effect,"consumer":consumer,"anchor":anchor,"instructions":instructions,"reuse":reuse}));
                adj.entry(producer).or_default().insert(consumer);
                adj.entry(consumer).or_default().insert(producer);
            }
        }
        if before == edges.len() {
            *rejected.entry("no_complete_anchor_program").or_insert(0) += 1;
        }
    }
    edges.sort_by_key(|e| {
        ["producer", "effect", "consumer", "anchor"].map(|k| e[k].as_u64().unwrap())
    });
    let mut ids = BTreeMap::new();
    let mut programs = vec![];
    for &start in adj.keys() {
        if ids.contains_key(&start) {
            continue;
        }
        let id = programs.len();
        let mut component = BTreeSet::new();
        let mut todo = vec![start];
        while let Some(i) = todo.pop() {
            if component.insert(i) {
                ids.insert(i, id);
                todo.extend(&adj[&i]);
            }
        }
        let (mut steps, mut entries) = (vec![], vec![]);
        for i in component {
            let r = &ir["rules"][i];
            let mut guards = vec![];
            let mut equalities = vec![];
            for (j, a) in array(&r["query"])?.iter().enumerate() {
                if a["call"].get("primitive").is_some() {
                    guards.push(j)
                } else if p
                    .views
                    .markers
                    .iter()
                    .any(|&t| p.tables[t].name == a["call"]["table"])
                {
                    entries.push(json!({"rule":i,"atom":j,"marker":a["call"]["table"],"arguments":a["args"]}));
                }
            }
            for (j, e) in array(&r["effects"])?.iter().enumerate() {
                if e["op"] == "union" {
                    equalities.push(json!({"op":j,"kind":"union","args":e["args"],"activation":"after-source-Run-repair"}))
                } else if e["op"] == "set"
                    && p.tables
                        .iter()
                        .any(|t| t.constructor && e["call"]["table"] == t.name)
                {
                    equalities.push(json!({"op":j,"kind":"constructor-result","table":e["call"]["table"],"args":e["args"],"result":e["value"],"activation":"after-source-Run-repair"}))
                }
            }
            steps.push(json!({"rule":i,"guards":guards,"effects":(0..array(&r["effects"])?.len()).collect::<Vec<_>>(),"equalities":equalities}));
        }
        programs.push(json!({"id":id,"steps":steps,"marker_entries":entries}));
    }
    for e in &mut edges {
        e["program"] = json!(ids[&(e["producer"].as_u64().unwrap() as usize)]);
    }
    let mut j = json!({"schema":"eggfuse-compositions/v1","recipe_sha256":ir["sha256"],"control":ir["control"],"marker_tables":p.artifact["reference_views"]["marker_tables"],"evidence":evidence,"programs":programs,"edges":edges,"rejected":rejected,"contract":{"activation":"original Run; full correlated anchor binding","completion":"certified rule steps, not a saturation certificate","effects":"complete original RHS in original order","late_input":"original delta pivots remain enabled","unknown":"ordinary matching; no omitted binding"}});
    j.sort_all_objects();
    j["sha256"] = json!(crate::checksum::artifact_digest(&j));
    Ok(j)
}
