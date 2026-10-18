//! Typed, versioned fragment markers. A marker is an admitted fragment row, not
//! a new egraph fact. All shared slots are joined as one tuple (never separately).
//! Delta anchors partition new combinations by their first changed original part.
use crate::{Binding, Value};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

type IndexKey = (usize, Vec<usize>);
type Index = BTreeMap<Vec<Value>, BTreeSet<usize>>;
#[derive(Default, Serialize)]
pub struct Stats {
    pub marker_rows_added: usize,
    pub marker_rows_removed: usize,
    pub index_probes: usize,
    pub indexed_rows_visited: usize,
    pub emitted_combinations: usize,
    pub live_index_entries: usize,
}

pub struct Markers {
    columns: Vec<Vec<usize>>,
    rows: BTreeMap<usize, (usize, Binding)>,
    indexes: BTreeMap<IndexKey, Index>,
    pub stats: Stats,
}
impl Markers {
    pub fn new(columns: Vec<Vec<usize>>) -> Self {
        Self {
            columns,
            rows: BTreeMap::new(),
            indexes: BTreeMap::new(),
            stats: Stats::default(),
        }
    }
    fn key(binding: &Binding, cols: &[usize]) -> Vec<Value> {
        cols.iter()
            .map(|i| binding[*i].expect("marker schema must bind its ports"))
            .collect()
    }
    pub fn combinations<'a>(
        &mut self,
        rows: impl Iterator<Item = (usize, usize, &'a Binding)>,
    ) -> Vec<Vec<usize>> {
        let incoming: BTreeMap<_, _> = rows.map(|(id, p, b)| (id, (p, b))).collect();
        let removed = self
            .rows
            .keys()
            .filter(|i| !incoming.contains_key(i))
            .copied()
            .collect::<Vec<_>>();
        for id in removed {
            let (part, row) = self.rows.remove(&id).unwrap();
            for ((p, cols), index) in &mut self.indexes {
                if *p == part {
                    let key = Self::key(&row, cols);
                    let bucket = index.get_mut(&key).unwrap();
                    bucket.remove(&id);
                    if bucket.is_empty() {
                        index.remove(&key);
                    }
                }
            }
            self.stats.marker_rows_removed += 1;
        }
        let mut added = BTreeSet::new();
        for (id, (part, row)) in incoming {
            if self.rows.contains_key(&id) {
                continue;
            }
            for ((p, cols), index) in &mut self.indexes {
                if *p == part {
                    index.entry(Self::key(row, cols)).or_default().insert(id);
                }
            }
            self.rows.insert(id, (part, row.clone()));
            added.insert(id);
            self.stats.marker_rows_added += 1;
        }
        let mut out = vec![];
        for anchor_id in &added {
            let (anchor, binding) = self.rows[anchor_id].clone();
            let mut chosen = vec![usize::MAX; self.columns.len()];
            chosen[anchor] = *anchor_id;
            let mut prefixes = vec![(chosen, binding)];
            let mut bound = self.columns[anchor]
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            for part in 0..self.columns.len() {
                if part == anchor {
                    continue;
                }
                let cols = self.columns[part]
                    .iter()
                    .filter(|c| bound.contains(c))
                    .copied()
                    .collect::<Vec<_>>();
                let ik = (part, cols.clone());
                if !self.indexes.contains_key(&ik) {
                    let mut index: Index = BTreeMap::new();
                    for (id, (p, b)) in &self.rows {
                        if *p == part {
                            index.entry(Self::key(b, &cols)).or_default().insert(*id);
                        }
                    }
                    self.indexes.insert(ik.clone(), index);
                }
                let mut next = vec![];
                for (ids, binding) in prefixes {
                    self.stats.index_probes += 1;
                    if let Some(bucket) = self.indexes[&ik].get(&Self::key(&binding, &cols)) {
                        for id in bucket {
                            self.stats.indexed_rows_visited += 1;
                            if part < anchor && added.contains(id) {
                                continue;
                            }
                            let mut ids = ids.clone();
                            ids[part] = *id;
                            let mut binding = binding.clone();
                            for slot in &self.columns[part] {
                                binding[*slot] = self.rows[id].1[*slot];
                            }
                            next.push((ids, binding));
                        }
                    }
                }
                prefixes = next;
                bound.extend(&self.columns[part]);
            }
            out.extend(prefixes.into_iter().map(|(ids, _)| ids));
        }
        self.stats.live_index_entries = self
            .indexes
            .values()
            .flat_map(|i| i.values())
            .map(BTreeSet::len)
            .sum();
        self.stats.emitted_combinations += out.len();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b(xs: &[Option<u32>]) -> Binding {
        xs.iter().map(|v| v.map(Value::new_const)).collect()
    }
    #[test]
    fn tuple_keys_keep_correlations_and_delta_anchors_do_not_replay() {
        let mut m = Markers::new(vec![vec![0, 1], vec![1, 2], vec![0, 2]]);
        let mut rows = BTreeMap::from([
            (0, (0, b(&[Some(1), Some(2), None]))),
            (1, (0, b(&[Some(3), Some(4), None]))),
            (2, (1, b(&[None, Some(2), Some(5)]))),
            (3, (1, b(&[None, Some(4), Some(6)]))),
            // Each column appears elsewhere, but this tuple is incompatible.
            (4, (2, b(&[Some(1), None, Some(6)]))),
        ]);
        assert!(
            m.combinations(rows.iter().map(|(i, (p, b))| (*i, *p, b)))
                .is_empty()
        );
        rows.insert(5, (2, b(&[Some(1), None, Some(5)])));
        assert_eq!(
            m.combinations(rows.iter().map(|(i, (p, b))| (*i, *p, b))),
            vec![vec![0, 2, 5]]
        );
        assert!(
            m.combinations(rows.iter().map(|(i, (p, b))| (*i, *p, b)))
                .is_empty()
        );
        rows.remove(&0);
        rows.insert(6, (0, b(&[Some(3), Some(2), None])));
        rows.insert(7, (2, b(&[Some(3), None, Some(5)])));
        assert_eq!(
            m.combinations(rows.iter().map(|(i, (p, b))| (*i, *p, b))),
            vec![vec![6, 2, 7]]
        );
        assert_eq!(m.stats.marker_rows_removed, 1);
    }
    #[test]
    fn disconnected_parts_keep_the_required_product() {
        let mut m = Markers::new(vec![vec![0], vec![1]]);
        let rows = [
            (0, 0, b(&[Some(1), None])),
            (1, 0, b(&[Some(2), None])),
            (2, 1, b(&[None, Some(3)])),
            (3, 1, b(&[None, Some(4)])),
        ];
        let mut actual = m.combinations(rows.iter().map(|(i, p, b)| (*i, *p, b)));
        actual.sort();
        assert_eq!(actual, vec![vec![0, 2], vec![0, 3], vec![1, 2], vec![1, 3]]);
    }
}
