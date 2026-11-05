//! Checked scalar propagation families. The shared attribute is the family ID.
//! Lane words compare in the same order as the typed rule input tuple.
use crate::{
    known_results::QueryFacts, physical_plan::ScalarMerge, program::*, slots::Slots, value::Value,
};
use serde::Serialize;
use smallvec::SmallVec;
use std::{collections::BTreeMap, rc::Rc};

#[derive(Clone, Copy, Debug, Serialize)]
enum Operand {
    Input(usize),
    Literal(i64),
    Temp(usize),
}
#[derive(Clone, Copy, Debug, Serialize)]
enum Arithmetic {
    Add,
    Sub,
    Mul,
    Min,
    Max,
}
#[derive(Clone, Debug, Serialize)]
struct Instruction {
    dst: usize,
    op: Arithmetic,
    a: Operand,
    b: Operand,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Plan {
    pub table: usize,
    pub op: usize,
    pub merge: ScalarMerge,
    key: usize,
    inputs: Vec<(usize, bool)>, // SSA slot, equality sort (otherwise signed i64).
    program: Vec<Instruction>,
    result: Operand,
}
pub(crate) fn derive(p: &Program) -> Vec<Option<Rc<Plan>>> {
    p.rules
        .iter()
        .enumerate()
        .map(|(rid, r)| {
            let layout = p.neighbor_plans[rid].as_ref()?;
            let write = p.writes[rid].as_ref()?;
            let table = layout.attribute;
            let merge = p.physical.merges[table]?;
            if !matches!(merge, ScalarMerge::Min | ScalarMerge::Max) {
                return None;
            }
            let inputs = r
                .inputs
                .iter()
                .map(|&s| {
                    Some((
                        s,
                        match p.templates[r.template].sorts[s]["kind"].as_str()? {
                            "eq" => true,
                            "i64" => false,
                            _ => return None,
                        },
                    ))
                })
                .collect::<Option<Vec<_>>>()?;
            // All ordinary pivots must use a complete checked register kernel.
            if r.query
                .iter()
                .enumerate()
                .any(|(i, a)| matches!(a.call, Call::Table(_)) && !layout.pivots.contains(&i))
            {
                return None;
            }
            if p.physical.ranges[rid].iter().any(|gs| !gs.is_empty()) {
                return None;
            }
            let [Arg::Slot(slot)] = write.key.as_slice() else {
                return None;
            };
            let key = inputs.iter().position(|&(s, eq)| s == *slot && eq)?;
            let mut values: BTreeMap<usize, Operand> = inputs
                .iter()
                .enumerate()
                .filter(|(_, (_, eq))| !*eq)
                .map(|(i, (s, _))| (*s, Operand::Input(i)))
                .collect();
            let operand = |a: &Arg, vs: &BTreeMap<usize, Operand>| match a {
                Arg::Literal(Value::Int(i)) => Some(Operand::Literal(*i)),
                Arg::Slot(s) => vs.get(s).copied(),
                _ => None,
            };
            let mut program = vec![];
            for (dst, name, sort, args) in &write.prefix {
                if sort != "i64" {
                    return None;
                }
                let [a, b] = args.as_slice() else { return None };
                let op = match name.as_str() {
                    "+" => Arithmetic::Add,
                    "-" => Arithmetic::Sub,
                    "*" => Arithmetic::Mul,
                    "min" => Arithmetic::Min,
                    "max" => Arithmetic::Max,
                    _ => return None,
                };
                program.push(Instruction {
                    dst: *dst,
                    op,
                    a: operand(a, &values)?,
                    b: operand(b, &values)?,
                });
                values.insert(*dst, Operand::Temp(program.len() - 1));
            }
            Some(Rc::new(Plan {
                table,
                op: write.op,
                merge,
                key,
                inputs,
                result: operand(&write.value, &values)?,
                program,
            }))
        })
        .collect()
}
const SIGN: u64 = 1 << 63;
pub(crate) struct Lane {
    words: SmallVec<[u64; 8]>,
    pub facts: QueryFacts,
    #[cfg(feature = "iterative-bake")]
    pub direct: Option<crate::stage_spine::DirectOrigin>,
}
#[derive(Default)]
pub(crate) struct Batch {
    pub lanes: Vec<Lane>,
}
impl Batch {
    pub fn push<'a>(
        &mut self,
        values: impl Iterator<Item = &'a Value>,
        facts: QueryFacts,
        #[cfg(feature = "iterative-bake")] direct: Option<crate::stage_spine::DirectOrigin>,
    ) {
        let words = values
            .map(|v| match v {
                Value::Eq(i) => *i,
                Value::Int(i) => (*i as u64) ^ SIGN,
                _ => unreachable!("checked lane sort"),
            })
            .collect();
        self.lanes.push(Lane {
            words,
            facts,
            #[cfg(feature = "iterative-bake")]
            direct,
        });
    }
    pub fn seal(&mut self) {
        self.lanes.sort_by(|a, b| a.words.cmp(&b.words));
        self.lanes.dedup_by(|a, b| a.words == b.words);
    }
}
impl Plan {
    pub fn values<'a>(&'a self, lane: &'a Lane) -> impl Iterator<Item = Value> + 'a {
        self.inputs.iter().zip(&lane.words).map(|((_, eq), &v)| {
            if *eq {
                Value::Eq(v)
            } else {
                Value::Int((v ^ SIGN) as i64)
            }
        })
    }
    pub fn slots(&self, lane: &Lane) -> Slots {
        self.inputs
            .iter()
            .map(|(s, _)| *s)
            .zip(self.values(lane))
            .collect()
    }
    pub fn key(&self, lane: &Lane) -> u64 {
        lane.words[self.key]
    }
    pub fn eval(&self, lane: &Lane) -> Result<(i64, SmallVec<[(usize, i64); 2]>)> {
        let read = |a, temps: &[(usize, i64)]| match a {
            Operand::Literal(v) => v,
            Operand::Input(i) => (lane.words[i] ^ SIGN) as i64,
            Operand::Temp(i) => temps[i].1,
        };
        let mut temps = SmallVec::<[(usize, i64); 2]>::new();
        for ins in &self.program {
            let (a, b) = (read(ins.a, &temps), read(ins.b, &temps));
            let v = match ins.op {
                Arithmetic::Add => a.checked_add(b),
                Arithmetic::Sub => a.checked_sub(b),
                Arithmetic::Mul => a.checked_mul(b),
                Arithmetic::Min => Some(a.min(b)),
                Arithmetic::Max => Some(a.max(b)),
            }
            .ok_or("RHS primitive failed: attribute arithmetic overflow")?;
            temps.push((ins.dst, v));
        }
        Ok((read(self.result, &temps), temps))
    }
}

