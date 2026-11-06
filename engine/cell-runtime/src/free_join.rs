//! A fused Free Join stage over the Cell query indexes. Subatoms project the
//! shared columns. A probe returns a witness subset for the next stage.
use super::*;
use smallvec::SmallVec;
#[cfg(not(feature="sparse-cells"))]
use std::collections::HashMap;
#[cfg(feature="sparse-cells")]
use rustc_hash::FxHashMap as HashMap;

#[cfg(feature = "projection-survey")]
#[path = "projection_survey.rs"]
mod projection_survey;

#[cfg(feature = "projection-semijoin")]
#[path = "semijoin.rs"]
mod semijoin;

#[cfg(feature = "projection-shared")]
#[path = "shared_frontier.rs"]
mod shared_frontier;
type Rows = Rc<Vec<Dependency>>;
#[cfg(feature = "sparse-cells")]
pub(super) type Domains = SmallVec<[Subtrie;1]>;
#[cfg(not(feature = "sparse-cells"))]
pub(super) type Domains = Vec<Subtrie>;
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
    pub(super) empty_entry_batches: u64,
    membership_only: u64,
    disjoint_key_reads: u64,
    disjoint_rule_skips: u64,
    shared_column_reads: u64,
    #[cfg(feature = "projection-stamps")]
    stamp_reuses: u64,
    #[cfg(feature = "projection-stamps")]
    stamp_misses: u64,
    entry_probes: u64,
    entry_cache_hits: u64,
    entry_rejections: u64,
    entry_delta_rejections: u64,
    entry_domains: u64,
    single_domain_seeds: u64,
    entry_bucket_rows_examined: u64,
    entry_bucket_rows_materialized: u64,
    pub entry_point_uses: u64,
}

#[cfg(feature = "projection-stamps")]
struct Stamped {
    checked: u64,
    result: Option<Rows>,
    stamp: crate::query::ProjectionStamp,
}
#[cfg(feature = "projection-stamps")]
type Cached = Stamped;
#[cfg(not(feature = "projection-stamps"))]
type Cached = Option<Rows>;

pub(super) struct FreeJoin {
    #[cfg(feature = "projection-shared")]
    shared: shared_frontier::Frontiers,
    #[cfg(feature = "projection-semijoin")]
    pub gates: semijoin::Gates,
    #[cfg(feature = "projection-survey")]
    pub survey: projection_survey::Survey,
    pub enabled: bool,
    pub stats: Stats,
    plans: Vec<Vec<Vec<Subatom>>>,
    disjoint: DisjointPlan,
    expansion: Vec<Vec<Option<usize>>>,
    entry: Option<Rc<crate::entry_join::Plan>>,
    pub planned: bool,
    // A target version guards the membership of positive and negative probe results.
    by_projection: Vec<(u64, HashMap<SmallVec<[u64; 4]>, Cached>)>,
    epoch: Option<u64>,
    results: HashMap<(usize, SmallVec<[u64; 4]>), Option<Rows>>,
}

