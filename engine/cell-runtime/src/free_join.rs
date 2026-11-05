//! A fused Free Join stage over the Cell query indexes. Subatoms project the
//! shared columns. A probe returns a witness subset for the next stage.
use super::*;
use smallvec::SmallVec;
use std::collections::HashMap;

type Rows = Rc<Vec<Dependency>>;
pub(super) struct Subtrie {
    pub atom: usize,
    pub rows: Rows,
    pub projected_columns: usize,
}
pub(super) struct Group {
    pub cover: SmallVec<[Dependency; 2]>,
    pub probes: SmallVec<[Subtrie; 2]>,
}

#[derive(Default, serde::Serialize)]
pub(super) struct Stats {
    stages: u64,
    cover_rows: u64,
    key_groups: u64,
    probes: u64,
    bucket_rows_examined: u64,
    bucket_rows_materialized: u64,
    rejected_cover_rows: u64,
    pub subtrie_uses: u64,
    reverse_stages: u64,
    reverse_keys: u64,
    reverse_refs: u64,
    reverse_epoch_bins: u64,
    reverse_empty: u64,
    cache_capacity_resets: u64,
    entry_rows: u64,
    entry_probes: u64,
    entry_cache_hits: u64,
    entry_rejections: u64,
    entry_delta_rejections: u64,
    entry_domains: u64,
    entry_bucket_rows_examined: u64,
    entry_bucket_rows_materialized: u64,
    pub entry_point_uses: u64,
}

pub(super) struct FreeJoin {
    pub enabled: bool,
    pub stats: Stats,
    plans: Vec<Vec<Vec<Subatom>>>,
    entry: Option<Rc<crate::entry_join::Plan>>,
    pub planned: bool,
    // A target version guards the membership of positive and negative probe results.
    by_projection: Vec<(u64, HashMap<SmallVec<[u64; 4]>, Option<Rows>>)>,
    epoch: Option<u64>,
    results: HashMap<(usize, SmallVec<[u64; 4]>), Option<Rows>>,
}

struct Subatom {
    atom: usize,
    table: usize,
    // (shared slot, column in the follower, column in the cover)
    links: Vec<(usize, usize, usize)>,
}
struct Probe {
    atom: usize,
    table: usize,
    selectors: SmallVec<[(usize, u64); 4]>,
    projection: SmallVec<[(usize, usize); 4]>,
}

fn subatoms(query: &[Atom], cover: usize) -> Vec<Subatom> {
    if !matches!(query[cover].call, Call::Table(_)) {
        return Vec::new();
    }
    let mut probes = Vec::new();
    for (atom, follower) in query.iter().enumerate() {
        let Call::Table(table) = follower.call else {
            continue;
        };
        if atom == cover {
            continue;
        }
        let links = follower
            .args
            .iter()
            .enumerate()
            .filter_map(|(col, arg)| {
                let Arg::Slot(slot) = arg else { return None };
                query[cover]
                    .args
                    .iter()
                    .position(|v| v == arg)
                    .map(|source| (*slot, col, source))
            })
            .collect::<Vec<_>>();
        if !links.is_empty() {
            probes.push(Subatom { atom, table, links });
        }
    }
    probes
}

impl FreeJoin {
    pub fn new(p: &Program) -> Self {
        let enabled = true;
        let planned = true;
        let plans = p
            .rules
            .iter()
            .map(|r| (0..r.query.len()).map(|i| subatoms(&r.query, i)).collect())
            .collect();
        let entry = Some(
            p.entry_join
                .clone()
                .unwrap_or_else(|| Rc::new(crate::entry_join::Plan::derive(p))),
        );
        let by_projection = if planned {
            (0..entry.as_ref().unwrap().projections.len())
                .map(|_| (0, HashMap::new()))
                .collect()
        } else {
            vec![]
        };
        Self {
            enabled,
            entry,
            planned,
            by_projection,
            epoch: None,
            results: HashMap::new(),
            plans,
            stats: Stats::default(),
        }
    }

