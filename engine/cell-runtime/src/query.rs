//! Exact queries over compact, incrementally maintained virtual-fact indexes.
//! Buckets store fingerprints and references, never decoded constructor rows.
//! Hash collisions only enlarge candidate sets: the matcher checks every column.
use crate::{
    graph::{Dependency, Graph, Row},
    program::Result,
    value::Value,
};
use rustc_hash::FxHashMap as HashMap;
use std::{
    collections::BTreeSet,
    hash::{Hash, Hasher},
    rc::Rc,
};

#[derive(Default)]
struct Bucket {
    #[cfg(feature = "projection-stamps")]
    membership_version: u64,
    facts: BTreeSet<Dependency>,
    view: Option<Rc<Vec<Dependency>>>,
    refinements: HashMap<Vec<(usize, u64)>, Rc<Vec<Dependency>>>,
}
struct Columns {
    #[cfg(feature = "projection-stamps")]
    write_version: u64,
    values: Vec<HashMap<u64, Bucket>>,
    keys: HashMap<Dependency, smallvec::SmallVec<[u64; 4]>>,
    version: u64,
    leased_reads: bool,
}
impl Columns {
    fn remove(&mut self, fact: Dependency) -> usize {
        let Some(keys) = self.keys.remove(&fact) else {
            return 0;
        };
        for (col, key) in keys.iter().enumerate() {
            let bucket = self.values[col].get_mut(key).unwrap();
            #[cfg(feature = "projection-stamps")]
            {
                bucket.membership_version = self.write_version;
            }
            bucket.facts.remove(&fact);
            bucket.view = None;
            bucket.refinements.clear();
            if bucket.facts.is_empty() {
                self.values[col].remove(key);
            }
        }
        keys.len()
    }
    fn put(&mut self, row: &Row, bits: u32, watch_reads: bool) -> (usize, usize) {
        if row.hidden {
            return (self.remove(row.witness), 0);
        }
        let keys = row
            .values
            .iter()
            .map(|v| fingerprint(v, bits))
            .collect::<smallvec::SmallVec<[u64; 4]>>();
        if self.keys.get(&row.witness) == Some(&keys) {
            // Equal fingerprints do not prove unchanged exact values (including
            // forced collisions). Every changed witness invalidates read leases.
            if watch_reads {
                for (col, key) in keys.iter().enumerate() {
                    let bucket = self.values[col].get_mut(key).unwrap();
                    bucket.view = None;
                    bucket.refinements.clear();
                }
            }
            return (0, 0);
        }
        let old = self.keys.insert(row.witness, keys.clone());
        let mut changed = 0;
        for (col, &key) in keys.iter().enumerate() {
            if let Some(prior) = old.as_ref() {
                let before = prior[col];
                if before == key {
                    let bucket = self.values[col].get_mut(&key).unwrap();
                    #[cfg(feature = "projection-stamps")]
                    {
                        bucket.membership_version = self.write_version;
                    }
                    bucket.refinements.clear();
                    if watch_reads {
                        bucket.view = None;
                    }
                    continue;
                }
                let bucket = self.values[col].get_mut(&before).unwrap();
                #[cfg(feature = "projection-stamps")]
                {
                    bucket.membership_version = self.write_version;
                }
                bucket.facts.remove(&row.witness);
                bucket.view = None;
                bucket.refinements.clear();
                if bucket.facts.is_empty() {
                    self.values[col].remove(&before);
                }
            }
            let bucket = self.values[col].entry(key).or_default();
            #[cfg(feature = "projection-stamps")]
            {
                bucket.membership_version = self.write_version;
            }
            bucket.facts.insert(row.witness);
            bucket.view = None;
            bucket.refinements.clear();
            changed += 1;
        }
        (if old.is_some() { changed } else { 0 }, changed)
    }
    fn best(&self, bound: &[Option<Value>], bits: u32) -> Option<(usize, u64, usize)> {
        let mut best = None;
        for (col, v) in bound.iter().enumerate() {
            if let Some(v) = v {
                let key = fingerprint(v, bits);
                let size = self.values[col].get(&key).map_or(0, |b| b.facts.len());
                if best.is_none_or(|(_, _, n)| size < n) {
                    best = Some((col, key, size));
                }
            }
        }
        best
    }
}
fn fingerprint(value: &Value, bits: u32) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    value.hash(&mut hasher);
    let hash = hasher.finish();
    if bits == 64 {
        hash
    } else {
        hash & ((1u64 << bits) - 1)
    }
}
#[derive(Default, serde::Serialize)]
pub struct RefinementStats {
    pub requests: u64,
    pub hits: u64,
    pub examined: u64,
    pub avoided: u64,
}
pub struct QueryIndexes {
    pub revision: u64,
    tables: HashMap<usize, Columns>,
    references: usize,
    bits: u32,
    intersection: bool,
    pub refinements: RefinementStats,
    pub changes: Vec<(Dependency, usize)>,
    pub track_changes: bool,
    pub peak_references: usize,
    pub decoded_facts: u64,
    pub updated_facts: u64,
    pub builds: u64,
}
pub(crate) struct ColumnWriter<'a> {
    table: usize,
    index: &'a mut Columns,
    bits: u32,
    references: &'a mut usize,
    updated: &'a mut u64,
    changes: Option<&'a mut Vec<(Dependency, usize)>>,
}
impl ColumnWriter<'_> {
    pub fn finish(self, version: u64) {
        self.index.version = version;
    }

    pub fn update(&mut self, witness: Dependency, row: Option<&Row>) {
        let (removed, added) = if let Some(row) = row {
            *self.updated += 1;
            self.index.put(row, self.bits, self.index.leased_reads)
        } else {
            (self.index.remove(witness), 0)
        };
        if added > 0
            && let Some(changes) = &mut self.changes
        {
            changes.push((witness, self.table));
        }
        *self.references = *self.references - removed + added;
    }
}
#[cfg(feature = "projection-stamps")]
pub(crate) struct ProjectionStamp {
    column: usize,
    hash: u64,
    // Absence is an independently verifiable negative certificate.
    version: Option<u64>,
}
pub enum Domain {
    Empty,
    One(Dependency),
    Many,
}
/// A lease on a complete candidate superset. Empty buckets need no tombstones:
/// they stay valid exactly while the current index still has no such bucket.
#[derive(Clone)]
pub enum ReadStamp {
    Bucket {
        table: usize,
        column: usize,
        hash: u64,
        view: Option<Rc<Vec<Dependency>>>,
    },
    Table {
        table: usize,
        version: u64,
    },
}

