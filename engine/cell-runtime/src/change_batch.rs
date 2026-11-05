//! Coalesced notifications for one Run, sealed when graph repair finishes.
//! Commit order and producer receipts have separate ownership in Spine.
use crate::graph::Dependency;
use rustc_hash::FxHashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Wake,
    Removed,
    RemovedAfterWake,
}

pub(crate) struct ChangeBatch {
    tables: Vec<FxHashMap<Dependency, Change>>,
    active: Vec<usize>,
    pub events: u64,
    pub distinct_events: u64,
}

impl ChangeBatch {
    pub fn new(tables: usize) -> Self {
        Self {
            tables: vec![FxHashMap::default(); tables],
            active: Vec::new(),
            events: 0,
            distinct_events: 0,
        }
    }

    fn table(&mut self, table: usize) -> &mut FxHashMap<Dependency, Change> {
        let rows = &mut self.tables[table];
        if rows.is_empty() {
            self.active.push(table);
        }
        rows
    }

    pub fn touch(&mut self, table: usize, witness: Dependency) {
        let previous = self.table(table).insert(witness, Change::Wake);
        self.events += 1;
        self.distinct_events += u64::from(matches!(previous, None | Some(Change::Removed)));
    }

    pub fn retire(&mut self, table: usize, witness: Dependency) {
        let change = self.table(table).entry(witness).or_insert(Change::Removed);
        if *change == Change::Wake {
            *change = Change::RemovedAfterWake;
        }
    }

    pub fn tables(&self) -> impl Iterator<Item = (usize, &FxHashMap<Dependency, Change>)> {
        self.active.iter().map(|&t| (t, &self.tables[t]))
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub fn clear(&mut self) {
        for t in self.active.drain(..) {
            self.tables[t].clear();
        }
        self.events = 0;
        self.distinct_events = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retirement_cancels_wakeup_but_keeps_delivery_counts() {
        let mut batch = ChangeBatch::new(2);
        let w = Dependency::Function(0);
        let survivor = Dependency::Function(1);
        batch.touch(1, w);
        batch.touch(1, w);
        batch.retire(1, w);
        batch.retire(1, w);
        batch.touch(1, survivor);
        let (table, rows) = batch.tables().next().unwrap();
        assert_eq!(table, 1);
        assert!(rows[&w] == Change::RemovedAfterWake);
        assert!(rows[&survivor] == Change::Wake);
        assert_eq!((batch.events, batch.distinct_events), (3, 2));
        batch.clear();
        assert!(batch.is_empty());
        batch.retire(1, w);
        assert_eq!((batch.events, batch.distinct_events), (0, 0));
    }
}
