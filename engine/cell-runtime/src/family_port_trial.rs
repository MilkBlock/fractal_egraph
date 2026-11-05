//! Isolated index-retirement calibration for a producer with no other query readers.
use super::*;
#[derive(Default, serde::Serialize)]
pub(super) struct Stats {
    headers: u64,
    union_run_headers: u64,
    published_ns: u64,
    refreshed_ns: u64,
    cold_rows: u64,
    refreshed_rows: u64,
    retired_headers: u64,
    pub generic_columns: bool,
}
pub(super) struct Trial {
    column: usize,
    cold: bool,
    pending: BTreeSet<FactRef>,
    pub directory: Directory,
    pub stats: Stats,
}
impl Trial {
    pub fn new(column: usize) -> Self {
        Self {
            column,
            cold: true,
            pending: BTreeSet::new(),
            directory: Default::default(),
            stats: Default::default(),
        }
    }
    pub fn published(
        &mut self,
        changes: &FxHashMap<Dependency, crate::change_batch::Change>,
        union: bool,
        profile: bool,
    ) {
        let clock = profile.then(Instant::now);
        self.stats.headers += changes.len() as u64;
        if union {
            self.stats.union_run_headers += changes.len() as u64
        }
        if !self.cold {
            self.pending.extend(changes.keys().filter_map(|w| match w {
                Dependency::Fact(f) => Some(*f),
                _ => None,
            }));
        }
        if let Some(c) = clock {
            self.stats.published_ns += c.elapsed().as_nanos() as u64;
        }
    }
    pub fn refresh(&mut self, g: &Graph, table: usize, profile: bool) -> Result<()> {
        if !self.cold && self.pending.is_empty() {
            return Ok(());
        }
        let clock = profile.then(Instant::now);
        if self.cold {
            let all = g.witnesses(table);
            self.stats.cold_rows += all.len() as u64;
            self.pending.extend(all.into_iter().filter_map(|w| match w {
                Dependency::Fact(f) => Some(f),
                _ => None,
            }));
            self.cold = false;
        }
        for f in std::mem::take(&mut self.pending) {
            let row = g.fact(f)?.filter(|r| !r.hidden);
            self.stats.refreshed_rows += 1;
            self.stats.retired_headers += u64::from(row.is_none());
            let id = ((u64::from(f.cell) << 32) | u64::from(f.op)) as usize;
            self.directory
                .update(self.column, id, row.as_ref().map(Tuple::flat));
        }
        if let Some(c) = clock {
            self.stats.refreshed_ns += c.elapsed().as_nanos() as u64;
        }
        Ok(())
    }
}
