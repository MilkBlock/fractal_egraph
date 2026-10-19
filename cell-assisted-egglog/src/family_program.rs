//! One static ruleset-family program over shared native tables and tuple indexes.
use crate::Result;
use egglog::{
    EGraph,
    cell_assist::{CellActionSink, CellOperand},
    cell_events::CellTable,
};
use egglog_numeric_id::NumericId;
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
pub fn compile(
    eg: &EGraph,
    sinks: &[CellActionSink],
    tables: &[CellTable],
    cells: Option<&crate::cell_prefix::Library>,
    profile: bool,
    certificates: bool,
    event_cells: bool,
) -> Result<Json> {
    let c = tables.iter().map(|t| t.keys + 1).max().unwrap_or(1);
    let p = sinks.iter().map(|q| q.atoms.len()).max().unwrap_or(1);
    let s = sinks.iter().map(|q| q.slots().len()).max().unwrap_or(1);
    let g = sinks
        .iter()
        .map(|q| q.guards.len())
        .max()
        .unwrap_or(0)
        .max(1);
    if sinks.is_empty() || p == 0 || s == 0 {
        return Err("empty shared query program".into());
    }
    let tids = tables
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.clone(), i))
        .collect::<BTreeMap<_, _>>();
    let mut meta = vec![];
    let mut atoms = vec![];
    let mut plans = vec![];
    let mut specs = BTreeMap::new();
    let mut index = vec![];
    let mut guards = vec![];
    let mut orders = vec![];
    for q in sinks {
        let mut local_pairs = BTreeSet::new();
        if let Some(lib) = cells {
            for i in 0..q.atoms.len() {
                for j in 0..q.atoms.len() {
                    if i != j && !lib.alternatives(q, &[i, j]).is_empty() {
                        local_pairs.insert((i, j));
                    }
                }
            }
        }
        meta.extend([
            q.atoms.len() as u32,
            q.slots().len() as u32,
            q.guards.len() as u32,
        ]);
        for part in 0..p {
            if let Some(a) = q.atoms.get(part) {
                atoms.extend([tids[&a.function] as u32, a.columns.len() as u32]);
                for col in 0..c {
                    atoms.extend(match a.columns.get(col) {
                        Some(CellOperand::Slot(s)) => [0, *s as u32],
                        Some(CellOperand::Constant { value, .. }) => [1, value.rep()],
                        None => [0, 0],
                    });
                }
            } else {
                atoms.extend(std::iter::repeat_n(0, 2 + 2 * c));
            }
        }
        for anchor in 0..p {
            let mut order = vec![anchor];
            let mut bound = BTreeSet::new();
            if anchor < q.atoms.len() {
                for o in &q.atoms[anchor].columns {
                    if let CellOperand::Slot(v) = o {
                        bound.insert(*v);
                    }
                }
            }
            let mut remain = (0..q.atoms.len())
                .filter(|v| *v != anchor)
                .collect::<BTreeSet<_>>();
            for depth in 0..p - 1 {
                if anchor >= q.atoms.len() || depth >= q.atoms.len() - 1 {
                    plans.extend([0, 0]);
                    continue;
                }
                let internal = remain
                    .iter()
                    .copied()
                    .find(|i| local_pairs.contains(&(*order.last().unwrap(), *i)));
                let part = internal
                    .or_else(|| {
                        // Enter a witnessed Cell fragment as soon as a column
                        // anchors it, even when the previous atom is external.
                        remain.iter().copied().find(|i| {
                            local_pairs
                                .iter()
                                .any(|(a, b)| a == i && remain.contains(b))
                                && q.atoms[*i]
                                    .columns
                                    .iter()
                                    .any(|o| matches!(o,CellOperand::Slot(v) if bound.contains(v)))
                        })
                    })
                    .or_else(|| {
                        remain.iter().copied().find(|i| {
                            q.atoms[*i]
                                .columns
                                .iter()
                                .any(|o| matches!(o,CellOperand::Slot(s) if bound.contains(s)))
                        })
                    })
                    .unwrap_or(*remain.first().unwrap());
                let a = &q.atoms[part];
                let cols = a
                    .columns
                    .iter()
                    .enumerate()
                    .filter_map(|(col, o)| match o {
                        CellOperand::Constant { .. } => Some(col),
                        CellOperand::Slot(s) if bound.contains(s) => Some(col),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let key = (tids[&a.function], cols.clone());
                let next = specs.len();
                let spec = *specs.entry(key).or_insert_with(|| {
                    index.extend([tids[&a.function] as u32, cols.len() as u32]);
                    index.extend(cols.iter().map(|c| *c as u32));
                    index.extend(std::iter::repeat_n(0, c - cols.len()));
                    next
                });
                plans.extend([part as u32, spec as u32]);
                order.push(part);
                remain.remove(&part);
                for o in &a.columns {
                    if let CellOperand::Slot(v) = o {
                        bound.insert(*v);
                    }
                }
            }
            orders.push(order);
        }
        for j in 0..g {
            if let Some(guard) = q.guards.get(j) {
                guards.push(guard.opcode);
                for o in &guard.operands {
                    match o {
                        CellOperand::Slot(v) => guards.extend([0, *v as u32, 0]),
                        CellOperand::Constant { value, .. } => {
                            let n = eg.value_to_base::<i64>(*value) as u64;
                            guards.extend([1, n as u32, (n >> 32) as u32]);
                        }
                    }
                }
            } else {
                guards.extend([0; 7]);
            }
        }
    }
    // One shared certificate per alpha-renamed prefix, even when several
    // consumers have different guards/RHS. The route transports its slots back
    // to the original rule's complete binding, without changing that rule.
    let mut route = vec![];
    let mut cell_meta = vec![];
    let mut variants = vec![];
    let mut cell_specs = BTreeMap::<String, usize>::new();
    let mut cell_fields = vec![];
    let (mut max_cell_steps, mut max_cell_vars) = (1, 1);
    let mut cell_steps = vec![];
    let mut cell_evidence = vec![];
    let mut cell_tables = BTreeSet::new();
    let mut drivers: Vec<Option<(usize, Vec<usize>, Vec<u32>)>> = vec![None; sinks.len()];
    for (qi, q) in sinks.iter().enumerate() {
        for anchor in 0..p {
            for position in 0..p {
                let full_order = &orders[qi * p + anchor];
                let order = if position < full_order.len() && anchor < q.atoms.len() {
                    &full_order[position..]
                } else {
                    &[]
                };
                let found = cells.and_then(|lib| {
                    let mut longest = None;
                    for n in 2..=order.len() {
                        let alts = lib.alternatives(q, &order[..n]);
                        if alts.is_empty() {
                            break;
                        } // A longer conjunction cannot repair an absent prefix.
                        longest = Some((n, alts));
                    }
                    longest
                });
                let Some((n, alts)) = found else {
                    route.extend([u32::MAX, 0]);
                    route.extend(std::iter::repeat_n(u32::MAX, s));
                    continue;
                };
                let mut renaming = BTreeMap::new();
                let mut key_atoms = vec![];
                for &part in &order[..n] {
                    let a = &q.atoms[part];
                    let cols = a
                        .columns
                        .iter()
                        .map(|o| match o {
                            CellOperand::Slot(v) => {
                                let next = renaming.len();
                                [0, *renaming.entry(*v).or_insert(next) as u32]
                            }
                            CellOperand::Constant { value, .. } => [1, value.rep()],
                        })
                        .collect::<Vec<_>>();
                    key_atoms.push(json!([a.function, a.sorts, cols]));
                }
                let normalized = alts
                    .iter()
                    .map(|(_, cs)| {
                        let mut out = vec![[0, 0]; renaming.len()];
                        for (&old, &new) in &renaming {
                            let mut c = cs[old];
                            if c[0] == 1 {
                                c[1] = renaming[&(c[1] as usize)] as u32;
                            }
                            out[new] = c;
                        }
                        out
                    })
                    .collect::<BTreeSet<_>>();
                let key = serde_json::to_string(&json!([key_atoms, normalized]))?;
                let id = if let Some(v) = cell_specs.get(&key) {
                    *v
                } else {
                    let id = cell_specs.len();
                    cell_tables.extend(order[..n].iter().map(|i| tids[&q.atoms[*i].function]));
                    cell_meta.extend([
                        qi as u32,
                        order[0] as u32,
                        (n - 1) as u32,
                        variants.len() as u32,
                        alts.len() as u32,
                        renaming.len() as u32,
                    ]);
                    max_cell_steps = max_cell_steps.max(n - 1);
                    max_cell_vars = max_cell_vars.max(renaming.len());
                    let fields = renaming
                        .iter()
                        .map(|(o, n)| (*n, *o))
                        .collect::<BTreeMap<_, _>>();
                    for i in 0..s {
                        cell_fields.push(fields.get(&i).map_or(u32::MAX, |v| *v as u32));
                    }
                    for (_, c) in &alts {
                        for i in 0..s {
                            variants.extend(c.get(i).copied().unwrap_or([0, 0]));
                        }
                    }
                    let mut bound = q.atoms[order[0]]
                        .columns
                        .iter()
                        .filter_map(|o| match o {
                            CellOperand::Slot(v) => Some(*v),
                            _ => None,
                        })
                        .collect::<BTreeSet<_>>();
                    for depth in 0..p - 1 {
                        if depth >= n - 1 {
                            cell_steps.extend([0, 0]);
                            continue;
                        }
                        let part = order[depth + 1];
                        let a = &q.atoms[part];
                        let cols = a
                            .columns
                            .iter()
                            .enumerate()
                            .filter_map(|(c, o)| match o {
                                CellOperand::Constant { .. } => Some(c),
                                CellOperand::Slot(v) if bound.contains(v) => Some(c),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        let next = specs.len();
                        let sp = *specs
                            .entry((tids[&a.function], cols.clone()))
                            .or_insert_with(|| {
                                index.extend([tids[&a.function] as u32, cols.len() as u32]);
                                index.extend(cols.iter().map(|c| *c as u32));
                                index.extend(std::iter::repeat_n(0, c - cols.len()));
                                next
                            });
                        cell_steps.extend([part as u32, sp as u32]);
                        bound.extend(a.columns.iter().filter_map(|o| match o {
                            CellOperand::Slot(v) => Some(*v),
                            _ => None,
                        }));
                    }
                    cell_evidence.push(json!({"plan":id,"rule":q.name,"atoms":&order[..n],"states":alts.iter().map(|(id,_)|id).collect::<BTreeSet<_>>(),"internal_joins":n-1}));
                    cell_specs.insert(key, id);
                    id
                };
                let inverse = renaming
                    .iter()
                    .map(|(o, n)| (*n, *o))
                    .collect::<BTreeMap<_, _>>();
                let wiring = (0..s)
                    .map(|i| inverse.get(&i).map_or(u32::MAX, |n| *n as u32))
                    .collect::<Vec<_>>();
                // One driving relation per consumer, with all remaining atoms
                // retained. Choose the largest witnessed functional fragment;
                // ties keep deterministic source order, never sample bindings.
                if drivers[qi]
                    .as_ref()
                    .is_none_or(|(_, parts, _)| parts.len() < n)
                {
                    drivers[qi] = Some((id, order[..n].to_vec(), wiring.clone()));
                }
                route.extend([id as u32, (n - 1) as u32]);
                for i in 0..s {
                    route.push(inverse.get(&i).map_or(u32::MAX, |n| *n as u32));
                }
            }
        }
    }
    let (mut port_meta, mut port_map, mut port_group, mut port_plan, mut owner) =
        (vec![], vec![], vec![], vec![], vec![]);
    for (qi, q) in sinks.iter().enumerate() {
        let driver = drivers[qi].as_ref().filter(|_| event_cells && certificates);
        let group = driver.map(|(_, g, _)| g.clone()).unwrap_or_default();
        let outside = (0..q.atoms.len())
            .filter(|i| !group.contains(i))
            .collect::<Vec<_>>();
        let priority = outside
            .iter()
            .copied()
            .chain(group.iter().copied())
            .collect::<Vec<_>>();
        for i in 0..p {
            owner.push(priority.iter().position(|x| *x == i).unwrap_or(i) as u32);
        }
        port_meta.extend([
            driver.map_or(u32::MAX, |d| d.0 as u32),
            group.first().copied().unwrap_or(0) as u32,
            outside.len() as u32,
        ]);
        port_map.extend(driver.map(|d| d.2.clone()).unwrap_or(vec![u32::MAX; s]));
        for i in 0..p {
            port_group.push(u32::from(group.contains(&i)));
        }
        if driver.is_none() {
            port_plan.extend(std::iter::repeat_n(0, 2 * p));
            continue;
        }
        let mut bound = group
            .iter()
            .flat_map(|i| {
                q.atoms[*i].columns.iter().filter_map(|o| match o {
                    CellOperand::Slot(v) => Some(*v),
                    _ => None,
                })
            })
            .collect::<BTreeSet<_>>();
        let mut remain = outside.into_iter().collect::<BTreeSet<_>>();
        for _ in 0..p {
            let Some(part) = remain
                .iter()
                .copied()
                .find(|i| {
                    q.atoms[*i]
                        .columns
                        .iter()
                        .any(|o| matches!(o,CellOperand::Slot(v) if bound.contains(v)))
                })
                .or_else(|| remain.first().copied())
            else {
                port_plan.extend([0, 0]);
                continue;
            };
            let a = &q.atoms[part];
            let cols = a
                .columns
                .iter()
                .enumerate()
                .filter_map(|(j, o)| match o {
                    CellOperand::Constant { .. } => Some(j),
                    CellOperand::Slot(v) if bound.contains(v) => Some(j),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let next = specs.len();
            let sp = *specs
                .entry((tids[&a.function], cols.clone()))
                .or_insert_with(|| {
                    index.extend([tids[&a.function] as u32, cols.len() as u32]);
                    index.extend(cols.iter().map(|x| *x as u32));
                    index.extend(std::iter::repeat_n(0, c - cols.len()));
                    next
                });
            port_plan.extend([part as u32, sp as u32]);
            remain.remove(&part);
            bound.extend(a.columns.iter().filter_map(|o| match o {
                CellOperand::Slot(v) => Some(*v),
                _ => None,
            }));
        }
    }
    // A Cell supplies the canonical boundary coordinates. Group only EXACT
    // alpha-equivalent residual query programs; guards/RHS remain per consumer.
    let mut join_groups = BTreeMap::<String, usize>::new();
    let (mut port_join_group, mut port_canon, mut port_uncanon) = (vec![], vec![], vec![]);
    for (qi, q) in sinks.iter().enumerate() {
        let mut renaming = BTreeMap::new();
        let mut shape = vec![];
        for &slot in &port_map[qi * s..(qi + 1) * s] {
            if slot != u32::MAX {
                let n = renaming.len();
                renaming.entry(slot as usize).or_insert(n);
            }
        }
        let ports = port_map[qi * s..(qi + 1) * s]
            .iter()
            .map(|v| {
                if *v == u32::MAX {
                    u32::MAX
                } else {
                    renaming[&(*v as usize)] as u32
                }
            })
            .collect::<Vec<_>>();
        for j in 0..port_meta[qi * 3 + 2] as usize {
            let a = &q.atoms[port_plan[(qi * p + j) * 2] as usize];
            let cols = a
                .columns
                .iter()
                .map(|o| match o {
                    CellOperand::Slot(v) => {
                        let n = renaming.len();
                        [0, *renaming.entry(*v).or_insert(n) as u32]
                    }
                    CellOperand::Constant { value, .. } => [1, value.rep()],
                })
                .collect::<Vec<_>>();
            shape.push(json!([a.function, a.sorts, cols]));
        }
        let eligible = port_meta[qi * 3] != u32::MAX && renaming.len() == q.slots().len();
        let key = if eligible {
            serde_json::to_string(&json!([port_meta[qi * 3], ports, shape]))?
        } else {
            format!("unshared:{qi}")
        };
        let next = join_groups.len();
        let group = *join_groups.entry(key).or_insert(next);
        port_join_group.push(group as u32);
        for slot in 0..s {
            port_canon.push(renaming.get(&slot).map_or(u32::MAX, |v| *v as u32));
        }
        let inverse = renaming
            .iter()
            .map(|(a, b)| (*b, *a))
            .collect::<BTreeMap<_, _>>();
        for slot in 0..s {
            port_uncanon.push(inverse.get(&slot).map_or(u32::MAX, |v| *v as u32));
        }
    }
    if specs.is_empty() {
        index.extend([u32::MAX, 0]);
        index.extend(std::iter::repeat_n(0, c));
    }
    let mut code = vec![];
    let mut shader = format!(
        "#include <metal_stdlib>\nusing namespace metal;\n#define C {c}\n#define P {p}\n#define S {s}\n#define G {g}\n#define NQ {}\n#define NI {}\n",
        sinks.len(),
        specs.len().max(1)
    );
    shader += &format!(
        "#define NC {}\n#define PROFILE_JOIN {}\n#define CELL_CERTIFICATES {}\n",
        cell_specs.len().max(1),
        u32::from(profile),
        u32::from(certificates && !cell_specs.is_empty())
    );
    let cell_header = if event_cells && certificates && !cell_specs.is_empty() {
        8
    } else {
        2
    };
    shader += &format!(
        "#define CK {max_cell_steps}\n#define CF {max_cell_vars}\n#define CH {cell_header}\n"
    );
    shader += &format!(
        "#define CELL_EVENTS {}\n",
        u32::from(event_cells && certificates && !cell_specs.is_empty())
    );
    for (name, values) in [
        ("QMETA", meta),
        ("ATOM", atoms),
        ("PLAN", plans),
        ("SPEC", index),
        ("GUARD", guards),
        ("CELL_ROUTE", route),
        ("CELL_META", cell_meta),
        ("CELL_STEP", cell_steps),
        ("CELL_FIELDS", cell_fields),
        ("CELL_VARIANT", variants),
        ("TABLE_KEYS", tables.iter().map(|t| t.keys as u32).collect()),
        ("PORT_META", port_meta.clone()),
        ("PORT_MAP", port_map),
        ("PORT_GROUP", port_group),
        ("PORT_PLAN", port_plan),
        ("OWNER", owner),
        ("PORT_JOIN_GROUP", port_join_group.clone()),
        ("PORT_CANON", port_canon),
        ("PORT_UNCANON", port_uncanon),
    ] {
        shader += &format!("#define {name} {}u\n", code.len());
        code.extend(values);
    }
    // Empty index set (all unary queries) is represented by one disabled spec.
    shader += include_str!("../integer.metal");
    shader += include_str!("../family.metal");
    shader += include_str!("../family_cells.metal");
    shader += include_str!("../family_events.metal");
    shader += include_str!("../family_ports.metal");
    shader += include_str!("../family_route.metal");
    Ok(
        json!({"version":2,"family_layout":5,"port_join_group":port_join_group,"port_join_groups":join_groups.len(),"columns":c,"parts":p,"slots":s,"queries":sinks.len(),"indexes":specs.len().max(1),"code":code,"shader":shader,"cell_plans":if certificates{cell_specs.len()}else{0},"cell_evidence":cell_evidence,"cell_stride":cell_header+2*max_cell_steps+3*max_cell_vars,"cell_steps":max_cell_steps,"cell_tables":cell_tables,"port_meta":port_meta,"event_cells":event_cells&&certificates&&!cell_specs.is_empty(),"profile":profile}),
    )
}