/// A shared attribute delta is identical for consumers of the same scheduler
/// node and watermark in a sealed Run. Reverse plans retain their smaller domain.
#[derive(Default)]
pub(crate) struct Frontiers {
    packets: rustc_hash::FxHashMap<(usize, u64), Rc<Vec<Rc<crate::entry_schedule::Packet>>>>,
    refs: usize,
    pub builds: u64,
    pub reuses: u64,
}
impl Frontiers {
    pub fn reset(&mut self) {
        self.packets.clear();
        self.refs = 0;
    }
    pub fn get(
        &mut self,
        batch: crate::entry_schedule::Batch<'_>,
    ) -> Option<Rc<Vec<Rc<crate::entry_schedule::Packet>>>> {
        let key = batch.frontier_key();
        if let Some(hit) = self.packets.get(&key) {
            self.reuses += 1;
            return Some(hit.clone());
        }
        let size = batch.delta_len()?;
        if size < 8 || size > 65536 - self.refs {
            return None;
        }
        let rows = Rc::new(
            batch
                .delta()
                .map(|&(_, w)| batch.packet(w).unwrap().clone())
                .collect::<Vec<_>>(),
        );
        self.refs += size;
        self.builds += 1;
        self.packets.insert(key, rows.clone());
        Some(rows)
    }
}