    pub fn entry_mode(&self) -> bool {
        self.entry.is_some()
    }
    pub fn has_entry(&self, rid: usize) -> bool {
        self.entry
            .as_ref()
            .is_some_and(|p| p.entries[rid].iter().any(|steps| !steps.is_empty()))
    }
    pub fn reverse(
        &mut self,
        g: &Graph,
        index: &mut QueryIndexes,
        rid: usize,
        batch: crate::entry_schedule::Batch<'_>,
    ) -> Result<Option<Vec<Dependency>>> {
        let plan = self.entry.as_ref().expect("planned entry");
        let steps = &plan.entries[rid][batch.atom()];
        let mut best = None;
        for step in steps {
            let shape = &plan.projections[step.projection];
            let [(to, crate::entry_join::Source::Cover(from))] = shape.columns.as_slice() else {
                continue;
            };
            let size = index
                .key_count(shape.table, *to)
                .unwrap_or_else(|| g.witness_count(shape.table));
            if best.is_none_or(|(n, _, _, _)| size < n) {
                best = Some((size, shape.table, *to, *from))
            }
        }
        let Some((size, table, to, from)) = best else {
            return Ok(None);
        };
        // Bound any speculative iteration by the small-side size. Require
        // at least four delta rows per enumerated key before inverting.
        let threshold = size.saturating_mul(4).max(8);
        if threshold >= batch.len() {
            return Ok(None);
        }
        if !batch.delta_exceeds(threshold, &mut self.stats.reverse_epoch_bins) {
            return Ok(None);
        }
        index.ensure(g, table)?;
        index.ensure(g, batch.table)?;
        let Some(rows) = index.reverse_cover(
            (batch.table, from),
            (table, to),
            threshold,
            &mut self.stats.reverse_keys,
        ) else {
            return Ok(None);
        };
        self.stats.reverse_stages += 1;
        self.stats.reverse_refs += rows.len() as u64;
        let mut candidates = rows
            .into_iter()
            .filter_map(|w| batch.packet(w).map(|p| (p.row.epoch, w)))
            .collect::<Vec<_>>();
        candidates.sort_unstable();
        if candidates.is_empty() {
            self.stats.reverse_empty += 1;
        }
        Ok(Some(candidates.into_iter().map(|(_, w)| w).collect()))
    }
    pub fn admit(
        &mut self,
        g: &Graph,
        index: &mut QueryIndexes,
        rid: usize,
        atom: usize,
        row: &Row,
    ) -> Result<Option<Vec<Subtrie>>> {
        let plan = self.entry.as_ref().expect("entry lane");
        let steps = &plan.entries[rid][atom];
        if steps.is_empty() {
            return Ok(Some(vec![]));
        }
        if !self.planned && self.epoch != Some(g.epoch) {
            self.results.clear();
            self.epoch = Some(g.epoch);
        }
        self.stats.entry_rows += 1;
        let mut domains = vec![];
        for step in steps {
            let shape = &plan.projections[step.projection];
            let hashes = index.hashes(row.table, row.witness);
            let key = shape
                .columns
                .iter()
                .map(|(_, source)| match source {
                    crate::entry_join::Source::Literal(v) => {
                        index.fingerprint(&canon(v, &g.parents))
                    }
                    crate::entry_join::Source::Cover(col) => {
                        hashes.map_or_else(|| index.fingerprint(&row.values[*col]), |h| h[*col])
                    }
                })
                .collect::<SmallVec<[_; 4]>>();
            let probe = |stats: &mut Stats,
                         selectors_key: &[u64],
                         index: &mut QueryIndexes|
             -> Result<Option<Rows>> {
                index.ensure(g, shape.table)?;
                let selectors = shape
                    .columns
                    .iter()
                    .zip(selectors_key)
                    .map(|((col, _), hash)| (*col, *hash))
                    .collect::<SmallVec<[_; 4]>>();
                stats.entry_probes += 1;
                Ok(index.probe(
                    shape.table,
                    &selectors,
                    &mut stats.entry_bucket_rows_examined,
                    &mut stats.entry_bucket_rows_materialized,
                ))
            };
            let result = if self.planned {
                let (version, cache) = &mut self.by_projection[step.projection];
                if *version != g.query_versions[shape.table] {
                    cache.clear();
                    *version = g.query_versions[shape.table];
                } else if cache.len()
                    > g.witness_count(row.table)
                        .saturating_add(g.witness_count(shape.table))
                        .max(64)
                {
                    cache.clear();
                    self.stats.cache_capacity_resets += 1;
                }
                match cache.entry(key) {
                    std::collections::hash_map::Entry::Occupied(h) => {
                        self.stats.entry_cache_hits += 1;
                        h.into_mut()
                    }
                    std::collections::hash_map::Entry::Vacant(m) => {
                        let v = probe(&mut self.stats, m.key(), index)?;
                        m.insert(v)
                    }
                }
            } else {
                match self.results.entry((step.projection, key)) {
                    std::collections::hash_map::Entry::Occupied(h) => {
                        self.stats.entry_cache_hits += 1;
                        h.into_mut()
                    }
                    std::collections::hash_map::Entry::Vacant(m) => {
                        let v = probe(&mut self.stats, &m.key().1, index)?;
                        m.insert(v)
                    }
                }
            };
            let Some(rows) = result else {
                self.stats.entry_rejections += 1;
                return Ok(None);
            };
            // Membership can survive a Run. Semi-naive ownership still uses the current epoch.
            if rows.len() == 1 {
                let w = rows[0];
                if let Some(epoch) = g.visible_epoch(w) {
                    if (epoch, w) > (row.epoch, row.witness)
                        || ((epoch, w) == (row.epoch, row.witness) && step.atom < atom)
                    {
                        self.stats.entry_delta_rejections += 1;
                        return Ok(None);
                    }
                }
            }
            domains.push(Subtrie {
                atom: step.atom,
                rows: rows.clone(),
                projected_columns: shape.columns.len(),
            });
        }
        self.stats.entry_domains += domains.len() as u64;
        Ok(Some(domains))
    }

