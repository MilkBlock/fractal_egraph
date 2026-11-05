//! Ordered monotone insertions share contiguous storage. Displaced rows
//! keep a tombstone until compaction; nonmonotone insertion promotes to a tree.
use std::collections::BTreeSet;
#[derive(Clone)]
pub(crate) enum OrderedRun<T> {
    Run {
        rows: Vec<T>,
        dead: Vec<u64>,
        live: usize,
    },
    Tree(BTreeSet<T>),
}
impl<T> Default for OrderedRun<T> {
    fn default() -> Self {
        Self::Run {
            rows: vec![],
            dead: vec![],
            live: 0,
        }
    }
}
impl<T: Ord + Copy> OrderedRun<T> {
    pub fn len(&self) -> usize {
        match self {
            Self::Run { live, .. } => *live,
            Self::Tree(t) => t.len(),
        }
    }
    fn alive(dead: &[u64], i: usize) -> bool {
        dead.get(i / 64)
            .is_none_or(|bits| bits & (1u64 << (i % 64)) == 0)
    }
    pub fn insert(&mut self, v: T) -> bool {
        let Self::Run { rows, live, dead } = self else {
            let Self::Tree(t) = self else { unreachable!() };
            return t.insert(v);
        };
        if rows.last().is_none_or(|w| *w < v) {
            rows.push(v);
            *live += 1;
            return true;
        }
        if let Ok(i) = rows.binary_search(&v) {
            if Self::alive(dead, i) {
                return false;
            }
            dead[i / 64] &= !(1u64 << (i % 64));
            *live += 1;
            return true;
        }
        let mut t: BTreeSet<_> = rows
            .iter()
            .enumerate()
            .filter(|(i, _)| Self::alive(dead, *i))
            .map(|(_, x)| *x)
            .collect();
        t.insert(v);
        *self = Self::Tree(t);
        true
    }
    pub fn remove(&mut self, v: &T) -> bool {
        match self {
            Self::Tree(t) => t.remove(v),
            Self::Run { rows, dead, live } => {
                let Ok(i) = rows.binary_search(v) else {
                    return false;
                };
                if !Self::alive(dead, i) {
                    return false;
                }
                if i / 64 >= dead.len() {
                    dead.resize(i / 64 + 1, 0)
                }
                dead[i / 64] |= 1u64 << (i % 64);
                *live -= 1;
                if rows.len() > 64 && *live < rows.len() / 2 && *live > 0 {
                    let mut pos = 0;
                    rows.retain(|_| {
                        let keep = Self::alive(dead, pos);
                        pos += 1;
                        keep
                    });
                    dead.clear();
                }
                true
            }
        }
    }
    pub fn iter(&self) -> Cursor<'_, T> {
        match self {
            Self::Tree(t) => Cursor::Tree(t.iter()),
            Self::Run { rows, dead, .. } => Cursor::Run {
                rows: rows.iter().enumerate(),
                dead,
            },
        }
    }
}
pub(crate) enum Cursor<'a, T> {
    Tree(std::collections::btree_set::Iter<'a, T>),
    Run {
        rows: std::iter::Enumerate<std::slice::Iter<'a, T>>,
        dead: &'a [u64],
    },
}
impl<'a, T: Ord + Copy> Iterator for Cursor<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Tree(t) => t.next(),
            Self::Run { rows, dead } => {
                rows.find_map(|(i, x)| OrderedRun::<T>::alive(dead, i).then_some(x))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_table_updates_match_sorted_membership() {
        let mut rows = OrderedRun::default();
        let mut expected = BTreeSet::new();
        for key in (0..240u32).step_by(2) {
            assert_eq!(rows.insert(key), expected.insert(key));
        }
        for key in (0..180).step_by(2) {
            assert_eq!(rows.remove(&key), expected.remove(&key));
        }
        for key in [194, 195, 4, 1, 240, 195] {
            assert_eq!(rows.insert(key), expected.insert(key));
        }
        for key in [0, 1, 194, 195, 195, 240] {
            assert_eq!(rows.remove(&key), expected.remove(&key));
        }
        assert!(rows.iter().eq(expected.iter()));
        assert_eq!(rows.len(), expected.len());
    }
}
