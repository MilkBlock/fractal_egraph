//! One exact Boolean conjunction serves all candidates with that condition set.
//! This Run-local DAG does not merge native bindings or change witness products.
use rustc_hash::FxHashMap as Map;
use serde::Serialize;

#[derive(Default, Serialize)]
pub struct Stats {
    pub candidates: usize,
    pub references: usize,
    pub new_steps: usize,
    pub reused_steps: usize,
    pub prefix_rejections: usize,
    pub rejected_candidates: usize,
    pub groups: usize,
}
pub struct Groups {
    edges: Map<(usize, usize), usize>,
    truth: Vec<bool>,
    pub stats: Stats,
}
impl Groups {
    pub fn new() -> Self {
        Self {
            edges: Map::default(),
            truth: vec![true],
            stats: Stats::default(),
        }
    }
    pub fn accepts(&mut self, links: &[usize], values: &[bool]) -> bool {
        self.stats.candidates += 1;
        self.stats.references += links.len();
        if links.len() == 1 {
            // Single-condition groups already have a unique slot in values.
            let ok = values[links[0]];
            self.stats.reused_steps += 1;
            self.stats.rejected_candidates += usize::from(!ok);
            return ok;
        }
        let mut ids = links.to_vec();
        ids.sort_unstable();
        ids.dedup();
        let mut parent = 0;
        for id in ids {
            if !self.truth[parent] {
                self.stats.prefix_rejections += 1;
                break;
            }
            parent = if let Some(next) = self.edges.get(&(parent, id)) {
                self.stats.reused_steps += 1;
                *next
            } else {
                let next = self.truth.len();
                self.edges.insert((parent, id), next);
                self.truth.push(values[id]);
                self.stats.new_steps += 1;
                next
            };
        }
        self.stats.groups = self.edges.len();
        let ok = self.truth[parent];
        self.stats.rejected_candidates += usize::from(!ok);
        ok
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permutations_aliases_and_shared_negative_prefixes() {
        let mut g = Groups::new();
        let values = [true, true, false, true];
        assert!(g.accepts(&[1, 0, 1], &values));
        let n = g.stats.new_steps;
        assert!(g.accepts(&[0, 1], &values));
        assert_eq!(g.stats.new_steps, n);
        assert!(!g.accepts(&[3, 2, 0], &values));
        assert!(!g.accepts(&[2, 0, 1], &values));
        assert!(g.accepts(&[], &values));
        assert!(g.accepts(&[3], &values));
    }
    #[test]
    fn all_small_conjunctions_equal_independent_full_evaluation() {
        for bits in 0..16 {
            let v = (0..4).map(|i| bits & (1 << i) != 0).collect::<Vec<_>>();
            let mut g = Groups::new();
            for mask in 0..256 {
                let ids = (0..4).map(|i| (mask >> (2 * i)) & 3).collect::<Vec<_>>();
                assert_eq!(g.accepts(&ids, &v), ids.iter().all(|i| v[*i]));
            }
        }
    }
}

/// A compiled atom decoder. All literal and repeated-variable tests remain;
/// variable-to-column positions are established once, not rebuilt per event.
pub struct Projection {
    pub bindings: Vec<(usize, usize)>,
    positions: Vec<usize>,
    tests: Vec<(usize, Check)>,
    arity: usize,
}
enum Check {
    Literal(super::Value),
    Equal(usize),
}
impl Projection {
    pub fn new(atom: &super::CellQueryAtom) -> Self {
        use super::CellOperand;
        let mut positions = vec![];
        let mut bindings = vec![];
        let mut tests = vec![];
        for (i, c) in atom.columns.iter().enumerate() {
            match c {
                CellOperand::Slot(s) => {
                    if positions.len() <= *s {
                        positions.resize(s + 1, usize::MAX);
                    }
                    if positions[*s] != usize::MAX {
                        tests.push((i, Check::Equal(positions[*s])));
                    } else {
                        positions[*s] = i;
                        bindings.push((*s, i));
                    }
                }
                CellOperand::Constant { value, .. } => tests.push((i, Check::Literal(*value))),
            }
        }
        Self {
            bindings,
            positions,
            tests,
            arity: atom.columns.len(),
        }
    }
    pub fn column(&self, slot: usize) -> usize {
        let c = self.positions[slot];
        assert_ne!(c, usize::MAX);
        c
    }
    pub fn matches(&self, row: &[super::Value]) -> bool {
        row.len() == self.arity
            && self.tests.iter().all(|(i, t)| match t {
                Check::Literal(v) => row[*i] == *v,
                Check::Equal(j) => row[*i] == row[*j],
            })
    }
}
#[cfg(test)]
mod projection_tests {
    use super::super::{CellOperand, CellQueryAtom, Value, bind};
    use super::*;
    #[test]
    fn fixed_positions_equal_full_binding_decode() {
        let atom = CellQueryAtom {
            function: "P".into(),
            columns: vec![
                CellOperand::Slot(3),
                CellOperand::Slot(1),
                CellOperand::Slot(3),
                CellOperand::Constant {
                    value: Value::new_const(2),
                    text: "2".into(),
                },
                CellOperand::Slot(0),
            ],
            sorts: vec!["T".into(); 5],
        };
        let p = Projection::new(&atom);
        for n in 0..1024 {
            let row = (0..5)
                .map(|i| Value::new_const((n >> (2 * i)) & 3))
                .collect::<Vec<_>>();
            let old = bind(&atom, &row);
            assert_eq!(p.matches(&row), old.is_some());
            if let Some(b) = old {
                for (s, v) in b {
                    assert_eq!(row[p.column(s)], v);
                }
            }
        }
    }
}
