//! Access paths and order-preserving projections derived from the typed recipe.
use crate::program::{Arg, Atom, Call, Program, Rule, Table};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum Bound {
    Lt,
    Le,
    Gt,
    Ge,
}
impl Bound {
    fn reverse(self) -> Self {
        match self {
            Self::Lt => Self::Gt,
            Self::Le => Self::Ge,
            Self::Gt => Self::Lt,
            Self::Ge => Self::Le,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Guard {
    pub atom: usize,
    pub column: usize,
    pub bound: Bound,
    pub threshold: Arg,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum ScalarMerge {
    Old,
    Min,
    Max,
}
#[derive(Default, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Plan {
    pub ranges: Vec<Vec<Vec<Guard>>>,
    pub dedup: Vec<Vec<usize>>,
    pub merges: Vec<Option<ScalarMerge>>,
}
impl Plan {
    pub fn derive(p: &Program) -> Self {
        Self {
            ranges: p
                .rules
                .iter()
                .map(|r| r.query.iter().map(|a| guards(p, r, a)).collect())
                .collect(),
            dedup: p.rules.iter().map(prefix_key).collect(),
            merges: p.tables.iter().map(scalar_merge).collect(),
        }
    }
}
fn guards(p: &Program, r: &Rule, scan: &Atom) -> Vec<Guard> {
    let Call::Table(t) = scan.call else {
        return vec![];
    };
    let mut plans = vec![];
    for (atom, guard) in r.query.iter().enumerate() {
        let Call::Primitive(name, output) = &guard.call else {
            continue;
        };
        if output != "Unit" || guard.args.len() != 3 {
            continue;
        }
        let bound = match name.as_str() {
            "<" => Bound::Lt,
            "<=" => Bound::Le,
            ">" => Bound::Gt,
            ">=" => Bound::Ge,
            _ => continue,
        };
        for side in 0..2 {
            let arg = &guard.args[side];
            if !matches!(arg, Arg::Slot(_)) || arg == &guard.args[1 - side] {
                continue;
            }
            if let Some(column) = scan.args.iter().position(|a| a == arg) {
                if p.tables[t].schema[column]["kind"] == "i64" {
                    plans.push(Guard {
                        atom,
                        column,
                        bound: if side == 0 { bound } else { bound.reverse() },
                        threshold: guard.args[1 - side].clone(),
                    });
                }
            }
        }
    }
    plans
}
fn prefix_key(rule: &Rule) -> Vec<usize> {
    let mut kept = vec![];
    let mut known = BTreeSet::new();
    for &slot in rule.inputs.iter() {
        // At equal retained prefixes a complete table key determines its output in
        // this repaired snapshot. Dropping only such columns preserves lexicographic order.
        loop {
            let before = known.len();
            for a in rule.query.iter() {
                if !matches!(a.call, Call::Table(_)) {
                    continue;
                }
                if let Some((Arg::Slot(result), key)) = a.args.split_last() {
                    if key.iter().all(|x| {
                        matches!(x, Arg::Literal(_))
                            || matches!(x,Arg::Slot(i) if known.contains(i))
                    }) {
                        known.insert(*result);
                    }
                }
            }
            if known.len() == before {
                break;
            }
        }
        if known.insert(slot) {
            kept.push(slot);
        }
    }
    kept
}
fn scalar_merge(t: &Table) -> Option<ScalarMerge> {
    if t.constructor {
        return None;
    }
    let e = &t.merge;
    if e["var"] == "old" {
        return Some(ScalarMerge::Old);
    }
    let args = e["args"].as_array()?;
    if t.schema.last()?["kind"] != "i64"
        || args.len() != 2
        || !((args[0]["var"] == "old" && args[1]["var"] == "new")
            || (args[0]["var"] == "new" && args[1]["var"] == "old"))
    {
        return None;
    }
    match e["call"]["primitive"].as_str()? {
        "min" => Some(ScalarMerge::Min),
        "max" => Some(ScalarMerge::Max),
        _ => None,
    }
}
