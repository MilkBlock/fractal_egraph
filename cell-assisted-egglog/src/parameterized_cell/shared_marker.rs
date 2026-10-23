//! Shared, exact prefix relations. Native commits invalidate their dependent
//! roots; queries refresh only dirty roots, independently of consumer cursors.
//! A marker retains correlated witnesses, never independent marginal domains.
use super::*;
use egglog::cell_assist::CellQueryAtom;

#[derive(Clone)]
pub(super) struct Use {
    pub marker: usize,
    /// Canonical prefix slot -> this consumer's native slot.
    pub slots: Vec<usize>,
    pub depth: usize,
}
#[derive(Clone)]
struct Frame {
    payload: Vec<Value>,
    parent: Option<Arc<Frame>>,
    /// Needed only for slots omitted by the selected payload.
    witnesses: Vec<Arc<Row>>,
    stamp: u32,
}
struct Entry {
    frames: Arc<Vec<Arc<Frame>>>,
    deps: Reads,
}
#[derive(Default, Serialize)]
struct Stats {
    root_refreshes: usize,
    cache_hits: usize,
    invalidations: usize,
    lookups: usize,
    candidate_rows: usize,
    published_bindings: usize,
    decoded_rows: usize,
    peak_bindings: usize,
    refresh_seconds: f64,
}
pub(super) struct Marker {
    signature: String,
    body_key: String,
    atoms: Vec<CellQueryAtom>,
    tables: Vec<usize>,
    accesses: Vec<Access>,
    slots: usize,
    carried: Vec<usize>,
    consumers: Vec<usize>,
    parent: Option<usize>,
    children: Vec<usize>,
    cache: FxHashMap<Key, Entry>,
    subscribers: FxHashMap<Read, FxHashSet<Key>>,
    bindings: usize,
    stats: Stats,
}

