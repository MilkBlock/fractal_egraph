//! Exact queries over compact, incrementally maintained virtual-fact indexes.
//! Buckets store fingerprints and references, never decoded constructor rows.
//! Hash collisions only enlarge candidate sets: the matcher checks every column.
use crate::{
    graph::{Dependency, Graph, Row},
    program::Result,
    value::Value,
};
use std::{
    collections::{BTreeSet, HashMap},
    hash::{Hash, Hasher},
    rc::Rc,
};

#[derive(Default)]
struct Bucket {
    facts: BTreeSet<Dependency>,
    view: Option<Rc<Vec<Dependency>>>,
    refinements: HashMap<Vec<(usize, u64)>, Rc<Vec<Dependency>>>,
}
struct Columns {
    values: Vec<HashMap<u64, Bucket>>,
    keys: HashMap<Dependency, Vec<u64>>,
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
            bucket.facts.remove(&fact);
            bucket.view = None;
            bucket.refinements.clear();
            if bucket.facts.is_empty() {
                self.values[col].remove(key);
            }
        }
        keys.len()
    }
    fn put(&mut self, row: Row, bits: u32, watch_reads: bool) -> (usize, usize) {
        if row.hidden {
            return (self.remove(row.witness), 0);
        }
        let keys = row
            .values
            .iter()
            .map(|v| fingerprint(v, bits))
            .collect::<Vec<_>>();
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
        let removed = self.remove(row.witness);
        let added = keys.len();
        for (col, key) in keys.iter().enumerate() {
            let bucket = self.values[col].entry(*key).or_default();
            bucket.facts.insert(row.witness);
            bucket.view = None;
            bucket.refinements.clear();
        }
        self.keys.insert(row.witness, keys);
        (removed, added)
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
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
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
    pub watch_contexts: bool,
    pub read_changes: Vec<(ReadKey, Dependency)>,
    pub read_reset: bool,
    pub watch_reads: bool,
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
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord,Hash)]
pub enum ReadKey { Table(usize), Bucket(usize,usize,u64) }
impl ReadStamp {
    pub fn key(&self)->ReadKey {match self {
        Self::Table{table,..}=>ReadKey::Table(*table),
        Self::Bucket{table,column,hash,..}=>ReadKey::Bucket(*table,*column,*hash),
    }}
}
impl Default for QueryIndexes {
    fn default() -> Self {
        Self {
            watch_contexts:false,read_changes:vec![],read_reset:false,
            watch_reads: false,
            revision: 0,
            tables: HashMap::new(),
            references: 0,
            intersection: std::env::var("CELL_CPU_JOIN_INTERSECTION").as_deref() == Ok("1"),
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
    pub fn read_key(&mut self,graph:&Graph,table:usize,bound:&[Option<Value>])->Result<ReadKey> {
        self.ensure(graph,table)?;
        let index=self.tables.get_mut(&table).unwrap();index.leased_reads=true;
        Ok(match index.best(bound,self.bits) {
            Some((column,hash,_))=>ReadKey::Bucket(table,column,hash),
            None=>ReadKey::Table(table),
        })
    }
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
        self.read_reset|=self.watch_contexts;
        self.revision += 1;
        self.tables.clear();
        self.references = 0;
    }
    pub fn invalidate(&mut self, graph: &Graph) {
        self.read_reset|=self.watch_contexts;
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
        if !graph.query_changes.is_empty() {
            self.revision += 1;
        }
        // Collapse all writes/union repairs within a Run to the final visible row.
        for (fact, table) in std::mem::take(&mut graph.query_changes) {
            if let Some(index) = self.tables.get_mut(&table) {
                if self.watch_contexts && index.leased_reads {
                    self.read_changes.push((ReadKey::Table(table),fact));
                    if let Some(keys)=index.keys.get(&fact){
                        self.read_changes.extend(keys.iter().enumerate().map(|(i,k)|(ReadKey::Bucket(table,i,*k),fact)));
                    }
                }
                let (removed, added) = if let Some(row) = graph.row(fact)? {
                    self.updated_facts += 1;
                    index.put(row, self.bits, self.watch_reads || index.leased_reads)
                } else {
                    (index.remove(fact), 0)
                };
                if self.track_changes && added > 0 {
                    self.changes.push((fact, table));
                }
                self.references = self.references - removed + added;
                if self.watch_contexts && index.leased_reads {
                    if let Some(keys)=index.keys.get(&fact){
                        self.read_changes.extend(keys.iter().enumerate().map(|(i,k)|(ReadKey::Bucket(table,i,*k),fact)));
                    }
                }
            }
        }
        for (table, index) in &mut self.tables {
            index.version = graph.query_versions[*table];
        }
        self.peak_references = self.peak_references.max(self.references);
        Ok(())
    }
    pub(crate) fn ensure(&mut self, graph: &Graph, table: usize) -> Result<()> {
        if self
            .tables
            .get(&table)
            .is_some_and(|c| c.version != graph.query_versions[table])
        {
            self.read_reset|=self.watch_contexts;
            let old = self.tables.remove(&table).unwrap();
            self.references -= old.keys.len() * old.values.len();
        }
        if !self.tables.contains_key(&table) {
            let mut index = Columns {
                values: (0..graph.program.tables[table].schema.len())
                    .map(|_| HashMap::new())
                    .collect(),
                keys: HashMap::new(),
                version: graph.query_versions[table],
                leased_reads: false,
            };
            for fact in graph.witnesses(table) {
                self.decoded_facts += 1;
                if let Some(row) = graph.row(fact)? {
                    let (_, added) = index.put(row, self.bits, self.watch_reads);
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
    pub fn refinement_storage(&self) -> (usize, usize) {
        self.tables
            .values()
            .flat_map(|t| t.values.iter())
            .flat_map(|v| v.values())
            .flat_map(|b| b.refinements.values())
            .fold((0, 0), |(n, r), v| (n + 1, r + v.len()))
    }
    pub(crate) fn hashes(&self, table: usize, w: Dependency) -> Option<&[u64]> {
        self.tables.get(&table)?.keys.get(&w).map(Vec::as_slice)
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
