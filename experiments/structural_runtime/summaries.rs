//! Solver-selected typed prefix relations, shared as an incremental DAG.
//! Complete correlated bindings remain here; only existential port features
//! enter the speculative vector. Original target-dependent views are masked.
use super::*;

#[derive(Clone, Deserialize)]
pub(super) struct Definition {
    pub name: String,
    pub identity: String,
    pub atoms: Vec<Json>,
    pub carried: Vec<usize>,
    pub sorts: Vec<String>,
    pub dependencies: Vec<String>,
}
type Relation = Map<Key, i64>;
type Arrangement = Map<Key, Relation>;
struct Stage {
    table: usize,
    columns: Vec<usize>,
    vars: Vec<usize>,
    shared: Vec<usize>,
    slots: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    base: Arrangement,
    parents: Arrangement,
    rows: Relation,
    expose: Vec<(usize, String, usize)>, // token, sort, slot
}
#[derive(Default, Serialize)]
pub(super) struct Metrics {
    pub committed_rows: usize,
    pub unchanged: usize,
    pub joined_rows: usize,
    pub binding_deltas: usize,
    pub feature_toggles: usize,
    pub signature_requests: usize,
    pub signature_hits: usize,
    pub maintenance_seconds: f64,
}
pub(super) struct Summaries {
    stages: Vec<Stage>,
    watch: Vec<Vec<usize>>,
    receipts: Vec<Map<Key, Key>>,
    support: Map<Node, Map<usize, i64>>,
    definitions: Vec<(String, usize)>,
    pub metrics: Metrics,
}
fn change(map: &mut Relation, key: Key, delta: i64) {
    let old = map.get(&key).copied().unwrap_or(0);
    let n = old.checked_add(delta).expect("summary support overflow");
    assert!(n >= 0, "negative summary support");
    if n == 0 {
        map.remove(&key);
    } else {
        map.insert(key, n);
    }
}
fn arrange(map: &mut Arrangement, key: Key, row: Key, delta: i64) {
    let rows = map.entry(key.clone()).or_default();
    change(rows, row, delta);
    if rows.is_empty() {
        map.remove(&key);
    }
}
impl Stage {
    fn local(&self, row: &[Value]) -> Option<Key> {
        if row.len() != self.columns.len() {
            panic!("summary row arity changed")
        }
        let mut b = BTreeMap::new();
        for (&s, &v) in self.columns.iter().zip(row) {
            if b.insert(s, v).is_some_and(|old| old != v) {
                return None;
            }
        }
        Some(self.vars.iter().map(|s| b[s]).collect())
    }
    fn base_key(&self, row: &[Value]) -> Key {
        self.shared
            .iter()
            .map(|s| row[self.vars.binary_search(s).unwrap()])
            .collect()
    }
    fn parent_key(&self, row: &[Value]) -> Key {
        self.shared.iter().map(|s| row[*s]).collect()
    }
    fn merge(&self, parent: &[Value], row: &[Value]) -> Key {
        let mut b = parent.to_vec();
        b.resize(self.slots, Value::new_const(0));
        for (&s, &v) in self.vars.iter().zip(row) {
            b[s] = v;
        }
        b
    }
}
impl Summaries {
    pub fn new(names: &[String], lib: &Library, eg: &EGraph) -> Result<Self> {
        let mut this = Self {
            stages: vec![],
            watch: vec![vec![]; names.len()],
            receipts: (0..names.len()).map(|_| Map::default()).collect(),
            support: Map::default(),
            definitions: vec![],
            metrics: Metrics::default(),
        };
        let mut intern = Map::<String, usize>::default();
        for d in &lib.summary_markers {
            let mut parent = None;
            let mut prefix = vec![];
            let mut known = Vec::<String>::new();
            for a in &d.atoms {
                let name = a["table"].as_str().ok_or("summary table")?;
                let t = names
                    .iter()
                    .position(|n| n == name)
                    .ok_or("unsubscribed summary table")?;
                let f = eg.get_function(name).ok_or("unknown summary table")?;
                let native_sorts = f
                    .schema()
                    .input
                    .iter()
                    .chain([&f.schema().output])
                    .map(|s| s.name())
                    .collect::<Vec<_>>();
                let cols = a["columns"].as_array().ok_or("summary columns")?;
                if cols.len() != native_sorts.len() {
                    return Err("summary/native arity mismatch".into());
                }
                let old = known.len();
                let mut columns = vec![];
                for (c, sort) in cols.iter().zip(native_sorts) {
                    let s = c["Slot"]
                        .as_u64()
                        .ok_or("summary adapter requires slot operands")?
                        as usize;
                    if c["sort"].as_str() != Some(sort) {
                        return Err("summary/native sort mismatch".into());
                    }
                    if s == known.len() {
                        known.push(sort.to_owned());
                    }
                    if known.get(s).map(String::as_str) != Some(sort) {
                        return Err("invalid summary slot order/type".into());
                    }
                    columns.push(s);
                }
                prefix.push(a.clone());
                let key = serde_json::to_string(&prefix)?;
                let id = if let Some(&id) = intern.get(&key) {
                    id
                } else {
                    let vars = columns
                        .iter()
                        .copied()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>();
                    let shared = vars
                        .iter()
                        .copied()
                        .filter(|s| *s < old)
                        .collect::<Vec<_>>();
                    if parent.is_some() && shared.is_empty() {
                        return Err("disconnected summary prefix".into());
                    }
                    let id = this.stages.len();
                    let parents = if parent.is_none() {
                        Map::from_iter([(vec![], Map::from_iter([(vec![], 1)]))])
                    } else {
                        Map::default()
                    };
                    this.stages.push(Stage {
                        table: t,
                        columns,
                        vars,
                        shared,
                        slots: known.len(),
                        parent,
                        children: vec![],
                        base: Map::default(),
                        parents,
                        rows: Map::default(),
                        expose: vec![],
                    });
                    if let Some(p) = parent {
                        this.stages[p].children.push(id);
                    }
                    this.watch[t].push(id);
                    intern.insert(key, id);
                    id
                };
                parent = Some(id);
            }
            if d.sorts != known {
                return Err("summary slot signature mismatch".into());
            }
            let id = parent.ok_or("empty summary")?;
            for (i, t) in lib.vocabulary.iter().enumerate() {
                if t.token.first() == Some(&json!("summary")) && t.token[1] == d.identity {
                    let slot = t.token[2].as_u64().ok_or("summary port")? as usize;
                    if !d.carried.contains(&slot) {
                        return Err("unexported summary port".into());
                    }
                    this.stages[id]
                        .expose
                        .push((i, d.sorts[slot].clone(), slot));
                }
            }
            this.definitions.push((d.name.clone(), id));
        }
        Ok(this)
    }
    fn emit(&mut self, id: usize, row: Key, delta: i64) {
        if delta == 0 {
            return;
        }
        self.metrics.binding_deltas += 1;
        change(&mut self.stages[id].rows, row.clone(), delta);
        for &(token, ref sort, slot) in &self.stages[id].expose {
            let key = (sort.clone(), row[slot]);
            let counts = self.support.entry(key.clone()).or_default();
            let old = counts.get(&token).copied().unwrap_or(0);
            let n = old
                .checked_add(delta)
                .expect("summary feature support overflow");
            assert!(n >= 0);
            self.metrics.feature_toggles += usize::from((old > 0) != (n > 0));
            if n == 0 {
                counts.remove(&token);
            } else {
                counts.insert(token, n);
            }
            if counts.is_empty() {
                self.support.remove(&key);
            }
        }
        for child in self.stages[id].children.clone() {
            let s = &mut self.stages[child];
            let key = s.parent_key(&row);
            arrange(&mut s.parents, key.clone(), row.clone(), delta);
            let work = s
                .base
                .get(&key)
                .into_iter()
                .flat_map(|rs| rs.iter())
                .map(|(b, n)| {
                    (
                        s.merge(&row, b),
                        delta
                            .checked_mul(*n)
                            .expect("summary multiplicity overflow"),
                    )
                })
                .collect::<Vec<_>>();
            self.metrics.joined_rows += work.len();
            for (b, n) in work {
                self.emit(child, b, n);
            }
        }
    }
    fn row_delta(&mut self, t: usize, row: &[Value], delta: i64) {
        // Each alias of a repeated table is updated in order. This telescopes
        // the product exactly, including self joins and simultaneous changes.
        for id in self.watch[t].clone() {
            let s = &mut self.stages[id];
            let Some(local) = s.local(row) else { continue };
            let key = s.base_key(&local);
            arrange(&mut s.base, key.clone(), local.clone(), delta);
            let work = s
                .parents
                .get(&key)
                .into_iter()
                .flat_map(|rs| rs.iter())
                .map(|(p, n)| {
                    (
                        s.merge(p, &local),
                        delta
                            .checked_mul(*n)
                            .expect("summary multiplicity overflow"),
                    )
                })
                .collect::<Vec<_>>();
            self.metrics.joined_rows += work.len();
            for (b, n) in work {
                self.emit(id, b, n);
            }
        }
    }
    pub fn update(&mut self, t: usize, key: &[Value], row: Option<&[Value]>) {
        if self.watch[t].is_empty() {
            return;
        }
        let started = Instant::now();
        self.metrics.committed_rows += 1;
        if self.receipts[t].get(key).map(Vec::as_slice) == row {
            self.metrics.unchanged += 1;
            return;
        }
        if let Some(old) = self.receipts[t].remove(key) {
            self.row_delta(t, &old, -1);
        }
        if let Some(r) = row {
            self.row_delta(t, r, 1);
            self.receipts[t].insert(key.to_vec(), r.to_vec());
        }
        self.metrics.maintenance_seconds += started.elapsed().as_secs_f64();
    }
    pub fn clear(&mut self, t: usize) {
        let keys = self.receipts[t].keys().cloned().collect::<Vec<_>>();
        for k in keys {
            self.update(t, &k, None);
        }
    }
    pub fn signature(&mut self, n: &Node) -> Vec<usize> {
        self.metrics.signature_requests += 1;
        let mut ts = self
            .support
            .get(n)
            .into_iter()
            .flat_map(|m| m.keys().copied())
            .collect::<Vec<_>>();
        ts.sort_unstable();
        self.metrics.signature_hits += usize::from(!ts.is_empty());
        ts
    }
    /// Independent materialized joins over current source receipts; diagnostic
    /// only. Rebuild from scratch, without reading incremental stage outputs.
    pub fn audit(&self) {
        let mut expected = Vec::<Relation>::new();
        let mut features = Map::<Node, Map<usize, i64>>::default();
        for s in &self.stages {
            let mut base = Arrangement::default();
            for r in self.receipts[s.table].values() {
                if let Some(b) = s.local(r) {
                    arrange(&mut base, s.base_key(&b), b, 1);
                }
            }
            let unit = Map::from_iter([(vec![], 1)]);
            let parents = s.parent.map(|p| &expected[p]).unwrap_or(&unit);
            let mut rows = Relation::default();
            for (p, n) in parents {
                for (b, m) in base
                    .get(&s.parent_key(p))
                    .into_iter()
                    .flat_map(|rs| rs.iter())
                {
                    change(&mut rows, s.merge(p, b), n * m);
                }
            }
            assert_eq!(
                rows, s.rows,
                "incremental summary differs from exact joint binding oracle"
            );
            for (b, n) in &rows {
                for (token, sort, slot) in &s.expose {
                    *features
                        .entry((sort.clone(), b[*slot]))
                        .or_default()
                        .entry(*token)
                        .or_default() += n;
                }
            }
            expected.push(rows);
        }
        assert_eq!(features, self.support, "summary port projection differs");
    }
    pub fn report(&self) -> Json {
        json!({"metrics":self.metrics,"definitions":self.definitions.iter().map(|(n,i)|json!({"name":n,"live_bindings":self.stages[*i].rows.len(),"atoms":self.depth(*i)})).collect::<Vec<_>>(),
            "shared_stages":self.stages.len(),"source_receipts":self.receipts.iter().map(Map::len).sum::<usize>(),"live_feature_nodes":self.support.len(),"exact_condition_discharges":0})
    }
    fn depth(&self, i: usize) -> usize {
        1 + self.stages[i].parent.map_or(0, |p| self.depth(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(n: u32) -> Value {
        Value::new_const(n)
    }
    fn fixture(self_join: bool) -> (Summaries, Vec<String>, EGraph) {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            "(datatype T (N i64) (R T T)) (relation P (T T)) (relation Q (T T))",
        )
        .unwrap();
        let names = vec!["R".into(), "P".into(), "Q".into()];
        let tables = if self_join {
            vec![
                ("R", vec![0, 1, 2]),
                ("R", vec![2, 3, 4]),
                ("R", vec![0, 3, 4]),
            ]
        } else {
            vec![
                ("R", vec![0, 1, 2]),
                ("P", vec![0, 3, 4]),
                ("Q", vec![1, 3, 5]),
            ]
        };
        let mut ds = vec![];
        let mut vocab = vec![];
        for depth in 1..=tables.len() {
            let mut sorts = BTreeMap::new();
            let atoms = tables[..depth]
                .iter()
                .map(|(t, cols)| {
                    let f = eg.get_function(t).unwrap();
                    let ss = f.schema().input.iter().chain([&f.schema().output]);
                    let columns = cols
                        .iter()
                        .zip(ss)
                        .map(|(i, s)| {
                            sorts.insert(*i, s.name().to_owned());
                            json!({"Slot":i,"sort":s.name()})
                        })
                        .collect::<Vec<_>>();
                    json!({"table":t,"columns":columns})
                })
                .collect();
            let sorts = sorts.into_values().collect::<Vec<_>>();
            let d = Definition {
                name: format!("M{depth}"),
                identity: format!("m{depth}"),
                atoms,
                carried: vec![0, 1, 2],
                sorts,
                dependencies: tables[..depth].iter().map(|(s, _)| s.to_string()).collect(),
            };
            vocab.push(Token {
                token: vec![json!("summary"), json!(d.identity), json!(2)],
                hash: 0,
            });
            ds.push(d);
        }
        let lib = Library {
            plans: vec![],
            models: vec![],
            summary_markers: ds,
            vocabulary: vocab,
            constructors: BTreeMap::new(),
        };
        (Summaries::new(&names, &lib, &eg).unwrap(), names, eg)
    }
    fn put(s: &mut Summaries, t: usize, k: u32, row: Option<&[u32]>) {
        let r = row.map(|r| r.iter().map(|x| v(*x)).collect::<Vec<_>>());
        s.update(t, &[v(k)], r.as_deref());
        s.audit();
    }
    #[test]
    fn late_joint_witnesses_retraction_and_replacement() {
        let (mut s, _, _) = fixture(false);
        put(&mut s, 0, 1, Some(&[1, 2, 3]));
        put(&mut s, 1, 2, Some(&[1, 4, 0]));
        put(&mut s, 2, 3, Some(&[2, 5, 0]));
        assert_eq!(s.signature(&("T".into(), v(3))), vec![0, 1]);
        put(&mut s, 2, 4, Some(&[2, 4, 0]));
        assert_eq!(s.signature(&("T".into(), v(3))), vec![0, 1, 2]);
        put(&mut s, 1, 5, Some(&[1, 5, 0]));
        put(&mut s, 2, 4, None); // second correlated witness still exists
        assert!(s.signature(&("T".into(), v(3))).contains(&2));
        put(&mut s, 2, 3, Some(&[2, 6, 0]));
        assert!(!s.signature(&("T".into(), v(3))).contains(&2));
        s.clear(1);
        s.audit();
        assert_eq!(s.signature(&("T".into(), v(3))), vec![0]);
        assert_eq!(s.stages.len(), 3); // three definitions reuse parents
    }
    #[test]
    fn repeated_table_aliases_and_self_cycle() {
        let (mut s, _, _) = fixture(true);
        put(&mut s, 0, 1, Some(&[1, 1, 1]));
        assert_eq!(s.signature(&("T".into(), v(1))), vec![0, 1, 2]);
        put(&mut s, 0, 2, Some(&[1, 2, 1]));
        put(&mut s, 0, 1, None);
        put(&mut s, 0, 2, None);
        assert!(s.support.is_empty());
    }
    #[test]
    fn randomized_commit_batches_preserve_all_binding_correlations() {
        let (mut s, _, _) = fixture(false);
        let mut seed = 9u64;
        for _ in 0..400 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let t = (seed % 3) as usize;
            let key = ((seed >> 9) % 30) as u32;
            let row = [
                ((seed >> 20) % 9) as u32,
                ((seed >> 26) % 9) as u32,
                if t == 0 { ((seed >> 32) % 9) as u32 } else { 0 },
            ];
            put(
                &mut s,
                t,
                key,
                if seed & 8 == 0 { None } else { Some(&row) },
            );
        }
        for t in 0..3 {
            s.clear(t);
        }
        s.audit();
        assert!(s.support.is_empty());
    }
    #[test]
    fn native_union_and_subsume_retract_and_publish_joint_markers() {
        let (mut s, names, mut eg) = fixture(false);
        let (mut feed, _) = eg.subscribe_cell_commits(&names).unwrap();
        for (code, expected) in [
            ("(R (N 1) (N 2)) (P (N 1) (N 4)) (Q (N 2) (N 5))", 0),
            ("(union (N 4) (N 5))", 1),
            ("(subsume (R (N 1) (N 2)))", 0),
        ] {
            eg.parse_and_run_program(None, code).unwrap();
            let b = eg.drain_cell_commits(&mut feed).unwrap();
            for t in b.clears {
                s.clear(t);
            }
            for c in b.rows {
                let r = if c.visible {
                    c.row.map(|mut r| {
                        r.truncate(feed.tables[c.table].keys + 1);
                        r
                    })
                } else {
                    None
                };
                s.update(c.table, &c.key, r.as_deref());
            }
            s.audit();
            assert_eq!(s.stages[2].rows.len(), expected);
        }
    }
}
