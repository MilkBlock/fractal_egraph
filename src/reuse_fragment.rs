//! Exact positive fragment matching in finite baked rows, not closure equivalence.
//! Only source-LHS candidates are tested; no library-pair/subgraph enumeration.
use super::*;

/// Check typed conjunctive matching against a visible baked body. Arity excludes
/// optional output columns. The caller supplies a validated completed body with
/// opaque marker and hidden rows removed. Variables may alias: this is a native
/// query homomorphism, not injective graph embedding or a trigger certificate.
/// Projection is irrelevant to this conjunctive-existence test.
pub fn contains(
    body: &Query,
    body_arities: &[usize],
    query: &Query,
    arities: &[usize],
) -> Result<bool> {
    for (q, a) in [(body, body_arities), (query, arities)] {
        if q.atoms.len() != a.len()
            || q.atoms.iter().zip(a).any(|((_, cols), arity)| {
                !cols.len().checked_sub(*arity).is_some_and(|n| n <= 1)
                    || cols.iter().any(|v| *v >= q.values.len())
            })
        {
            return Err("malformed fragment schema or value slot".into());
        }
    }
    let mut domains: Vec<Vec<usize>> = vec![];
    for (i, (op, cols)) in query.atoms.iter().enumerate() {
        let rows = body
            .atoms
            .iter()
            .enumerate()
            .filter_map(|(j, (candidate, bcols))| {
                (op == candidate
                    && arities[i] == body_arities[j]
                    && cols.len() <= bcols.len()
                    && cols.iter().zip(bcols).all(|(a, b)| {
                        query.values[*a].0 == body.values[*b].0
                            && query.values[*a]
                                .1
                                .as_ref()
                                .is_none_or(|lit| body.values[*b].1.as_ref() == Some(lit))
                    }))
                .then_some(j)
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Ok(false);
        }
        domains.push(rows);
    }
    let mut order: Vec<_> = (0..query.atoms.len()).collect();
    order.sort_by_key(|i| domains[*i].len());
    // Iterative search avoids a call-stack limit. Bindings are symbolic body
    // slots only, and are discarded after this static fragment validation.
    let mut mapping = vec![None; query.values.len()];
    let mut next = vec![0; order.len()];
    let mut trails = vec![vec![]; order.len()];
    let mut depth = 0;
    loop {
        if depth == order.len() {
            return Ok(true);
        }
        let atom = order[depth];
        if next[depth] == domains[atom].len() {
            next[depth] = 0;
            if depth == 0 {
                return Ok(false);
            }
            depth -= 1;
            for v in trails[depth].drain(..) {
                mapping[v] = None;
            }
            continue;
        }
        let row = domains[atom][next[depth]];
        next[depth] += 1;
        let mut valid = true;
        for (&q, &b) in query.atoms[atom].1.iter().zip(&body.atoms[row].1) {
            if let Some(old) = mapping[q] {
                if old != b {
                    valid = false;
                    break;
                }
            } else {
                mapping[q] = Some(b);
                trails[depth].push(q);
            }
        }
        if valid {
            depth += 1;
        } else {
            for v in trails[depth].drain(..) {
                mapping[v] = None;
            }
        }
    }
}