/// Alpha normalization preserves table types, argument positions, literals,
/// aliases and the chosen access order. Guards remain with each consumer.
fn normalize(k: &Kernel, depth: usize) -> (String, Vec<usize>, Vec<CellQueryAtom>, Vec<usize>) {
    let ids = std::iter::once(k.seed)
        .chain(k.steps[..depth].iter().map(|(i, _)| *i))
        .collect::<Vec<_>>();
    let mut slots = vec![];
    let mut atoms = vec![];
    let mut description = vec![];
    for i in &ids {
        let mut a = k.sink.atoms[*i].clone();
        let mut columns = vec![];
        for (o, sort) in a.columns.iter_mut().zip(&a.sorts) {
            match o {
                CellOperand::Slot(s) => {
                    let n = slots.iter().position(|v| v == s).unwrap_or_else(|| {
                        slots.push(*s);
                        slots.len() - 1
                    });
                    *s = n;
                    columns.push(json!({"Slot":n,"sort":sort}));
                }
                CellOperand::Constant { text, .. } => {
                    columns.push(json!({"Literal":text,"sort":sort}))
                }
            }
        }
        description.push(json!({"table":a.function,"columns":columns}));
        atoms.push(a);
    }
    let body = json!({"atoms":description,"accesses":k.steps[..depth].iter().map(|(_,a)|a).collect::<Vec<_>>()});
    (
        body.to_string(),
        slots,
        atoms,
        ids.iter().map(|i| k.tables[*i]).collect(),
    )
}
pub(super) fn build(kernels: &mut [Kernel]) -> (Vec<Marker>, Vec<Vec<usize>>) {
    let mut markers: Vec<Marker> = vec![];
    let mut keys = BTreeMap::new();
    for (id, k) in kernels.iter_mut().enumerate() {
        if !k.share_prefix {
            continue;
        }
        let (depth, selected) = k.payload_layout.as_ref().unwrap();
        let mut parent = None;
        let mut result = None;
        for depth in 0..=*depth {
            let (body_key, slots, atoms, tables) = normalize(k, depth);
            let carried = slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    (depth < k.payload_layout.as_ref().unwrap().0 || selected.contains(s))
                        .then_some(i)
                })
                .collect::<Vec<_>>();
            let signature = json!({"body":body_key,"carried":carried}).to_string();
            let marker = *keys.entry(signature.clone()).or_insert_with(|| {
                let marker = markers.len();
                markers.push(Marker {
                    signature,
                    body_key,
                    atoms,
                    tables,
                    slots: slots.len(),
                    carried,
                    accesses: k.steps[..depth].iter().map(|(_, a)| *a).collect(),
                    consumers: vec![],
                    parent,
                    children: vec![],
                    cache: Default::default(),
                    subscribers: Default::default(),
                    bindings: 0,
                    stats: Default::default(),
                });
                if let Some(p) = parent {
                    markers[p].children.push(marker);
                }
                marker
            });
            result = Some(Use {
                marker,
                slots,
                depth,
            });
            parent = Some(marker);
        }
        let use_ = result.unwrap();
        markers[use_.marker].consumers.push(id);
        k.shared = Some(use_);
    }
    let mut watch = vec![];
    for (i, m) in markers.iter().enumerate() {
        let t = *m.tables.last().unwrap();
        watch.resize_with(watch.len().max(t + 1), Vec::new);
        watch[t].push(i);
    }
    (markers, watch)
}
impl Marker {
    fn remove(&mut self, key: &Key) {
        if let Some(entry) = self.cache.remove(key) {
            self.bindings -= entry.frames.len();
            self.stats.invalidations += 1;
            for read in entry.deps {
                if let Some(xs) = self.subscribers.get_mut(&read) {
                    xs.remove(key);
                    if xs.is_empty() {
                        self.subscribers.remove(&read);
                    }
                }
            }
        }
    }
    fn walk(
        &mut self,
        rows: &Rows,
        step: usize,
        b: Binding,
        stamp: u32,
        witnesses: Vec<Arc<Row>>,
        parent: Option<Arc<Frame>>,
        deps: &mut Reads,
        out: &mut BTreeMap<Vec<Value>, Frame>,
    ) -> Result<()> {
        if step == self.atoms.len() {
            let values = b
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or("unbound shared prefix slot")?;
            let frame = Frame {
                parent: if self.carried.len() == self.slots {
                    None
                } else {
                    parent.clone()
                },
                payload: self.carried.iter().map(|i| values[*i]).collect(),
                witnesses: if self.carried.len() == self.slots {
                    vec![]
                } else {
                    witnesses
                },
                stamp,
            };
            if let Some(old) = out.get_mut(&values) {
                old.stamp = old.stamp.max(stamp)
            } else {
                out.insert(values, frame);
            }
            return Ok(());
        }
        let table = self.tables[step];
        let atom = &self.atoms[step];
        let n = atom.columns.len() - 1;
        let read = match self.accesses[step - 1] {
            Access::Key => Read::Key(
                table,
                atom.columns[..n].iter().map(|o| value(o, &b)).collect(),
            ),
            Access::Output => Read::Output(table, value(&atom.columns[n], &b)),
            Access::Columns(mask) => Read::Columns(
                table,
                mask,
                atom.columns
                    .iter()
                    .enumerate()
                    .filter_map(|(i, o)| (mask & (1 << i) != 0).then(|| value(o, &b)))
                    .collect(),
            ),
        };
        deps.push(read.clone());
        self.stats.lookups += 1;
        let found: Vec<Arc<Row>> = match &read {
            Read::Key(t, k) => rows
                .keys
                .get(&(*t, k.clone()))
                .into_iter()
                .cloned()
                .collect(),
            Read::Output(t, v) => rows
                .outputs
                .get(&(*t, *v))
                .into_iter()
                .flatten()
                .map(|k| rows.keys[&(*t, k.clone())].clone())
                .collect(),
            Read::Columns(t, m, k) => rows
                .projected
                .get(&(*t, *m, k.clone()))
                .into_iter()
                .flat_map(|p| p.keys(0))
                .map(|k| rows.keys[&(*t, k.clone())].clone())
                .collect(),
        };
        for row in found {
            self.stats.candidate_rows += 1;
            let mut next = b.clone();
            if unify(&self.atoms[step].columns, &row.values, &mut next) {
                let mut ws = witnesses.clone();
                ws.push(row.clone());
                self.walk(
                    rows,
                    step + 1,
                    next,
                    stamp.max(row.stamp),
                    ws,
                    parent.clone(),
                    deps,
                    out,
                )?;
            }
        }
        Ok(())
    }
    fn refresh(
        &mut self,
        rows: &Rows,
        key: &Key,
        parents: Option<Arc<Vec<Arc<Frame>>>>,
    ) -> Result<Arc<Vec<Arc<Frame>>>> {
        let start = std::time::Instant::now();
        self.stats.root_refreshes += 1;
        let mut deps = Reads::new();
        let mut out = BTreeMap::new();
        if let Some(parents) = parents {
            for parent in parents.iter() {
                let mut b = smallvec![None;self.slots];
                for (i, v) in parent.payload.iter().enumerate() {
                    b[i] = Some(*v);
                }
                self.walk(
                    rows,
                    self.atoms.len() - 1,
                    b,
                    parent.stamp,
                    vec![],
                    Some(parent.clone()),
                    &mut deps,
                    &mut out,
                )?;
            }
        } else {
            let Some(row) = rows.keys.get(&(self.tables[0], key.clone())).cloned() else {
                return Ok(Arc::new(vec![]));
            };
            let mut b = smallvec![None;self.slots];
            self.stats.candidate_rows += 1;
            if unify(&self.atoms[0].columns, &row.values, &mut b) {
                self.walk(rows, 1, b, row.stamp, vec![row], None, &mut deps, &mut out)?;
            }
        }
        deps.sort();
        deps.dedup();
        for read in &deps {
            self.subscribers
                .entry(read.clone())
                .or_default()
                .insert(key.clone());
        }
        let frames = Arc::new(out.into_values().map(Arc::new).collect::<Vec<_>>());
        self.bindings += frames.len();
        self.stats.published_bindings += frames.len();
        self.stats.peak_bindings = self.stats.peak_bindings.max(self.bindings);
        self.cache.insert(
            key.clone(),
            Entry {
                frames: frames.clone(),
                deps,
            },
        );
        self.stats.refresh_seconds += start.elapsed().as_secs_f64();
        Ok(frames)
    }
    fn bind(&mut self, frame: &Frame, use_: &Use, k: &Kernel) -> Result<Binding> {
        if self.carried.len() == self.slots {
            let mut b = smallvec![None;k.slot_count];
            for (i, v) in frame.payload.iter().enumerate() {
                b[use_.slots[i]] = Some(*v);
            }
            return Ok(b);
        }
        let mut local = smallvec![None;self.slots];
        for (s, v) in self.carried.iter().zip(&frame.payload) {
            local[*s] = Some(*v);
        }
        if self.carried.len() != self.slots {
            if let Some(p) = &frame.parent {
                for (i, v) in p.payload.iter().enumerate() {
                    local[i] = Some(*v);
                }
            }
            for (atom, row) in self.atoms[self.atoms.len() - frame.witnesses.len()..]
                .iter()
                .zip(&frame.witnesses)
            {
                self.stats.decoded_rows += 1;
                if !unify(&atom.columns, &row.values, &mut local) {
                    return Err("shared marker receipt contradicts its payload".into());
                }
            }
        }
        let mut b = smallvec![None;k.slot_count];
        for (i, s) in use_.slots.iter().enumerate() {
            b[*s] = local[i];
        }
        Ok(b)
    }
    pub fn register(
        &self,
        book: &mut crate::certificate::Book,
        rulesets: Vec<String>,
    ) -> Result<()> {
        let id=book.checked(crate::certificate::Document{method:"typed-shared-prefix/v1".into(),strength:crate::certificate::Strength::CompleteQuery,
            rulesets,contract:json!({"prefix":self.body_key,"carried":self.carried,"parent":self.parent}),
            evidence:json!({"all_correlated_alternatives":true,"consumer_guards_and_cursors_retained":true,"sat_skip_authority":false})});
        let mut sorts = BTreeMap::new();
        for a in &self.atoms {
            for (o, s) in a.columns.iter().zip(&a.sorts) {
                if let CellOperand::Slot(i) = o {
                    sorts.insert(*i, s.clone());
                }
            }
        }
        book.marker(
            id,
            self.atoms[0].function.clone(),
            self.carried
                .iter()
                .map(|i| (format!("s{i}"), sorts[i].clone()))
                .collect(),
            vec![
                "committed row/rebuild versions and missing-port subscriptions".into(),
                "independent original consumer watermarks".into(),
            ],
        )?;
        Ok(())
    }
    pub fn report(&self) -> Json {
        json!({"signature":self.signature,"body_key":self.body_key,"root":self.atoms[0].function,"prefix_atoms":self.atoms.len(),
            "stored_slots":self.carried,"consumers":self.consumers,"parent":self.parent,"children":self.children,"live_roots":self.cache.len(),"live_bindings":self.bindings,
            "dependency_edges":self.subscribers.values().map(|x|x.len()).sum::<usize>(),"stats":self.stats})
    }
}
fn get(
    markers: &mut [Marker],
    id: usize,
    rows: &Rows,
    key: &Key,
    lookups: &mut usize,
) -> Result<Arc<Vec<Arc<Frame>>>> {
    if let Some(e) = markers[id].cache.get(key) {
        let answer = e.frames.clone();
        markers[id].stats.cache_hits += 1;
        return Ok(answer);
    }
    let parents = markers[id]
        .parent
        .map(|p| get(markers, p, rows, key, lookups))
        .transpose()?;
    let before = markers[id].stats.lookups;
    let answer = markers[id].refresh(rows, key, parents)?;
    *lookups += markers[id].stats.lookups - before;
    Ok(answer)
}
pub(super) fn wake(
    markers: &mut [Marker],
    watch: &[Vec<usize>],
    read: &Read,
    kernels: &mut [Kernel],
) {
    let table = match read {
        Read::Key(t, _) | Read::Output(t, _) | Read::Columns(t, ..) => *t,
    };
    let mut todo = vec![];
    for id in watch.get(table).into_iter().flatten() {
        let m = &markers[*id];
        todo.extend(
            m.subscribers
                .get(read)
                .into_iter()
                .flatten()
                .map(|k| (*id, k.clone())),
        );
        if m.parent.is_none() {
            if let Read::Key(_, key) = read {
                todo.push((*id, key.clone()));
            }
        }
    }
    let mut seen = FxHashSet::default();
    while let Some((id, key)) = todo.pop() {
        if !seen.insert((id, key.clone())) {
            continue;
        }
        let m = &mut markers[id];
        m.remove(&key);
        for consumer in &m.consumers {
            kernels[*consumer].dirty.insert(key.clone());
        }
        todo.extend(m.children.iter().map(|i| (*i, key.clone())));
    }
}
impl Runtime {
    pub(super) fn boundary_matches(&mut self, id: usize) -> Result<Vec<Vec<Value>>> {
        let Some(use_) = self.kernels[id].shared.clone() else {
            return self.kernels[id].matches(&self.eg, &self.rows, &mut self.stats.keyed_reads);
        };
        let k = &mut self.kernels[id];
        let watermark = self.eg.cell_query_watermark(&k.sink);
        let mut out = vec![];
        let mut lookups = 0;
        for key in std::mem::take(&mut k.dirty) {
            k.executions += 1;
            if !self
                .rows
                .keys
                .contains_key(&(k.tables[k.seed], key.clone()))
            {
                k.install_dependencies(key, None);
                continue;
            }
            let frames = get(
                &mut self.shared_markers,
                use_.marker,
                &self.rows,
                &key,
                &mut self.stats.keyed_reads,
            )?;
            let mut deps = Reads::new();
            for frame in frames.iter() {
                k.shared_prefix_uses += 1;
                let b = self.shared_markers[use_.marker].bind(frame, &use_, k)?;
                k.read_program(
                    &self.eg,
                    &self.rows,
                    use_.depth,
                    b,
                    frame.stamp,
                    watermark,
                    &mut deps,
                    &mut out,
                    &mut lookups,
                    &mut 0,
                )?;
            }
            deps.sort();
            deps.dedup();
            k.install_dependencies(key, Some(deps));
        }
        k.index_reads += lookups;
        self.stats.keyed_reads += lookups;
        out.sort();
        if out.windows(2).any(|p| p[0] == p[1]) {
            return Err("duplicate shared-marker binding".into());
        }
        k.bindings += out.len();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn program(shared: bool, carry: bool) -> Runtime {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
          (datatype E (N i64) (Loop E E) (C E E) (D E E))
          (relation Ctx (E E)) (function Cost (E) i64 :merge (min old new))
          (relation Hit (E E i64)) (relation Other (E E i64))
          (ruleset a) (ruleset b)
          (rule ((= r (Loop x y)) (Ctx y ctx) (= cost (Cost x)) (< cost 10) (= c (C z ctx)))
                ((Hit r c cost)) :ruleset a :name "q1")
          (rule ((= owner (Loop aa bb)) (Ctx bb cc) (= n (Cost aa)) (>= n 5) (= d (D zz cc)))
                ((Other owner d n)) :ruleset b :name "q2")
          (Loop (N 1) (N 2)) (set (Cost (N 1)) 20)
          (C (N 7) (N 3)) (D (N 8) (N 3))
        "#,
        )
        .unwrap();
        let report =
            boundary::inspect(&mut eg, &["a".into(), "b".into()], &BTreeSet::new()).unwrap();
        let specs = report["queries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| {
                let raw = q["plans"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|p| {
                        p["shape"]["atoms"][p["seed"].as_u64().unwrap() as usize]["table"] == "Loop"
                    })
                    .unwrap();
                let mut p: boundary::Plan = serde_json::from_value(raw.clone()).unwrap();
                p.prefix_len = 2;
                p.shared_marker = shared;
                p.carried_slots = if carry {
                    (0..raw["shape"]["slots"].as_array().unwrap().len()).collect()
                } else {
                    vec![]
                };
                boundary::Spec {
                    ruleset: q["ruleset"].as_str().unwrap().into(),
                    rule: q["rule"].as_str().unwrap().into(),
                    root: "Loop".into(),
                    plan: Some(p),
                }
            })
            .collect::<Vec<_>>();
        Runtime::new_with_boundaries(eg, &[], &BTreeMap::new(), vec![], None, &specs).unwrap()
    }
    fn query(r: &mut Runtime, set: &str, name: &str) {
        let sink = r.eg.prepare_cell_action(set, name).unwrap();
        let query = r.eg.prepare_cell_query(&sink).unwrap();
        let mut expected = r.eg.run_cell_queries(&[&query]).unwrap().pop().unwrap();
        expected.sort();
        expected.dedup();
        let actual = r.query_boundary(set, &[&sink]).unwrap().pop().unwrap().1;
        assert_eq!(actual, expected, "{set}: {}", r.report());
        r.eg.apply_completed_cell_ruleset(&[(&sink, &actual)])
            .unwrap();
    }
    #[test]
    fn correlated_shared_prefix_has_independent_consumer_frontiers_and_guards() {
        for carry in [false, true] {
            let mut r = program(true, carry);
            assert_eq!(r.shared_markers.len(), 3);
            for input in [
                "",
                "(Ctx (N 2) (N 3))",
                "(set (Cost (N 1)) 8)",
                "(Ctx (N 2) (N 4)) (C (N 9) (N 4)) (D (N 10) (N 4))",
                "(union (N 3) (N 4))",
                "(delete (Ctx (N 2) (N 3)))",
                "(Ctx (N 2) (N 3))",
                "(set (Cost (N 1)) 4)",
                "(subsume (Ctx (N 2) (N 3)))",
                "(Ctx (N 2) (N 3))",
                "(subsume (Loop (N 1) (N 2)))",
            ] {
                if !input.is_empty() {
                    r.input(input).unwrap();
                }
                query(&mut r, "a", "q1");
                // The second consumer has its OWN watermark even though the
                // shared relation was already refreshed by the first consumer.
                query(&mut r, "a", "q1");
                query(&mut r, "b", "q2");
                r.sync().unwrap();
                r.audit_mirror().unwrap();
            }
            let m = &r.shared_markers[2];
            assert!(m.stats.cache_hits > 0);
            assert!(m.stats.peak_bindings >= 2);
            assert_eq!(m.stats.decoded_rows > 0, !carry);
            assert_eq!(m.bindings, 0);
        }
    }
    #[test]
    fn two_consumers_pay_one_prefix_read_and_suffix_changes_do_not_refresh_it() {
        let mut shared = program(true, true);
        let mut private = program(false, true);
        for r in [&mut shared, &mut private] {
            r.input("(Ctx (N 2) (N 3)) (set (Cost (N 1)) 8)").unwrap();
            query(r, "a", "q1");
            query(r, "b", "q2");
        }
        let m = &shared.shared_markers[2];
        assert_eq!(m.stats.root_refreshes, 1);
        assert_eq!(
            shared
                .shared_markers
                .iter()
                .map(|m| m.stats.lookups)
                .sum::<usize>(),
            2
        );
        assert!(shared.stats.keyed_reads < private.stats.keyed_reads);
        let before = shared.shared_markers[2].stats.root_refreshes;
        shared.input("(C (N 100) (N 3)) (D (N 101) (N 3))").unwrap();
        query(&mut shared, "a", "q1");
        query(&mut shared, "b", "q2");
        assert_eq!(shared.shared_markers[2].stats.root_refreshes, before);
        // An unrelated row in the same table must not invalidate this context.
        shared.input("(Ctx (N 88) (N 99))").unwrap();
        query(&mut shared, "a", "q1");
        assert_eq!(shared.shared_markers[2].stats.root_refreshes, before);
    }
}
