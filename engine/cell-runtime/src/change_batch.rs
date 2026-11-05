//! Coalesced notifications for one Run, sealed when graph repair finishes.
//! Commit order and producer receipts have separate ownership in Spine.
use crate::{
    graph::{Dependency, Graph, Row},
    neighbor::Context,
    program::Result,
    query::QueryIndexes,
    range_index::RangeIndex,
};
use rustc_hash::FxHashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    // Includes a subsumed fact: readers wake and inspect its hidden bit.
    Wake,
    Removed,
    // Keep the original distinct event count when repair retires an earlier write.
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

impl Change {
    pub fn row(&self, graph: &Graph, w: Dependency) -> Result<Option<Row>> {
        if *self == Change::Wake {
            graph.row(w)
        } else {
            Ok(None)
        }
    }
}

#[derive(Default, serde::Serialize)]
pub(crate) struct Publication {
    row_reads: u64,
    headers: u64,
    header_only: u64,
    column_rows: u64,
    range_rows: u64,
    neighbor_rows: u64,
}
impl Publication {
    /// Resolve each requested witness once, after repair. Column and range indexes
    /// retain their own previous keys so a removal needs only the witness.
    pub fn publish(
        &mut self,
        g: &Graph,
        columns: &mut QueryIndexes,
        mut ranges: Option<&mut RangeIndex>,
        neighbor: &mut Context,
    ) -> Result<()> {
        columns.begin_publication(g);
        for (t, changes) in g.changes.tables() {
            let count = changes.len() as u64;
            self.headers += count;
            let version = g.query_versions[t];
            let mut writer = columns.writer(t);
            let mut range = ranges.as_deref_mut().filter(|r| r.observes(t));
            let neighbors = neighbor.observes(t);
            if writer.is_none() && range.is_none() && !neighbors {
                self.header_only += count;
                continue;
            }
            self.column_rows += count * u64::from(writer.is_some());
            self.range_rows += count * u64::from(range.is_some());
            self.neighbor_rows += count * u64::from(neighbors);
            for (&w, change) in changes {
                let row = change.row(g, w)?;
                self.row_reads += u64::from(*change == Change::Wake);
                let row = row.as_ref();
                if let Some(writer) = &mut writer {
                    writer.update(w, row);
                }
                if let Some(range) = &mut range {
                    range.update_row(t, version, w, row)?;
                }
                if neighbors {
                    neighbor.changed(t, w, row);
                }
            }
            if let Some(writer) = writer {
                writer.finish(version);
            }
        }
        columns.finish_publication();
        neighbor.finish_publication(g);
        Ok(())
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
