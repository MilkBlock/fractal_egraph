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
    pub pivot: usize,
    pub seed: Vec<(usize, usize)>,
    pub steps: Vec<Step>,
}
pub(crate) type Kernels = Vec<Vec<Option<Rc<Kernel>>>>;
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
    Some(Kernel { pivot, seed, steps })
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
