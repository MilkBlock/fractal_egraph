//! A necessary first Free Join projection, incrementally maintained at Run barriers.
//! The surviving frontier is still checked by the ordinary entry pipeline.
use super::*;
use crate::entry_join::{GuardShape, Projection, Source};
use rustc_hash::{FxHashMap, FxHashSet};

type Key = SmallVec<[u64; 4]>;
const WINDOW: u64 = 512;
const MAX_STATES: usize = 8;
const ROW_LIMIT: usize = 65536;

#[derive(Default, serde::Serialize)]
pub(crate) struct Stats {
    builds: u64,
    maintenance_ns: u64,
    build_ns: u64,
    evictions: u64,
    build_rows: u64,
    driver_updates: u64,
    target_updates: u64,
    support_flips: u64,
    flip_visits: u64,
    stages: u64,
    input_rows: u64,
    delivered_rows: u64,
    frontier_visits: u64,
    over_budget: u64,
}
#[derive(Default)]
struct Trial {
    rows: u64,
    keys: u64,
    passed: u64,
    seen: FxHashSet<Key>,
    last: Option<(u64, usize, usize)>,
    pause: u64,
    backoff: u64,
    ready: bool,
}
impl Trial {
    fn sample(&mut self, epoch: u64, rid: usize, pivot: usize, key: &Key) -> bool {
        if self.ready {
            return false;
        }
        if self.pause > 0 {
            self.pause -= 1;
            return false;
        }
        if self.last != Some((epoch, rid, pivot)) {
            self.seen.clear();
            self.last = Some((epoch, rid, pivot));
        }
        self.rows += 1;
        self.keys += u64::from(self.seen.insert(key.clone()));
        true
    }
    fn finish(&mut self, passes: bool) {
        self.passed += u64::from(passes);
        if self.rows >= WINDOW {
            self.ready = 2 * (self.keys + self.passed) < self.rows;
            self.backoff = (self.backoff * 2).clamp(WINDOW, 65536);
            self.pause = self.backoff;
            self.rows = 0;
            self.keys = 0;
            self.passed = 0;
            self.seen.clear();
        }
    }
}
struct State {
    targets: FxHashMap<Dependency, Key>,
    counts: FxHashMap<Key, usize>,
    epochs: FxHashMap<Dependency, u64>,
    frontier: BTreeSet<(u64, Dependency)>,
}
impl State {
    fn set_driver(&mut self, w: Dependency, epoch: Option<u64>) {
        if let Some(old) = self.epochs.get(&w).copied() {
            if epoch == Some(old) {
                return;
            }
            self.frontier.remove(&(old, w));
            self.epochs.remove(&w);
        }
        if let Some(e) = epoch {
            self.epochs.insert(w, e);
            self.frontier.insert((e, w));
        }
    }
}
struct Guard {
    driver: usize,
    projection: usize,
    uses: u64,
    rejected: u64,
    target: usize,
    to: SmallVec<[usize; 4]>,
    from: SmallVec<[Source; 4]>,
    driver_cols: SmallVec<[usize; 4]>,
    trial: Trial,
    state: Option<State>,
}
impl Guard {
    fn new(spec: &GuardShape, projection: &Projection) -> Self {
        Self {
            driver: spec.driver,
            projection: spec.projection,
            uses: 0,
            rejected: 0,
            target: projection.table,
            to: projection.columns.iter().map(|(c, _)| *c).collect(),
            from: projection.columns.iter().map(|(_, s)| s.clone()).collect(),
            driver_cols: projection
                .columns
                .iter()
                .filter_map(|(_, s)| match s {
                    Source::Cover(c) => Some(*c),
                    _ => None,
                })
                .collect(),
            trial: Default::default(),
            state: None,
        }
    }
    fn driver_key(from: &[Source], hashes: &[u64], ix: &QueryIndexes) -> Key {
        from.iter()
            .map(|s| match s {
                Source::Cover(c) => hashes[*c],
                Source::Literal(v) => ix.fingerprint(v),
            })
            .collect()
    }
    fn target_key(to: &[usize], from: &[Source], hashes: &[u64], ix: &QueryIndexes) -> Option<Key> {
        let key: Key = to.iter().map(|&c| hashes[c]).collect();
        from.iter()
            .zip(&key)
            .all(|(s, &h)| match s {
                Source::Literal(v) => ix.fingerprint(v) == h,
                _ => true,
            })
            .then_some(key)
    }
    fn build(&mut self, g: &Graph, ix: &mut QueryIndexes, stats: &mut Stats) -> Result<()> {
        ix.ensure(g, self.target)?;
        ix.ensure(g, self.driver)?;
        let mut state = State {
            targets: Default::default(),
            counts: Default::default(),
            epochs: Default::default(),
            frontier: Default::default(),
        };
        for w in ix.indexed_witnesses(self.target) {
            stats.build_rows += 1;
            if let Some(k) =
                Self::target_key(&self.to, &self.from, ix.hashes(self.target, w).unwrap(), ix)
            {
                *state.counts.entry(k.clone()).or_default() += 1;
                state.targets.insert(w, k);
            }
        }
        for w in ix.indexed_witnesses(self.driver) {
            stats.build_rows += 1;
            let k = Self::driver_key(&self.from, ix.hashes(self.driver, w).unwrap(), ix);
            if state.counts.contains_key(&k) {
                state.set_driver(w, g.visible_epoch(w));
            }
        }
        self.state = Some(state);
        stats.builds += 1;
        Ok(())
    }
    fn publish(&mut self, g: &Graph, ix: &QueryIndexes, stats: &mut Stats) {
        let state = self.state.as_mut().unwrap();
        let mut before = FxHashMap::default();
        for &w in g.changes.for_table(self.target).keys() {
            stats.target_updates += 1;
            let next = ix
                .hashes(self.target, w)
                .and_then(|h| Self::target_key(&self.to, &self.from, h, ix));
            if state.targets.get(&w) == next.as_ref() {
                continue;
            }
            if let Some(old) = state.targets.remove(&w) {
                before.entry(old.clone()).or_insert(true);
                let n = state.counts.get_mut(&old).unwrap();
                *n -= 1;
                if *n == 0 {
                    state.counts.remove(&old);
                }
            }
            if let Some(k) = next {
                before
                    .entry(k.clone())
                    .or_insert_with(|| state.counts.contains_key(&k));
                *state.counts.entry(k.clone()).or_default() += 1;
                state.targets.insert(w, k);
            }
        }
        // All index updates have finished. Apply net support transitions once,
        // including removal/reinsertion of different witnesses at one key.
        for (key, previous) in before {
            let present = state.counts.contains_key(&key);
            if previous == present {
                continue;
            }
            stats.support_flips += 1;
            let selectors: Key = self
                .from
                .iter()
                .zip(&key)
                .filter_map(|(s, h)| matches!(s, Source::Cover(_)).then_some(*h))
                .collect();
            for w in ix.hash_candidates(self.driver, &self.driver_cols, &selectors) {
                stats.flip_visits += 1;
                state.set_driver(w, if present { g.visible_epoch(w) } else { None });
            }
        }
        for &w in g.changes.for_table(self.driver).keys() {
            stats.driver_updates += 1;
            let supported = ix.hashes(self.driver, w).is_some_and(|h| {
                state
                    .counts
                    .contains_key(&Self::driver_key(&self.from, h, ix))
            });
            state.set_driver(w, if supported { g.visible_epoch(w) } else { None });
        }
    }
}
pub(crate) struct Gates {
    by_entry: Vec<Vec<Option<usize>>>,
    guards: Vec<Guard>,
    active: Vec<usize>,
    stats: Stats,
    profile: bool,
}
impl Gates {
    pub fn new(plan: &crate::entry_join::Plan) -> Self {
        let mut by_entry: Vec<_> = plan.entries.iter().map(|v| vec![None; v.len()]).collect();
        let mut guards = vec![];
        for spec in plan.guards.iter().flatten() {
            let id = guards.len();
            for &(r, p) in &spec.consumers {
                by_entry[r][p] = Some(id);
            }
            guards.push(Guard::new(spec, &plan.projections[spec.projection]));
        }
        Self {
            by_entry,
            guards,
            active: vec![],
            stats: Default::default(),
            profile: std::env::var_os("CELL_CPU_PROFILE").is_some(),
        }
    }
    pub fn sample(&mut self, epoch: u64, rid: usize, pivot: usize, key: &Key) -> Option<usize> {
        let id = self.by_entry[rid][pivot]?;
        self.guards[id]
            .trial
            .sample(epoch, rid, pivot, key)
            .then_some(id)
    }
    pub fn finish_sample(&mut self, id: usize, supported: bool) {
        self.guards[id].trial.finish(supported);
    }
    pub fn restrict(
        &mut self,
        g: &Graph,
        ix: &mut QueryIndexes,
        rid: usize,
        batch: crate::entry_schedule::Batch<'_>,
    ) -> Result<Option<Vec<Dependency>>> {
        let Some(id) = self.by_entry[rid][batch.atom()] else {
            return Ok(None);
        };
        let guard = &mut self.guards[id];
        if !guard.trial.ready {
            return Ok(None);
        }
        let delta = batch.delta_len().unwrap_or(0);
        if delta < 32 {
            return Ok(None);
        }
        if guard.state.is_none() {
            if self.active.len() >= MAX_STATES
                || g.witness_count(guard.target) + g.witness_count(guard.driver) > ROW_LIMIT
            {
                return Ok(None);
            }
            let clock = self.profile.then(Instant::now);
            guard.build(g, ix, &mut self.stats)?;
            if let Some(clock) = clock {
                self.stats.build_ns += clock.elapsed().as_nanos() as u64;
            }
            self.active.push(id);
        }
        let state = guard.state.as_ref().unwrap();
        let mut rows = vec![];
        let start = (
            batch.watermark(),
            Dependency::Fact(FactRef { cell: 0, op: 0 }),
        );
        for (i, &(_, w)) in state.frontier.range(start..).enumerate() {
            self.stats.frontier_visits += 1;
            if i >= delta / 2 {
                self.stats.over_budget += 1;
                return Ok(None);
            }
            if batch.packet(w).is_some() {
                rows.push(w);
            }
        }
        guard.uses += 1;
        guard.rejected += (delta - rows.len()) as u64;
        self.stats.stages += 1;
        self.stats.input_rows += delta as u64;
        self.stats.delivered_rows += rows.len() as u64;
        Ok(Some(rows))
    }
    pub fn publish(&mut self, g: &Graph, ix: &QueryIndexes) {
        if self.active.is_empty() || g.changes.is_empty() {
            return;
        }
        let clock = self.profile.then(Instant::now);
        self.active.retain(|&id| {
            let guard = &mut self.guards[id];
            if g.witness_count(guard.target) + g.witness_count(guard.driver) > ROW_LIMIT {
                guard.state = None;
                self.stats.evictions += 1;
                false
            } else {
                guard.publish(g, ix, &mut self.stats);
                true
            }
        });
        if let Some(clock) = clock {
            self.stats.maintenance_ns += clock.elapsed().as_nanos() as u64;
        }
    }
    pub fn report(&self) -> J {
        let mut result = json!(&self.stats);
        result["by_guard"]=json!(self.guards.iter().filter(|g|g.uses>0).map(|g|json!({"driver":g.driver,"projection":g.projection,"stages":g.uses,"skipped":g.rejected})).collect::<Vec<_>>());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_support_and_driver_epochs_survive_replacement_and_repair() {
        let mut g = crate::query::test_projection_graph();
        let mut ix = crate::query::test_projection_indexes(64);
        let mut stats = Stats::default();
        let mut guard = Guard::new(
            &GuardShape {
                driver: 0,
                projection: 0,
                consumers: vec![],
            },
            &Projection {
                table: 1,
                columns: vec![(0, Source::Cover(0)), (2, Source::Literal(Value::Int(7)))],
            },
        );
        let key = |a, b| vec![Value::Eq(a), Value::Int(b)];
        g.epoch = 1;
        g.set_function(0, key(1, 0), Value::Int(0)).unwrap();
        g.set_function(0, key(2, 0), Value::Int(0)).unwrap();
        g.set_function(1, key(1, 0), Value::Int(7)).unwrap();
        g.set_function(1, key(1, 1), Value::Int(7)).unwrap();
        guard.build(&g, &mut ix, &mut stats).unwrap();
        g.changes.clear();
        let check = |g: &Graph, guard: &Guard| {
            let mut expected = BTreeSet::new();
            for w in g.witnesses(0) {
                let row = g.row(w).unwrap().unwrap();
                if g.witnesses(1).into_iter().any(|t| {
                    let r = g.row(t).unwrap().unwrap();
                    r.values[0] == row.values[0] && r.values[2] == Value::Int(7)
                }) {
                    expected.insert((row.epoch, w));
                }
            }
            assert_eq!(guard.state.as_ref().unwrap().frontier, expected);
        };
        let publish =
            |g: &mut Graph, ix: &mut QueryIndexes, guard: &mut Guard, stats: &mut Stats| {
                g.repair().unwrap();
                ix.update(g).unwrap();
                guard.publish(g, ix, stats);
                g.changes.clear();
            };
        check(&g, &guard);
        g.epoch = 2;
        g.hide(1, &key(1, 0), true).unwrap();
        g.set_function(1, key(2, 0), Value::Int(7)).unwrap();
        publish(&mut g, &mut ix, &mut guard, &mut stats);
        check(&g, &guard);
        g.epoch = 3;
        g.set_function(1, key(1, 1), Value::Int(8)).unwrap();
        g.hide(1, &key(2, 0), true).unwrap();
        g.set_function(1, key(2, 1), Value::Int(7)).unwrap();
        g.set_function(0, key(2, 0), Value::Int(42)).unwrap();
        publish(&mut g, &mut ix, &mut guard, &mut stats);
        check(&g, &guard);
        g.epoch = 4;
        g.enqueue_union(Value::Eq(1), Value::Eq(2)).unwrap();
        publish(&mut g, &mut ix, &mut guard, &mut stats);
        check(&g, &guard);
    }
}
