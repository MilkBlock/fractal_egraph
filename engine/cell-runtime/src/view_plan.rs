//! A Unit view can deliver sealed rows directly to a family of binary joins.
//! Every rule reader is accounted for. Unsupported query shapes keep the normal plan.
use crate::{multistage::normalize, program::*, value::Value};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

/// Immutable fields exported at the repaired barrier. A projected Unit key
/// shares the producer's array until repair switches it to identity wiring.
#[cfg(feature = "view-consumers")]
#[derive(Clone)]
pub(crate) struct Tuple {
    pub witness: crate::graph::Dependency,
    pub epoch: u64,
    pub fields: crate::graph::RowFields,
    pub columns: Option<Rc<Vec<usize>>>,
}
#[cfg(feature = "view-consumers")]
impl Tuple {
    pub fn port(&self, column: usize) -> &Value {
        match &self.columns {
            Some(cols) if column == cols.len() => &Value::Int(0),
            Some(cols) => &self.fields[cols[column]],
            None => &self.fields[column],
        }
    }
    pub fn flat(row: &crate::graph::Row) -> Self {
        Self {
            witness: row.witness,
            epoch: row.epoch,
            fields: row.values.clone(),
            columns: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Port {
    Driver(usize),
    Peer(usize),
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Comparison {
    Less,
    Greater,
    Equal,
    LessEqual,
    GreaterEqual,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Test {
    pub left: Port,
    pub right: Port,
    pub comparison: Comparison,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Member {
    pub rule: usize,
    pub driver_atom: usize,
    pub peer_atom: usize,
    pub tests: Vec<Test>,
    pub output: Vec<(usize, Port)>,
}
/// A disjoint and exhaustive three-way comparison routes a pair to one member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Dispatch {
    pub left: Port,
    pub right: Port,
    pub less: usize,
    pub equal: usize,
    pub greater: usize,
}
impl Dispatch {
    fn derive(members: &[Member]) -> Option<Self> {
        if members.len() != 3 {
            return None;
        }
        let [test] = members[0].tests.as_slice() else {
            return None;
        };
        let (left, right) = (test.left, test.right);
        let mut slots = [None; 3];
        for (i, m) in members.iter().enumerate() {
            let [t] = m.tests.as_slice() else { return None };
            let normal = (t.left, t.right) == (left, right);
            if !normal && (t.left, t.right) != (right, left) {
                return None;
            }
            let branch = match t.comparison {
                Comparison::Less => {
                    if normal {
                        0
                    } else {
                        2
                    }
                }
                Comparison::Equal => 1,
                Comparison::Greater => {
                    if normal {
                        2
                    } else {
                        0
                    }
                }
                _ => return None,
            };
            if slots[branch].replace(i).is_some() {
                return None;
            }
        }
        Some(Self {
            left,
            right,
            less: slots[0]?,
            equal: slots[1]?,
            greater: slots[2]?,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Join {
    pub driver: usize,
    pub peer: usize,
    pub driver_key: usize,
    pub peer_key: usize,
    pub members: Vec<Member>,
    pub dispatch: Option<Dispatch>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Write {
    pub rule: usize,
    pub op: usize,
    pub old: Vec<Arg>,
    pub cols: Rc<Vec<usize>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Block {
    pub view: usize,
    pub producer: Join,
    pub consumers: Join,
    pub writes: Vec<Write>,
}

fn slots(a: &Atom) -> Option<Vec<usize>> {
    let v = a
        .args
        .iter()
        .map(|x| match x {
            Arg::Slot(i) => Some(*i),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (v.iter().collect::<BTreeSet<_>>().len() == v.len()).then_some(v)
}

fn member(
    p: &Program,
    rid: usize,
    driver: usize,
    spine: &BTreeSet<usize>,
) -> Option<(usize, usize, usize, Member)> {
    let r = &p.rules[rid];
    if !r.seminaive || spine.contains(&rid) {
        return None;
    }
    let tables = r
        .query
        .iter()
        .enumerate()
        .filter_map(|(i, a)| match a.call {
            Call::Table(t) => Some((i, t)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [a, b] = tables.as_slice() else {
        return None;
    };
    let (di, pi, peer) = if a.1 == driver {
        (a.0, b.0, b.1)
    } else if b.1 == driver {
        (b.0, a.0, a.1)
    } else {
        return None;
    };
    if peer == driver || !p.tables[peer].constructor {
        return None;
    }
    let left = slots(&r.query[di])?;
    let right = slots(&r.query[pi])?;
    let mut fields: BTreeMap<_, _> = left
        .iter()
        .enumerate()
        .map(|(i, s)| (*s, Port::Driver(i)))
        .collect();
    let mut join = None;
    let mut tests = vec![];
    for (i, s) in right.iter().enumerate() {
        if let Some(&prev) = fields.get(s) {
            let Port::Driver(k) = prev else { return None };
            if join.is_none() && p.tables[peer].schema[i]["kind"] == "eq" {
                join = Some((k, i));
            } else {
                tests.push(Test {
                    left: prev,
                    right: Port::Peer(i),
                    comparison: Comparison::Equal,
                });
            }
        } else {
            fields.insert(*s, Port::Peer(i));
        }
    }
    for a in r.query.iter() {
        if let Call::Primitive(name, out) = &a.call {
            let [Arg::Slot(x), Arg::Slot(y), Arg::Slot(result)] = a.args.as_slice() else {
                return None;
            };
            if out != "Unit"
                || [x, y]
                    .iter()
                    .any(|k| p.templates[r.template].sorts[**k]["name"] != "i64")
            {
                return None;
            }
            let comparison = match name.as_str() {
                "<" => Comparison::Less,
                ">" => Comparison::Greater,
                "<=" => Comparison::LessEqual,
                ">=" => Comparison::GreaterEqual,
                _ => return None,
            };
            tests.push(Test {
                left: *fields.get(x)?,
                right: *fields.get(y)?,
                comparison,
            });
            // A previously bound primitive result is an additional equality predicate.
            if let Some(prior) = fields.insert(*result, Port::Unit) {
                tests.push(Test {
                    left: prior,
                    right: Port::Unit,
                    comparison: Comparison::Equal,
                });
            }
        }
    }
    let output = r
        .inputs
        .iter()
        .map(|s| Some((*s, *fields.get(s)?)))
        .collect::<Option<_>>()?;
    let (dk, pk) = join?;
    Some((
        peer,
        dk,
        pk,
        Member {
            rule: rid,
            driver_atom: di,
            peer_atom: pi,
            tests,
            output,
        },
    ))
}

fn write(p: &Program, rid: usize, op: usize, table: usize, view_atom: usize) -> Option<Write> {
    let r = &p.rules[rid];
    let t = &p.templates[r.template];
    let Op::Set(_, args, Arg::Literal(Value::Int(0))) = &t.ops[op] else {
        return None;
    };
    let relevant = t.ops[..op]
        .iter()
        .rev()
        .take_while(|x| !matches!(x, Op::Hide(..)))
        .any(|x| match x {
            Op::Union(..) => true,
            Op::Set(t, _, _) => p.tables[*t].constructor,
            _ => false,
        });
    if !relevant {
        return None;
    }
    let old = &r.query[view_atom].args[..p.tables[table].schema.len() - 1];
    let next: Vec<_> = args.iter().map(|a| normalize(t, a)).collect();
    if old == next
        || old
            .iter()
            .any(|a| matches!(a,Arg::Slot(s) if !r.inputs.contains(s)))
    {
        return None;
    }
    let cols = next
        .iter()
        .map(|a| old.iter().position(|b| b == a))
        .collect::<Option<Vec<_>>>()?;
    Some(Write {
        rule: rid,
        op,
        old: old.to_vec(),
        cols: Rc::new(cols),
    })
}

pub(crate) fn derive(p: &Program, spine: &BTreeSet<usize>) -> Vec<Block> {
    let mut reads = vec![BTreeSet::new(); p.tables.len()];
    let mut writes = vec![vec![]; p.tables.len()];
    for (rid, r) in p.rules.iter().enumerate() {
        for a in r.query.iter() {
            if let Call::Table(t) = a.call {
                reads[t].insert(rid);
            }
        }
        for (i, op) in p.templates[r.template].ops.iter().enumerate() {
            if let Op::Set(t, _, _) = op {
                writes[*t].push((rid, i));
            }
        }
    }
    let mut result = vec![];
    let mut claimed = BTreeSet::new();
    for (view, table) in p.tables.iter().enumerate() {
        if table.virtual_fact
            || table.schema.last().is_none_or(|s| s["kind"] != "Unit")
            || table.merge != serde_json::json!({"var":"old"})
            || reads[view].len() < 2
        {
            continue;
        }
        if p.sets.values().any(|rs| {
            let count = reads[view].iter().filter(|r| rs.contains(r)).count();
            count > 0 && count < reads[view].len()
        }) {
            continue;
        }
        let Some(members) = reads[view]
            .iter()
            .map(|&r| member(p, r, view, spine))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let (peer, dk, pk, _) = members[0];
        if members
            .iter()
            .any(|(t, i, j, _)| (*t, *i, *j) != (peer, dk, pk))
        {
            continue;
        }
        let owners: Vec<_> = writes[view]
            .iter()
            .filter(|(r, _)| !reads[view].contains(r))
            .collect();
        let [owner] = owners.as_slice() else {
            continue;
        };
        let (producer, op) = **owner;
        let r = &p.rules[producer];
        if !p.templates[r.template]
            .ops
            .iter()
            .enumerate()
            .all(|(n, o)| n == op || matches!(o, Op::Copy(..)))
        {
            continue;
        }
        let Some(base) = r.query.iter().find_map(|a| match a.call {
            Call::Table(t) if t != peer && p.tables[t].constructor => Some(t),
            _ => None,
        }) else {
            continue;
        };
        let Some((pt, dkp, pkp, pm)) = member(p, producer, base, spine) else {
            continue;
        };
        if pt != peer {
            continue;
        }
        let mut aliases = vec![];
        for (_, _, _, m) in &members {
            for &(rid, op) in &writes[view] {
                if rid == m.rule
                    && let Some(w) = write(p, rid, op, view, m.driver_atom)
                {
                    aliases.push(w)
                }
            }
        }
        if aliases.is_empty()
            || claimed.contains(&producer)
            || reads[view].iter().any(|r| claimed.contains(r))
        {
            continue;
        }
        claimed.insert(producer);
        claimed.extend(&reads[view]);
        let members: Vec<_> = members.into_iter().map(|v| v.3).collect();
        result.push(Block {
            view,
            producer: Join {
                driver: base,
                peer,
                driver_key: dkp,
                peer_key: pkp,
                members: vec![pm],
                dispatch: None,
            },
            consumers: Join {
                driver: view,
                peer,
                driver_key: dk,
                peer_key: pk,
                dispatch: Dispatch::derive(&members),
                members,
            },
            writes: aliases,
        });
    }
    result
}
