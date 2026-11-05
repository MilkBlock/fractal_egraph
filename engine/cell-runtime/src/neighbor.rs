//! Demand-driven context for attributes read on a constructor's children.
//! Only uncovered delta entries reach these caches, inside the ordinary kernel.
use crate::{
    graph::{Dependency, Graph, Row},
    program::*,
    value::Value,
};
use rustc_hash::FxHashMap;
use serde::Serialize;
use std::{rc::Rc, time::Instant};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Plan {
    pub structure: usize,
    pub attribute: usize,
    pub children: Vec<(usize, usize)>, // (LHS atom, constructor column)
    pub pivots: Vec<usize>,
}

pub(crate) fn derive(p: &Program) -> Vec<Option<Plan>> {
    p.rules
        .iter()
        .enumerate()
        .map(|(rid, r)| {
            let write = p.writes[rid].as_ref()?;
            let [Arg::Slot(parent)] = write.key.as_slice() else {
                return None;
            };
            let mut constructors = r
                .query
                .iter()
                .enumerate()
                .filter(|(_, a)| matches!(a.call,Call::Table(t) if p.tables[t].constructor));
            let (structure, a) = constructors.next()?;
            let (result, fields) = a.args.split_last()?;
            if constructors.next().is_some() || result != &Arg::Slot(*parent) || !r.seminaive {
                return None;
            }
            let mut children = vec![];
            for (i, atom) in r.query.iter().enumerate() {
                if i == structure {
                    continue;
                }
                if let Call::Table(t) = atom.call {
                    let [key @ Arg::Slot(_), _] = atom.args.as_slice() else {
                        return None;
                    };
                    if t != write.table || p.tables[t].schema[0]["kind"] != "eq" {
                        return None;
                    }
                    children.push((i, fields.iter().position(|arg| arg == key)?));
                }
            }
            // Each expansion must enumerate this constructor through a bound expression.
            let pivots = p.kernels[rid]
                .iter()
                .enumerate()
                .filter_map(|(pivot, k)| {
                    let k = k.as_ref()?;
                    k.steps
                        .iter()
                        .all(|s| {
                            !matches!(s.access, crate::row_kernel::Access::Expand)
                                || s.atom == structure
                        })
                        .then_some(pivot)
                })
                .collect::<Vec<_>>();
            (!children.is_empty() && !pivots.is_empty()).then_some(Plan {
                pivots,
                structure,
                attribute: write.table,
                children,
            })
        })
        .collect()
}

