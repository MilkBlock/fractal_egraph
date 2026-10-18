use crate::{
    Result,
    compile::Arena,
    numeric::{Compare, Expr, Merge, Plan},
};
fn bits(v: i64) -> [u32; 2] {
    [v as u32, ((v as u64) >> 32) as u32]
}
pub(crate) fn lower(a: &mut Arena, n: Option<&Plan>, lanes: usize) -> Result<()> {
    let empty = Plan::default();
    let p = n.unwrap_or(&empty);
    let count = if n.is_some() { lanes } else { 0 };
    let width = p.inputs.first().map_or(0, Vec::len).max(1);
    let sw = p.state_bindings.first().map_or(0, Vec::len).max(1);
    a.add(
        "numeric_inputs",
        "u32",
        count,
        width * 2,
        &[],
        p.inputs
            .iter()
            .flat_map(|row| {
                let mut r = row.iter().flat_map(|v| bits(*v)).collect::<Vec<_>>();
                r.resize(width * 2, 0);
                r
            })
            .collect(),
    )?;
    a.add(
        "numeric_state_bindings",
        "u32",
        count,
        sw,
        &[],
        p.state_bindings
            .iter()
            .flat_map(|r| {
                let mut r = r.iter().map(|v| *v as u32).collect::<Vec<_>>();
                r.resize(sw, 0);
                r
            })
            .collect(),
    )?;
    let exprs = p
        .exprs
        .iter()
        .map(|e| {
            let (op, slot, value) = match e {
                Expr::Input { slot } => (0, *slot, 0),
                Expr::State { slot } => (1, *slot, 0),
                Expr::Const { value } => (2, 0, *value),
                Expr::Add { .. } => (3, 0, 0),
                Expr::Sub { .. } => (4, 0, 0),
                Expr::Mul { .. } => (5, 0, 0),
                Expr::Min { .. } => (6, 0, 0),
                Expr::Max { .. } => (7, 0, 0),
            };
            let args = e.args();
            let v = bits(value);
            vec![
                op,
                slot as u32,
                args.first().copied().unwrap_or(0) as u32,
                args.get(1).copied().unwrap_or(0) as u32,
                v[0],
                v[1],
            ]
        })
        .collect();
    a.table(
        "numeric_ops",
        &["op", "slot", "left", "right", "lo", "hi"],
        exprs,
    )?;
    a.zero("numeric_values", "u32", count, p.exprs.len().max(1) * 2)?;
    a.zero("numeric_defined", "u32", count, p.exprs.len().max(1))?;
    a.zero("numeric_valid", "u32", count, 1)?;
    a.zero("numeric_selected", "u32", count, 1)?;
    a.table(
        "numeric_guards",
        &["channel", "left", "right", "compare", "depth"],
        p.guards
            .iter()
            .map(|g| {
                vec![
                    g.channel as u32,
                    g.left as u32,
                    g.right as u32,
                    match g.compare {
                        Compare::Lt => 0,
                        Compare::Le => 1,
                        Compare::Gt => 2,
                        Compare::Ge => 3,
                        Compare::Eq => 4,
                        Compare::Ne => 5,
                    },
                    g.depth as u32,
                ]
            })
            .collect(),
    )?;
    a.zero("numeric_evidence", "u32", p.guards.len(), 1)?;
    a.table(
        "numeric_writes",
        &["channel", "group", "value"],
        p.writes
            .iter()
            .map(|w| vec![w.channel as u32, w.group as u32, w.value as u32])
            .collect(),
    )?;
    a.table(
        "numeric_proposals",
        &["lo", "hi", "status", "committed"],
        vec![vec![0; 4]; p.writes.len()],
    )?;
    a.table(
        "numeric_state",
        &["lo", "hi", "present", "version"],
        p.groups
            .iter()
            .map(|g| {
                let v = bits(g.initial.unwrap_or(0));
                vec![v[0], v[1], u32::from(g.initial.is_some()), 0]
            })
            .collect(),
    )?;
    let (mut gi, mut wi, mut rows) = (vec![], vec![], vec![]);
    for c in 0..lanes {
        let gs = gi.len();
        let ws = wi.len();
        gi.extend(
            p.guards
                .iter()
                .enumerate()
                .filter(|(_, g)| g.channel == c)
                .map(|(i, _)| vec![i as u32]),
        );
        wi.extend(
            p.writes
                .iter()
                .enumerate()
                .filter(|(_, w)| w.channel == c)
                .map(|(i, _)| vec![i as u32]),
        );
        rows.push(vec![
            gs as u32,
            (gi.len() - gs) as u32,
            ws as u32,
            (wi.len() - ws) as u32,
        ]);
    }
    a.table(
        "numeric_lanes",
        &["guard_start", "guard_count", "write_start", "write_count"],
        rows,
    )?;
    a.table("numeric_guard_index", &["id"], gi)?;
    a.table("numeric_write_index", &["id"], wi)?;
    let (mut index, mut groups) = (vec![], vec![]);
    for (g, group) in p.groups.iter().enumerate() {
        let start = index.len();
        index.extend(
            p.writes
                .iter()
                .enumerate()
                .filter(|(_, w)| w.group == g)
                .map(|(i, _)| vec![i as u32]),
        );
        groups.push(vec![
            match group.merge {
                Merge::Min => 0,
                Merge::Max => 1,
            },
            start as u32,
            (index.len() - start) as u32,
        ]);
    }
    a.table("numeric_groups", &["merge", "start", "count"], groups)?;
    a.table("numeric_group_index", &["id"], index)?;
    a.zero("numeric_error", "u32", 1, 1)?;
    a.table(
        "numeric_counters",
        &["expressions", "guards", "proposals", "reductions"],
        vec![vec![0; 4]],
    )?;
    Ok(())
}
