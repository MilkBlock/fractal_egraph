//! Sound negative lookahead using the already maintained tuple indexes.
//! A nonempty hash bucket is deliberately inconclusive (collisions, retired
//! rows and ignored predicates only add false positives). An empty bucket
//! proves that a necessary projection of an unvisited atom is absent.
use egglog::cell_assist::{CellActionSink, CellOperand};
use std::collections::{BTreeMap, BTreeSet};

fn same_operand(a: &CellOperand, b: &CellOperand) -> bool {
    match (a, b) {
        (CellOperand::Slot(a), CellOperand::Slot(b)) => a == b,
        (CellOperand::Constant { value: a, .. }, CellOperand::Constant { value: b, .. }) => a == b,
        _ => false,
    }
}

fn close_numeric(q: &CellActionSink, bound: &mut BTreeSet<usize>) {
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
}

/// Compare supersets of an unbound shared slot, conditioned on the anchor's
/// known coordinates. Each (existing index, output column) is shared globally.
pub(crate) fn domains(
    q: &CellActionSink,
    anchor: usize,
    tids: &BTreeMap<String, usize>,
    specs: &BTreeMap<(usize, Vec<usize>), usize>,
    summaries: &mut BTreeMap<(usize, usize), usize>,
) -> Vec<[u32; 4]> {
    let mut bound = q.atoms[anchor]
        .columns
        .iter()
        .filter_map(|o| match o {
            CellOperand::Slot(s) => Some(*s),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    close_numeric(q, &mut bound);
    let choices = q
        .atoms
        .iter()
        .map(|a| {
            specs
                .iter()
                .filter(|((t, cols), _)| {
                    *t == tids[&a.function]
                        && !cols.is_empty()
                        && cols.iter().any(|c| a.sorts[*c] != "Unit")
                        && cols.iter().all(|c| match a.columns[*c] {
                            CellOperand::Slot(s) => bound.contains(&s),
                            _ => true,
                        })
                })
                .max_by_key(|((_, cols), _)| cols.len())
        })
        .collect::<Vec<_>>();
    let mut checks = BTreeSet::new();
    for a in 0..q.atoms.len() {
        if a == anchor {
            continue;
        }
        let Some(((_, ka), &sa)) = choices[a] else {
            continue;
        };
        for b in a + 1..q.atoms.len() {
            if b == anchor {
                continue;
            }
            let Some(((_, kb), &sb)) = choices[b] else {
                continue;
            };
            for (ca, va) in q.atoms[a].columns.iter().enumerate() {
                let CellOperand::Slot(slot) = va else {
                    continue;
                };
                if bound.contains(slot) || q.atoms[a].sorts[ca] == "Unit" {
                    continue;
                }
                for (cb, vb) in q.atoms[b].columns.iter().enumerate() {
                    if !matches!(vb,CellOperand::Slot(other) if other == slot)
                        || q.atoms[a].sorts[ca] != q.atoms[b].sorts[cb]
                    {
                        continue;
                    }
                    if sa == sb
                        && ca == cb
                        && ka.iter().zip(kb).all(|(x, y)| {
                            same_operand(&q.atoms[a].columns[*x], &q.atoms[b].columns[*y])
                        })
                    {
                        continue;
                    }
                    let next = summaries.len();
                    let da = *summaries.entry((sa, ca)).or_insert(next);
                    let next = summaries.len();
                    let db = *summaries.entry((sb, cb)).or_insert(next);
                    checks.insert([a as u32, da as u32, b as u32, db as u32]);
                }
            }
        }
    }
    checks.into_iter().collect()
}

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
            close_numeric(q, &mut bound);
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
    fn shared_slot_domains_keep_both_conditioning_keys_and_reuse_summaries() {
        let mut eg = egglog::EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
          (relation Frame (i64 i64)) (relation L (i64 i64)) (relation R (i64 i64))
          (relation Hit (i64))
          (rule ((Frame x z) (L x y) (R y z)) ((Hit y)) :name "q")
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
        let anchor = q.atoms.iter().position(|a| a.function == "Frame").unwrap();
        let specs = BTreeMap::from([((tids["L"], vec![0]), 0), ((tids["R"], vec![1]), 1)]);
        let mut summaries = BTreeMap::new();
        let checks = domains(&q, anchor, &tids, &specs, &mut summaries);
        assert_eq!(checks.len(), 1);
        assert_eq!(summaries.len(), 2);
        assert_eq!(domains(&q, anchor, &tids, &specs, &mut summaries), checks);
        assert_eq!(
            summaries.len(),
            2,
            "identical projections are shared across consumers"
        );
    }
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

/// Condition complete numeric port fibers from installed Cell programs on the
/// currently bound key. Native indexes provide all rows, never only Cell hits.
/// Marginal intervals are supersets: useful to disprove a guard, never to prove
/// that a correlated multi-slot binding exists.
pub(crate) fn ranges(
    eg:&egglog::EGraph, q:&CellActionSink, order:&[usize],
    tids:&BTreeMap<String,usize>,specs:&BTreeMap<(usize,Vec<usize>),usize>,
    summaries:&mut BTreeMap<(usize,usize),usize>,lib:&crate::cell_connection::Library,
    sources:&mut BTreeSet<String>,
)->Vec<Vec<[u32;9]>> {
    let mut bound=BTreeSet::new();let mut tested=BTreeSet::new();
    let guards=q.primitives.iter().filter(|op|matches!(op.opcode,0..=3|5))
        .map(|op|(op.opcode,&op.args[0],&op.args[1]))
        .chain(q.guards.iter().map(|g|(g.opcode,&g.operands[0],&g.operands[1]))).collect::<Vec<_>>();
    order.iter().map(|part| {
        bound.extend(q.atoms[*part].columns.iter().filter_map(|o|match o{CellOperand::Slot(s)=>Some(*s),_=>None}));
        close_numeric(q,&mut bound);
        let mut out=vec![];
        for (op,a,b) in &guards {
            let known=|o:&CellOperand|match o{CellOperand::Slot(s)=>bound.contains(s),_=>true};
            if known(a)&&known(b){continue}
            let mut evidence=BTreeSet::new();
            let mut operand=|o:&CellOperand|->Option<[u32;4]> {
                match o {
                    CellOperand::Constant{value,..}=>{let n=eg.value_to_base::<i64>(*value) as u64;Some([1,n as u32,(n>>32) as u32,0])},
                    CellOperand::Slot(s) if bound.contains(s)=>Some([0,*s as u32,0,0]),
                    CellOperand::Slot(s)=>{
                        let mut choices=vec![];
                        for (t,atom) in q.atoms.iter().enumerate(){for (col,v) in atom.columns.iter().enumerate(){
                            if !matches!(v,CellOperand::Slot(v) if v==s){continue}
                            let Some(from)=lib.range_sources(&atom.function,col) else{continue};
                            for ((table,keys),sp) in specs {
                                if *table==tids[&atom.function] && !keys.is_empty() && keys.iter().any(|c|atom.sorts[*c]!="Unit") && keys.iter().all(|c|known(&atom.columns[*c])) {
                                    choices.push((keys.len(),std::cmp::Reverse((t,col,*sp)),from));
                                }
                            }
                        }}
                        let (_,std::cmp::Reverse((t,col,sp)),from)=choices.into_iter().max()?;
                        evidence.extend(from.iter().cloned());
                        let next=summaries.len();let lo=*summaries.entry((sp,col|0x40000000)).or_insert(next);
                        let next=summaries.len();let hi=*summaries.entry((sp,col|0x80000000)).or_insert(next);
                        Some([2,t as u32,lo as u32,hi as u32])
                    }
                }
            };
            let Some(a)=operand(a) else{continue};let Some(b)=operand(b) else{continue};
            let check=[*op,a[0],a[1],a[2],a[3],b[0],b[1],b[2],b[3]];
            if tested.insert(check){out.push(check);sources.extend(evidence);}
        }
        out
    }).collect()
}