impl Default for QueryIndexes {
    fn default() -> Self {
        Self {
            revision: 0,
            tables: HashMap::default(),
            references: 0,
            intersection: true,
            refinements: RefinementStats::default(),
            changes: vec![],
            track_changes: false,
            bits: std::env::var("CELL_CPU_HASH_BITS")
                .ok()
                .map(|v| v.parse::<u32>().expect("hash bits").min(64))
                .unwrap_or(64),
            peak_references: 0,
            decoded_facts: 0,
            updated_facts: 0,
            builds: 0,
        }
    }
}
impl QueryIndexes {
    /// Subscription to a candidate superset without pinning historical bucket
    /// vectors. Contexts consume old+new row notifications at each publication.

    pub fn read_stamp(
        &mut self,
        graph: &Graph,
        table: usize,
        bound: &[Option<Value>],
    ) -> Result<ReadStamp> {
        self.ensure(graph, table)?;
        let index = self.tables.get_mut(&table).unwrap();
        // This table supplies exact read evidence. Do not impose its lease
        // invalidation work on unrelated indexes used only for ordinary joins.
        index.leased_reads = true;
        if let Some((column, hash, _)) = index.best(bound, self.bits) {
            let view = index.values[column].get_mut(&hash).map(|bucket| {
                bucket
                    .view
                    .get_or_insert_with(|| Rc::new(bucket.facts.iter().copied().collect()))
                    .clone()
            });
            Ok(ReadStamp::Bucket {
                table,
                column,
                hash,
                view,
            })
        } else {
            Ok(ReadStamp::Table {
                table,
                version: index.version,
            })
        }
    }
    #[cfg(feature = "projection-stamps")]
    pub(crate) fn projection_stamp(
        &self,
        table: usize,
        selectors: &[(usize, u64)],
    ) -> ProjectionStamp {
        let index = &self.tables[&table];
        let &(column, hash) = selectors
            .iter()
            .min_by_key(|(c, k)| index.values[*c].get(k).map_or(0, |b| b.facts.len()))
            .unwrap();
        ProjectionStamp {
            column,
            hash,
            version: index.values[column]
                .get(&hash)
                .map(|b| b.membership_version),
        }
    }
    #[cfg(feature = "projection-stamps")]
    pub(crate) fn valid_projection(&self, g: &Graph, t: usize, s: &ProjectionStamp) -> bool {
        self.tables.get(&t).is_some_and(|i| {
            i.version == g.query_versions[t]
                && i.values[s.column]
                    .get(&s.hash)
                    .map(|b| b.membership_version)
                    == s.version
        })
    }
    pub fn valid_read(&self, graph: &Graph, stamp: &ReadStamp) -> bool {
        let table = match stamp {
            ReadStamp::Bucket { table, .. } | ReadStamp::Table { table, .. } => *table,
        };
        let Some(index) = self.tables.get(&table) else {
            return false;
        };
        if index.version != graph.query_versions[table] {
            return false;
        }
        match stamp {
            ReadStamp::Table { version, .. } => index.version == *version,
            ReadStamp::Bucket {
                column, hash, view, ..
            } => match (view, index.values[*column].get(hash)) {
                (None, None) => true,
                (Some(old), Some(bucket)) => {
                    bucket.view.as_ref().is_some_and(|now| Rc::ptr_eq(old, now))
                }
                _ => false,
            },
        }
    }
    pub fn domain(
        &mut self,
        graph: &Graph,
        table: usize,
        bound: &[Option<Value>],
    ) -> Result<Domain> {
        self.ensure(graph, table)?;
        let index = &self.tables[&table];
        if let Some((col, key, count)) = index.best(bound, self.bits) {
            Ok(match count {
                0 => Domain::Empty,
                1 => Domain::One(*index.values[col][&key].facts.first().unwrap()),
                _ => Domain::Many,
            })
        } else {
            Ok(match index.keys.len() {
                0 => Domain::Empty,
                1 => Domain::One(*index.keys.keys().next().unwrap()),
                _ => Domain::Many,
            })
        }
    }
    pub fn clear(&mut self) {
        self.revision += 1;
        self.tables.clear();
        self.references = 0;
    }
    pub fn invalidate(&mut self, graph: &Graph) {
        self.revision += 1;
        self.tables.retain(|table, index| {
            if index.version != graph.query_versions[*table] {
                self.references -= index.keys.len() * index.values.len();
                false
            } else {
                true
            }
        });
    }
    pub fn update(&mut self, graph: &mut Graph) -> Result<()> {
        self.begin_publication(graph);
        for (table, changes) in graph.changes.tables() {
            if let Some(mut writer) = self.writer(table, graph.query_versions[table]) {
                for (&w, change) in changes {
                    let row = change.row(graph, w)?;
                    writer.update(w, row.as_ref());
                }
                writer.finish(graph.query_versions[table]);
            }
        }
        self.finish_publication();
        Ok(())
    }

