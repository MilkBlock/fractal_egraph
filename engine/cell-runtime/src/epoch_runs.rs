//! A newly published epoch arrives in witness order. Store that run in a
//! contiguous array. Re-keying can retire an old entry without shifting the
//! range; an out-of-order insertion falls back to a tree for that epoch.
use crate::graph::Dependency;
use std::collections::BTreeMap;
use crate::ordered_run::OrderedRun;
type Item = (u64, Dependency);
#[derive(Default)]
pub(super) struct Epochs {
    bins: BTreeMap<u64, OrderedRun<Item>>,
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