    pub fn partition(
        &mut self,
        g: &Graph,
        index: &mut QueryIndexes,
        rid: usize,
        query: &[Atom],
        cover: usize,
        remaining: &[bool],
        env: &QueryEnv,
        rows: &[Dependency],
    ) -> Result<Option<SmallVec<[Group; 2]>>> {
        if rows.is_empty() {
            return Ok(None);
        }
        let Call::Table(table) = query[cover].call else {
            return Ok(None);
        };
        let mut columns = SmallVec::<[usize; 4]>::new();
        let mut probes = SmallVec::<[Probe; 2]>::new();
        for sub in &self.plans[rid][cover] {
            if !remaining[sub.atom] {
                continue;
            }
            let mut projection = SmallVec::new();
            for &(slot, col, pos) in &sub.links {
                if env.values[slot].is_some() {
                    continue;
                }
                let key_col = if let Some(i) = columns.iter().position(|&c| c == pos) {
                    i
                } else {
                    columns.push(pos);
                    columns.len() - 1
                };
                projection.push((col, key_col));
            }
            if !projection.is_empty() {
                let mut selectors = query[sub.atom]
                    .args
                    .iter()
                    .enumerate()
                    .filter_map(|(col, arg)| {
                        operand(arg, env, g).map(|v| (col, index.fingerprint(&v)))
                    })
                    .collect::<SmallVec<[_; 4]>>();
                for (col, _) in &mut projection {
                    let source_col = *col;
                    *col = selectors.len();
                    selectors.push((source_col, 0));
                }
                index.ensure(g, sub.table)?;
                probes.push(Probe {
                    atom: sub.atom,
                    table: sub.table,
                    selectors,
                    projection,
                });
            }
        }
        if probes.is_empty() {
            return Ok(None);
        }
        index.ensure(g, table)?;
        self.stats.stages += 1;
        self.stats.cover_rows += rows.len() as u64;
        let mut positions = HashMap::new();
        let mut groups = SmallVec::<[Group; 2]>::new();
        // If the prefix and the current bindings already determine the cover's
        // functional key, there is no exact group reuse to amortize a hash map.
        let cache = rows.len() > 1
            && query[cover].args[..query[cover].args.len() - 1]
                .iter()
                .enumerate()
                .any(|(col, a)| {
                    matches!(a,Arg::Slot(slot) if env.values[*slot].is_none())
                        && !columns.contains(&col)
                });
        for &row in rows {
            // The cover may come from an ordinary table scan, including hidden
            // rows. Only visible rows have a current query-index entry.
            let Some(values) = index.hashes(table, row) else {
                continue;
            };
            let key = columns
                .iter()
                .map(|&col| values[col])
                .collect::<SmallVec<[_; 4]>>();
            let group = if let Some(cached) = cache.then(|| positions.get(&key).copied()).flatten()
            {
                cached
            } else {
                self.stats.key_groups += 1;
                let mut children = SmallVec::new();
                for probe in &mut probes {
                    for &(pos, i) in &probe.projection {
                        probe.selectors[pos].1 = key[i];
                    }
                    self.stats.probes += 1;
                    let Some(candidates) = index.probe(
                        probe.table,
                        &probe.selectors,
                        &mut self.stats.bucket_rows_examined,
                        &mut self.stats.bucket_rows_materialized,
                    ) else {
                        break;
                    };
                    children.push(Subtrie {
                        atom: probe.atom,
                        rows: candidates,
                        projected_columns: probe.selectors.len(),
                    });
                }
                let result = if children.len() == probes.len() {
                    let i = groups.len();
                    groups.push(Group {
                        cover: SmallVec::new(),
                        probes: children,
                    });
                    Some(i)
                } else {
                    None
                };
                if cache {
                    positions.insert(key, result);
                }
                result
            };
            if let Some(i) = group {
                groups[i].cover.push(row)
            } else {
                self.stats.rejected_cover_rows += 1;
            }
        }
        Ok(Some(groups))
    }
}
