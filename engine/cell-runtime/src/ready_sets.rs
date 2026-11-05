//! Coalesced rule notifications in source order. A clean rule's logical
//! watermark is the latest completed occurrence of any set containing it.
use crate::program::Result;
use smallvec::SmallVec;
use std::collections::BTreeMap;
struct Set {
    rules: Vec<usize>,
    bits: Vec<u64>,
    last: u64,
}
pub(super) struct ReadySets {
    names: BTreeMap<String, usize>,
    sets: Vec<Set>,
    positions: Vec<SmallVec<[(usize, usize); 2]>>,
}
impl ReadySets {
    pub(super) fn new(sets: &BTreeMap<String, Vec<usize>>, rule_count: usize) -> Self {
        let mut this = Self {
            names: BTreeMap::new(),
            sets: Vec::new(),
            positions: vec![SmallVec::new(); rule_count],
        };
        for (name, ids) in sets {
            let set = this.sets.len();
            this.names.insert(name.clone(), set);
            let mut bits = vec![0; ids.len().div_ceil(64)];
            for (at, &rule) in ids.iter().enumerate() {
                bits[at / 64] |= 1 << (at % 64);
                this.positions[rule].push((set, at));
            }
            this.sets.push(Set {
                rules: ids.clone(),
                bits,
                last: 0,
            });
        }
        this
    }
    pub(super) fn wake(&mut self, rule: usize) {
        for &(set, pos) in &self.positions[rule] {
            self.sets[set].bits[pos / 64] |= 1 << (pos % 64);
        }
    }
    pub(super) fn clean(&mut self, rule: usize) {
        for &(set, pos) in &self.positions[rule] {
            self.sets[set].bits[pos / 64] &= !(1 << (pos % 64));
        }
    }
    pub(super) fn watermark(&self, rule: usize) -> u64 {
        self.positions[rule]
            .iter()
            .map(|&(s, _)| self.sets[s].last)
            .max()
            .unwrap_or(0)
    }
    pub(super) fn select(&self, name: &str) -> Result<(usize, Vec<usize>)> {
        let id = *self
            .names
            .get(name)
            .ok_or_else(|| format!("unknown ruleset {name}"))?;
        let set = &self.sets[id];
        let mut out = Vec::new();
        for (n, &bits) in set.bits.iter().enumerate() {
            let mut word = bits;
            while word != 0 {
                out.push(set.rules[64 * n + word.trailing_zeros() as usize]);
                word &= word - 1;
            }
        }
        Ok((id, out))
    }
    pub(super) fn finish(&mut self, id: usize, epoch: u64) {
        self.sets[id].last = epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_memberships_advance_clean_watermarks_and_wake_in_source_order() {
        let source = BTreeMap::from([("both".into(), vec![1, 0, 1]), ("one".into(), vec![1])]);
        let mut frontier = ReadySets::new(&source, 2);
        let (both, ids) = frontier.select("both").unwrap();
        assert_eq!(ids, [1, 0, 1]);
        let (one, _) = frontier.select("one").unwrap();
        frontier.clean(1);
        frontier.finish(one, 4);
        assert_eq!(frontier.select("both").unwrap().1, [0]);
        assert_eq!(frontier.watermark(0), 0);
        assert_eq!(frontier.watermark(1), 4);
        frontier.finish(both, 7);
        assert_eq!(frontier.watermark(1), 7);
        frontier.wake(1);
        assert_eq!(frontier.select("both").unwrap().1, [1, 0, 1]);
        assert_eq!(frontier.select("one").unwrap().1, [1]);
    }
}
