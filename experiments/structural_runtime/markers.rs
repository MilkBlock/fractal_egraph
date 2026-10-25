//! Compile the finite feature vocabulary into differential unary views.
//! No query-time graph walk and no general per-node adjacency/row arena.
//! A path channel is an existential projection E(parent,child) ⋈ F(child).
//! Only required column-pair arrangements survive compilation. Support counts
//! make deletion, duplicate projections and native rebuild exact.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct N(usize, Value);
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Feature {
    Head(usize),
    Port(usize, usize),
    Alias(usize, usize, usize),
    Return(usize, usize),
    Path(usize, usize, Box<Feature>),
}
enum Base {
    Head,
    Port(usize),
    Alias(usize, usize),
    Return(usize),
}
struct Schema {
    args: Vec<usize>,
    result: usize,
    base: Vec<(usize, Base)>,
    edges: Vec<(usize, usize)>,
}
struct Channel {
    rank: usize,
    tokens: Vec<usize>,
    counts: Map<N, u64>,
    uses: Vec<(usize, usize)>,
}
#[derive(Default)]
struct Edge {
    parents: Map<N, Map<N, u64>>,
    rules: Vec<(usize, usize)>,
    pending: Map<(N, N), i64>,
}
struct Receipt {
    result: Value,
    inputs: Option<Vec<Value>>,
}
#[derive(Default, Serialize)]
pub(super) struct Metrics {
    pub commit_batches: usize,
    pub notifications: usize,
    pub unchanged_receipts: usize,
    pub base_deltas: usize,
    pub link_deltas: usize,
    pub propagated_links: usize,
    pub bit_changes: usize,
    pub requests: usize,
    pub reference_checks: usize,
    pub channels: usize,
    pub link_families: usize,
    pub words_per_marker: usize,
    pub receipts: usize,
    pub input_overrides: usize,
    pub links: usize,
    pub support_entries: usize,
    pub live_markers: usize,
    pub receipt_key_words: usize,
    pub general_adjacency_rows: usize,
    pub query_graph_reads: usize,
}
pub(super) struct Markers {
    sorts: Map<String, usize>,
    schemas: Vec<Option<Schema>>,
    channels: Vec<Channel>,
    edges: Vec<Edge>,
    receipts: Vec<Map<Key, Receipt>>,
    pending: Vec<Map<(usize, N), i64>>,
    bits: Map<N, Vec<u64>>,
    width: usize,
    reference: Option<coordinates::Coordinates>,
    pub metrics: Metrics,
}
fn add<K: std::hash::Hash + Eq + Copy>(map: &mut Map<K, i64>, key: K, delta: i64) {
    if delta == 0 {
        return;
    }
    let n = map.entry(key).or_default();
    *n += delta;
    if *n == 0 {
        map.remove(&key);
    }
}
fn count_delta(old: u64, delta: i64) -> u64 {
    if delta < 0 {
        old.checked_sub(delta.unsigned_abs())
            .expect("negative marker support")
    } else {
        old.checked_add(delta as u64)
            .expect("marker support overflow")
    }
}
impl Markers {
    pub fn new(names: &[String], lib: &Library, audit: bool) -> Self {
        let mut sorts = Map::default();
        let mut sid = |s: &str| {
            let n = sorts.len();
            *sorts.entry(s.to_owned()).or_insert(n)
        };
        let schemas = names
            .iter()
            .map(|n| {
                lib.constructors.get(n).map(|(args, out)| Schema {
                    args: args.iter().map(|s| sid(s)).collect(),
                    result: sid(out),
                    base: vec![],
                    edges: vec![],
                })
            })
            .collect();
        for m in &lib.models {
            sid(&m.sort);
        }
        let ids: Map<_, _> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();
        let mut this = Self {
            sorts,
            schemas,
            channels: vec![],
            edges: vec![],
            receipts: (0..names.len()).map(|_| Map::default()).collect(),
            pending: vec![Map::default()],
            bits: Map::default(),
            width: lib.vocabulary.len().div_ceil(64),
            reference: audit.then(|| coordinates::Coordinates::new(names, lib)),
            metrics: Metrics::default(),
        };
        let mut features = Map::default();
        let mut edges = Map::default();
        for (i, token) in lib.vocabulary.iter().enumerate() {
            let t = &token.token;
            let op = ids[t[1].as_str().unwrap()];
            let n = |i: usize| t[i].as_u64().unwrap() as usize;
            let f = match t[0].as_str().unwrap() {
                "root" => Feature::Head(op),
                "port" => Feature::Port(op, n(2)),
                "alias" => Feature::Alias(op, n(2), n(3)),
                "return-alias" => Feature::Return(op, n(2)),
                "path" => Feature::Path(
                    op,
                    n(2),
                    Box::new(Feature::Head(ids[t[3].as_str().unwrap()])),
                ),
                "path2" => Feature::Path(
                    op,
                    n(2),
                    Box::new(Feature::Path(
                        ids[t[3].as_str().unwrap()],
                        n(4),
                        Box::new(Feature::Head(ids[t[5].as_str().unwrap()])),
                    )),
                ),
                _ => panic!("unsupported marker feature"),
            };
            let c = this.intern(f, &mut features, &mut edges);
            this.channels[c].tokens.push(i);
        }
        this.metrics.channels = this.channels.len();
        this.metrics.link_families = this.edges.len();
        this.metrics.words_per_marker = this.width;
        this
    }
    fn intern(
        &mut self,
        f: Feature,
        features: &mut Map<Feature, usize>,
        edges: &mut Map<(usize, usize), usize>,
    ) -> usize {
        if let Some(c) = features.get(&f) {
            return *c;
        }
        let child = if let Feature::Path(_, _, child) = &f {
            Some(self.intern(*child.clone(), features, edges))
        } else {
            None
        };
        let rank = child.map_or(0, |c| self.channels[c].rank + 1);
        let id = self.channels.len();
        self.channels.push(Channel {
            rank,
            tokens: vec![],
            counts: Map::default(),
            uses: vec![],
        });
        while self.pending.len() <= rank {
            self.pending.push(Map::default());
        }
        match &f {
            Feature::Head(t) => self.schemas[*t]
                .as_mut()
                .unwrap()
                .base
                .push((id, Base::Head)),
            Feature::Port(t, p) => self.schemas[*t]
                .as_mut()
                .unwrap()
                .base
                .push((id, Base::Port(*p))),
            Feature::Alias(t, a, b) => self.schemas[*t]
                .as_mut()
                .unwrap()
                .base
                .push((id, Base::Alias(*a, *b))),
            Feature::Return(t, p) => self.schemas[*t]
                .as_mut()
                .unwrap()
                .base
                .push((id, Base::Return(*p))),
            Feature::Path(t, p, _) => {
                let e = if let Some(e) = edges.get(&(*t, *p)) {
                    *e
                } else {
                    let e = self.edges.len();
                    self.edges.push(Edge::default());
                    edges.insert((*t, *p), e);
                    self.schemas[*t].as_mut().unwrap().edges.push((e, *p));
                    e
                };
                self.edges[e].rules.push((id, child.unwrap()));
                self.channels[child.unwrap()].uses.push((e, id));
            }
        }
        features.insert(f, id);
        id
    }
    fn stage(&mut self, t: usize, args: &[Value], result: Value, delta: i64) {
        let s = self.schemas[t].as_ref().unwrap();
        let r = N(s.result, result);
        for (channel, base) in &s.base {
            let node = match base {
                Base::Head => Some(r),
                Base::Port(p) => Some(N(s.args[*p], args[*p])),
                Base::Alias(a, b) => {
                    (s.args[*a] == s.args[*b] && args[*a] == args[*b]).then_some(r)
                }
                Base::Return(p) => (s.args[*p] == s.result && args[*p] == result).then_some(r),
            };
            if let Some(n) = node {
                add(&mut self.pending[0], (*channel, n), delta);
            }
        }
        for (edge, p) in &s.edges {
            add(
                &mut self.edges[*edge].pending,
                (N(s.args[*p], args[*p]), r),
                delta,
            );
        }
    }
    /// Native keys identify removals. Most receipts need only the result; keep
    /// an input override only if a native journal key differs from its payload.
    pub fn update(&mut self, t: usize, key: &[Value], row: Option<&[Value]>) {
        if self.schemas[t].is_none() {
            return;
        }
        self.metrics.notifications += 1;
        let width = self.schemas[t].as_ref().unwrap().args.len();
        if let (Some(old), Some(row)) = (self.receipts[t].get(key), row) {
            if old.result == row[width] && old.inputs.as_deref().unwrap_or(key) == &row[..width] {
                self.metrics.unchanged_receipts += 1;
                return;
            }
        }
        if let Some(old) = self.receipts[t].remove(key) {
            self.metrics.receipts -= 1;
            self.metrics.receipt_key_words -= key.len();
            self.metrics.input_overrides -= usize::from(old.inputs.is_some());
            self.stage(t, old.inputs.as_deref().unwrap_or(key), old.result, -1);
            if let Some(r) = &mut self.reference {
                r.remove(t, key);
            }
        }
        if let Some(row) = row {
            self.stage(t, &row[..width], row[width], 1);
            let inputs = (&row[..width] != key).then(|| row[..width].to_vec());
            self.metrics.receipts += 1;
            self.metrics.receipt_key_words += key.len();
            self.metrics.input_overrides += usize::from(inputs.is_some());
            self.receipts[t].insert(
                key.to_vec(),
                Receipt {
                    result: row[width],
                    inputs,
                },
            );
            if let Some(r) = &mut self.reference {
                r.insert(t, key, row);
            }
        }
    }
    pub fn clear(&mut self, t: usize) {
        let old = std::mem::take(&mut self.receipts[t]);
        for (k, r) in old {
            self.metrics.receipts -= 1;
            self.metrics.receipt_key_words -= k.len();
            self.metrics.input_overrides -= usize::from(r.inputs.is_some());
            self.stage(t, r.inputs.as_deref().unwrap_or(&k), r.result, -1);
            if let Some(reference) = &mut self.reference {
                reference.remove(t, &k);
            }
        }
    }
    pub fn commit(&mut self) {
        self.metrics.commit_batches += 1;
        // Δ(E·X) = ΔE·X_old + E_new·ΔX. First install column projections and
        // account for old child markers; then propagate net Boolean toggles in
        // feature-depth order. Graph cycles cannot create a feature-cycle.
        for e in 0..self.edges.len() {
            let pending = std::mem::take(&mut self.edges[e].pending);
            self.metrics.link_deltas += pending.len();
            for ((child, parent), delta) in pending {
                for &(out, input) in &self.edges[e].rules {
                    if self.channels[input].counts.contains_key(&child) {
                        let rank = self.channels[out].rank;
                        add(&mut self.pending[rank], (out, parent), delta);
                    }
                }
                let parents = self.edges[e].parents.entry(child).or_default();
                let old = parents.get(&parent).copied().unwrap_or(0);
                let new = count_delta(old, delta);
                if new == 0 {
                    parents.remove(&parent);
                    self.metrics.links -= 1;
                } else {
                    parents.insert(parent, new);
                    self.metrics.links += usize::from(old == 0);
                }
                if parents.is_empty() {
                    self.edges[e].parents.remove(&child);
                }
            }
        }
        for rank in 0..self.pending.len() {
            let pending = std::mem::take(&mut self.pending[rank]);
            if rank == 0 {
                self.metrics.base_deltas += pending.len();
            }
            for ((id, n), delta) in pending {
                let channel = &mut self.channels[id];
                let old = channel.counts.get(&n).copied().unwrap_or(0);
                let new = count_delta(old, delta);
                if new == 0 {
                    channel.counts.remove(&n);
                    self.metrics.support_entries -= 1;
                } else {
                    channel.counts.insert(n, new);
                    self.metrics.support_entries += usize::from(old == 0);
                }
                let toggle = match (old == 0, new == 0) {
                    (true, false) => 1,
                    (false, true) => -1,
                    _ => 0,
                };
                if toggle == 0 {
                    continue;
                }
                if !channel.tokens.is_empty() {
                    let bits = self.bits.entry(n).or_insert_with(|| vec![0; self.width]);
                    for &token in &channel.tokens {
                        let bit = 1u64 << (token % 64);
                        if toggle > 0 {
                            bits[token / 64] |= bit;
                        } else {
                            bits[token / 64] &= !bit;
                        }
                        self.metrics.bit_changes += 1;
                    }
                    if bits.iter().all(|w| *w == 0) {
                        self.bits.remove(&n);
                    }
                }
                for &(e, out) in &self.channels[id].uses {
                    if let Some(parents) = self.edges[e].parents.get(&n) {
                        self.metrics.propagated_links += parents.len();
                        for (&parent, &count) in parents {
                            add(
                                &mut self.pending[rank + 1],
                                (out, parent),
                                toggle * (count as i64),
                            );
                        }
                    }
                }
            }
        }
        self.metrics.live_markers = self.bits.len();
        if let Some(r) = &mut self.reference {
            r.commit();
        }
    }
    pub fn signature(&mut self, sort: &str, value: Value, audit: bool) -> Vec<usize> {
        self.metrics.requests += 1;
        let n = N(self.sorts[sort], value);
        let mut result = vec![];
        if let Some(bits) = self.bits.get(&n) {
            for (word, &value) in bits.iter().enumerate() {
                let mut value = value;
                while value != 0 {
                    let bit = value.trailing_zeros() as usize;
                    result.push(word * 64 + bit);
                    value &= value - 1;
                }
            }
        }
        if audit {
            let oracle = self
                .reference
                .as_mut()
                .expect("marker audit needs independent graph oracle");
            assert_eq!(
                result,
                oracle.signature(sort, value, true),
                "differential marker differs from graph feature"
            );
            self.metrics.reference_checks += 1;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(n: u32) -> Value {
        Value::new_const(n)
    }
    fn fixture() -> Markers {
        let mut lib = Library {
            plans: vec![],
            models: vec![Model {
                sort: "T".into(),
                channel: "".into(),
                sort_hash: 0,
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
        Markers::new(&["A".into(), "B".into(), "L".into()], &lib, true)
    }
    #[test]
    fn late_heads_multiple_projections_and_retraction() {
        let mut m = fixture();
        m.update(0, &[v(1), v(2)], Some(&[v(1), v(2), v(3)]));
        m.update(0, &[v(1), v(5)], Some(&[v(1), v(5), v(3)]));
        m.commit();
        assert_eq!(m.signature("T", v(3), true), vec![0]);
        m.update(1, &[v(4)], Some(&[v(4), v(1)]));
        m.update(2, &[v(8)], Some(&[v(8), v(4)]));
        m.commit();
        assert_eq!(m.signature("T", v(3), true), vec![0, 2, 3]);
        m.update(0, &[v(1), v(2)], None);
        m.commit();
        assert_eq!(m.signature("T", v(3), true), vec![0, 2, 3]);
        m.update(2, &[v(8)], None);
        m.commit();
        assert_eq!(m.signature("T", v(3), true), vec![0, 2]);
        m.clear(1);
        m.commit();
        assert_eq!(m.signature("T", v(3), true), vec![0]);
    }
    #[test]
    fn child_and_edges_change_in_same_batch_without_double_counting() {
        let mut m = fixture();
        m.update(0, &[v(1), v(1)], Some(&[v(1), v(1), v(1)]));
        m.update(1, &[v(1)], Some(&[v(1), v(1)]));
        m.update(2, &[v(0)], Some(&[v(0), v(1)]));
        m.commit();
        assert_eq!(m.signature("T", v(1), true), vec![0, 1, 2, 3, 4, 5]);
        m.clear(0);
        m.clear(1);
        m.clear(2);
        m.commit();
        assert!(m.signature("T", v(1), true).is_empty());
        assert_eq!(m.metrics.links, 0);
        assert_eq!(m.metrics.support_entries, 0);
    }
    #[test]
    fn differential_program_matches_graph_through_batched_rebuilds() {
        let mut m = fixture();
        let mut seed = 7u32;
        for batch in 0..300 {
            for _ in 0..7 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let a = seed % 13;
                let t = (seed % 3) as usize;
                let key = if t == 0 {
                    vec![v(a), v((seed / 7) % 13)]
                } else {
                    vec![v(a)]
                };
                let mut row = key.clone();
                row.push(v((seed / 13) % 13));
                m.update(t, &key, if seed & 4 != 0 { Some(&row) } else { None });
            }
            if batch % 41 == 0 {
                m.clear(1);
            }
            m.commit();
            for n in 0..13 {
                m.signature("T", v(n), true);
            }
        }
    }
    #[test]
    fn journal_key_payload_difference_uses_only_an_undo_override() {
        let mut m = fixture();
        m.update(0, &[v(8), v(9)], Some(&[v(1), v(1), v(1)]));
        m.commit();
        assert_eq!(m.metrics.input_overrides, 1);
        assert_eq!(m.signature("T", v(1), true), vec![0, 1, 4, 5]);
        m.update(0, &[v(8), v(9)], None);
        m.commit();
        assert!(m.signature("T", v(1), true).is_empty());
    }
}
