//! Demand-built exact indexes for equality prefixes and an ordered i64 column.
//! The same post-repair change set as the regular query index maintains them.
use crate::{
    graph::{Dependency, Graph, Row},
    physical_plan::Bound,
    program::Result,
    value::Value,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ops::Bound::{Excluded, Included, Unbounded},
    rc::Rc,
    time::Instant,
};

pub(crate) const MIN_DOMAIN: usize = 32;
pub(crate) const MIN_COMPUTED_DOMAIN: usize = 8;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub(crate) enum Column {
    Raw(usize),
    Sum(usize, usize),
}
impl Column {
    fn read(self, row: &Row) -> Result<Option<i64>> {
        let int = |i: usize| match row.values[i] {
            Value::Int(v) => Ok(v),
            _ => Err("non-integer index column".to_owned()),
        };
        match self {
            Self::Raw(i) => Ok(Some(int(i)?)),
            Self::Sum(a, b) => Ok(int(a)?.checked_add(int(b)?)),
        }
    }
}
#[derive(Default)]
struct Axis {
    keys: HashMap<Dependency, (Value, i64)>,
    buckets: HashMap<Value, BTreeMap<i64, BTreeSet<Dependency>>>,
}
impl Axis {
    fn update(
        &mut self,
        w: Dependency,
        row: Option<&Row>,
        prefix: usize,
        column: Column,
    ) -> Result<bool> {
        let new = if let Some(row) = row.filter(|r| !r.hidden) {
            column.read(row)?.map(|v| (&row.values[prefix], v))
        } else {
            None
        };
        if self.keys.get(&w).map(|(k, v)| (k, *v)) == new {
            return Ok(false);
        }
        if let Some((key, value)) = self.keys.remove(&w) {
            let bucket = self.buckets.get_mut(&key).expect("indexed prefix");
            let refs = bucket.get_mut(&value).expect("indexed value");
            refs.remove(&w);
            if refs.is_empty() {
                bucket.remove(&value);
            }
            if bucket.is_empty() {
                self.buckets.remove(&key);
            }
        }
        if let Some((key, value)) = new {
            self.buckets
                .entry(key.clone())
                .or_default()
                .entry(value)
                .or_default()
                .insert(w);
            self.keys.insert(w, (key.clone(), value));
        }
        Ok(true)
    }
}
#[derive(Default)]
struct TableRanges {
    version: u64,
    axes: HashMap<(usize, Column), Axis>,
}
#[derive(Default, Serialize)]
pub(crate) struct Stats {
    pub uses: u64,
    pub hits: u64,
    pub builds: u64,
    pub build_rows: u64,
    pub input_rows: u64,
    pub output_rows: u64,
    pub empty_domains: u64,
    pub mixed_prefixes: u64,
    pub partial_domains: u64,
    pub partial_input_rows: u64,
    pub partial_output_rows: u64,
    pub update_rows: u64,
    pub changed_entries: u64,
    pub subset_checks: u64,
    pub range_refs: u64,
    pub broad_fallbacks: u64,
    pub invalidated_axes: u64,
    pub maintenance_ns: u64,
}
#[derive(Default)]
pub(crate) struct RangeIndex {
    tables: HashMap<usize, TableRanges>,
    pub stats: Stats,
}
impl RangeIndex {
    pub fn update(&mut self, g: &Graph) -> Result<()> {
        if self.tables.is_empty() || g.query_changes.is_empty() {
            return Ok(());
        }
        let clock = Instant::now();
        for (&w, &t) in &g.query_changes {
            let Some(table) = self.tables.get_mut(&t) else {
                continue;
            };
            let row = g.row(w)?;
            self.stats.update_rows += 1;
            for (&(prefix, column), axis) in &mut table.axes {
                self.stats.changed_entries +=
                    u64::from(axis.update(w, row.as_ref(), prefix, column)?);
            }
            table.version = g.query_versions[t];
        }
        self.stats.maintenance_ns += clock.elapsed().as_nanos() as u64;
        Ok(())
    }
    pub fn select(
        &mut self,
        g: &Graph,
        table: usize,
        prefix: (usize, Value),
        column: usize,
        bound: Bound,
        threshold: i64,
        rows: &Rc<Vec<Dependency>>,
    ) -> Result<Rc<Vec<Dependency>>> {
        self.select_axis(
            g,
            table,
            prefix,
            Column::Raw(column),
            Some(bound),
            threshold,
            rows,
        )
    }
    pub fn select_axis(
        &mut self,
        g: &Graph,
        table: usize,
        prefix: (usize, Value),
        column: Column,
        comparison: Option<Bound>,
        threshold: i64,
        rows: &Rc<Vec<Dependency>>,
    ) -> Result<Rc<Vec<Dependency>>> {
        let cached = self.tables.entry(table).or_default();
        if cached.version != g.query_versions[table] {
            self.stats.invalidated_axes += cached.axes.len() as u64;
            cached.axes.clear();
            cached.version = g.query_versions[table];
        }
        let axis = match cached.axes.entry((prefix.0, column)) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                self.stats.hits += 1;
                entry.into_mut()
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let clock = Instant::now();
                let mut axis = Axis::default();
                for w in g.witnesses(table) {
                    self.stats.build_rows += 1;
                    axis.update(w, g.row(w)?.as_ref(), prefix.0, column)?;
                }
                self.stats.builds += 1;
                let view = entry.insert(axis);
                self.stats.maintenance_ns += clock.elapsed().as_nanos() as u64;
                view
            }
        };
        self.stats.uses += 1;
        self.stats.input_rows += rows.len() as u64;
        let Some(bucket) = axis.buckets.get(&prefix.1) else {
            self.stats.empty_domains += 1;
            return Ok(Rc::new(vec![]));
        };
        let Some(bound) = comparison else {
            let Some(exact) = bucket.get(&threshold) else {
                self.stats.empty_domains += 1;
                return Ok(Rc::new(vec![]));
            };
            // The bound prefix guarantees an ordered witness domain from either
            // QueryIndexes::candidates or Free Join's column projection probe.
            let result: Vec<_> = if exact.len() < rows.len() {
                self.stats.subset_checks += exact.len() as u64;
                exact
                    .iter()
                    .copied()
                    .filter(|w| rows.binary_search(w).is_ok())
                    .collect()
            } else {
                self.stats.subset_checks += rows.len() as u64;
                rows.iter().copied().filter(|w| exact.contains(w)).collect()
            };
            self.stats.output_rows += result.len() as u64;
            return Ok(Rc::new(result));
        };
        let lo = *bucket.first_key_value().expect("nonempty prefix").0;
        let hi = *bucket.last_key_value().unwrap().0;
        let accepts = |v: i64| match bound {
            Bound::Lt => v < threshold,
            Bound::Le => v <= threshold,
            Bound::Gt => v > threshold,
            Bound::Ge => v >= threshold,
        };
        if !accepts(lo) && !accepts(hi) {
            self.stats.empty_domains += 1;
            return Ok(Rc::new(vec![]));
        }
        if accepts(lo) && accepts(hi) {
            self.stats.output_rows += rows.len() as u64;
            return Ok(rows.clone());
        }
        self.stats.mixed_prefixes += 1;
        let bounds = match bound {
            Bound::Lt => (Unbounded, Excluded(threshold)),
            Bound::Le => (Unbounded, Included(threshold)),
            Bound::Gt => (Excluded(threshold), Unbounded),
            Bound::Ge => (Included(threshold), Unbounded),
        };
        let mut survivors = HashSet::new();
        for (_, refs) in bucket.range(bounds) {
            // The existing subtrie can be much smaller than the full prefix.
            // Avoid collecting a broad range just to refine a small domain.
            if refs.len() > (rows.len() / 2).saturating_sub(survivors.len()) {
                self.stats.broad_fallbacks += 1;
                self.stats.output_rows += rows.len() as u64;
                return Ok(rows.clone());
            }
            survivors.extend(refs.iter().copied());
            self.stats.range_refs += refs.len() as u64;
        }
        self.stats.subset_checks += rows.len() as u64;
        // Free Join may already supply a narrower subtrie. Intersect and retain
        // its original witness order, including when fingerprints collide.
        let narrowed: Vec<_> = rows
            .iter()
            .copied()
            .filter(|w| survivors.contains(w))
            .collect();
        if narrowed.is_empty() {
            self.stats.empty_domains += 1;
        } else if narrowed.len() < rows.len() {
            self.stats.partial_domains += 1;
            self.stats.partial_input_rows += rows.len() as u64;
            self.stats.partial_output_rows += narrowed.len() as u64;
        }
        self.stats.output_rows += narrowed.len() as u64;
        Ok(Rc::new(narrowed))
    }
}
