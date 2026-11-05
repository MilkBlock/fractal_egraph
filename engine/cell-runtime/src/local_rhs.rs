//! Compile a chain of virtual constructor reads with an optional terminal Set.
//! The ordinary graph owns every new fact. Existing results can live in the
//! current RHS frame until a miss needs that frame as its physical owner.
use crate::{known_results::Plans, program::*};
use serde::Serialize;
use std::rc::Rc;
#[derive(Clone, Debug, Serialize)]
pub(crate) enum ResultUse {
    Let(usize),
    Set(Arg),
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Step {
    pub op: usize,
    pub table: usize,
    pub key: Vec<Arg>,
    pub result: ResultUse,
    pub skip_before: u64,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Block {
    pub steps: Vec<Step>,
    pub trailing_aliases: u64,
    pub intermediate_ops: Vec<usize>,
}
pub(crate) fn derive(p: &Program, known: &Plans) -> Vec<Option<Rc<Block>>> {
    p.rules
        .iter()
        .enumerate()
        .map(|(r, rule)| {
            let mut steps = vec![];
            let mut skipped = 0;
            let mut terminal = false;
            let mut origins = std::collections::BTreeMap::new();
            let mut consumed = std::collections::BTreeSet::new();
            for (op, ins) in p.templates[rule.template].ops.iter().enumerate() {
                if known.static_skip[r][op] {
                    if let Op::Let(dst, ..) = ins {
                        if let Some(Arg::Slot(source)) = &known.static_slots[r][*dst]
                            && let Some(&prior) = origins.get(source)
                        {
                            origins.insert(*dst, prior);
                        }
                    }
                    skipped += 1;
                    continue;
                }
                if let Op::Copy(dst, a) = ins {
                    if let Arg::Slot(s) = a
                        && let Some(&prior) = origins.get(s)
                    {
                        origins.insert(*dst, prior);
                    }
                    continue;
                }
                if terminal {
                    return None;
                }
                let (t, key, result) = match ins {
                    Op::Let(s, Call::Table(t), key) => (*t, key, ResultUse::Let(*s)),
                    Op::Set(t, key, v) => {
                        terminal = true;
                        (*t, key, ResultUse::Set(v.clone()))
                    }
                    _ => return None,
                };
                if !p.tables[t].constructor || !p.tables[t].virtual_fact {
                    return None;
                }
                let set_value = if let ResultUse::Set(v) = &result {
                    Some(v)
                } else {
                    None
                };
                for a in key.iter().chain(set_value) {
                    if let Arg::Slot(s) = a
                        && let Some(&prior) = origins.get(s)
                    {
                        consumed.insert(prior);
                    }
                }
                if let ResultUse::Let(dst) = result {
                    origins.insert(dst, op);
                }
                steps.push(Step {
                    op,
                    table: t,
                    key: key.clone(),
                    result,
                    skip_before: std::mem::take(&mut skipped),
                });
            }
            (!steps.is_empty()).then(|| {
                Rc::new(Block {
                    steps,
                    trailing_aliases: skipped,
                    intermediate_ops: consumed.into_iter().collect(),
                })
            })
        })
        .collect()
}
#[derive(Default, Serialize)]
pub(crate) struct Counts {
    pub enter: u64,
    pub local_complete: u64,
    pub promoted: u64,
    pub block_local_intermediates: u64,
    pub local_let_results: u64,
    pub prefix_lets_on_promotion: u64,
    pub no_op_unions: u64,
    pub ordered_unions: u64,
}

impl Program {
    /// Portable admission key. All argument wiring and effect legality are
    /// derived again from the target rule before the hint can select it.
    pub fn rhs_operator_shape(&self, rule: usize) -> Option<Vec<[String; 2]>> {
        Some(
            self.local_rhs
                .get(rule)?
                .as_ref()?
                .steps
                .iter()
                .map(|s| {
                    [
                        match s.result {
                            ResultUse::Let(_) => "Let",
                            ResultUse::Set(_) => "Set",
                        }
                        .into(),
                        self.tables[s.table].name.clone(),
                    ]
                })
                .collect(),
        )
    }
}