    pub(crate) fn begin_publication(&mut self, g: &Graph) {
        self.revision += u64::from(!g.changes.is_empty());
    }

    pub(crate) fn writer(&mut self, table: usize, _version: u64) -> Option<ColumnWriter<'_>> {
        let index = self.tables.get_mut(&table)?;
        #[cfg(feature = "projection-stamps")]
        {
            index.write_version = _version;
        }
        Some(ColumnWriter {
            table,
            index,
            bits: self.bits,
            references: &mut self.references,
            updated: &mut self.updated_facts,
            changes: self.track_changes.then_some(&mut self.changes),
        })
    }

    pub(crate) fn finish_publication(&mut self) {
        self.peak_references = self.peak_references.max(self.references);
    }
    pub(crate) fn ensure(&mut self, graph: &Graph, table: usize) -> Result<()> {
        if self
            .tables
            .get(&table)
            .is_some_and(|c| c.version != graph.query_versions[table])
        {
            let old = self.tables.remove(&table).unwrap();
            self.references -= old.keys.len() * old.values.len();
        }
        if !self.tables.contains_key(&table) {
            let mut index = Columns {
                #[cfg(feature = "projection-stamps")]
                write_version: graph.query_versions[table],
                values: (0..graph.program.tables[table].schema.len())
                    .map(|_| HashMap::default())
                    .collect(),
                keys: HashMap::default(),
                version: graph.query_versions[table],
                leased_reads: false,
            };
            for fact in graph.witnesses(table) {
                self.decoded_facts += 1;
                if let Some(row) = graph.row(fact)? {
                    let (_, added) = index.put(&row, self.bits, false);
                    self.references += added;
                }
            }
            self.tables.insert(table, index);
            self.builds += 1;
            self.peak_references = self.peak_references.max(self.references);
        }
        Ok(())
    }
    pub fn count(&mut self, graph: &Graph, table: usize, bound: &[Option<Value>]) -> Result<usize> {
        if !bound.iter().any(Option::is_some) {
            return Ok(graph.witness_count(table));
        }
        self.ensure(graph, table)?;
        Ok(self.tables[&table].best(bound, self.bits).unwrap().2)
    }
    pub fn candidates(
        &mut self,
        graph: &Graph,
        table: usize,
        bound: &[Option<Value>],
    ) -> Result<Rc<Vec<Dependency>>> {
        if !bound.iter().any(Option::is_some) {
            return Ok(Rc::new(graph.witnesses(table)));
        }
        self.ensure(graph, table)?;
        let index = self.tables.get_mut(&table).unwrap();
        let (col, key, size) = index.best(bound, self.bits).unwrap();
        if size == 0 {
            return Ok(Rc::new(vec![]));
        }
        let bucket = index.values[col].get_mut(&key).unwrap();
        let view = bucket
            .view
            .get_or_insert_with(|| Rc::new(bucket.facts.iter().copied().collect()))
            .clone();
        if !self.intersection {
            return Ok(view);
        }
        let selectors = bound
            .iter()
            .enumerate()
            .filter_map(|(c, v)| {
                if c == col {
                    None
                } else {
                    v.as_ref().map(|v| (c, fingerprint(v, self.bits)))
                }
            })
            .collect::<Vec<_>>();
        if selectors.is_empty() {
            return Ok(view);
        }
        self.refinements.requests += 1;
        if let Some(hit) = bucket.refinements.get(&selectors) {
            self.refinements.hits += 1;
            self.refinements.avoided += (view.len() - hit.len()) as u64;
            return Ok(hit.clone());
        }
        self.refinements.examined += view.len() as u64;
        let result = Rc::new(
            view.iter()
                .copied()
                .filter(|w| selectors.iter().all(|(c, k)| index.keys[w][*c] == *k))
                .collect::<Vec<_>>(),
        );
        self.refinements.avoided += (view.len() - result.len()) as u64;
        // Cache only nonempty proper partitions, not a history of missing keys.
        // Any changed tuple removes/reinserts its witness in ALL its buckets.
        if !result.is_empty() && result.len() < view.len() {
            bucket.refinements.insert(selectors, result.clone());
        }
        Ok(result)
    }
    pub(crate) fn fingerprint(&self, value: &Value) -> u64 {
        fingerprint(value, self.bits)
    }
    /// Probe a subatom by its full projected prefix. Fingerprint collisions
    /// only enlarge the domain; the executor verifies every actual row.
    pub(crate) fn probe(
        &mut self,
        table: usize,
        selectors: &[(usize, u64)],
        examined: &mut u64,
        materialized: &mut u64,
    ) -> Option<Rc<Vec<Dependency>>> {
        let index = self.tables.get_mut(&table).expect("prepared probe index");
        let mut best = None;
        for &(col, key) in selectors {
            let size = index.values[col].get(&key)?.facts.len();
            if best.is_none_or(|(_, _, n)| size < n) {
                best = Some((col, key, size));
            }
        }
        let (col, key, size) = best.expect("nonempty subatom");
        let bucket = index.values[col].get_mut(&key).unwrap();
        if selectors.len() > 1 {
            let matches = |w: &Dependency| selectors.iter().all(|&(c, k)| index.keys[w][c] == k);
            *examined += size as u64;
            if size == 1 {
                if !matches(bucket.facts.first().unwrap()) {
                    return None;
                }
            } else {
                let subset = bucket
                    .facts
                    .iter()
                    .copied()
                    .filter(matches)
                    .collect::<Vec<_>>();
                return (!subset.is_empty()).then(|| Rc::new(subset));
            }
        }
        Some(
            bucket
                .view
                .get_or_insert_with(|| {
                    *materialized += size as u64;
                    Rc::new(bucket.facts.iter().copied().collect())
                })
                .clone(),
        )
    }
    #[cfg(feature = "projection-shared")]
    pub(crate) fn supports(
        &self,
        t: usize,
        selectors: &[(usize, u64)],
        examined: &mut u64,
    ) -> bool {
        let index = &self.tables[&t];
        let Some(&(col, hash)) = selectors
            .iter()
            .min_by_key(|(c, h)| index.values[*c].get(h).map_or(0, |b| b.facts.len()))
        else {
            return false;
        };
        let Some(bucket) = index.values[col].get(&hash) else {
            return false;
        };
        selectors.len() == 1
            || bucket.facts.iter().any(|w| {
                *examined += 1;
                selectors.iter().all(|(c, h)| index.keys[w][*c] == *h)
            })
    }
    pub(crate) fn key_count(&self, table: usize, col: usize) -> Option<usize> {
        Some(self.tables.get(&table)?.values[col].len())
    }
    /// Enumerate follower keys and their matching cover references. Abort
    /// before copying a bucket that would exceed the forward-scan budget.
    pub(crate) fn reverse_cover(
        &self,
        driver: (usize, usize),
        follower: (usize, usize),
        budget: usize,
        keys: &mut u64,
    ) -> Option<Vec<Dependency>> {
        let drivers = &self.tables.get(&driver.0)?.values[driver.1];
        let followers = &self.tables.get(&follower.0)?.values[follower.1];
        let limit = budget.checked_sub(followers.len())?;
        let mut hits = vec![];
        let mut size = 0;
        for key in followers.keys() {
            *keys += 1;
            if let Some(bucket) = drivers.get(key) {
                size += bucket.facts.len();
                if size > limit {
                    return None;
                }
                hits.push(bucket);
            }
        }
        let mut rows = Vec::with_capacity(size);
        for bucket in hits {
            rows.extend(&bucket.facts);
        }
        Some(rows)
    }
    pub fn refinement_storage(&self) -> (usize, usize) {
        self.tables
            .values()
            .flat_map(|t| t.values.iter())
            .flat_map(|v| v.values())
            .flat_map(|b| b.refinements.values())
            .fold((0, 0), |(n, r), v| (n + 1, r + v.len()))
    }
    pub(crate) fn hashes(&self, table: usize, w: Dependency) -> Option<&[u64]> {
        self.tables.get(&table)?.keys.get(&w).map(|k| k.as_slice())
    }
    pub(crate) fn indexed_witnesses(&self, table: usize) -> Vec<Dependency> {
        let mut out = self.tables[&table].keys.keys().copied().collect::<Vec<_>>();
        out.sort();
        out
    }
    pub(crate) fn hash_candidates(
        &self,
        table: usize,
        cols: &[usize],
        keys: &[u64],
    ) -> Vec<Dependency> {
        let index = &self.tables[&table];
        let Some((c, k)) = cols
            .iter()
            .zip(keys)
            .min_by_key(|(c, k)| index.values[**c].get(k).map_or(0, |b| b.facts.len()))
        else {
            return vec![];
        };
        let Some(bucket) = index.values[*c].get(k) else {
            return vec![];
        };
        bucket
            .facts
            .iter()
            .copied()
            .filter(|w| cols.iter().zip(keys).all(|(c, k)| index.keys[w][*c] == *k))
            .collect()
    }
}

