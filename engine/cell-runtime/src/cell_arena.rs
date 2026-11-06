//! Dense live Cells with stable logical IDs. Removing a Cell relocates at
//! most one physical record and fixes its reverse slot; FactRef IDs never move.
use super::Cell;
#[cfg(feature="port-frames")]
#[path="cell_locations.rs"]
mod locations;
#[cfg(feature="port-frames")]
pub(crate) use locations::{SharedDirectory,PORT_TAG};
#[derive(Default)]
pub struct Cells {
    records: Vec<(u32, Cell)>,
    #[cfg(feature="port-frames")]
    directory:SharedDirectory,
    #[cfg(not(feature="port-frames"))]
    positions: Vec<u32>,
    #[cfg(not(feature="port-frames"))]
    outliers: rustc_hash::FxHashMap<u32, u32>,
}
impl Cells {
    #[cfg(feature="port-frames")]
    pub(crate) fn directory(&self)->SharedDirectory{self.directory.clone()}
    #[inline]
    fn position(&self, id: &u32) -> Option<usize> {
        #[cfg(feature="port-frames")]
        {
            let word=self.directory.borrow().get(*id);
            if word&PORT_TAG!=0{return None;}
            word.checked_sub(1).map(|i|i as usize)
        }
        #[cfg(not(feature="port-frames"))]
        {
            let v = self.positions.get(*id as usize).copied().unwrap_or(0);
            if v != 0 {
                Some((v - 1) as usize)
            } else if self.outliers.is_empty() {
                None
            } else {
                self.outliers.get(id).map(|n| (*n - 1) as usize)
            }
        }
    }
    #[inline]
    pub fn get(&self, id: &u32) -> Option<&Cell> {
        Some(&self.records[self.position(id)?].1)
    }
    #[inline]
    pub fn get_mut(&mut self, id: &u32) -> Option<&mut Cell> {
        let i = self.position(id)?;
        Some(&mut self.records[i].1)
    }
    pub fn contains_key(&self, id: &u32) -> bool {
        self.position(id).is_some()
    }
    pub fn insert(&mut self, id: u32, c: Cell) -> Option<Cell> {
        if let Some(i) = self.position(&id) {
            return Some(std::mem::replace(&mut self.records[i].1, c));
        }
        #[cfg(not(feature="port-frames"))]
        let n = id as usize;
        let slot = u32::try_from(self.records.len() + 1)
            .expect("more live Cells than reserved identities");
        #[cfg(feature="port-frames")]
        {assert!(slot<PORT_TAG);assert_eq!(self.directory.borrow().get(id),0,"detach a port before promotion");}
        self.records.push((id, c));
        #[cfg(feature="port-frames")]
        self.directory.borrow_mut().set(id,slot);
        #[cfg(not(feature="port-frames"))]
        if n <= self.positions.len().saturating_add(65_536) {
            if n >= self.positions.len() {
                self.positions.resize(n + 1, 0);
            }
            self.positions[n] = slot;
        } else {
            self.outliers.insert(id, slot);
        }
        None
    }
    pub fn remove(&mut self, id: &u32) -> Option<Cell> {
        let i = self.position(id)?;
        #[cfg(feature="port-frames")]
        self.directory.borrow_mut().set(*id,0);
        #[cfg(not(feature="port-frames"))]
        if self.positions.get(*id as usize).is_some_and(|n| *n != 0) {
            self.positions[*id as usize] = 0;
        } else {
            self.outliers.remove(id);
        }
        let (_, c) = self.records.swap_remove(i);
        if let Some((moved, _)) = self.records.get(i) {
            #[cfg(feature="port-frames")]
            self.directory.borrow_mut().set(*moved,i as u32+1);
            #[cfg(not(feature="port-frames"))]
            if self.positions.get(*moved as usize).is_some_and(|n| *n != 0) {
                self.positions[*moved as usize] = i as u32 + 1;
            } else {
                self.outliers.insert(*moved, i as u32 + 1);
            }
        }
        Some(c)
    }
    pub fn values(&self) -> impl Iterator<Item = &Cell> {
        self.records.iter().map(|(_, c)| c)
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}
impl std::ops::Index<&u32> for Cells {
    type Output = Cell;
    fn index(&self, k: &u32) -> &Cell {
        self.get(k).expect("missing Cell")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn cell(t: usize) -> Cell {
        Cell {
            template: t,
            started_epoch: 0,
            parent: None,
            values: Default::default(),
            facts: Default::default(),
            completed: false,
            completed_prefix: 0,
            recipe_depth: 0,
        }
    }
    #[test]
    fn swapped_record_and_outlier_keep_their_logical_ids() {
        let mut a = Cells::default();
        a.insert(3, cell(0));
        a.insert(u32::MAX, cell(1));
        a.insert(9, cell(2));
        assert_eq!(a.remove(&3).unwrap().template, 0);
        assert_eq!(a[&9].template, 2);
        #[cfg(not(feature="port-frames"))]
        assert!(a.positions.len() < 100);
        #[cfg(feature="port-frames")]
        assert!(a.directory.borrow().capacity()<100);
        assert_eq!(a.insert(u32::MAX, cell(3)).unwrap().template, 1);
        a.remove(&9);
        assert_eq!(a[&u32::MAX].template, 3);
    }
}
