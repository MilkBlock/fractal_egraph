//! Read-only trace of actual residual Free Join admission. Timers stop before
//! observer bookkeeping. Shared entry enumeration alone receives no time credit.
use crate::graph::Dependency;
use rustc_hash::FxHashMap as Map;
use std::{collections::BTreeSet, time::Instant};

#[derive(Default, serde::Serialize)]
struct Tally {
    events: u64,
    repeat: u64,
    repeat_cache_hits: u64,
    repeat_probes: u64,
    repeat_ns: u64,
    members: BTreeSet<usize>,
}
#[derive(Default)]
struct Memo {
    epoch: u64,
    cache: Map<(u64, Dependency), (usize, Option<std::rc::Rc<Vec<Dependency>>>)>,
    ns: u64,
}
#[derive(Default)]
pub(crate) struct Overlap {
    initialized: bool,
    sets: Vec<String>,
    set: usize,
    seeds: Map<(u64, usize, Dependency), usize>,
    reads: Map<(usize, usize, usize, bool), Memo>,
    epoch: u64,
    drivers: Map<(usize, usize, usize), Tally>,
    projections: Map<(usize, usize, usize, bool), Tally>,
    floor: u64,
}
impl Overlap {
    pub fn start(&mut self, _epoch: u64, set: &str) {
        self.seeds.clear();
        self.epoch = _epoch;
        self.set = self.sets.iter().position(|s| s == set).unwrap_or_else(|| {
            self.sets.push(set.into());
            self.sets.len() - 1
        });
        READS.with_borrow_mut(|v| {
            v.set = self.set;
            v.seen.clear();
            if !self.initialized {
                v.ledger.clear();
            }
        });
        if !self.initialized {
            self.initialized = true;
            let pairs: Vec<_> = (0..2048)
                .map(|_| Instant::now().elapsed().as_nanos() as u64)
                .collect();
            self.floor = pairs.iter().sum::<u64>() / pairs.len() as u64;
        }
    }
    pub fn seed(&mut self, wm: u64, rid: usize, node: usize, table: usize, w: Dependency) {
        let old = self.seeds.insert((wm, node, w), rid);
        let t = self.drivers.entry((self.set, table, node)).or_default();
        t.members.insert(rid);
        t.events += 1;
        t.repeat += u64::from(old.is_some_and(|p| p != rid));
    }
    pub fn read(
        &mut self,
        wm: u64,
        rid: usize,
        table: usize,
        projection: usize,
        tie: bool,
        w: Dependency,
        probed: bool,
        ns: u64,
        result: &Option<std::rc::Rc<Vec<Dependency>>>,
    ) {
        let route = (self.set, table, projection, tie);
        let memo = self.reads.entry(route).or_default();
        let clock = Instant::now();
        if memo.epoch != self.epoch {
            memo.cache.clear();
            memo.epoch = self.epoch;
        }
        let old = match memo.cache.entry((wm, w)) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert((rid, result.clone()));
                None
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                let (last, value) = e.get_mut();
                std::hint::black_box(value.clone());
                Some(std::mem::replace(last, rid))
            }
        };
        memo.ns += clock.elapsed().as_nanos() as u64;
        let t = self.projections.entry(route).or_default();
        t.events += 1;
        t.members.insert(rid);
        if old.is_some_and(|p| p != rid) {
            t.repeat += 1;
            t.repeat_probes += u64::from(probed);
            t.repeat_cache_hits += u64::from(!probed);
            t.repeat_ns += ns;
        }
    }
    pub fn report(&self) -> serde_json::Value {
        let mut drivers: Vec<_> = self.drivers.iter().filter(|(_, v)| v.repeat > 0).collect();
        let mut reads: Vec<_> = self
            .projections
            .iter()
            .filter(|(_, v)| v.repeat > 0)
            .collect();
        drivers.sort_by_key(|x| x.0);
        reads.sort_by_key(|x| x.0);
        let memo: Vec<_> = reads.iter().map(|(k, _)| (*k, self.reads[*k].ns)).collect();
        let physical = READS.with_borrow(|v| {
            let mut rows: Vec<_> = v.ledger.iter().filter(|(_, t)| t.repeat > 0).collect();
            rows.sort_by_key(|x| x.0);
            serde_json::json!(rows)
        });
        serde_json::json!({"physical":physical,"memo_ns":memo,"sets":self.sets,"clock_floor_ns":self.floor,"drivers":drivers,"projections":reads})
    }
}

use crate::value::Value;
use smallvec::SmallVec;
use std::cell::{Cell, RefCell};
type Selectors = SmallVec<[(usize, Value); 4]>;
#[derive(Default)]
struct Physical {
    set: usize,
    rule: Option<(usize, u64)>,
    seen: Map<(u64, usize, bool, Selectors), usize>,
    ledger: Map<(usize, usize, u8), Tally>,
}
thread_local! {
    static ACTIVE:Cell<bool>=const { Cell::new(false) };
    static READS:RefCell<Physical>=RefCell::new(Physical::default());
}
pub(crate) fn begin_reads(r: usize, wm: u64) {
    READS.with_borrow_mut(|v| v.rule = Some((r, wm)));
    ACTIVE.set(true);
}
pub(crate) fn end_reads() {
    ACTIVE.set(false);
}
pub(crate) fn clock() -> Option<Instant> {
    ACTIVE.get().then(Instant::now)
}
// Read channels 0 and 1 return the same logical point lookup. Channel 2
// supplies a candidate domain and is kept separate, even for the same key.
pub(crate) fn physical(
    table: usize,
    channel: u8,
    clock: Option<Instant>,
    selectors: impl FnOnce() -> Selectors,
) {
    let Some(start) = clock else { return };
    let ns = start.elapsed().as_nanos() as u64;
    READS.with_borrow_mut(|s| {
        let (rid, wm) = s.rule.unwrap();
        let old = s.seen.insert((wm, table, channel == 2, selectors()), rid);
        let t = s.ledger.entry((s.set, table, channel)).or_default();
        t.events += 1;
        t.members.insert(rid);
        if old.is_some_and(|other| other != rid) {
            t.repeat += 1;
            t.repeat_ns += ns;
        }
    });
}
