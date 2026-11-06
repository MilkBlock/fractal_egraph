//! One logical-ID directory for both ordinary records and compact port blocks.
pub(crate) const PORT_TAG: u32 = 1 << 31;
pub(crate) type SharedDirectory = std::rc::Rc<std::cell::RefCell<Directory>>;
#[derive(Default)]
pub(crate) struct Directory {
    positions: Vec<u32>,
    outliers: rustc_hash::FxHashMap<u32, u32>,
}
impl Directory {
    #[inline]
    pub fn get(&self, id: u32) -> u32 {
        let word = self.positions.get(id as usize).copied().unwrap_or(0);
        if word != 0 || self.outliers.is_empty() {
            word
        } else {
            self.outliers.get(&id).copied().unwrap_or(0)
        }
    }
    pub fn set(&mut self, id: u32, word: u32) {
        let n = id as usize;
        if word == 0 {
            if let Some(v) = self.positions.get_mut(n) {
                *v = 0;
            }
            self.outliers.remove(&id);
            return;
        }
        if n <= self.positions.len().saturating_add(65_536) {
            if n >= self.positions.len() {
                self.positions.resize(n + 1, 0);
            }
            self.positions[n] = word;
            self.outliers.remove(&id);
        } else {
            self.outliers.insert(id, word);
        }
    }
    pub fn capacity(&self) -> usize {
        self.positions.capacity()
    }
}