#[derive(Default, Serialize)]
pub(crate) struct Stats {
    pub spine_entries: u64,
    pub residual_entries: u64,
    pub seeds: u64,
    pub bindings: u64,
    pub point_hits: u64,
    pub point_probes: u64,
    pub expansion_visits: u64,
    pub group_hits: u64,
    pub cached_parent_visits: u64,
    pub group_builds: u64,
    pub build_visits: u64,
    pub owner_skips: u64,
    pub maintenance_ns: u64,
    resets: u64,
}
#[derive(Default)]
struct Ports {
    version: u64,
    rows: FxHashMap<u64, Option<Row>>,
    keys: FxHashMap<usize, u64>,
}
#[derive(Default, Serialize)]
struct Publication {
    checked: u64,
    refreshed: u64,
    retired: u64,
    cleared: u64,
    nanoseconds: u64,
}
struct Neighborhood {
    // Holding the original domain prevents allocator address reuse.
    _domain: Rc<Vec<Dependency>>,
    rows: Rc<Vec<Row>>,
}
#[derive(Default)]
struct Groups {
    version: u64,
    rows: usize,
    by_domain: FxHashMap<usize, Neighborhood>,
}
// Storage is demand bounded per shared input table, independent of root-table size.
const KEY_LIMIT: usize = 4096;
const ROW_LIMIT: usize = 65536;
pub(crate) struct Context {
    ports: Vec<Option<Ports>>,
    subscribed: Vec<usize>,
    published: Publication,
    groups: FxHashMap<usize, Groups>,
    stats: Vec<Stats>,
    profiling: bool,
}
impl Context {
    pub fn new(p: &Program, profiling: bool) -> Self {
        Self {
            ports: (0..p.tables.len()).map(|_| None).collect(),
            subscribed: vec![],
            published: Publication::default(),
            groups: FxHashMap::default(),
            stats: (0..p.rules.len()).map(|_| Stats::default()).collect(),
            profiling,
        }
    }
    pub fn stats(&mut self, rid: usize) -> &mut Stats {
        &mut self.stats[rid]
    }
    pub fn report(&self, p: &Program) -> serde_json::Value {
        let rules = p
            .neighbor_plans
            .iter()
            .enumerate()
            .filter_map(|(r, plan)| {
                Some((
                    r,
                    serde_json::json!({"plan":plan.as_ref()?,"stats":self.stats[r]}),
                ))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let demanded_keys: usize = self
            .ports
            .iter()
            .flatten()
            .map(|cache| cache.rows.len())
            .sum();
        let parent_refs: usize = self.groups.values().map(|cache| cache.rows).sum();
        serde_json::json!({"demanded_keys":demanded_keys,"parent_refs":parent_refs,"mode":"neighbors","publication":self.published,"timed_maintenance":self.profiling,"rules":rules})
    }
    pub fn point(
        &mut self,
        g: &mut Graph,
        rid: usize,
        t: usize,
        key: &[Value],
    ) -> Result<Option<Row>> {
        let [Value::Eq(id)] = key else {
            return Err("expected compiler-proved expression key".into());
        };
        let cache = self.ports[t].get_or_insert_with(|| {
            self.subscribed.push(t);
            Ports {
                version: g.query_versions[t],
                ..Default::default()
            }
        });
        let stats = &mut self.stats[rid];
        if cache.version == g.query_versions[t]
            && let Some(row) = cache.rows.get(id)
        {
            stats.point_hits += 1;
            return Ok(row.clone());
        }
        let clock = self.profiling.then(Instant::now);
        if cache.version != g.query_versions[t] || cache.rows.len() >= ROW_LIMIT {
            stats.resets += 1;
            cache.version = g.query_versions[t];
            cache.rows.clear();
            cache.keys.clear();
        }
        stats.point_probes += 1;
        g.stats.residual_candidates += 1;
        let row = g.lookup_canonical(t, key)?;
        if let Some(Row {
            witness: Dependency::Function(i),
            ..
        }) = &row
        {
            cache.keys.insert(*i, *id);
        }
        cache.rows.insert(*id, row.clone());
        if let Some(clock) = clock {
            stats.maintenance_ns += clock.elapsed().as_nanos() as u64;
        }
        Ok(row)
    }
    pub(crate) fn observes(&self, t: usize) -> bool {
        self.ports[t]
            .as_ref()
            .is_some_and(|cache| !cache.rows.is_empty())
    }
    /// A demanded but absent key must see an insertion. Replacement and
    /// retirement can arrive in either order after same-key coalescence.
    pub fn changed(&mut self, t: usize, fact: Dependency, row: Option<&Row>) {
        let Some(cache) = self.ports[t].as_mut() else {
            return;
        };
        let Dependency::Function(i) = fact else {
            return;
        };
        let clock = self.profiling.then(Instant::now);
        self.published.checked += 1;
        let current =
            row.filter(|r| r.table == t && !r.hidden)
                .and_then(|r| match r.values.first() {
                    Some(Value::Eq(k)) => Some((*k, r)),
                    _ => None,
                });
        if let Some(&old) = cache.keys.get(&i)
            && current.is_none_or(|(k, _)| k != old)
        {
            cache.keys.remove(&i);
            if let Some(slot) = cache.rows.get_mut(&old)
                && slot.as_ref().is_some_and(|r| r.witness == fact)
            {
                *slot = None;
                self.published.retired += 1;
            }
        }
        if let Some((k, row)) = current
            && let Some(slot) = cache.rows.get_mut(&k)
        {
            *slot = Some(row.clone());
            cache.keys.insert(i, k);
            self.published.refreshed += 1;
        }
        if let Some(clock) = clock {
            self.published.nanoseconds += clock.elapsed().as_nanos() as u64;
        }
    }
    pub fn finish_publication(&mut self, g: &Graph) {
        if self.subscribed.is_empty() {
            return;
        }
        let clock = self.profiling.then(Instant::now);
        for &t in &self.subscribed {
            let cache = self.ports[t].as_mut().unwrap();
            cache.version = g.query_versions[t];
        }
        if let Some(clock) = clock {
            self.published.nanoseconds += clock.elapsed().as_nanos() as u64;
        }
    }
    pub fn neighborhood(
        &mut self,
        g: &Graph,
        rid: usize,
        t: usize,
        domain: &Rc<Vec<Dependency>>,
    ) -> Access {
        if domain.len() < 2 || domain.len() > ROW_LIMIT {
            return Access::Direct;
        }
        let cache = self.groups.entry(t).or_default();
        let stats = &mut self.stats[rid];
        let version = g.query_versions[t];
        let id = Rc::as_ptr(domain) as usize;
        if version == cache.version
            && let Some(hit) = cache.by_domain.get(&id)
        {
            stats.group_hits += 1;
            return Access::Cached(hit.rows.clone());
        }
        let clock = self.profiling.then(Instant::now);
        if cache.version != version
            || cache.rows + domain.len() > ROW_LIMIT
            || cache.by_domain.len() >= KEY_LIMIT
        {
            stats.resets += 1;
            cache.by_domain.clear();
            cache.rows = 0;
            cache.version = version;
        }
        let rows = Vec::with_capacity(domain.len());
        if let Some(clock) = clock {
            stats.maintenance_ns += clock.elapsed().as_nanos() as u64;
        }
        Access::Build(rows)
    }
    pub fn finish_group(
        &mut self,
        rid: usize,
        table: usize,
        domain: Rc<Vec<Dependency>>,
        mut rows: Vec<Row>,
    ) {
        let clock = self.profiling.then(Instant::now);
        rows.sort_unstable_by_key(|r| (r.epoch, r.witness));
        let cache = self.groups.get_mut(&table).unwrap();
        cache.rows += domain.len();
        self.stats[rid].group_builds += 1;
        self.stats[rid].build_visits += domain.len() as u64;
        cache.by_domain.insert(
            Rc::as_ptr(&domain) as usize,
            Neighborhood {
                _domain: domain,
                rows: Rc::new(rows),
            },
        );
        if let Some(clock) = clock {
            self.stats[rid].maintenance_ns += clock.elapsed().as_nanos() as u64;
        }
    }
}
pub(crate) enum Access {
    Direct,
    Build(Vec<Row>),
    Cached(Rc<Vec<Row>>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_and_retirement_commute_for_a_subscribed_key() {
        let original = Row {
            table: 0,
            values: Rc::new(vec![Value::Eq(7), Value::Int(1)]),
            epoch: 1,
            witness: Dependency::Function(3),
            hidden: false,
        };
        let replacement = Row {
            values: Rc::new(vec![Value::Eq(7), Value::Int(9)]),
            epoch: 2,
            witness: Dependency::Function(4),
            ..original.clone()
        };
        for insert_first in [false, true] {
            let mut cache = Ports::default();
            cache.rows.insert(7, Some(original.clone()));
            cache.keys.insert(3, 7);
            let mut ctx = Context {
                ports: vec![Some(cache)],
                subscribed: vec![0],
                groups: Default::default(),
                published: Default::default(),
                stats: vec![],
                profiling: false,
            };
            if insert_first {
                ctx.changed(0, replacement.witness, Some(&replacement));
            }
            ctx.changed(0, original.witness, None);
            if !insert_first {
                ctx.changed(0, replacement.witness, Some(&replacement));
            }
            let ports = ctx.ports[0].as_ref().unwrap();
            assert_eq!(
                ports.rows[&7].as_ref().unwrap().witness,
                replacement.witness
            );
            assert_eq!(ports.keys.len(), 1);
            assert_eq!(ports.keys[&4], 7);
        }
    }
}
