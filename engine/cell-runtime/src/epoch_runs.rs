//! A newly published epoch arrives in witness order. Store that run in a
//! contiguous array. Re-keying can retire an old entry without shifting the
//! range; an out-of-order insertion falls back to a tree for that epoch.
use crate::graph::Dependency;
use std::collections::{BTreeMap, BTreeSet};
type Item = (u64, Dependency);
#[derive(Default)]
pub(super) struct Epochs {
    bins: BTreeMap<u64, Bin>,
}
enum Bin {
    Run {
        rows: Vec<Item>,
        dead: Vec<u64>,
        live: usize,
    },
    Tree(BTreeSet<Item>),
}
impl Default for Bin {
    fn default() -> Self {
        Self::Run {
            rows: vec![],
            dead: vec![],
            live: 0,
        }
    }
}
impl Bin {
    fn len(&self) -> usize {
        match self {
            Self::Run { live, .. } => *live,
            Self::Tree(t) => t.len(),
        }
    }
    fn alive(dead: &[u64], i: usize) -> bool {
        dead.get(i / 64)
            .is_none_or(|bits| bits & (1u64 << (i % 64)) == 0)
    }
    fn insert(&mut self, v: Item) -> bool {
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
    fn remove(&mut self, v: &Item) -> bool {
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
    fn iter(&self) -> Cursor<'_> {
        match self {
            Self::Tree(t) => Cursor::Tree(t.iter()),
            Self::Run { rows, dead, .. } => Cursor::Run {
                rows: rows.iter().enumerate(),
                dead,
            },
        }
    }
}
enum Cursor<'a> {
    Tree(std::collections::btree_set::Iter<'a, Item>),
    Run {
        rows: std::iter::Enumerate<std::slice::Iter<'a, Item>>,
        dead: &'a [u64],
    },
}
impl<'a> Iterator for Cursor<'a> {
    type Item = &'a Item;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Tree(t) => t.next(),
            Self::Run { rows, dead } => rows.find_map(|(i, x)| Bin::alive(dead, i).then_some(x)),
        }
    }
}
impl Epochs {
    pub fn clear(&mut self) {
        self.bins.clear()
    }
    pub fn insert(&mut self, x: Item) -> bool {
        self.bins.entry(x.0).or_default().insert(x)
    }
    pub fn remove(&mut self, x: &Item) -> bool {
        let Some(b) = self.bins.get_mut(&x.0) else {
            return false;
        };
        let ok = b.remove(x);
        if b.len() == 0 {
            self.bins.remove(&x.0);
        }
        ok
    }
    pub fn range(&self, epoch: u64) -> impl Iterator<Item = &Item> {
        self.bins.range(epoch..).flat_map(|(_, b)| b.iter())
    }
    pub fn counts(&self, epoch: u64) -> impl Iterator<Item = usize> + '_ {
        self.bins.range(epoch..).map(|(_, b)| b.len())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retire_and_reinsert_across_epoch_ranges() {
        let mut e = Epochs::default();
        for i in 0..160 {
            e.insert((7, Dependency::Function(i)));
        }
        for i in 0..110 {
            assert!(e.remove(&(7, Dependency::Function(i))));
        }
        e.insert((7, Dependency::Function(12)));
        e.insert((8, Dependency::Function(11)));
        assert_eq!(e.counts(7).sum::<usize>(), 52);
        let v: Vec<_> = e.range(7).copied().collect();
        assert!(v.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(v[0], (7, Dependency::Function(12)));
        assert_eq!(e.range(8).count(), 1);
    }
}
