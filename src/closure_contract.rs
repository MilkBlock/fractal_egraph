//! Versioned evidence leases. Valid evidence is not permission to substitute tier0.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Resource {
    Token(usize),
    Table(String),
    Scope,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Change {
    pub boundary: usize,
    pub tokens: Vec<usize>,
    pub tables: Vec<String>,
    pub unions: Vec<(usize, usize)>,
    pub reset: bool,
}

/// Preconditions of one isolated, completed closure. `empty_frontier` retains
/// absent premises, including rules that did not match during Bake.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalDependencies {
    pub tables: BTreeSet<String>,
    pub empty_frontier: BTreeSet<String>,
    pub rulesets: BTreeSet<String>,
}
impl LocalDependencies {
    /// All equality identities of the concrete instance must be supplied,
    /// including internal/non-root identities, not only its public inputs.
    /// Reset the scope on a rule/merge/native-runtime/schedule-policy change.
    pub fn resources(
        &self,
        instance_tokens: impl IntoIterator<Item = usize>,
    ) -> BTreeSet<Resource> {
        self.tables
            .iter()
            .chain(&self.empty_frontier)
            .cloned()
            .map(Resource::Table)
            .chain(instance_tokens.into_iter().map(Resource::Token))
            .chain(std::iter::once(Resource::Scope))
            .collect()
    }
}
#[derive(Default)]
pub struct Registry {
    versions: BTreeMap<Resource, u64>,
    parents: BTreeMap<usize, usize>,
    sizes: BTreeMap<usize, usize>,
    subscribers: BTreeMap<Resource, BTreeSet<usize>>,
    leases: BTreeMap<usize, Vec<(Resource, u64)>>,
    tick: u64,
    pub notifications: usize,
    pub wakes: usize,
}
impl Registry {
    pub fn watch_local(
        &mut self,
        id: usize,
        dependencies: &LocalDependencies,
        instance_tokens: impl IntoIterator<Item = usize>,
    ) {
        self.watch(id, dependencies.resources(instance_tokens));
    }
    fn root(&self, mut v: usize) -> usize {
        while let Some(p) = self.parents.get(&v) {
            if *p == v {
                break;
            }
            v = *p;
        }
        v
    }
    fn canonical(&self, r: &Resource) -> Resource {
        match r {
            Resource::Token(v) => Resource::Token(self.root(*v)),
            r => r.clone(),
        }
    }
    pub fn watch(&mut self, id: usize, resources: impl IntoIterator<Item = Resource>) {
        assert!(
            !self.leases.contains_key(&id),
            "leases are immutable; new evidence needs a new unit"
        );
        let mut rs: BTreeSet<_> = resources.into_iter().map(|r| self.canonical(&r)).collect();
        rs.insert(Resource::Scope);
        let lease = rs
            .into_iter()
            .map(|r| {
                self.subscribers.entry(r.clone()).or_default().insert(id);
                let v = *self.versions.get(&r).unwrap_or(&0);
                (r, v)
            })
            .collect();
        self.leases.insert(id, lease);
    }
    pub fn watched(&self, id: usize) -> bool {
        self.leases.contains_key(&id)
    }
    pub fn valid(&self, id: usize) -> bool {
        self.leases.get(&id).is_some_and(|s| {
            s.iter()
                .all(|(r, v)| *self.versions.get(&self.canonical(r)).unwrap_or(&0) == *v)
        })
    }
    pub fn stamp(&self, id: usize) -> serde_json::Value {
        serde_json::json!({"resources":self.leases.get(&id),"current":self.valid(id),"scope":"versioned historical evidence; not a live tier0 substitution certificate"})
    }
    pub fn apply(&mut self, e: &Change) -> BTreeSet<usize> {
        let mut changed: BTreeSet<_> = e
            .tokens
            .iter()
            .map(|i| Resource::Token(self.root(*i)))
            .chain(e.tables.iter().cloned().map(Resource::Table))
            .collect();
        for &(a, b) in &e.unions {
            let (a, b) = (self.root(a), self.root(b));
            if a != b {
                let sa = *self.sizes.get(&a).unwrap_or(&1);
                let sb = *self.sizes.get(&b).unwrap_or(&1);
                let (lo, hi) = if sa > sb || (sa == sb && a < b) {
                    (a, b)
                } else {
                    (b, a)
                };
                self.sizes.insert(lo, sa + sb);
                self.sizes.remove(&hi);
                self.parents.insert(hi, lo);
                if let Some(ids) = self.subscribers.remove(&Resource::Token(hi)) {
                    self.subscribers
                        .entry(Resource::Token(lo))
                        .or_default()
                        .extend(ids);
                }
                changed.insert(Resource::Token(lo));
            }
        }
        if e.reset {
            changed.insert(Resource::Scope);
        }
        self.tick += 1;
        let mut wake = BTreeSet::new();
        for r in changed {
            let r = self.canonical(&r);
            self.versions.insert(r.clone(), self.tick);
            self.notifications += 1;
            if let Some(ids) = self.subscribers.get(&r) {
                wake.extend(ids);
            }
        }
        self.wakes += wake.len();
        wake
    }
}
