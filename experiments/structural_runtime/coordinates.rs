//! A compiled, event-maintained view of the unchanged two-edge feature program.
//! Only subscribed coordinates are published. Dependencies include absent heads
//! and ports, so later insertions invalidate negative answers as well.
use super::*;
use rustc_hash::FxHashSet as Set;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct N(usize, Value);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Dep {
    Rows(N, usize),
    Head(N, usize),
    Port(N, usize, usize),
}
enum Op {
    Root(usize),
    Port(usize, usize),
    Alias(usize, usize, usize),
    Return(usize, usize),
    Path(usize, usize, usize),
    Path2(usize, usize, usize, usize, usize),
}
struct Schema {
    args: Vec<usize>,
    result: usize,
}
struct R {
    table: usize,
    args: Vec<N>,
    result: N,
}
#[derive(Default)]
struct State {
    rows: Vec<usize>,
    heads: Map<usize, usize>,
    ports: Map<(usize, usize), usize>,
}
struct Published {
    signature: Vec<usize>,
    deps: Set<Dep>,
}
#[derive(Default, Serialize)]
pub(super) struct Metrics {
    pub commit_batches: usize,
    pub updates: usize,
    pub subscriptions: usize,
    pub publications: usize,
    pub unchanged_publications: usize,
    pub notifications: usize,
    pub requests: usize,
    pub hits: usize,
    pub reference_checks: usize,
}
pub(super) struct Coordinates {
    sorts: Map<String, usize>,
    schemas: Vec<Option<Schema>>,
    program: Vec<Op>,
    keys: Vec<Map<Key, usize>>,
    rows: Vec<Option<R>>,
    free: Vec<usize>,
    states: Map<N, State>,
    published: Map<N, Published>,
    watchers: Map<Dep, Set<N>>,
    dirty: Set<N>,
    pub metrics: Metrics,
}
impl Coordinates {
    pub fn new(names: &[String], lib: &Library) -> Self {
        let mut sorts = Map::default();
        let mut sid = |s: &str| {
            let n = sorts.len();
            *sorts.entry(s.to_owned()).or_insert(n)
        };
        let schemas = names
            .iter()
            .map(|n| {
                lib.constructors.get(n).map(|(args, result)| Schema {
                    args: args.iter().map(|s| sid(s)).collect(),
                    result: sid(result),
                })
            })
            .collect::<Vec<_>>();
        for m in &lib.models {
            sid(&m.sort);
        }
        let ids: Map<_, _> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();
        let program = lib
            .vocabulary
            .iter()
            .map(|v| {
                let t = &v.token;
                let op = ids[t[1].as_str().unwrap()];
                let int = |i: usize| t[i].as_u64().unwrap() as usize;
                match t[0].as_str().unwrap() {
                    "root" => Op::Root(op),
                    "port" => Op::Port(op, int(2)),
                    "alias" => Op::Alias(op, int(2), int(3)),
                    "return-alias" => Op::Return(op, int(2)),
                    "path" => Op::Path(op, int(2), ids[t[3].as_str().unwrap()]),
                    "path2" => Op::Path2(
                        op,
                        int(2),
                        ids[t[3].as_str().unwrap()],
                        int(4),
                        ids[t[5].as_str().unwrap()],
                    ),
                    _ => panic!("unsupported coordinate program"),
                }
            })
            .collect();
        Self {
            sorts,
            schemas,
            program,
            keys: (0..names.len()).map(|_| Map::default()).collect(),
            rows: vec![],
            free: vec![],
            states: Map::default(),
            published: Map::default(),
            watchers: Map::default(),
            dirty: Set::default(),
            metrics: Metrics::default(),
        }
    }
    fn notify(&mut self, d: Dep) {
        if let Some(readers) = self.watchers.get(&d) {
            for n in readers {
                self.metrics.notifications += usize::from(self.dirty.insert(*n));
            }
        }
    }
    fn local(&mut self, n: N) {
        if self.published.contains_key(&n) {
            self.metrics.notifications += usize::from(self.dirty.insert(n));
        }
    }
    pub fn remove(&mut self, table: usize, key: &[Value]) {
        let Some(id) = self.keys[table].remove(key) else {
            return;
        };
        let r = self.rows[id].take().unwrap();
        self.free.push(id);
        self.metrics.updates += 1;
        let state = self.states.get_mut(&r.result).unwrap();
        let p = state.rows.iter().position(|x| *x == id).unwrap();
        state.rows.swap_remove(p);
        let count = state.heads.get_mut(&table).unwrap();
        *count -= 1;
        let head_changed = *count == 0;
        if head_changed {
            state.heads.remove(&table);
        }
        self.local(r.result);
        self.notify(Dep::Rows(r.result, table));
        if head_changed {
            self.notify(Dep::Head(r.result, table));
        }
        for (i, n) in r.args.iter().enumerate() {
            let state = self.states.get_mut(n).unwrap();
            let count = state.ports.get_mut(&(table, i)).unwrap();
            *count -= 1;
            if *count == 0 {
                state.ports.remove(&(table, i));
                self.local(*n);
                self.notify(Dep::Port(*n, table, i));
            }
        }
    }
    pub fn insert(&mut self, table: usize, key: &[Value], values: &[Value]) {
        let Some(schema) = &self.schemas[table] else {
            return;
        };
        let r = R {
            table,
            args: schema
                .args
                .iter()
                .zip(values)
                .map(|(s, v)| N(*s, *v))
                .collect(),
            result: N(schema.result, values[schema.args.len()]),
        };
        debug_assert!(!self.keys[table].contains_key(key));
        let id = self.free.pop().unwrap_or_else(|| {
            self.rows.push(None);
            self.rows.len() - 1
        });
        self.keys[table].insert(key.to_vec(), id);
        let state = self.states.entry(r.result).or_default();
        state.rows.push(id);
        let count = state.heads.entry(table).or_default();
        *count += 1;
        let head_changed = *count == 1;
        self.local(r.result);
        self.notify(Dep::Rows(r.result, table));
        if head_changed {
            self.notify(Dep::Head(r.result, table));
        }
        for (i, n) in r.args.iter().enumerate() {
            let count = self
                .states
                .entry(*n)
                .or_default()
                .ports
                .entry((table, i))
                .or_default();
            *count += 1;
            if *count == 1 {
                self.local(*n);
                self.notify(Dep::Port(*n, table, i));
            }
        }
        self.rows[id] = Some(r);
        self.metrics.updates += 1;
    }
    fn head(&self, n: N, op: usize, deps: &mut Set<Dep>) -> bool {
        deps.insert(Dep::Head(n, op));
        self.states
            .get(&n)
            .is_some_and(|s| s.heads.contains_key(&op))
    }
    fn rows(&self, n: N, op: usize, deps: &mut Set<Dep>) -> impl Iterator<Item = &R> {
        deps.insert(Dep::Rows(n, op));
        self.states
            .get(&n)
            .into_iter()
            .flat_map(|s| s.rows.iter())
            .filter_map(|id| self.rows[*id].as_ref())
            .filter(move |r| r.table == op)
    }
    fn compute(&self, n: N) -> Published {
        let mut signature = vec![];
        let mut deps = Set::default();
        for (i, op) in self.program.iter().enumerate() {
            let holds = match *op {
                Op::Root(op) => self.head(n, op, &mut deps),
                Op::Port(op, p) => {
                    deps.insert(Dep::Port(n, op, p));
                    self.states
                        .get(&n)
                        .is_some_and(|s| s.ports.contains_key(&(op, p)))
                }
                Op::Alias(op, a, b) => self.rows(n, op, &mut deps).any(|r| r.args[a] == r.args[b]),
                Op::Return(op, a) => self.rows(n, op, &mut deps).any(|r| r.args[a] == n),
                Op::Path(op, a, leaf) => {
                    let children = self
                        .rows(n, op, &mut deps)
                        .map(|r| r.args[a])
                        .collect::<Vec<_>>();
                    children.into_iter().any(|c| self.head(c, leaf, &mut deps))
                }
                Op::Path2(op, a, middle, b, leaf) => {
                    let children = self
                        .rows(n, op, &mut deps)
                        .map(|r| r.args[a])
                        .collect::<Vec<_>>();
                    children.into_iter().any(|c| {
                        let leaves = self
                            .rows(c, middle, &mut deps)
                            .map(|r| r.args[b])
                            .collect::<Vec<_>>();
                        leaves.into_iter().any(|l| self.head(l, leaf, &mut deps))
                    })
                }
            };
            if holds {
                signature.push(i);
            }
        }
        // A coordinate's own row/port changes already have a direct publisher.
        // Subscribe only across nodes; do not allocate dozens of per-operator
        // self-dependencies for every one of thousands of input slots.
        deps.retain(|d| match d {
            Dep::Rows(node, _) | Dep::Head(node, _) | Dep::Port(node, _, _) => *node != n,
        });
        Published { signature, deps }
    }
    fn publish(&mut self, n: N) {
        let new = self.compute(n);
        if let Some(old) = self.published.remove(&n) {
            self.metrics.unchanged_publications += usize::from(new.signature == old.signature);
            for d in old.deps {
                let readers = self.watchers.get_mut(&d).unwrap();
                readers.remove(&n);
                if readers.is_empty() {
                    self.watchers.remove(&d);
                }
            }
        }
        for d in &new.deps {
            self.watchers.entry(*d).or_default().insert(n);
        }
        self.published.insert(n, new);
        self.metrics.publications += 1;
    }
    /// Events coalesce a dirty frontier. Publish at the consumer boundary, not
    /// for every previously observed coordinate after unrelated Run stages.
    pub fn commit(&mut self) {
        self.metrics.commit_batches += 1;
    }
    pub fn signature(&mut self, sort: &str, value: Value, audit: bool) -> Vec<usize> {
        self.metrics.requests += 1;
        let n = N(self.sorts[sort], value);
        if !self.published.contains_key(&n) {
            self.publish(n);
            self.metrics.subscriptions += 1;
        } else if self.dirty.remove(&n) {
            self.publish(n);
        } else {
            self.metrics.hits += 1;
        }
        let sig = self.published[&n].signature.clone();
        if audit {
            self.metrics.reference_checks += 1;
            assert_eq!(sig, self.compute(n).signature, "stale published coordinate");
        }
        sig
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(n: u32) -> Value {
        Value::new_const(n)
    }
    fn graph() -> Coordinates {
        let mut lib = Library {
            summary_markers: vec![],
            plans: vec![],
            models: vec![Model {
                sort: "T".into(),
                channel: "".into(),
                sort_hash: 0,
                excluded_tokens: vec![],
                reference_model: None,
                weights: vec![],
                bias: 0.0,
            }],
            constructors: BTreeMap::from([
                ("A".into(), (vec!["T".into(), "T".into()], "T".into())),
                ("B".into(), (vec!["T".into()], "T".into())),
                ("L".into(), (vec!["i64".into()], "T".into())),
            ]),
            vocabulary: vec![],
        };
        for token in [
            json!(["root", "A"]),
            json!(["port", "A", 0]),
            json!(["path", "A", 0, "B"]),
            json!(["path2", "A", 0, "B", 0, "L"]),
            json!(["alias", "A", 0, 1]),
            json!(["return-alias", "A", 0]),
        ] {
            lib.vocabulary.push(Token {
                token: token.as_array().unwrap().clone(),
                hash: 0,
            });
        }
        Coordinates::new(&["A".into(), "B".into(), "L".into()], &lib)
    }
    #[test]
    fn absent_heads_and_new_middle_rows_publish_late_paths() {
        let mut g = graph();
        g.insert(0, &[v(1), v(2)], &[v(1), v(2), v(3)]);
        g.commit();
        assert_eq!(g.signature("T", v(3), true), vec![0]);
        g.insert(1, &[v(4)], &[v(4), v(1)]);
        g.commit();
        assert_eq!(g.signature("T", v(3), true), vec![0, 2]);
        g.insert(2, &[v(8)], &[v(8), v(4)]);
        g.commit();
        assert_eq!(g.signature("T", v(3), true), vec![0, 2, 3]);
        g.remove(1, &[v(4)]);
        g.commit();
        assert_eq!(g.signature("T", v(3), true), vec![0]);
    }
    #[test]
    fn multiple_witnesses_removal_alias_and_unrelated_change() {
        let mut g = graph();
        g.insert(0, &[v(1), v(1)], &[v(1), v(1), v(1)]);
        g.commit();
        assert_eq!(g.signature("T", v(1), true), vec![0, 1, 4, 5]);
        let before = g.metrics.publications;
        g.insert(2, &[v(5)], &[v(5), v(9)]);
        g.commit();
        assert_eq!(before, g.metrics.publications);
        g.insert(0, &[v(1), v(2)], &[v(1), v(2), v(1)]);
        g.remove(0, &[v(1), v(1)]);
        g.commit();
        assert_eq!(g.signature("T", v(1), true), vec![0, 1, 5]);
        g.remove(0, &[v(1), v(2)]);
        g.commit();
        assert!(g.signature("T", v(1), true).is_empty());
    }
    #[test]
    fn coordinate_cache_matches_uncached_definition_through_rebuild_like_updates() {
        let mut g = graph();
        let mut seed = 7u32;
        for _ in 0..150 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let a = seed % 9;
            let table = (seed % 3) as usize;
            let key = if table == 0 {
                vec![v(a), v((a + 1) % 9)]
            } else {
                vec![v(a)]
            };
            g.remove(table, &key);
            if seed & 4 != 0 {
                let mut row = key.clone();
                row.push(v((seed / 13) % 9));
                g.insert(table, &key, &row)
            }
            g.commit();
            for n in 0..9 {
                g.signature("T", v(n), true);
            }
        }
    }
}
