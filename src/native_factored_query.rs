//! Exact factorization after functionally determined values. Each factor still
//! runs through egglog's native indexed query engine; no new pattern matcher.
use crate::pipeline::Result;
use egglog::{CompiledFactQuery, EGraph, Value, ast::Literal};
use std::collections::{BTreeMap, BTreeSet, HashSet};

struct Piece {
    query: CompiledFactQuery,
    columns: Vec<usize>, // positions in the original projected tuple
}

pub(super) struct Query {
    pieces: Vec<Piece>,
    width: usize,
}
fn root(parents: &mut [usize], x: usize) -> usize {
    if parents[x] != x {
        parents[x] = root(parents, parents[x]);
    }
    parents[x]
}
impl Query {
    pub fn compile(
        eg: &mut EGraph,
        values: &[(String, Option<Literal>)],
        atoms: &[(String, Vec<usize>)],
        projection: &[usize],
    ) -> Result<Self> {
        let bound: BTreeSet<_> = atoms
            .iter()
            .flat_map(|(_, xs)| xs.iter().copied())
            .collect();
        if bound.iter().any(|i| *i >= values.len()) || projection.iter().any(|i| !bound.contains(i))
        {
            return Err("invalid factored query variable/projection".into());
        }
        let arities: Vec<_> = atoms
            .iter()
            .map(|(op, _)| {
                eg.get_function(op)
                    .map(|f| f.schema().input.len())
                    .ok_or_else(|| format!("unknown factored query table: {op}"))
            })
            .collect::<std::result::Result<_, _>>()?;
        for ((_, xs), arity) in atoms.iter().zip(&arities) {
            if xs.len() != *arity && xs.len() != arity + 1 {
                return Err("factored query atom arity mismatch".into());
            }
        }
        // Outputs are determined by the key. No reverse dependency, injectivity
        // or inequality between different query variables is assumed.
        let mut fixed: BTreeSet<_> = values
            .iter()
            .enumerate()
            .filter_map(|(i, (_, v))| v.is_some().then_some(i))
            .collect();
        loop {
            let before = fixed.len();
            for ((_, xs), &arity) in atoms.iter().zip(&arities) {
                if xs.len() == arity + 1 && xs[..arity].iter().all(|i| fixed.contains(i)) {
                    fixed.insert(xs[arity]);
                }
            }
            if fixed.len() == before {
                break;
            }
        }
        let mut parents: Vec<_> = (0..values.len()).collect();
        for (_, xs) in atoms {
            let free: Vec<_> = xs.iter().filter(|i| !fixed.contains(i)).copied().collect();
            if let Some(&first) = free.first() {
                for x in free.iter().skip(1) {
                    let a = root(&mut parents, first);
                    let b = root(&mut parents, *x);
                    parents[b] = a;
                }
            }
        }
        let mut groups = BTreeMap::<usize, Vec<usize>>::new();
        let mut ground = vec![];
        for (i, (_, xs)) in atoms.iter().enumerate() {
            if let Some(x) = xs.iter().find(|i| !fixed.contains(i)) {
                groups.entry(root(&mut parents, *x)).or_default().push(i);
            } else {
                ground.push(i);
            }
        }
        if groups.len() <= 1 {
            return Ok(Self {
                pieces: vec![Piece {
                    query: eg.compile_fact_query(values, atoms, projection)?,
                    columns: (0..projection.len()).collect(),
                }],
                width: projection.len(),
            });
        }
        let mut pieces = vec![];
        let mut assigned = vec![false; projection.len()];
        for (id, mut indices) in groups {
            // Repeat deterministic anchor atoms, never their possible bindings.
            indices.extend(&ground);
            indices.sort_unstable();
            let bound_here: BTreeSet<_> = indices
                .iter()
                .flat_map(|i| atoms[*i].1.iter().copied())
                .collect();
            let columns: Vec<_> = projection
                .iter()
                .enumerate()
                .filter_map(|(column, &v)| {
                    (if fixed.contains(&v) {
                        !assigned[column] && bound_here.contains(&v)
                    } else {
                        root(&mut parents, v) == id
                    })
                    .then_some(column)
                })
                .collect();
            for &column in &columns {
                assigned[column] = true;
            }
            let projected: Vec<_> = columns.iter().map(|i| projection[*i]).collect();
            let fragment: Vec<_> = indices.iter().map(|i| atoms[*i].clone()).collect();
            match eg.compile_fact_query(values, &fragment, &projected) {
                Ok(query) => pieces.push(Piece { query, columns }),
                Err(error) => {
                    for p in pieces {
                        eg.free_fact_query(p.query);
                    }
                    return Err(error.into());
                }
            }
        }
        Ok(Self {
            pieces,
            width: projection.len(),
        })
    }
    pub fn parts(&self) -> usize {
        self.pieces.len()
    }
    pub fn free(self, eg: &mut EGraph) {
        for p in self.pieces {
            eg.free_fact_query(p.query);
        }
    }
    fn put(&self, part: usize, values: &[Value], output: &mut [Value]) {
        for (&column, &value) in self.pieces[part].columns.iter().zip(values) {
            output[column] = value;
        }
    }
    pub fn all(&self, eg: &mut EGraph) -> Result<Vec<Vec<Value>>> {
        if self.pieces.len() == 1 {
            return Ok(eg.run_fact_query(&self.pieces[0].query)?);
        }
        let mut product = vec![vec![Value::new_const(0); self.width]];
        for (i, p) in self.pieces.iter().enumerate() {
            let rows: BTreeSet<_> = eg.run_fact_query(&p.query)?.into_iter().collect();
            if rows.is_empty() {
                return Ok(vec![]);
            }
            let mut next = vec![];
            for prefix in product {
                for row in &rows {
                    let mut out = prefix.clone();
                    self.put(i, row, &mut out);
                    next.push(out);
                }
            }
            product = next;
        }
        Ok(product)
    }
    pub fn difference(
        &self,
        eg: &mut EGraph,
        excluded: HashSet<Vec<Value>>,
    ) -> Result<(Option<Vec<Value>>, Option<Vec<Value>>)> {
        if self.pieces.len() == 1 {
            return Ok(eg.run_fact_query_difference(&self.pieces[0].query, excluded)?);
        }
        if excluded.iter().any(|r| r.len() != self.width) {
            return Err("factored exclusion tuple width mismatch".into());
        }
        let mut matched = vec![Value::new_const(0); self.width];
        let mut fresh_piece = None;
        let mut old_rows = vec![];
        for (i, p) in self.pieces.iter().enumerate() {
            // One fresh factor already proves whole-tuple freshness. Remaining
            // factors only need an existence witness, not exhaustive freshness.
            let exists_only = fresh_piece.is_some() || p.columns.is_empty();
            let old = if exists_only {
                HashSet::new()
            } else {
                excluded
                    .iter()
                    .map(|r| p.columns.iter().map(|j| r[*j]).collect())
                    .collect()
            };
            let (one, fresh, seen) = eg.run_fact_query_partition(&p.query, old)?;
            let Some(one) = one else {
                return Ok((None, None));
            };
            self.put(i, &one, &mut matched);
            if !exists_only {
                if let Some(row) = fresh {
                    fresh_piece = Some((i, row));
                }
            }
            // An existential factor has exactly one possible projected tuple.
            old_rows.push(if p.columns.is_empty() {
                vec![vec![]]
            } else {
                seen.into_iter()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            });
        }
        if !excluded.contains(&matched) {
            return Ok((Some(matched.clone()), Some(matched)));
        }
        if let Some((i, row)) = fresh_piece {
            let mut fresh = matched.clone();
            self.put(i, &row, &mut fresh);
            return Ok((Some(matched), Some(fresh)));
        }
        // All factors are inside old projections, but old component bindings can
        // recombine into a NEW whole tuple. At most |excluded|+1 distinct complete
        // tuples are visited before success: an exact set bound, not a budget.
        fn search(
            q: &Query,
            rows: &[Vec<Vec<Value>>],
            old: &HashSet<Vec<Value>>,
            i: usize,
            out: &mut Vec<Value>,
        ) -> Option<Vec<Value>> {
            if i == rows.len() {
                return (!old.contains(out)).then(|| out.clone());
            }
            for row in &rows[i] {
                q.put(i, row, out);
                if let Some(found) = search(q, rows, old, i + 1, out) {
                    return Some(found);
                }
            }
            None
        }
        let fresh = search(self, &old_rows, &excluded, 0, &mut matched.clone());
        Ok((Some(matched), fresh))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(eg: &mut EGraph, q: &Query, flat: &CompiledFactQuery) -> BTreeSet<Vec<Value>> {
        let expected: BTreeSet<_> = eg.run_fact_query(flat).unwrap().into_iter().collect();
        assert_eq!(
            q.all(eg).unwrap().into_iter().collect::<BTreeSet<_>>(),
            expected
        );
        for stride in [0, 1, 2, 3] {
            let old: HashSet<_> = expected
                .iter()
                .enumerate()
                .filter(|(i, _)| stride != 0 && i % stride == 0)
                .map(|(_, r)| r.clone())
                .collect();
            let (matched, fresh) = q.difference(eg, old.clone()).unwrap();
            assert_eq!(matched.is_some(), !expected.is_empty());
            assert_eq!(fresh.is_some(), expected.iter().any(|r| !old.contains(r)));
            if let Some(r) = matched {
                assert!(expected.contains(&r));
            }
            if let Some(r) = fresh {
                assert!(expected.contains(&r) && !old.contains(&r));
            }
        }
        expected
    }

    #[test]
    fn shared_determined_anchor_preserves_union_update_and_visibility() {
        for projection in [vec![0, 1, 2, 3], vec![3, 1], vec![]] {
            let mut eg = EGraph::default();
            eg.parse_and_run_program(None,"(datatype E (V i64)) (function anchor (i64) E :merge new) (relation P (E E)) (relation Q (E E)) (relation Unexpected ()) (rule ((P x y)) ((Unexpected))) (set (anchor -1) (V 0)) (P (V 0) (V 1)) (P (V 0) (V 2)) (Q (V 0) (V 3)) (Q (V 0) (V 4)) (Q (V 5) (V 6))").unwrap();
            let values = vec![
                ("E".into(), None),
                ("E".into(), None),
                ("E".into(), None),
                ("i64".into(), Some(Literal::Int(-1))),
            ];
            let atoms = vec![
                ("anchor".into(), vec![3, 0]),
                ("P".into(), vec![0, 1]),
                ("Q".into(), vec![0, 2]),
            ];
            let q = Query::compile(&mut eg, &values, &atoms, &projection).unwrap();
            assert_eq!(
                q.parts(),
                2,
                "constant-keyed output is a shared determined anchor"
            );
            let flat = eg.compile_fact_query(&values, &atoms, &projection).unwrap();
            for update in [
                "",
                "(union (V 0) (V 5))",
                "(subsume (P (V 0) (V 2)))",
                "(set (anchor -1) (V 9))",
                "(P (V 9) (V 10)) (Q (V 9) (V 10))",
            ] {
                let before = q.all(&mut eg).unwrap();
                eg.parse_and_run_program(None, update).unwrap();
                let old: HashSet<_> = before
                    .iter()
                    .map(|row| {
                        row.iter()
                            .zip(&projection)
                            .map(|(v, i)| {
                                let sort = eg.get_arcsort_by(|s| s.name() == values[*i].0);
                                eg.get_canonical_value(*v, &sort)
                            })
                            .collect()
                    })
                    .collect();
                let expected = compare(&mut eg, &q, &flat);
                let (matched, fresh) = q.difference(&mut eg, old.clone()).unwrap();
                assert_eq!(matched.is_some(), !expected.is_empty());
                assert_eq!(fresh.is_some(), expected.iter().any(|r| !old.contains(r)));
                assert_eq!(
                    eg.get_size("Unexpected"),
                    0,
                    "query must not execute user rules"
                );
            }
            q.free(&mut eg);
            eg.free_fact_query(flat);
        }
    }

    #[test]
    fn shared_free_variable_is_not_an_independent_factor() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            "(relation P (i64 i64)) (relation Q (i64 i64)) (P 1 7) (Q 2 8)",
        )
        .unwrap();
        let values = vec![("i64".into(), None); 3];
        let atoms = vec![("P".into(), vec![0, 1]), ("Q".into(), vec![0, 2])];
        let q = Query::compile(&mut eg, &values, &atoms, &[0, 1, 2]).unwrap();
        assert_eq!(q.parts(), 1);
        let flat = eg.compile_fact_query(&values, &atoms, &[0, 1, 2]).unwrap();
        assert!(compare(&mut eg, &q, &flat).is_empty());
        eg.parse_and_run_program(None, "(Q 1 8)").unwrap();
        assert_eq!(compare(&mut eg, &q, &flat).len(), 1);
        q.free(&mut eg);
        eg.free_fact_query(flat);
    }

    #[test]
    fn old_factor_values_can_form_a_fresh_combination() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            "(relation P (i64)) (relation Q (i64)) (P 0) (P 1) (Q 0) (Q 1)",
        )
        .unwrap();
        let values = vec![("i64".into(), None); 2];
        let atoms = vec![("P".into(), vec![0]), ("Q".into(), vec![1])];
        let q = Query::compile(&mut eg, &values, &atoms, &[0, 1]).unwrap();
        assert_eq!(q.parts(), 2);
        let full = q.all(&mut eg).unwrap();
        assert_eq!(full.len(), 4);
        for mask in 0..16 {
            let old: HashSet<_> = full
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, r)| r.clone())
                .collect();
            let (_, fresh) = q.difference(&mut eg, old.clone()).unwrap();
            assert_eq!(fresh.is_some(), old.len() != 4);
            if let Some(row) = fresh {
                assert!(full.contains(&row) && !old.contains(&row));
            }
        }
        let first = q.difference(&mut eg, HashSet::new()).unwrap().0.unwrap();
        let opposite = full
            .iter()
            .find(|r| r[0] != first[0] && r[1] != first[1])
            .unwrap()
            .clone();
        let old = HashSet::from([first, opposite]);
        for col in 0..2 {
            assert_eq!(
                full.iter().map(|r| r[col]).collect::<BTreeSet<_>>(),
                old.iter().map(|r| r[col]).collect()
            );
        }
        let fresh = q
            .difference(&mut eg, old.clone())
            .unwrap()
            .1
            .expect("new combination despite no new component value");
        assert!(full.contains(&fresh) && !old.contains(&fresh));
        assert!(
            q.difference(&mut eg, full.into_iter().collect())
                .unwrap()
                .1
                .is_none()
        );
        q.free(&mut eg);
    }

    #[test]
    fn missing_anchor_and_repeated_projection_columns_are_preserved() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None,"(datatype E (Anchor i64)) (relation P (E i64)) (relation Q (E i64)) (relation Gate ())").unwrap();
        let values = vec![
            ("i64".into(), Some(Literal::Int(5))),
            ("E".into(), None),
            ("i64".into(), None),
            ("i64".into(), None),
        ];
        let atoms = vec![
            ("Anchor".into(), vec![0, 1]),
            ("P".into(), vec![1, 2]),
            ("Q".into(), vec![1, 3]),
            ("Gate".into(), vec![]),
        ];
        let projection = [2, 2, 3, 1];
        let q = Query::compile(&mut eg, &values, &atoms, &projection).unwrap();
        let flat = eg.compile_fact_query(&values, &atoms, &projection).unwrap();
        assert!(compare(&mut eg, &q, &flat).is_empty());
        eg.parse_and_run_program(None, "(P (Anchor 5) 1) (Q (Anchor 5) 2)")
            .unwrap();
        assert!(
            compare(&mut eg, &q, &flat).is_empty(),
            "missing ground gate rejects the whole product"
        );
        eg.parse_and_run_program(None, "(Gate)").unwrap();
        assert_eq!(compare(&mut eg, &q, &flat).len(), 1);
        q.free(&mut eg);
        eg.free_fact_query(flat);
    }

    #[test]
    fn projected_literal_is_owned_by_a_factor_that_actually_binds_it() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            "(relation P (i64)) (relation Q (i64 i64)) (P 1) (P 2) (Q 5 9)",
        )
        .unwrap();
        let values = vec![
            ("i64".into(), Some(Literal::Int(5))),
            ("i64".into(), None),
            ("i64".into(), None),
        ];
        let atoms = vec![("P".into(), vec![1]), ("Q".into(), vec![0, 2])];
        let q = Query::compile(&mut eg, &values, &atoms, &[0, 1, 2]).unwrap();
        assert_eq!(q.parts(), 2);
        let flat = eg.compile_fact_query(&values, &atoms, &[0, 1, 2]).unwrap();
        assert_eq!(compare(&mut eg, &q, &flat).len(), 2);
        q.free(&mut eg);
        eg.free_fact_query(flat);
    }
}
