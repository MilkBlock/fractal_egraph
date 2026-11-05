//! Checked register programs for connected queries with at most one index
//! expansion. Every other read is a complete key or a bound primitive.
use crate::program::*;
use serde::Serialize;
use std::{collections::BTreeSet, rc::Rc};
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) enum Access {
    Key,
    Expand,
    Primitive,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Step {
    pub atom: usize,
    pub access: Access,
    pub assign: Vec<(usize, usize)>,
    pub check: Vec<(usize, Arg)>,
    pub bound: Vec<(usize, Arg)>,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Kernel {
    #[serde(skip)]
    pub computed: Option<Computed>,
    pub pivot: usize,
    pub seed: Vec<(usize, usize)>,
    pub steps: Vec<Step>,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Computed {
    pub atom: usize,
    pub prefix: usize,
    pub operand: Arg,
    pub column: crate::range_index::Column,
    pub target: Arg,
}
fn computed(r: &Rule, steps: &[Step]) -> Option<Computed> {
    use crate::range_index::Column;
    let pos = steps
        .iter()
        .position(|s| matches!(s.access, Access::Expand))?;
    let expand = &steps[pos];
    let args = &r.query[expand.atom].args;
    let available = |a: &Arg| !expand.assign.iter().any(|(_, s)| a == &Arg::Slot(*s));
    let sum = |step: &Step| {
        let atom = &r.query[step.atom];
        matches!(&atom.call,Call::Primitive(name,sort) if name=="+" && sort=="i64").then_some(atom)
    };
    // Reverse: two fields of this row must sum to an already bound result.
    if let Some(guard) = steps.get(pos + 1)
        && let Some(atom) = sum(guard)
        && let [a, b, target] = atom.args.as_slice()
        && available(target)
        && guard.assign.is_empty()
        && let Some(left) = args.iter().position(|v| v == a)
        && let Some(right) = args.iter().position(|v| v == b)
        && let Some((prefix, operand)) = expand.bound.first()
    {
        return Some(Computed {
            atom: expand.atom,
            prefix: *prefix,
            operand: operand.clone(),
            column: Column::Sum(left, right),
            target: target.clone(),
        });
    }
    // Forward: a preceding sum binds a second equality column after seed admission.
    for step in &steps[..pos] {
        if let Some(atom) = sum(step)
            && let [_, _, target] = atom.args.as_slice()
            && let Some((column, _)) = expand.bound.iter().find(|(_, v)| v == target)
            && let Some((prefix, operand)) = expand.bound.iter().find(|(col, _)| col != column)
        {
            return Some(Computed {
                atom: expand.atom,
                prefix: *prefix,
                operand: operand.clone(),
                column: Column::Raw(*column),
                target: target.clone(),
            });
        }
    }
    None
}
pub(crate) type Kernels = Vec<Vec<Option<Rc<Kernel>>>>;
pub(crate) fn computed_plans(kernels: &Kernels) -> Vec<(usize, usize, &Computed)> {
    kernels
        .iter()
        .enumerate()
        .flat_map(|(rid, ks)| {
            ks.iter()
                .flatten()
                .filter_map(move |k| k.computed.as_ref().map(|c| (rid, k.pivot, c)))
        })
        .collect()
}
pub(crate) fn derive(rules: &[Rule]) -> Kernels {
    rules
        .iter()
        .map(|r| {
            (0..r.query.len())
                .map(|pivot| plan(r, pivot).map(Rc::new))
                .collect()
        })
        .collect()
}
fn plan(r: &Rule, pivot: usize) -> Option<Kernel> {
    if !r.seminaive || r.direct.is_some() || !matches!(r.query[pivot].call, Call::Table(_)) {
        return None;
    }
    let mut known = BTreeSet::new();
    let mut seed = vec![];
    for (col, a) in r.query[pivot].args.iter().enumerate() {
        if let Arg::Slot(s) = a
            && known.insert(*s)
        {
            seed.push((col, *s));
        }
    }
    let mut remaining = (0..r.query.len())
        .filter(|&i| i != pivot)
        .collect::<BTreeSet<_>>();
    let mut expanded = false;
    let mut steps = vec![];
    while !remaining.is_empty() {
        let available =
            |v: &Arg| matches!(v, Arg::Literal(_)) || matches!(v,Arg::Slot(s) if known.contains(s));
        let (rank, atom) = remaining
            .iter()
            .filter_map(|&i| {
                let a = &r.query[i];
                let (_, keys) = a.args.split_last()?;
                if keys.iter().all(available) {
                    Some((usize::from(matches!(a.call, Call::Table(_))), i))
                } else if !expanded
                    && matches!(a.call, Call::Table(_))
                    && a.args.iter().any(available)
                {
                    Some((2, i))
                } else {
                    None
                }
            })
            .min()?;
        let a = &r.query[atom];
        let access = match (&a.call, rank) {
            (Call::Primitive(..), _) => Access::Primitive,
            (_, 2) => {
                expanded = true;
                Access::Expand
            }
            _ => Access::Key,
        };
        let bound = a
            .args
            .iter()
            .enumerate()
            .filter(|(_, v)| available(v))
            .map(|(c, v)| (c, v.clone()))
            .collect();
        let mut assign = vec![];
        let mut check = vec![];
        let last = a.args.len() - 1;
        for (col, v) in a.args.iter().enumerate() {
            if let Arg::Slot(s) = v
                && known.insert(*s)
            {
                assign.push((col, *s));
            } else if col == last || matches!(access, Access::Expand) {
                check.push((col, v.clone()));
            }
        }
        steps.push(Step {
            atom,
            access,
            assign,
            check,
            bound,
        });
        remaining.remove(&atom);
    }
    Some(Kernel {
        computed: computed(r, &steps),
        pivot,
        seed,
        steps,
    })
}

/// A single functional write with a pure operand prefix. Its committed tuple
/// is also the only possible published anchor of this RHS.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Write {
    pub op: usize,
    pub table: usize,
    pub key: Vec<Arg>,
    pub value: Arg,
    pub prefix: Vec<(usize, String, String, Vec<Arg>)>,
}
pub(crate) fn writes(p: &Program) -> Vec<Option<Rc<Write>>> {
    p.rules
        .iter()
        .map(|r| {
            let t = &p.templates[r.template];
            let (Op::Set(table, key, value), prefix) = t.ops.split_last()? else {
                return None;
            };
            if p.tables[*table].virtual_fact
                || (!p.tables[*table].merge.is_null() && p.physical.merges[*table].is_none())
            {
                return None;
            }
            let norm = |a: &Arg| crate::multistage::normalize(t, a);
            let mut calc = vec![];
            for ins in prefix {
                match ins {
                    Op::Copy(..) => {}
                    Op::Let(dst, Call::Primitive(name, sort), args)
                        if !name.starts_with("cell-key-") =>
                    {
                        calc.push((
                            *dst,
                            name.clone(),
                            sort.clone(),
                            args.iter().map(norm).collect(),
                        ))
                    }
                    _ => return None,
                }
            }
            Some(Rc::new(Write {
                op: prefix.len(),
                table: *table,
                key: key.iter().map(norm).collect(),
                value: norm(value),
                prefix: calc,
            }))
        })
        .collect()
}
