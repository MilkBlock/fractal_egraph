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
}
struct Columns {
    values: Vec<HashMap<u64, Bucket>>,
    keys: HashMap<Dependency, Vec<u64>>,
    version: u64,
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
            if bucket.facts.is_empty() {
                self.values[col].remove(key);
            }
        }
        keys.len()
    }
    fn put(&mut self, row: Row, bits: u32) -> (usize, usize) {
        if row.hidden {
            return (self.remove(row.witness), 0);
        }
        let keys = row
            .values
            .iter()
            .map(|v| fingerprint(v, bits))
            .collect::<Vec<_>>();
        if self.keys.get(&row.witness) == Some(&keys) {
            return (0, 0);
        }
        let removed = self.remove(row.witness);
        let added = keys.len();
        for (col, key) in keys.iter().enumerate() {
            let bucket = self.values[col].entry(*key).or_default();
            bucket.facts.insert(row.witness);
            bucket.view = None;
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
pub struct QueryIndexes {
    tables: HashMap<usize, Columns>,
    references: usize,
    bits: u32,
    pub peak_references: usize,
    pub decoded_facts: u64,
    pub updated_facts: u64,
    pub builds: u64,
}
impl Default for QueryIndexes {
    fn default() -> Self {
        Self {
            tables: HashMap::new(),
            references: 0,
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
    pub fn clear(&mut self) {
        self.tables.clear();
        self.references = 0;
    }
    pub fn invalidate(&mut self, graph: &Graph) {
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
        // Collapse all writes/union repairs within a Run to the final visible row.
        for (fact, table) in std::mem::take(&mut graph.query_changes) {
            if let Some(index) = self.tables.get_mut(&table) {
                let (removed, added) = if let Some(row) = graph.row(fact)? {
                    self.updated_facts += 1;
                    index.put(row, self.bits)
                } else {
                    (index.remove(fact), 0)
                };
                self.references = self.references - removed + added;
            }
        }
        for (table, index) in &mut self.tables {
            index.version = graph.query_versions[*table];
        }
        self.peak_references = self.peak_references.max(self.references);
        Ok(())
    }
    fn ensure(&mut self, graph: &Graph, table: usize) -> Result<()> {
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
                values: (0..graph.program.tables[table].schema.len())
                    .map(|_| HashMap::new())
                    .collect(),
                keys: HashMap::new(),
                version: graph.query_versions[table],
            };
            for fact in graph.witnesses(table) {
                self.decoded_facts += 1;
                if let Some(row) = graph.row(fact)? {
                    let (_, added) = index.put(row, self.bits);
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
        Ok(bucket
            .view
            .get_or_insert_with(|| Rc::new(bucket.facts.iter().copied().collect()))
            .clone())
    }
}
