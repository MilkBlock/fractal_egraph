//! Equality IDs encode a producing Cell and slot. Most Cell IDs have no
//! displaced slot at all; a one-bit negative directory avoids probing the
//! parent hash table for them. Large external IDs retain the full hash path.
use rustc_hash::FxHashMap;
#[derive(Clone, Debug, Default)]
pub struct Parents {
    map: FxHashMap<u64, u64>,
    cells: Vec<u64>,
}
const WORD_LIMIT: usize = 1 << 20;
impl Parents {
    #[inline]
    fn address(x: u64) -> (usize, u64) {
        let cell = (x >> 32) as usize;
        (cell / 64, 1u64 << (cell % 64))
    }
    #[inline]
    fn maybe(&self, x: u64) -> bool {
        let (word, bit) = Self::address(x);
        word >= WORD_LIMIT || self.cells.get(word).is_some_and(|v| v & bit != 0)
    }
    #[inline]
    pub fn get(&self, x: &u64) -> Option<&u64> {
        if self.maybe(*x) {
            self.map.get(x)
        } else {
            None
        }
    }
    pub fn contains_key(&self, x: &u64) -> bool {
        self.get(x).is_some()
    }
    pub fn insert(&mut self, x: u64, to: u64) -> Option<u64> {
        let (word, bit) = Self::address(x);
        if word < WORD_LIMIT {
            if word >= self.cells.len() {
                self.cells.resize(word + 1, 0);
            }
            self.cells[word] |= bit;
        }
        self.map.insert(x, to)
    }
    pub fn clear(&mut self) {
        self.cells.clear();
        self.map.clear()
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
