//! Sound negative lookahead using the already maintained tuple indexes.
//! A nonempty hash bucket is deliberately inconclusive (collisions, retired
//! rows and ignored predicates only add false positives). An empty bucket
//! proves that a necessary projection of an unvisited atom is absent.
use egglog::cell_assist::{CellActionSink, CellOperand};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn compile(
    q: &CellActionSink,
    order: &[usize],
    tids: &BTreeMap<String, usize>,
    specs: &BTreeMap<(usize, Vec<usize>), usize>,
) -> Vec<Vec<[u32; 2]>> {
    let mut bound = BTreeSet::new();
    let mut tested = BTreeSet::new();
    order
        .iter()
        .enumerate()
        .map(|(depth, atom)| {
            bound.extend(q.atoms[*atom].columns.iter().filter_map(|o| match o {
                CellOperand::Slot(s) => Some(*s),
                _ => None,
            }));
            loop {
                let before = bound.len();
                for op in &q.primitives {
                    if op.args.iter().all(|o| match o {
                        CellOperand::Slot(s) => bound.contains(s),
                        _ => true,
                    }) {
                        if let Some(CellOperand::Slot(s)) = &op.result {
                            bound.insert(*s);
                        }
                    }
                }
                if before == bound.len() {
                    break;
                }
            }
            let mut checks = Vec::new();
            // The immediate next atom is already probed by the normal Join.
            for &target in order.iter().skip(depth + 2) {
                let a = &q.atoms[target];
                let table = tids[&a.function];
                let best = specs
                    .iter()
                    .filter(|((t, cols), _)| {
                        *t == table
                            && !cols.is_empty()
                            && cols.iter().any(|c| a.sorts[*c] != "Unit")
                            && cols.iter().all(|c| match &a.columns[*c] {
                                CellOperand::Slot(s) => bound.contains(s),
                                _ => true,
                            })
                    })
                    .max_by_key(|((_, cols), _)| cols.len());
                if let Some((_, &sp)) = best {
                    if tested.insert((target, sp)) {
                        checks.push([target as u32, sp as u32]);
                    }
                }
            }
            checks
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_already_bound_coordinates_can_refute_a_future_atom() {
        let mut eg = egglog::EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
          (relation P (i64)) (relation Q (i64 i64))
          (relation R (i64 i64)) (relation Hit (i64))
          (rule ((P x) (Q x y) (R x z)) ((Hit z)) :name "q")
        "#,
        )
        .unwrap();
        let q = eg.prepare_gpu_cell_action("", "q").unwrap();
        let tids = q
            .atoms
            .iter()
            .enumerate()
            .map(|(i, a)| (a.function.clone(), i))
            .collect::<BTreeMap<_, _>>();
        let order =
            ["P", "Q", "R"].map(|name| q.atoms.iter().position(|a| a.function == name).unwrap());
        let r = tids["R"];
        let specs = BTreeMap::from([
            ((r, vec![0]), 0), // x is known after P
            ((r, vec![1]), 1), // z is not
            ((r, vec![0, 1]), 2),
            ((r, vec![2]), 3), // Unit is not a useful refutation key
        ]);
        let checks = compile(&q, &order, &tids, &specs);
        assert_eq!(checks[0], vec![[order[2] as u32, 0]]);
        assert!(checks[1].is_empty());
        assert!(checks[2].is_empty());
    }
}
