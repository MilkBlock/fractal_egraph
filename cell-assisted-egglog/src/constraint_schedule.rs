//! Place a pure constraint at its first fully bound Join stage. Subsequent
//! branches inherit its result; they need not re-evaluate the same prefix.
use egglog::cell_assist::{CellActionSink, CellOperand};
use std::collections::BTreeSet;

pub(crate) fn stages(q: &CellActionSink, order: &[usize]) -> Option<Vec<Vec<u32>>> {
    let mut bound = BTreeSet::new();
    let mut done = vec![false; q.primitives.len()];
    let mut stages = Vec::new();
    for atom in order {
        bound.extend(q.atoms[*atom].columns.iter().filter_map(|x| match x {
            CellOperand::Slot(s) => Some(*s),
            _ => None,
        }));
        let mut stage = Vec::new();
        loop {
            let before = stage.len();
            for (i, op) in q.primitives.iter().enumerate() {
                if done[i]
                    || !op.args.iter().all(|x| match x {
                        CellOperand::Slot(s) => bound.contains(s),
                        _ => true,
                    })
                {
                    continue;
                }
                stage.push(i as u32);
                done[i] = true;
                if let Some(CellOperand::Slot(s)) = &op.result {
                    bound.insert(*s);
                }
            }
            if before == stage.len() {
                break;
            }
        }
        stages.push(stage);
    }
    done.iter().all(|x| *x).then_some(stages)
}

/// Two bits per slot: absent, a native ID, or a computed integer awaiting its
/// native ID. Values of ancestor slots cannot change after a successful bind.
pub(crate) fn binding_states(
    q: &CellActionSink,
    order: &[usize],
    stages: &[Vec<u32>],
    width: usize,
) -> Vec<Vec<u32>> {
    let mut state = vec![0u32; width];
    order
        .iter()
        .zip(stages)
        .map(|(atom, ops)| {
            for x in &q.atoms[*atom].columns {
                if let CellOperand::Slot(s) = x {
                    state[*s] = 1;
                }
            }
            for op in ops {
                if let Some(CellOperand::Slot(s)) = &q.primitives[*op as usize].result {
                    if state[*s] == 0 {
                        state[*s] = 2;
                    }
                }
            }
            state
                .chunks(16)
                .map(|chunk| {
                    chunk
                        .iter()
                        .enumerate()
                        .fold(0, |mask, (i, v)| mask | (*v << (2 * i)))
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn computed_key_then_late_check_runs_each_constraint_once() {
        let mut eg = egglog::EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
            (relation Input (i64)) (relation At (i64 i64)) (relation Output (i64))
            (rule ((Input x) (At (+ x 1) y) (> y x)) ((Output y)) :name "q")
        "#,
        )
        .unwrap();
        let q = eg.prepare_gpu_cell_action("", "q").unwrap();
        for order in [vec![0, 1], vec![1, 0]] {
            let plan = stages(&q, &order).unwrap();
            let mut all = plan.iter().flatten().copied().collect::<Vec<_>>();
            all.sort_unstable();
            assert_eq!(all, (0..q.primitives.len() as u32).collect::<Vec<_>>());
            assert!(!plan[1].is_empty(), "the late guard needs both atoms");
            let states = binding_states(&q, &order, &plan, q.device_slots());
            let last = states.last().unwrap();
            for s in 0..q.slot_count() {
                assert_eq!(
                    (last[s / 16] >> (2 * (s % 16))) & 3,
                    1,
                    "every returned slot needs its actual native ID"
                );
            }
            if q.atoms[order[0]].function == "Input" {
                assert!(
                    (0..q.device_slots()).any(|s| (states[0][s / 16] >> (2 * (s % 16))) & 3 == 2),
                    "computed key is numeric before the corresponding row binds its ID"
                );
            }
        }
    }
}
