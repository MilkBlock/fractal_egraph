//! Opt-in native table work during rule-set search/apply. Compiled out by default.
//! Counts actual row positions visited, including stale/constraint-rejected rows.
//! Index construction and refinements count as work; cached index probes and
//! hash bucket comparisons are not row scans. No count denotes a mutation.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JoinReadProfile {
    pub scan_calls: usize,
    pub scan_rows: usize,
    pub refine_rows: usize,
    pub key_lookups: usize,
}
#[derive(Default)]
pub struct JoinReadCounters {
    active: AtomicBool,
    scan_calls: AtomicUsize,
    scan_rows: AtomicUsize,
    refine_rows: AtomicUsize,
    key_lookups: AtomicUsize,
}
impl JoinReadCounters {
    pub(crate) fn scan(&self, rows: usize) {
        if self.active.load(Ordering::Relaxed) {
            self.scan_calls.fetch_add(1, Ordering::Relaxed);
            self.scan_rows.fetch_add(rows, Ordering::Relaxed);
        }
    }
    pub(crate) fn refine(&self, rows: usize) {
        if self.active.load(Ordering::Relaxed) {
            self.refine_rows.fetch_add(rows, Ordering::Relaxed);
        }
    }
    pub(crate) fn lookup(&self) {
        if self.active.load(Ordering::Relaxed) {
            self.key_lookups.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn snapshot(&self) -> JoinReadProfile {
        JoinReadProfile {
            scan_calls: self.scan_calls.load(Ordering::Relaxed),
            scan_rows: self.scan_rows.load(Ordering::Relaxed),
            refine_rows: self.refine_rows.load(Ordering::Relaxed),
            key_lookups: self.key_lookups.load(Ordering::Relaxed),
        }
    }
}
pub(crate) struct JoinReadGuard(Vec<(Arc<JoinReadCounters>, bool)>);
impl JoinReadGuard {
    pub(crate) fn new(counters: impl Iterator<Item = Arc<JoinReadCounters>>) -> Self {
        Self(
            counters
                .map(|c| {
                    let previous = c.active.swap(true, Ordering::Relaxed);
                    (c, previous)
                })
                .collect(),
        )
    }
}
impl Drop for JoinReadGuard {
    fn drop(&mut self) {
        for (counter, previous) in &self.0 {
            counter.active.store(*previous, Ordering::Relaxed);
        }
    }
}
