//! Immutable parameters of a completed functional-write block. The function
//! row stores the *merged* value; these ports store the original RHS inputs and
//! computed proposal. They must never alias a mutable function-row payload.
use crate::{program::*, slots::Slots, value::Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const WIDTH: usize = 2;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    Eq,
    Int,
    Float,
}
impl Kind {
    fn encode(self, v: &Value) -> Option<u64> {
        match (self, v) {
            (Self::Eq, Value::Eq(v)) => Some(*v),
            (Self::Int, Value::Int(v)) | (Self::Float, Value::Float(v)) => Some(*v as u64),
            _ => None,
        }
    }
    fn decode(self, v: u64) -> Value {
        match self {
            Self::Eq => Value::Eq(v),
            Self::Int => Value::Int(v as i64),
            Self::Float => Value::Float(v as i64),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Plan {
    pub table: usize,
    pub op: usize,
    pub slots: Vec<(usize, Kind)>,
    pub omitted: Vec<usize>,
    pub execution_inputs: Vec<usize>,
}
pub(crate) fn derive(p: &Program, routes: &crate::multistage::MultiStage) -> Vec<Option<Plan>> {
    let mut out = vec![None; p.templates.len()];
    for r in &p.rules {
        let t = &p.templates[r.template];
        let Some((Op::Set(table, key, value), prefix)) = t.ops.split_last() else {
            continue;
        };
        if p.tables[*table].virtual_fact {
            continue;
        }
        let mut available: BTreeSet<_> = r.inputs.iter().copied().collect();
        let mut supported = true;
        for op in prefix {
            match op {
                Op::Copy(..) => (), // reconstructed from the checked SSA recipe
                Op::Let(dst, Call::Primitive(..), _) => {
                    available.insert(*dst);
                }
                _ => {
                    supported = false;
                    break;
                }
            }
        }
        if !supported {
            continue;
        }
        // The published tuple is always available, including when this producer
        // is retained as a parent. Local continuation plans may also read source
        // query columns: retain their complete projection conservatively.
        let mut slots = BTreeSet::new();
        let mut need = |a: &Arg| {
            if let Arg::Slot(s) = crate::multistage::normalize(t, a) {
                slots.insert(s);
            }
        };
        for a in key.iter().chain(std::iter::once(value)) {
            need(a);
        }
        for ((producer, _, _, _), route) in &routes.routes {
            if *producer == r.template {
                for read in &route.reads {
                    for a in &read.columns {
                        need(a);
                    }
                }
            }
        }
        if slots.len() > 64 || !slots.is_subset(&available) {
            continue;
        }
        let omitted = available.difference(&slots).copied().collect();
        let mut execution_inputs = BTreeSet::new();
        for op in t.ops.iter() {
            let args: Vec<_> = match op {
                Op::Copy(_, a) => vec![a],
                Op::Let(_, _, aa) => aa.iter().collect(),
                Op::Set(_, aa, v) => aa.iter().chain(std::iter::once(v)).collect(),
                _ => unreachable!(),
            };
            for a in args {
                if let Arg::Slot(s) = crate::multistage::normalize(t, a)
                    && !t.defs.contains_key(&s)
                {
                    execution_inputs.insert(s);
                }
            }
        }
        let slots = slots
            .into_iter()
            .map(|s| {
                Some((
                    s,
                    match t.sorts.get(s)?["kind"].as_str()? {
                        "eq" => Kind::Eq,
                        "i64" | "bool" | "Unit" => Kind::Int,
                        "f64" => Kind::Float,
                        _ => return None,
                    },
                ))
            })
            .collect::<Option<Vec<_>>>();
        if let Some(slots) = slots {
            out[r.template] = Some(Plan {
                table: *table,
                op: prefix.len(),
                slots,
                omitted,
                execution_inputs: execution_inputs.into_iter().collect(),
            });
        }
    }
    out
}
#[derive(Clone, Debug)]
pub(crate) enum Parameters {
    Ordinary(Slots),
    Ports { words: [u64; WIDTH], present: u64 },
    Wide { words: Box<[u64]>, present: u64 },
}
impl Default for Parameters {
    fn default() -> Self {
        Self::Ordinary(Slots::default())
    }
}
impl Parameters {
    pub fn pack(
        values: Slots,
        plan: Option<&Plan>,
        template: &Template,
        borrowed: bool,
        reuse: Option<Box<[u64]>>,
    ) -> Self {
        let Some(plan) = plan else {
            return Self::Ordinary(values);
        };
        let mut ports = [0; WIDTH];
        let count = plan
            .slots
            .iter()
            .filter(|(s, _)| values.contains_key(s))
            .count();
        let mut wide = if count > WIDTH {
            Some(
                reuse
                    .filter(|v| v.len() == count)
                    .unwrap_or_else(|| vec![0; count].into_boxed_slice()),
            )
        } else {
            None
        };
        let mut present = 0u64;
        let mut n = 0;
        for (i, (s, kind)) in plan.slots.iter().enumerate() {
            let Some(v) = values.get(s) else {
                if borrowed && template.parent_columns.contains_key(s) {
                    continue;
                }
                return Self::Ordinary(values);
            };
            let Some(word) = kind.encode(v) else {
                return Self::Ordinary(values);
            };
            if let Some(w) = &mut wide {
                w[n] = word;
            } else {
                ports[n] = word;
            }
            n += 1;
            present |= 1u64 << i;
        }
        // General RHS execution can eagerly record Copy slots; the single-write
        // path leaves them lazy. Only those checked aliases may be omitted.
        if values.keys().any(|s| {
            plan.slots.binary_search_by_key(s, |x| x.0).is_err()
                && plan.omitted.binary_search(s).is_err()
                && !template
                    .defs
                    .get(s)
                    .is_some_and(|&i| matches!(template.ops[i], Op::Copy(..)))
        }) {
            return Self::Ordinary(values);
        }
        if let Some(words) = wide {
            Self::Wide { words, present }
        } else {
            Self::Ports {
                words: ports,
                present,
            }
        }
    }
    pub fn unpack(self, plan: Option<&Plan>) -> Slots {
        match self {
            Self::Ordinary(v) => v,
            Self::Ports { words, present } => {
                let mut n = 0;
                plan.expect("checked function block")
                    .slots
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| present & (1u64 << i) != 0)
                    .map(|(_, (slot, kind))| {
                        let v = (*slot, kind.decode(words[n]));
                        n += 1;
                        v
                    })
                    .collect()
            }
            Self::Wide { words, present } => plan
                .expect("checked function block")
                .slots
                .iter()
                .enumerate()
                .filter(|(i, _)| present & (1u64 << i) != 0)
                .zip(words.iter())
                .map(|((_, (slot, kind)), v)| (*slot, kind.decode(*v)))
                .collect(),
        }
    }
    pub fn get(&self, plan: Option<&Plan>, slot: usize) -> Option<Value> {
        match self {
            Self::Ordinary(v) => v.get(&slot).cloned(),
            Self::Ports { words, present } => Self::read_words(words, *present, plan, slot),
            Self::Wide { words, present } => Self::read_words(words, *present, plan, slot),
        }
    }
    fn read_words(words: &[u64], present: u64, plan: Option<&Plan>, slot: usize) -> Option<Value> {
        let p = plan?;
        let at = p.slots.binary_search_by_key(&slot, |x| x.0).ok()?;
        if present & (1u64 << at) == 0 {
            return None;
        }
        let rank = (present & ((1u64 << at) - 1)).count_ones() as usize;
        Some(p.slots[at].1.decode(words[rank]))
    }
    pub fn is_packed(&self) -> bool {
        matches!(self, Self::Ports { .. } | Self::Wide { .. })
    }
    pub fn stored_slots(&self) -> usize {
        match self {
            Self::Ordinary(v) => v.len(),
            Self::Ports { present, .. } | Self::Wide { present, .. } => {
                present.count_ones() as usize
            }
        }
    }
    pub fn take_wide(&mut self) -> Option<Box<[u64]>> {
        if matches!(self, Self::Wide { .. })
            && let Self::Wide { words, .. } = std::mem::take(self)
        {
            Some(words)
        } else {
            None
        }
    }
    pub fn wide_len(&self) -> usize {
        if let Self::Wide { words, .. } = self {
            words.len()
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ports_preserve_signed_integers_eq_ids_and_float_bits() {
        let mut t = Template {
            name: String::new(),
            sorts: vec![],
            defs: Default::default(),
            ops: std::rc::Rc::new(vec![]),
            parent_columns: Default::default(),
        };
        let p = Plan {
            table: 0,
            op: 0,
            slots: vec![(2, Kind::Eq), (4, Kind::Int), (6, Kind::Float)],
            omitted: vec![],
            execution_inputs: vec![],
        };
        let original: Slots = [
            (2, Value::Eq(u64::MAX)),
            (4, Value::Int(i64::MIN)),
            (6, Value::Float(-1)),
        ]
        .into_iter()
        .collect();
        let packed = Parameters::pack(original.clone(), Some(&p), &t, false, None);
        assert!(packed.is_packed());
        assert_eq!(packed.get(Some(&p), 4), Some(Value::Int(i64::MIN)));
        assert_eq!(packed.unpack(Some(&p)), original);
        let wrong: Slots = [(2, Value::Int(1))].into_iter().collect();
        assert!(!Parameters::pack(wrong, Some(&p), &t, false, None).is_packed());
        t.parent_columns.insert(2, 0);
        let borrowed: Slots = [(4, Value::Int(-3)), (6, Value::Float(0))]
            .into_iter()
            .collect();
        let packed = Parameters::pack(borrowed.clone(), Some(&p), &t, true, None);
        assert!(packed.is_packed());
        assert_eq!(packed.get(Some(&p), 2), None);
        assert_eq!(packed.get(Some(&p), 4), Some(Value::Int(-3)));
        assert_eq!(packed.unpack(Some(&p)), borrowed);
    }
}