#[cfg(all(
    test,
    any(
        feature = "projection-semijoin",
        feature = "projection-stamps",
        feature = "projection-shared"
    )
))]
pub(crate) fn test_projection_indexes(bits: u32) -> QueryIndexes {
    QueryIndexes {
        bits,
        ..Default::default()
    }
}

#[cfg(all(
    test,
    any(
        feature = "projection-semijoin",
        feature = "projection-stamps",
        feature = "projection-shared"
    )
))]
pub(crate) fn test_projection_graph() -> Graph {
    let tables: Vec<_> = ["left", "right"]
        .iter()
        .map(|name| {
            serde_json::json!({
                "name":name,"schema":["i64","i64","i64"],"constructor":false,"virtual_fact":false,
                "internal":false,"merge":{"var":"new"}
            })
        })
        .collect();
    let p=serde_json::from_value(serde_json::json!({
        "multi_stage":crate::multistage::MultiStage::default(),
        "entry_schedule":crate::entry_schedule::Plan::default(),"tables":tables,"templates":[],"rules":[],
        "sets":{},"control":[],"initial":[],"table_templates":[],"artifact":{}
    })).unwrap();
    let mut graph = Graph::new(p).unwrap();
    graph.merge_programs_enabled = false; // This fixture exercises storage updates without a compiled RHS.
    graph
}

