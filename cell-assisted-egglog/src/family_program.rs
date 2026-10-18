//! One static ruleset-family program over shared native tables and tuple indexes.
use egglog_numeric_id::NumericId;
use crate::Result;
use egglog::{
    EGraph,
    cell_assist::{CellActionSink, CellOperand},
    cell_events::CellTable,
};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
pub fn compile(eg: &EGraph, sinks: &[CellActionSink], tables: &[CellTable]) -> Result<Json> {
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
    for q in sinks {
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
                let part = remain
                    .iter()
                    .copied()
                    .find(|i| {
                        q.atoms[*i]
                            .columns
                            .iter()
                            .any(|o| matches!(o,CellOperand::Slot(s) if bound.contains(s)))
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
                remain.remove(&part);
                for o in &a.columns {
                    if let CellOperand::Slot(v) = o {
                        bound.insert(*v);
                    }
                }
            }
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
    for (name, values) in [
        ("QMETA", meta),
        ("ATOM", atoms),
        ("PLAN", plans),
        ("SPEC", index),
        ("GUARD", guards),
    ] {
        shader += &format!("#define {name} {}u\n", code.len());
        code.extend(values);
    }
    // Empty index set (all unary queries) is represented by one disabled spec.
    shader += include_str!("../integer.metal");
    shader += include_str!("../family.metal");
    Ok(
        json!({"version":1,"columns":c,"parts":p,"slots":s,"queries":sinks.len(),"indexes":specs.len().max(1),"code":code,"shader":shader}),
    )
}
