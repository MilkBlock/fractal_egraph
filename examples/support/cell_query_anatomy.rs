//! Successful-read pair diagnostics, never a proof of static query equivalence.
use serde::Serialize;
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone)]
pub struct Read {
    pub op: String,
    // Sort, step-local identity, optional literal. Identities are alpha-renamed.
    pub cols: Vec<(String, String, Option<String>)>,
    pub cells: BTreeSet<usize>,
}
#[derive(Default, Serialize)]
pub struct PairStats {
    pub pair_occurrences: usize,
    pub match_events: usize,
    pub rules: BTreeSet<String>,
    pub same_cell: usize,
    pub separate_cells: usize,
    pub one_side_in_cell: usize,
    pub neither_in_cell: usize,
    pub effective_pair_occurrences: usize,
    pub union_effect_pair_occurrences: usize,
    pub distinct_bindings_per_step_sum: usize,
    #[serde(skip)]
    bindings: BTreeSet<String>,
}
#[derive(Default)]
pub struct Anatomy {
    pairs: BTreeMap<String, PairStats>,
    pub eligible_matches: usize,
    pub unsupported_matches: usize,
}
fn encode(a: &Read, b: &Read) -> String {
    let mut names = BTreeMap::new();
    let atoms: Vec<_> = [a, b]
        .iter()
        .map(|r| {
            let cols: Vec<_> = r
                .cols
                .iter()
                .map(|(sort, id, lit)| {
                    if let Some(lit) = lit {
                        json!(["literal", sort, lit])
                    } else {
                        let next = names.len();
                        let n = *names.entry((sort, id)).or_insert(next);
                        json!(["slot", sort, n])
                    }
                })
                .collect();
            json!([r.op, cols])
        })
        .collect();
    serde_json::to_string(&atoms).unwrap()
}
fn key(a: &Read, b: &Read) -> String {
    encode(a, b).min(encode(b, a))
}
fn connected(a: &Read, b: &Read) -> bool {
    // Common literal constants do not establish a shared variable/anchor.
    a.cols.iter().any(|(s, v, l)| {
        l.is_none()
            && b.cols
                .iter()
                .any(|(t, w, m)| m.is_none() && s == t && v == w)
    })
}
impl Anatomy {
    pub fn record(&mut self, rule: &str, reads: &[Read], effective: bool, union: bool) {
        self.eligible_matches += 1;
        let mut seen = BTreeSet::new();
        for i in 0..reads.len() {
            for b in &reads[i + 1..] {
                let a = &reads[i];
                if !connected(a, b) {
                    continue;
                }
                let k = key(a, b);
                let s = self.pairs.entry(k.clone()).or_default();
                s.pair_occurrences += 1;
                s.match_events += usize::from(seen.insert(k));
                s.rules.insert(rule.to_owned());
                s.effective_pair_occurrences += usize::from(effective);
                s.union_effect_pair_occurrences += usize::from(union);
                if !a.cells.is_disjoint(&b.cells) {
                    s.same_cell += 1;
                } else if !a.cells.is_empty() && !b.cells.is_empty() {
                    s.separate_cells += 1;
                } else if !a.cells.is_empty() || !b.cells.is_empty() {
                    s.one_side_in_cell += 1;
                } else {
                    s.neither_in_cell += 1;
                }
                // Pair order independent, retain concrete identities only until step ends.
                let mut rows = vec![
                    json!([a.op, a.cols]).to_string(),
                    json!([b.op, b.cols]).to_string(),
                ];
                rows.sort();
                s.bindings.insert(serde_json::to_string(&rows).unwrap());
            }
        }
    }
    pub fn finish_step(&mut self) {
        for s in self.pairs.values_mut() {
            s.distinct_bindings_per_step_sum += s.bindings.len();
            s.bindings.clear();
        }
    }
    pub fn report(&self) -> Json {
        let mut pairs: Vec<_> = self
            .pairs
            .iter()
            .map(|(k, s)| json!({"read_shape":serde_json::from_str::<Json>(k).unwrap(),"stats":s}))
            .collect();
        pairs.sort_by_key(|p| std::cmp::Reverse(p["stats"]["pair_occurrences"].as_u64().unwrap()));
        json!({"eligible_matches":self.eligible_matches,"unsupported_matches":self.unsupported_matches,
          "pair_shapes":pairs.len(),"cross_rule_shapes":self.pairs.values().filter(|s|s.rules.len()>1).count(),"pairs":pairs,
          "scope":"Pre-step positive Cell embeddings; successful physical reads only. Observed aliases are not static rule equalities. No failed branches, pure Join timing, negative certificates, or optimizer rewrites inferred. Occurrences may overlap; counts must not be summed as saved work. Union counts describe effects of these matches, not incoming invalidations.",
          "cost":"O(sum of squared LHS read counts plus pair encoding); no arbitrary subgraph enumeration. Concrete binding deduplication is discarded after each step."})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn r(op: &str, ids: &[&str], cells: &[usize]) -> Read {
        Read {
            op: op.into(),
            cols: ids
                .iter()
                .map(|v| ("Expr".into(), v.to_string(), None))
                .collect(),
            cells: cells.iter().copied().collect(),
        }
    }
    #[test]
    fn alpha_and_atom_order_preserve_aliases() {
        assert_eq!(
            key(&r("P", &["x", "a"], &[]), &r("Q", &["a", "y"], &[])),
            key(&r("Q", &["b", "z"], &[]), &r("P", &["w", "b"], &[]))
        );
        assert_ne!(
            key(&r("P", &["x", "a"], &[]), &r("Q", &["a", "x"], &[])),
            key(&r("P", &["x", "a"], &[]), &r("Q", &["a", "y"], &[]))
        );
    }
    #[test]
    fn shared_shapes_and_boundary_categories_are_distinct() {
        let mut a = Anatomy::default();
        a.record(
            "R1",
            &[r("P", &["x", "a"], &[1]), r("Q", &["a", "y"], &[1])],
            false,
            false,
        );
        a.record(
            "R2",
            &[r("Q", &["a", "y"], &[]), r("P", &["x", "a"], &[1])],
            true,
            true,
        );
        a.finish_step();
        let s = a.pairs.values().next().unwrap();
        assert_eq!(
            (
                s.same_cell,
                s.one_side_in_cell,
                s.rules.len(),
                s.distinct_bindings_per_step_sum
            ),
            (1, 1, 2, 1)
        );
    }
}
