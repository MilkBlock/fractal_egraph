//! Checked, conditional congruence programs. They only union existing immutable
//! constructor results. They never construct a future fact or run a source rule.
use crate::program::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Merge {
    pub left: usize,
    pub right: usize,
    pub requires: BTreeSet<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub guards: Vec<(Arg, Arg)>,
    pub merges: Vec<Merge>,
    pub requires: BTreeSet<u32>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Congruence {
    pub templates: BTreeMap<usize, Rc<Vec<Group>>>,
    pub reorder_safe: bool,
}
type Proof = (BTreeSet<(Arg, Arg)>, BTreeSet<u32>);

fn norm(t: &Template, a: &Arg) -> Arg {
    if let Arg::Slot(i) = a {
        if let Some(&op) = t.defs.get(i) {
            if let Op::Copy(_, from) = &t.ops[op] {
                return norm(t, from);
            }
        }
    }
    a.clone()
}
fn constructor<'a>(p: &Program, t: &'a Template, a: &Arg) -> Option<(usize, usize, &'a [Arg])> {
    let Arg::Slot(i) = a else { return None };
    let op = *t.defs.get(i)?;
    match &t.ops[op] {
        Op::Let(_, Call::Table(table), args) if p.tables[*table].constructor => {
            Some((op, *table, args))
        }
        _ => None,
    }
}
fn prove(
    p: &Program,
    t: &Template,
    a: &Arg,
    b: &Arg,
    memo: &mut BTreeMap<(Arg, Arg), Option<Proof>>,
) -> Option<Proof> {
    let (a, b) = (norm(t, a), norm(t, b));
    let pair = if a <= b { (a, b) } else { (b, a) };
    if pair.0 == pair.1 {
        return Some((BTreeSet::new(), BTreeSet::new()));
    }
    if let Some(proof) = memo.get(&pair) {
        return proof.clone();
    }
    let proof = match (constructor(p, t, &pair.0), constructor(p, t, &pair.1)) {
        (Some((i, x, aa)), Some((j, y, bb))) if x == y => {
            let mut guards = BTreeSet::new();
            let mut required = BTreeSet::from([i as u32, j as u32]);
            for (a, b) in aa.iter().zip(bb) {
                let Some((g, r)) = prove(p, t, a, b, memo) else {
                    memo.insert(pair, None);
                    return None;
                };
                guards.extend(g);
                required.extend(r);
            }
            Some((guards, required))
        }
        _ if matches!((&pair.0, &pair.1), (Arg::Literal(_), Arg::Literal(_))) => None,
        _ => Some((BTreeSet::from([pair.clone()]), BTreeSet::new())),
    };
    memo.insert(pair, proof.clone());
    proof
}

impl Congruence {
    pub fn parse(p: &Program, raw: &J) -> Result<Self> {
        let mut out = Self::default();
        if raw.is_null() {
            return Ok(out);
        }
        if raw["schema"] != "eggfuse-local-congruence/v1" {
            return Err("congruence schema".into());
        }
        // Moving constructor equalities ahead of ordered/effectful callbacks can
        // change the callback's observable inputs. Only pure lattice reductions
        // are admitted; arbitrary merge programs retain the existing repair order.
        out.reorder_safe = p.tables.iter().all(|t| {
            if t.virtual_fact {
                return true;
            }
            let m = &t.merge;
            let args = m["args"].as_array();
            t.schema.last().is_some_and(|s| s["kind"] == "i64")
                && matches!(m["call"]["primitive"].as_str(), Some("min" | "max"))
                && args.is_some_and(|a| {
                    a.len() == 2
                        && ((a[0]["var"] == "old" && a[1]["var"] == "new")
                            || (a[1]["var"] == "old" && a[0]["var"] == "new"))
                })
        });
        for entry in array(&raw["templates"])? {
            let rid = entry["rule"].as_u64().ok_or("congruence rule")? as usize;
            let r = p.rules.get(rid).ok_or("congruence rule range")?;
            let t = &p.templates[r.template];
            let mut groups = vec![];
            let mut memo = BTreeMap::new();
            for group in array(&entry["groups"])? {
                let mut guards = BTreeSet::new();
                for g in array(&group["guards"])? {
                    let aa = array(g)?;
                    if aa.len() != 2 {
                        return Err("congruence guard arity".into());
                    }
                    let (a, b) = (arg(&aa[0])?, arg(&aa[1])?);
                    guards.insert(if a <= b { (a, b) } else { (b, a) });
                }
                let mut merges = vec![];
                for m in array(&group["merges"])? {
                    let left = m["left"].as_u64().ok_or("congruence left")? as usize;
                    let right = m["right"].as_u64().ok_or("congruence right")? as usize;
                    let dst = |op: usize| match t.ops.get(op) {
                        Some(Op::Let(slot, Call::Table(table), _))
                            if p.tables[*table].constructor =>
                        {
                            Ok(Arg::Slot(*slot))
                        }
                        _ => Err("congruence is not a constructor".to_string()),
                    };
                    let (expected, required) = prove(p, t, &dst(left)?, &dst(right)?, &mut memo)
                        .ok_or("invalid congruence proof")?;
                    let requires = array(&m["requires"])?
                        .iter()
                        .map(|x| {
                            x.as_u64()
                                .and_then(|v| u32::try_from(v).ok())
                                .ok_or("congruence prerequisite".to_string())
                        })
                        .collect::<Result<BTreeSet<_>>>()?;
                    if expected != guards
                        || required != requires
                        || left == right
                        || !required.contains(&(left as u32))
                        || !required.contains(&(right as u32))
                    {
                        return Err("congruence proof differs from constructor wiring".into());
                    }
                    merges.push(Merge {
                        left,
                        right,
                        requires,
                    });
                }
                groups.push(Group {
                    guards: guards.into_iter().collect(),
                    requires: merges
                        .iter()
                        .flat_map(|m| m.requires.iter().copied())
                        .collect(),
                    merges,
                });
            }
            if out.templates.insert(r.template, Rc::new(groups)).is_some() {
                return Err("duplicate congruence template".into());
            }
        }
        Ok(out)
    }
}
