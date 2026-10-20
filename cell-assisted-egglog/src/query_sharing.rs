//! Exact whole-query sharing inside one frozen Run. No shape approximation and
//! no cross-boundary cache: every consumer keeps its native watermark/effects.
use egglog::cell_assist::{CellActionSink, CellOperand};
use egglog_numeric_id::NumericId;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Queries {
    pub groups: Vec<usize>,
    // For each canonical coordinate, the original native binding slot.
    wiring: Vec<Vec<usize>>,
    pub group_count: usize,
}
fn operand(o: &CellOperand, names: &mut BTreeMap<usize, usize>) -> Json {
    match o {
        CellOperand::Slot(i) => {
            let next = names.len();
            json!(["slot", *names.entry(*i).or_insert(next)])
        }
        CellOperand::Constant { value, text } => json!(["literal", value.rep(), text]),
    }
}
impl Queries {
    pub fn compile(sinks: &[CellActionSink]) -> Self {
        let mut out = Self::default();
        let mut groups = BTreeMap::new();
        for q in sinks {
            let mut names = BTreeMap::new();
            let atoms = q
                .atoms
                .iter()
                .map(|a| {
                    let cols = a
                        .columns
                        .iter()
                        .map(|o| operand(o, &mut names))
                        .collect::<Vec<_>>();
                    json!([a.function, a.sorts, cols])
                })
                .collect::<Vec<_>>();
            let native = q.slots().len();
            assert_eq!(names.len(), native, "native interface must be table-bound");
            let mut wiring = vec![0; native];
            for (original, canonical) in &names {
                wiring[*canonical] = *original;
            }
            let guards = q
                .guards
                .iter()
                .map(|g| {
                    json!([
                        g.opcode,
                        g.operands
                            .iter()
                            .map(|o| operand(o, &mut names))
                            .collect::<Vec<_>>()
                    ])
                })
                .collect::<Vec<_>>();
            let ops = q
                .primitives
                .iter()
                .map(|p| {
                    let args = p
                        .args
                        .iter()
                        .map(|o| operand(o, &mut names))
                        .collect::<Vec<_>>();
                    let result = p.result.as_ref().map(|o| operand(o, &mut names));
                    json!([p.opcode, args, result])
                })
                .collect::<Vec<_>>();
            // Atom order, repeated-variable constraints, literal types and all
            // predicates remain in this exact key. RHS and variable names do not.
            let key = json!([atoms, guards, ops, native, q.device_slots()]).to_string();
            let next = groups.len();
            out.groups.push(*groups.entry(key).or_insert(next));
            out.wiring.push(wiring);
        }
        out.group_count = groups.len();
        out
    }
    pub fn transport(
        &self,
        leader: usize,
        consumer: usize,
        rows: &[Vec<egglog::Value>],
    ) -> Vec<Vec<egglog::Value>> {
        assert_eq!(self.groups[leader], self.groups[consumer]);
        rows.iter()
            .map(|row| {
                let mut output = row.clone();
                for (from, to) in self.wiring[leader].iter().zip(&self.wiring[consumer]) {
                    output[*to] = row[*from];
                }
                output
            })
            .collect()
    }
}