#[cfg(all(test, feature = "projection-stamps"))]
mod stamp_tests {
    use super::*;
    #[test]
    fn stamp_survives_unrelated_updates_and_rechecks_empty_or_moved_rows() {
        let mut g = test_projection_graph();
        let mut ix = test_projection_indexes(64);
        let key = |a, b| vec![Value::Int(a), Value::Int(b)];
        g.set_function(0, key(1, 0), Value::Int(7)).unwrap();
        ix.ensure(&g, 0).unwrap();
        g.changes.clear();
        let h = |x| fingerprint(&Value::Int(x), ix.bits);
        let needle = [(0, h(1)), (2, h(7))];
        let absent = [(0, h(3))];
        let hit = ix.projection_stamp(0, &needle);
        let miss = ix.projection_stamp(0, &absent);
        g.set_function(0, key(2, 0), Value::Int(6)).unwrap();
        ix.update(&mut g).unwrap();
        g.changes.clear();
        assert!(ix.valid_projection(&g, 0, &hit));
        assert!(ix.valid_projection(&g, 0, &miss));
        g.set_function(0, key(1, 0), Value::Int(8)).unwrap();
        g.set_function(0, key(3, 0), Value::Int(7)).unwrap();
        ix.update(&mut g).unwrap();
        assert!(!ix.valid_projection(&g, 0, &hit));
        assert!(!ix.valid_projection(&g, 0, &miss));
        assert!(ix.probe(0, &needle, &mut 0, &mut 0).is_none());
    }
}
