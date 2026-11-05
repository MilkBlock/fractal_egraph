//! Run-local semijoin frontiers. Two consumers may share only the same checked
//! projection, entry node and watermark on the same sealed graph.
use super::*;
use crate::entry_join::{Plan, Source};
use rustc_hash::FxHashMap;
type Key = SmallVec<[u64; 4]>;
type FrontierKey = (usize, u64, usize);

enum Frontier {
    Seen,
    Deferred,
    Rows(Rc<Vec<Dependency>>),
}
#[derive(Default, serde::Serialize)]
pub(crate) struct Stats {
    builds: u64,
    reuses: u64,
    stopped: u64,
    examined: u64,
    distinct_keys: u64,
    supported_key_probes: u64,
    support_candidates: u64,
    source_rows: u64,
    delivered_rows: u64,
}
pub(crate) struct Frontiers {
    projection: Vec<Vec<Option<usize>>>,
    epoch: Option<u64>,
    cached: FxHashMap<FrontierKey, Frontier>,
    retained: usize,
    pub stats: Stats,
}
impl Frontiers {
    pub fn new(plan: &Plan) -> Self {
        let mut projection: Vec<_> = plan.entries.iter().map(|v| vec![None; v.len()]).collect();
        for shape in plan
            .guards
            .iter()
            .flatten()
            .filter(|s| s.consumers.len() >= 3)
        {
            for &(r, a) in &shape.consumers {
                projection[r][a] = Some(shape.projection);
            }
        }
        Self {
            projection,
            epoch: None,
            cached: Default::default(),
            retained: 0,
            stats: Default::default(),
        }
    }
    pub fn restrict(
        &mut self,
        g: &Graph,
        ix: &mut QueryIndexes,
        plan: &Plan,
        rid: usize,
        batch: crate::entry_schedule::Batch<'_>,
    ) -> Result<Option<Rc<Vec<Dependency>>>> {
        let Some(p) = self.projection[rid][batch.atom()] else {
            return Ok(None);
        };
        if self.epoch != Some(g.epoch) {
            self.cached.clear();
            self.retained = 0;
            self.epoch = Some(g.epoch);
        }
        let id = (batch.node(), batch.watermark(), p);
        match self.cached.get(&id) {
            None => {
                self.cached.insert(id, Frontier::Seen);
                return Ok(None);
            }
            Some(Frontier::Deferred) => return Ok(None),
            Some(Frontier::Rows(rows)) => {
                self.stats.reuses += 1;
                self.stats.source_rows += batch.delta_len().unwrap_or(0) as u64;
                self.stats.delivered_rows += rows.len() as u64;
                return Ok(Some(rows.clone()));
            }
            Some(Frontier::Seen) => {}
        }
        let delta = batch.delta_len().unwrap_or(0);
        self.cached.insert(id, Frontier::Deferred);
        if !(32..=65536).contains(&delta) || self.retained >= 65536 {
            return Ok(None);
        }
        let shape = &plan.projections[p];
        ix.ensure(g, shape.table)?;
        let mut groups = FxHashMap::<Key, bool>::default();
        let mut rows = Vec::new();
        for &(_, w) in batch.delta() {
            self.stats.examined += 1;
            let row = &batch.packet(w).expect("sealed frontier").row;
            let hashes = ix.hashes(row.table, w);
            let key: Key = shape
                .columns
                .iter()
                .map(|(_, src)| match src {
                    Source::Cover(c) => {
                        hashes.map_or_else(|| ix.fingerprint(&row.values[*c]), |h| h[*c])
                    }
                    Source::Literal(v) => ix.fingerprint(v),
                })
                .collect();
            let visible = match groups.entry(key) {
                std::collections::hash_map::Entry::Occupied(e) => *e.get(),
                std::collections::hash_map::Entry::Vacant(e) => {
                    let selectors: SmallVec<[(usize, u64); 4]> = shape
                        .columns
                        .iter()
                        .zip(e.key())
                        .map(|((c, _), h)| (*c, *h))
                        .collect();
                    self.stats.distinct_keys += 1;
                    let hit =
                        ix.supports(shape.table, &selectors, &mut self.stats.support_candidates);
                    self.stats.supported_key_probes += u64::from(hit);
                    *e.insert(hit)
                }
            };
            if visible {
                rows.push(w);
            }
            if groups.len() + rows.len() > delta / 2 || self.retained + rows.len() > 65536 {
                self.stats.stopped += 1;
                return Ok(None);
            }
        }
        self.stats.builds += 1;
        self.stats.source_rows += delta as u64;
        self.stats.delivered_rows += rows.len() as u64;
        self.retained += rows.len();
        let rows = Rc::new(rows);
        self.cached.insert(id, Frontier::Rows(rows.clone()));
        Ok(Some(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn correlated_columns_require_one_common_witness_under_both_fingerprint_widths() {
        for bits in [0, 64] {
            let mut g = crate::query::test_projection_graph();
            let mut ix = crate::query::test_projection_indexes(bits);
            g.set_function(0, vec![Value::Int(1), Value::Int(0)], Value::Int(7))
                .unwrap();
            g.set_function(0, vec![Value::Int(2), Value::Int(0)], Value::Int(8))
                .unwrap();
            ix.ensure(&g, 0).unwrap();
            for result in [7, 8] {
                let keys = [
                    (0, ix.fingerprint(&Value::Int(1))),
                    (2, ix.fingerprint(&Value::Int(result))),
                ];
                let supported = ix.supports(0, &keys, &mut 0);
                assert_eq!(supported, ix.probe(0, &keys, &mut 0, &mut 0).is_some());
                assert_eq!(supported, bits == 0 || result == 7);
            }
        }
    }
}
