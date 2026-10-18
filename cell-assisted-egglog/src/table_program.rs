//! Rule-shaped table program. No concrete row pairs, per-instance channels or
//! online MIP: orders, tuple keys and numeric DAG are compiled once.
use crate::{
    Result,
    numeric::{Compare, Expr, Merge},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub fn compile(
    columns: &[Vec<usize>],
    slots: usize,
    numeric_slots: &[usize],
    exprs: &[Expr],
    guards: &[(usize, usize, Compare)],
    rhs: usize,
    keys: &[(u32, u32)],
    merge: Merge,
) -> Result<Value> {
    if columns.is_empty() || slots == 0 || rhs >= exprs.len() {
        return Err("invalid table program".into());
    }
    let p = columns.len();
    let mut data = vec![];
    let mut defines = String::new();
    fn table(data: &mut Vec<u32>, defines: &mut String, name: &str, values: Vec<u32>) {
        defines.push_str(&format!("#define {name} {}u\n", data.len()));
        data.extend(values);
    }
    table(
        &mut data,
        &mut defines,
        "COLS",
        columns
            .iter()
            .flat_map(|c| (0..slots).map(move |s| u32::from(c.contains(&s))))
            .collect(),
    );
    let mut specs = BTreeMap::new();
    let mut index_rows = vec![];
    let mut plans = vec![];
    for anchor in 0..p {
        let mut bound = columns[anchor].iter().copied().collect::<BTreeSet<_>>();
        let mut remaining = (0..p).filter(|x| *x != anchor).collect::<BTreeSet<_>>();
        for _ in 1..p {
            // A static connectivity order, independent of row values/cardinality.
            let part = remaining
                .iter()
                .copied()
                .find(|i| columns[*i].iter().any(|s| bound.contains(s)))
                .unwrap_or_else(|| *remaining.first().unwrap());
            let common = columns[part]
                .iter()
                .filter(|s| bound.contains(s))
                .copied()
                .collect::<Vec<_>>();
            let key = (part, common.clone());
            let next = specs.len();
            let id = *specs.entry(key).or_insert_with(|| {
                index_rows.extend([part as u32, common.len() as u32]);
                index_rows.extend(common.iter().map(|s| *s as u32));
                index_rows.extend(std::iter::repeat_n(0, slots - common.len()));
                next
            });
            plans.push(id as u32);
            remaining.remove(&part);
            bound.extend(&columns[part]);
        }
    }
    table(&mut data, &mut defines, "PLANS", plans);
    table(&mut data, &mut defines, "INDEX", index_rows);
    let owners = numeric_slots
        .iter()
        .map(|s| {
            columns
                .iter()
                .position(|p| p.contains(s))
                .map(|p| p as u32)
                .ok_or("numeric slot has no provider")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    table(&mut data, &mut defines, "NUM_OWNERS", owners);
    let mut ops = vec![];
    for (i, e) in exprs.iter().enumerate() {
        if e.args().iter().any(|a| *a >= i) {
            return Err("numeric DAG must be topological".into());
        }
        let (op, slot, value) = match e {
            Expr::Input { slot } if *slot < numeric_slots.len() => (0, *slot, 0),
            Expr::Const { value } => (2, 0, *value as u64),
            Expr::Add { .. } => (3, 0, 0),
            Expr::Sub { .. } => (4, 0, 0),
            Expr::Mul { .. } => (5, 0, 0),
            Expr::Min { .. } => (6, 0, 0),
            Expr::Max { .. } => (7, 0, 0),
            _ => return Err("table frontend requires native table-bound numeric inputs".into()),
        };
        let args = e.args();
        ops.extend([
            op,
            slot as u32,
            args.first().copied().unwrap_or(0) as u32,
            args.get(1).copied().unwrap_or(0) as u32,
            value as u32,
            (value >> 32) as u32,
        ]);
    }
    table(&mut data, &mut defines, "EXPR", ops);
    table(
        &mut data,
        &mut defines,
        "GUARDS",
        guards
            .iter()
            .flat_map(|(a, b, c)| {
                [
                    *a as u32,
                    *b as u32,
                    match c {
                        Compare::Lt => 0,
                        Compare::Le => 1,
                        Compare::Gt => 2,
                        Compare::Ge => 3,
                        Compare::Eq => 4,
                        Compare::Ne => 5,
                    },
                ]
            })
            .collect(),
    );
    let mut key_rows = vec![];
    for (kind, value) in keys {
        let owner = if *kind == 0 {
            columns
                .iter()
                .position(|p| p.contains(&(*value as usize)))
                .ok_or("key slot has no provider")? as u32
        } else {
            0
        };
        key_rows.extend([*kind, *value, owner]);
    }
    table(&mut data, &mut defines, "KEYS", key_rows);
    let mut shader = format!(
        "#include <metal_stdlib>\nusing namespace metal;\n#define PARTS {}\n#define SLOTS {}\n#define NUMS {}\n#define EXPRS {}\n#define NINDEX {}\n#define NGUARDS {}\n#define NKEYS {}\n#define RHS {}\n#define MIN_MERGE {}\n",
        p,
        slots,
        numeric_slots.len(),
        exprs.len(),
        specs.len(),
        guards.len(),
        keys.len(),
        rhs,
        u32::from(merge == Merge::Min)
    );
    shader += &defines;
    shader += include_str!("../integer.metal");
    shader += include_str!("../tables.metal");
    Ok(
        json!({"version":1,"parts":p,"slots":slots,"numbers":numeric_slots.len(),"indexes":specs.len(),"code":data,"shader":shader,
        "scope":"admitted typed fragments; GPU delta joins + exact i64 DAG + proposal reduction; native canonicalization and final merge"}),
    )
}