#[derive(Default)]
struct DisjointPlan {
    rules: Vec<Vec<usize>>,
    pairs: Vec<((usize, usize), (usize, usize))>,
    checked: Vec<Option<((u64, u64), bool)>>,
}
impl DisjointPlan {
    fn derive(program: &Program, plan: &crate::entry_join::Plan) -> Self {
        let mut indices = std::collections::BTreeMap::new();
        let mut result = Self::default();
        for (rid, pivots) in plan.entries.iter().enumerate() {
            let mut used = std::collections::BTreeSet::new();
            for (pivot, steps) in pivots.iter().enumerate() {
                let Call::Table(driver) = program.rules[rid].query[pivot].call else { continue };
                for step in steps {
                    let projected = &plan.projections[step.projection];
                    for (out, source) in &projected.columns {
                        let crate::entry_join::Source::Cover(input) = source else { continue };
                        let a = (driver, *input);
                        let b = (projected.table, *out);
                        if a == b { continue; }
                        let pair = (a.min(b), a.max(b));
                        let id = *indices.entry(pair).or_insert_with(|| {
                            let i = result.pairs.len();
                            result.pairs.push(pair);
                            i
                        });
                        used.insert(id);
                    }
                }
            }
            result.rules.push(used.into_iter().collect());
        }
        result.checked.resize(result.pairs.len(), None);
        result
    }
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
                .map(|_| (0, HashMap::default()))
                .collect()
        } else {
            vec![]
        };
        Self {
            #[cfg(feature = "projection-shared")]
            shared: shared_frontier::Frontiers::new(entry.as_ref().unwrap()),
            #[cfg(feature = "projection-semijoin")]
            gates: semijoin::Gates::new(entry.as_ref().unwrap()),
            #[cfg(feature = "projection-survey")]
            survey: Default::default(),
            enabled,
            entry: entry.clone(),
            planned,
            by_projection,
            disjoint: DisjointPlan::derive(p, entry.as_ref().unwrap()),
            expansion: p.kernels.iter().enumerate().map(|(r, ks)| ks.iter().map(|k| {
                let at = k.as_ref()?.steps.iter()
                    .find(|s| matches!(s.access,crate::row_kernel::Access::Expand))?.atom;
                p.physical.ranges.get(r)?.get(at)?.is_empty().then_some(at)
            }).collect()).collect(),
            epoch: None,
            results: HashMap::default(),
            plans,
            stats: Stats::default(),
        }
    }

    #[cfg(feature = "projection-shared")]
    pub fn shared_frontier(
        &mut self,
        g: &Graph,
        ix: &mut QueryIndexes,
        rid: usize,
        batch: crate::entry_schedule::Batch<'_>,
    ) -> Result<Option<Rc<Vec<Dependency>>>> {
        self.shared
            .restrict(g, ix, self.entry.as_ref().unwrap(), rid, batch)
    }
    pub fn semijoin_report(&self) -> J {
        #[cfg(feature = "projection-semijoin")]
        let gates = self.gates.report();
        #[cfg(not(feature = "projection-semijoin"))]
        let gates = J::Null;
        let mode = match (
            cfg!(feature = "projection-semijoin"),
            cfg!(feature = "projection-stamps"),
        ) {
            (true, true) => "both",
            (false, true) => "precise",
            (true, false) => "group",
            (false, false) => "off",
        };
        #[cfg(feature = "projection-shared")]
        {
            let label = if mode == "off" {
                "shared".to_owned()
            } else {
                format!("{mode}+shared")
            };
            return json!({"mode":label,"gates":gates,"shared":self.shared.stats});
        }
        #[cfg(not(feature = "projection-shared"))]
        json!({"mode":mode,"gates":gates})
    }
    pub fn entry_mode(&self) -> bool {
        self.entry.is_some()
    }
    pub(crate) fn disjoint(&mut self, rid: usize, graph: &Graph, ix: &QueryIndexes) -> bool {
        let plan = &mut self.disjoint;
        for pos in 0..plan.rules[rid].len() {
            let id = plan.rules[rid][pos];
            let (a,b) = plan.pairs[id];
            let version = (graph.query_versions[a.0], graph.query_versions[b.0]);
            let absent = match plan.checked[id] {
                Some((v, result)) if v == version => result,
                _ => {
                    let result = ix.disjoint_small(a,b,version,&mut self.stats.disjoint_key_reads);
                    plan.checked[id] = Some((version,result));
                    result
                }
            };
            if absent {
                self.stats.disjoint_rule_skips += 1;
                plan.rules[rid].swap(0,pos);
                return true;
            }
        }
        false
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
        index.ensure_selected(g,table,[to])?;
        index.ensure_selected(g,batch.table,[from])?;
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
        #[cfg(feature = "family-overlap")] overlap: (&mut crate::family_overlap::Overlap, u64),
    ) -> Result<Option<Domains>> {
        let plan = self.entry.as_ref().expect("entry lane");
        let steps = &plan.entries[rid][atom];
        if steps.is_empty() {
            return Ok(Some(Domains::new()));
        }
        if !self.planned && self.epoch != Some(g.epoch) {
            self.results.clear();
            self.epoch = Some(g.epoch);
        }
        self.stats.entry_rows += 1;
        let mut domains = Domains::new();
        for step in steps {
            #[cfg(feature = "family-overlap")]
            let timer = (Instant::now(), self.stats.entry_probes);
            let shape = &plan.projections[step.projection];
            #[cfg(not(feature="sparse-cells"))]
            let hashes = index.hashes(row.table, row.witness);
            let key = shape
                .columns
                .iter()
                .map(|(_, source)| match source {
                    crate::entry_join::Source::Literal(v) => {
                        index.fingerprint(&canon(v, &g.parents))
                    }
                    crate::entry_join::Source::Cover(col) => {
                        #[cfg(feature="sparse-cells")]
                        {
                            let value=&row.values[*col];
                            match value {
                                Value::Eq(_) | Value::Int(_) => index.fingerprint(value),
                                _ => index.hashes(row.table,row.witness).map_or_else(||index.fingerprint(value),|h|h[*col]),
                            }
                        }
                        #[cfg(not(feature="sparse-cells"))]
                        { hashes.map_or_else(|| index.fingerprint(&row.values[*col]), |h| h[*col]) }
                    }
                })
                .collect::<SmallVec<[_; 4]>>();
            #[cfg(all(feature = "sparse-cells", not(any(feature = "projection-survey",
                feature = "projection-stamps", feature = "projection-semijoin", feature = "family-overlap",
                feature = "family-frontiers"))))]
            if self.planned && self.expansion[rid][atom].is_some_and(|expanded| expanded != step.atom)
                && let [(column, _)] = shape.columns.as_slice() {
                // A later complete-key register read needs support and delta
                // ownership here, never a materialized expansion subtrie.
                self.stats.membership_only += 1;
                self.stats.entry_probes += 1;
                match index.single_support(g,shape.table,*column,key[0])? {
                    crate::query::Domain::Empty => {
                        self.stats.entry_rejections += 1;
                        return Ok(None);
                    }
                    crate::query::Domain::One(w) if g.visible_epoch(w).is_some_and(|epoch| {
                        (epoch,w) > (row.epoch,row.witness)
                            || ((epoch,w) == (row.epoch,row.witness) && step.atom < atom)
                    }) => {
                        self.stats.entry_delta_rejections += 1;
                        return Ok(None);
                    }
                    _ => continue,
                }
            }
            let probe = |stats: &mut Stats,
                         selectors_key: &[u64],
                         index: &mut QueryIndexes|
             -> Result<Option<Rows>> {
                index.ensure_selected(g,shape.table,shape.columns.iter().map(|(col,_)|*col))?;
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
            #[cfg(feature = "projection-semijoin")]
            let sample = if std::ptr::eq(step, &steps[0]) {
                self.gates.sample(g.epoch, rid, atom, &key)
            } else {
                None
            };
            #[cfg(feature = "projection-survey")]
            let survey_key = key.clone();
            let direct;
            let result: &Option<Rows> = if cfg!(feature = "sparse-cells")
                && !cfg!(feature = "projection-stamps") && self.planned && shape.columns.len() == 1 {
                self.stats.shared_column_reads += 1;
                self.stats.entry_probes += 1;
                direct = index.single_projection(g, shape.table, shape.columns[0].0, key[0],
                    &mut self.stats.entry_bucket_rows_materialized)?;
                &direct
            } else if self.planned {
                let (version, cache) = &mut self.by_projection[step.projection];
                #[cfg(not(feature = "projection-stamps"))]
                if *version != g.query_versions[shape.table] {
                    cache.clear();
                    *version = g.query_versions[shape.table];
                }
                if cache.len()
                    > g.witness_count(row.table)
                        .saturating_add(g.witness_count(shape.table))
                        .max(64)
                {
                    cache.clear();
                    self.stats.cache_capacity_resets += 1;
                }
                #[cfg(not(feature = "projection-stamps"))]
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
                #[cfg(feature = "projection-stamps")]
                {
                    *version = g.query_versions[shape.table];
                    let entry = cache.entry(key);
                    let fresh = match &entry {
                        std::collections::hash_map::Entry::Occupied(e) => {
                            let hit = e.get();
                            if hit.checked == *version {
                                true
                            } else if index.valid_projection(g, shape.table, &hit.stamp) {
                                self.stats.stamp_reuses += 1;
                                true
                            } else {
                                self.stats.stamp_misses += 1;
                                false
                            }
                        }
                        _ => false,
                    };
                    if fresh {
                        self.stats.entry_cache_hits += 1;
                        let std::collections::hash_map::Entry::Occupied(e) = entry else {
                            unreachable!()
                        };
                        let hit = e.into_mut();
                        hit.checked = *version;
                        &hit.result
                    } else {
                        let result = probe(&mut self.stats, entry.key(), index)?;
                        let selectors: SmallVec<[(usize, u64); 4]> = shape
                            .columns
                            .iter()
                            .zip(entry.key())
                            .map(|((c, _), k)| (*c, *k))
                            .collect();
                        let stamp = index.projection_stamp(shape.table, &selectors);
                        &entry
                            .insert_entry(Stamped {
                                result,
                                stamp,
                                checked: *version,
                            })
                            .into_mut()
                            .result
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
            #[cfg(feature = "projection-survey")]
            self.survey.observe(
                g.epoch,
                rid,
                atom,
                step.projection,
                g.query_versions[shape.table],
                survey_key,
                result,
            );
            #[cfg(feature = "projection-semijoin")]
            if let Some(id) = sample {
                self.gates.finish_sample(id, result.is_some());
            }
            #[cfg(feature = "family-overlap")]
            overlap.0.read(
                overlap.1,
                rid,
                row.table,
                step.projection,
                step.atom < atom,
                row.witness,
                self.stats.entry_probes != timer.1,
                timer.0.elapsed().as_nanos() as u64,
                result,
            );
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
        self.stats.single_domain_seeds += u64::from(domains.len()==1);
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
                if env.values.get(slot).is_some() {
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
                index.ensure_selected(g,sub.table,selectors.iter().map(|(col,_)|*col))?;
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
        let mut positions: HashMap<_, _> = HashMap::default();
        let mut groups = SmallVec::<[Group; 2]>::new();
        // If the prefix and the current bindings already determine the cover's
        // functional key, there is no exact group reuse to amortize a hash map.
        let cache = rows.len() > 1
            && query[cover].args[..query[cover].args.len() - 1]
                .iter()
                .enumerate()
                .any(|(col, a)| {
                    matches!(a,Arg::Slot(slot) if env.values.get(*slot).is_none())
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
