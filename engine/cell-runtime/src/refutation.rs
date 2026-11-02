//! Exact, online negative receipts for marker-anchored residual queries.
//! These are NOT RipSat completeness certificates. A receipt is admitted only
//! after an unrestricted, unsuccessful residual search, and retains every read
//! range that justified that failure. Hashes choose supersets, never truth.
use crate::{
    graph::Graph,
    query::{QueryIndexes, ReadStamp},
    value::Value,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Operand {
    Bound(Value),
    Variable(usize),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Operator {
    Table(usize),
    Primitive(String, String),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    // Alpha-normalized residual conjunction; RHS and ruleset do not affect
    // existence on the same immutable visible snapshot. Atom order is retained.
    pub atoms: Vec<(Operator, Vec<Operand>)>,
}

/// Diagnostic only. Depth counts table atoms (seed = 1), never index handles.
/// Existing singleton point projections are outside this profile's read counts.
#[derive(Default, serde::Serialize)]
struct DepthWork {
    seeds: u64,
    reads: BTreeMap<usize, u64>,
    candidate_rows: BTreeMap<usize, u64>,
    refuted_before_depth: BTreeMap<usize, u64>,
    deepest_seed: BTreeMap<usize, u64>,
}
pub struct DepthProfile {
    enabled: bool,
    active: Option<(usize, usize)>,
    rules: BTreeMap<usize, DepthWork>,
}
impl DepthProfile {
    pub fn new() -> Self {
        Self {
            enabled: std::env::var("CELL_CPU_REFUTATION_PROFILE").as_deref() == Ok("1"),
            active: None,
            rules: BTreeMap::new(),
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn begin(&mut self, rule: usize) {
        if self.enabled {
            self.active = Some((rule, 1));
            self.rules.entry(rule).or_default().seeds += 1;
        }
    }
    pub fn read(&mut self, depth: usize) {
        if let Some((r, max)) = &mut self.active {
            *max = (*max).max(depth);
            *self
                .rules
                .get_mut(r)
                .unwrap()
                .reads
                .entry(depth)
                .or_default() += 1;
        }
    }
    pub fn row(&mut self, depth: usize) {
        if let Some((r, _)) = self.active {
            *self
                .rules
                .get_mut(&r)
                .unwrap()
                .candidate_rows
                .entry(depth)
                .or_default() += 1;
        }
    }
    pub fn reject(&mut self, depth: usize) {
        if let Some((r, _)) = self.active {
            *self
                .rules
                .get_mut(&r)
                .unwrap()
                .refuted_before_depth
                .entry(depth)
                .or_default() += 1;
        }
    }
    pub fn finish(&mut self) {
        if let Some((r, depth)) = self.active.take() {
            *self
                .rules
                .get_mut(&r)
                .unwrap()
                .deepest_seed
                .entry(depth)
                .or_default() += 1;
        }
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"rules":self.rules,
        "scope":"matched delta seeds; residual solver and compiled merge/marker reads; depth counts bound table atoms including seed; singleton point projections excluded; not native Free Join"})
    }
}
impl Key {
    pub fn project(&self, positions: &BTreeSet<usize>) -> Self {
        let mut variables = HashMap::new();
        Self {
            atoms: self
                .atoms
                .iter()
                .enumerate()
                .filter(|(i, _)| positions.contains(i))
                .map(|(_, a)| {
                    (
                        a.0.clone(),
                        a.1.iter()
                            .map(|v| match v {
                                Operand::Variable(i) => {
                                    let n = variables.len();
                                    Operand::Variable(*variables.entry(*i).or_insert(n))
                                }
                                _ => v.clone(),
                            })
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}
/// A complete current port domain has no value in this integer half-line.
/// i128 normalizes strict i64 bounds without overflowing at MIN/MAX.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct GuardGroup {
    port: Key,
    variable: usize,
    lower: bool,
}
fn guard_form(atoms: &[(Operator, Vec<Operand>)]) -> Option<(GuardGroup, i128)> {
    if atoms.len() != 2 {
        return None;
    }
    let (table, guard) = match (&atoms[0].0, &atoms[1].0) {
        (Operator::Table(_), Operator::Primitive(..)) => (0, 1),
        (Operator::Primitive(..), Operator::Table(_)) => (1, 0),
        _ => return None,
    };
    let (Operator::Primitive(name, sort), args) = &atoms[guard] else {
        return None;
    };
    if sort != "Unit" || args.len() != 3 {
        return None;
    }
    let (v, t, op) = match (&args[0], &args[1]) {
        (Operand::Variable(v), Operand::Bound(Value::Int(t))) => (*v, *t, name.as_str()),
        (Operand::Bound(Value::Int(t)), Operand::Variable(v)) => (
            *v,
            *t,
            match name.as_str() {
                "<" => ">",
                ">" => "<",
                "<=" => ">=",
                ">=" => "<=",
                _ => return None,
            },
        ),
        _ => return None,
    };
    match &args[2] {
        Operand::Bound(Value::Int(0)) => {}
        Operand::Variable(o) if *o != v && !atoms[table].1.contains(&Operand::Variable(*o)) => {}
        _ => return None,
    }
    let mut vars = HashMap::new();
    for a in &atoms[table].1 {
        if let Operand::Variable(i) = a {
            let n = vars.len();
            vars.entry(*i).or_insert(n);
        }
    }
    let variable = *vars.get(&v)?;
    let (lower, bound) = match op {
        "<" => (false, t as i128 - 1),
        "<=" => (false, t as i128),
        ">" => (true, t as i128 + 1),
        ">=" => (true, t as i128),
        _ => return None,
    };
    Some((
        GuardGroup {
            port: Key {
                atoms: vec![(
                    atoms[table].0.clone(),
                    atoms[table]
                        .1
                        .iter()
                        .map(|a| match a {
                            Operand::Variable(i) => Operand::Variable(vars[i]),
                            _ => a.clone(),
                        })
                        .collect(),
                )],
            },
            variable,
            lower,
        },
        bound,
    ))
}
#[derive(Default, Clone)]
pub struct Evidence {
    pub shadowing: bool,
    // Search-local atom ids. Never replay these ids into a different query.
    pub visited: Vec<usize>,
    pub reads: Vec<ReadStamp>,
    pub delta_restrictions: u64,
    pub query_reads: u64,
}
struct Receipt {
    id: u64,
    tables: BTreeSet<usize>,
    evidence: Evidence,
    candidates: u64,
    rule: usize,
    epoch: u64,
}
#[derive(Default, serde::Serialize)]
pub struct Stats {
    pub bound_probes: u64,
    pub bound_hits: u64,
    pub bound_changed_threshold_hits: u64,
    pub bound_proofs: u64,
    pub validation_checks: u64,
    pub affected_proofs: u64,
    pub core_input_atoms: u64,
    pub core_atoms: u64,
    pub sliced_proofs: u64,
    pub admitted_sliced_proofs: u64,
    pub admitted_removed_atoms: u64,
    pub admitted_core_histogram: BTreeMap<usize, u64>,
    pub core_histogram: BTreeMap<usize, u64>,
    pub port_guard_proofs: u64,
    pub compiled_attempts: u64,
    pub compiled_hits: u64,
    pub shadow_checks: u64,
    pub shadow_candidates: u64,
    pub shadow_keys: u64,
    pub shadow_by_rule: BTreeMap<usize, (u64, u64, u64)>,
    pub lookups: u64,
    pub hits: u64,
    pub admitted: u64,
    pub invalidated: u64,
    pub delta_incomplete: u64,
    pub positive_searches: u64,
    pub certification_candidates: u64,
    /// Historical search work attached to hits; NOT measured work saved.
    pub prior_search_candidates_on_hits: u64,
    pub peak_receipts: usize,
    pub peak_read_ranges: usize,
    pub per_rule_hits: HashMap<usize, u64>,
    pub stronger_query_hits: u64,
    pub later_epoch_hits: u64,
    pub other_ruleset_hits: u64,
    pub continuation_entries: u64,
    pub max_continuation_depth: u32,
    pub containment_checks: u64,
    pub proof_index_nodes: u64,
    pub index_only_failures: u64,
}
pub struct Refutation {
    pub enabled: bool,
    pub multistage: bool,
    pub variant: String,
    pub scoped: bool,
    pub cores: bool,
    pub shadow: bool,
    pub bounds: bool,
    guard_index: HashMap<GuardGroup, BTreeMap<i128, Vec<Key>>>,
    pub compiled_entries: bool,
    watchers: BTreeMap<usize, BTreeSet<u64>>,
    by_id: BTreeMap<u64, Key>,
    next_id: u64,
    receipts: HashMap<Key, Receipt>,
    shapes: HashMap<Vec<Operator>, ProofIndex>,
    read_ranges: usize,
    published_revision: u64,
    pub stats: Stats,
}
impl Refutation {
    pub fn new(enabled: bool) -> Self {
        let variant =
            std::env::var("CELL_CPU_REFUTATION_VARIANT").unwrap_or_else(|_| "legacy".into());
        assert!(
            matches!(
                variant.as_str(),
                "legacy" | "scoped" | "cores" | "bounds" | "shadow"
            ),
            "invalid refutation variant"
        );
        Self {
            scoped: variant != "legacy",
            cores: matches!(variant.as_str(), "cores" | "bounds" | "shadow"),
            shadow: variant == "shadow",
            bounds: variant == "bounds"
                || (variant == "shadow"
                    && std::env::var("CELL_CPU_REFUTATION_BOUNDS").as_deref() != Ok("0")),
            variant,
            guard_index: HashMap::new(),
            compiled_entries: std::env::var("CELL_CPU_REFUTATION_ENTRIES").as_deref() != Ok("0"),
            watchers: BTreeMap::new(),
            by_id: BTreeMap::new(),
            next_id: 0,
            enabled,
            multistage: std::env::var("CELL_CPU_REFUTATION_SCOPE").as_deref() == Ok("multistage"),
            receipts: HashMap::new(),
            shapes: HashMap::new(),
            read_ranges: 0,
            published_revision: 0,
            stats: Stats::default(),
        }
    }
    pub fn reject(
        &mut self,
        rule: usize,
        key: &Key,
        indexes: &QueryIndexes,
        graph: &Graph,
    ) -> Option<Evidence> {
        self.stats.lookups += 1;
        if let Some(r) = self.receipts.get(key) {
            if r.evidence.reads.iter().all(|s| {
                self.stats.validation_checks += 1;
                indexes.valid_read(graph, s)
            }) {
                self.stats.hits += 1;
                *self.stats.per_rule_hits.entry(rule).or_default() += 1;
                self.stats.prior_search_candidates_on_hits += r.candidates;
                self.record_origin(rule, r.rule, r.epoch, graph);
                return Some(self.receipts[key].evidence.clone());
            }
            self.remove(key);
            self.stats.invalidated += 1;
        }
        if self.bounds && !self.guard_index.is_empty() {
            for pair in key.atoms.windows(2) {
                let Some((group, bound)) = guard_form(pair) else {
                    continue;
                };
                self.stats.bound_probes += 1;
                let Some(index) = self.guard_index.get(&group) else {
                    continue;
                };
                let candidates: Box<dyn Iterator<Item = (&i128, &Vec<Key>)> + '_> = if group.lower {
                    Box::new(index.range(..=bound))
                } else {
                    Box::new(index.range(bound..).rev())
                };
                let mut found = None;
                'outer: for (threshold, keys) in candidates {
                    for k in keys {
                        let Some(r) = self.receipts.get(k) else {
                            continue;
                        };
                        if r.evidence.reads.iter().all(|s| {
                            self.stats.validation_checks += 1;
                            indexes.valid_read(graph, s)
                        }) {
                            found = Some((k.clone(), *threshold));
                            break 'outer;
                        }
                    }
                }
                if let Some((k, threshold)) = found {
                    let r = &self.receipts[&k];
                    let evidence = r.evidence.clone();
                    let origin = (r.rule, r.epoch, r.candidates);
                    self.stats.hits += 1;
                    self.stats.bound_hits += 1;
                    self.stats.bound_changed_threshold_hits += u64::from(threshold != bound);
                    *self.stats.per_rule_hits.entry(rule).or_default() += 1;
                    self.stats.prior_search_candidates_on_hits += origin.2;
                    self.record_origin(rule, origin.0, origin.1, graph);
                    return Some(evidence);
                }
            }
        }
        if self.multistage {
            // A negative subprogram also refutes a stronger later query. Only
            // ordered contiguous fragments are indexed; no subset/permutation
            // search or representatives. Not finding a proof always falls back.
            let ops = key.atoms.iter().map(|a| a.0.clone()).collect::<Vec<_>>();
            for start in 0..ops.len() {
                for end in start + 2..=ops.len() {
                    let Some(index) = self.shapes.get(&ops[start..end]) else {
                        continue;
                    };
                    let values = key.atoms[start..end]
                        .iter()
                        .flat_map(|a| a.1.iter())
                        .collect::<Vec<_>>();
                    let mut nodes = 0;
                    let mut checks = 0;
                    let negative = index.find(&values, 0, &mut vec![], &mut nodes, &mut |k| {
                        checks += 1;
                        k != key
                            && self.receipts.get(k).is_some_and(|r| {
                                r.evidence.reads.iter().all(|s| {
                                    self.stats.validation_checks += 1;
                                    indexes.valid_read(graph, s)
                                })
                            })
                    });
                    self.stats.proof_index_nodes += nodes;
                    self.stats.containment_checks += checks;
                    if let Some(negative) = negative {
                        debug_assert!(entails(&key.atoms[start..end], &negative.atoms));
                        let r = &self.receipts[negative];
                        let evidence = r.evidence.clone();
                        let origin = (r.rule, r.epoch, r.candidates);
                        self.stats.hits += 1;
                        self.stats.stronger_query_hits += 1;
                        *self.stats.per_rule_hits.entry(rule).or_default() += 1;
                        self.stats.prior_search_candidates_on_hits += origin.2;
                        self.record_origin(rule, origin.0, origin.1, graph);
                        return Some(evidence);
                    }
                }
            }
        }
        None
    }
    fn record_origin(&mut self, rule: usize, origin: usize, epoch: u64, graph: &Graph) {
        if epoch < graph.epoch {
            self.stats.later_epoch_hits += 1;
        }
        let rs = &graph.program.artifact["recipe"]["rules"];
        if rs[rule]["metadata"]["source_set"] != rs[origin]["metadata"]["source_set"] {
            self.stats.other_ruleset_hits += 1;
        }
    }
    pub fn finish(
        &mut self,
        rule: usize,
        epoch: u64,
        mut key: Key,
        mut evidence: Evidence,
        candidates: u64,
        found: bool,
    ) {
        let input_atoms = key.atoms.len();
        if self.cores && !found && evidence.delta_restrictions == 0 {
            self.stats.core_input_atoms += key.atoms.len() as u64;
            let core = key.project(&evidence.visited.iter().copied().collect());
            self.stats.core_atoms += core.atoms.len() as u64;
            self.stats.sliced_proofs += u64::from(core.atoms.len() < key.atoms.len());
            *self
                .stats
                .core_histogram
                .entry(core.atoms.len())
                .or_default() += 1;
            key = core;
        }
        evidence.visited.clear();
        self.stats.certification_candidates += candidates;
        if found {
            self.stats.positive_searches += 1;
        } else if evidence.delta_restrictions > 0 {
            // "No new match in this delta partition" is not "no match".
            self.stats.delta_incomplete += 1;
        } else if (self.multistage && !self.cores && evidence.query_reads < 2)
            || (self.cores
                && (key.atoms.len() < 2
                    || evidence.query_reads == 0
                    || (evidence.query_reads < 2 && candidates == 0)))
        {
            // A one-read failure is already represented by an empty index/key
            // lookup. Do not replicate it once for every consumer conjunction.
            self.stats.index_only_failures += 1;
        } else {
            if self.cores {
                self.stats.admitted_sliced_proofs += u64::from(key.atoms.len() < input_atoms);
                self.stats.admitted_removed_atoms += (input_atoms - key.atoms.len()) as u64;
                *self
                    .stats
                    .admitted_core_histogram
                    .entry(key.atoms.len())
                    .or_default() += 1;
            }
            if self.cores && evidence.query_reads == 1 {
                self.stats.port_guard_proofs += 1;
            }
            if self.scoped {
                self.remove(&key);
            }
            let tables = if self.scoped {
                evidence
                    .reads
                    .iter()
                    .map(|r| match r {
                        ReadStamp::Table { table, .. } | ReadStamp::Bucket { table, .. } => *table,
                    })
                    .collect()
            } else {
                BTreeSet::new()
            };
            let id = self.next_id;
            self.next_id = self
                .next_id
                .checked_add(1)
                .expect("refutation id exhausted");
            if self.scoped {
                self.by_id.insert(id, key.clone());
                for &t in &tables {
                    self.watchers.entry(t).or_default().insert(id);
                }
            }
            if self.multistage && !self.receipts.contains_key(&key) {
                let shape = key.atoms.iter().map(|a| a.0.clone()).collect();
                self.shapes
                    .entry(shape)
                    .or_default()
                    .insert(&tokens(&key), key.clone());
            }
            if self.bounds {
                if let Some((group, bound)) = guard_form(&key.atoms) {
                    self.guard_index
                        .entry(group)
                        .or_default()
                        .entry(bound)
                        .or_default()
                        .push(key.clone());
                    self.stats.bound_proofs += 1;
                }
            }
            self.read_ranges += evidence.reads.len();
            if let Some(old) = self.receipts.insert(
                key,
                Receipt {
                    id,
                    tables,
                    evidence,
                    candidates,
                    rule,
                    epoch,
                },
            ) {
                self.read_ranges -= old.evidence.reads.len();
            }
            self.stats.admitted += 1;
            self.stats.peak_receipts = self.stats.peak_receipts.max(self.receipts.len());
            self.stats.peak_read_ranges = self.stats.peak_read_ranges.max(self.read_ranges);
        }
    }
    pub fn publish(&mut self, indexes: &QueryIndexes, graph: &Graph, changed: &BTreeSet<usize>) {
        if self.scoped {
            let affected = changed
                .iter()
                .filter_map(|t| self.watchers.get(t))
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            self.stats.affected_proofs += affected.len() as u64;
            for id in affected {
                let key = &self.by_id[&id];
                let r = &self.receipts[key];
                if !r.evidence.reads.iter().all(|s| {
                    self.stats.validation_checks += 1;
                    indexes.valid_read(graph, s)
                }) {
                    let key = key.clone();
                    self.remove(&key);
                    self.stats.invalidated += 1;
                }
            }
            return;
        }
        if self.published_revision == indexes.revision {
            return;
        }
        self.published_revision = indexes.revision;
        // Reclaim stale evidence at the published snapshot, including hidden
        // markers. Cached references do not pin graph frames or provider chains.
        let removed = self
            .receipts
            .iter()
            .filter(|(_, r)| {
                !r.evidence.reads.iter().all(|s| {
                    self.stats.validation_checks += 1;
                    indexes.valid_read(graph, s)
                })
            })
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>();
        self.stats.invalidated += removed.len() as u64;
        for key in removed {
            self.remove(&key);
        }
    }
    fn remove(&mut self, key: &Key) {
        if self.bounds {
            if let Some((group, bound)) = guard_form(&key.atoms) {
                if let Some(index) = self.guard_index.get_mut(&group) {
                    if let Some(keys) = index.get_mut(&bound) {
                        keys.retain(|k| k != key);
                        if keys.is_empty() {
                            index.remove(&bound);
                        }
                    }
                    if index.is_empty() {
                        self.guard_index.remove(&group);
                    }
                }
            }
        }
        if let Some(old) = self.receipts.remove(key) {
            self.read_ranges -= old.evidence.reads.len();
            if self.scoped {
                self.by_id.remove(&old.id);
                for t in old.tables {
                    let w = self.watchers.get_mut(&t).unwrap();
                    w.remove(&old.id);
                    if w.is_empty() {
                        self.watchers.remove(&t);
                    }
                }
            }
        }
        let shape = key.atoms.iter().map(|a| a.0.clone()).collect::<Vec<_>>();
        if let Some(index) = self.shapes.get_mut(&shape) {
            index.remove(&tokens(key));
            if index.empty() {
                self.shapes.remove(&shape);
            }
        }
    }
    pub fn shadow_work(&mut self, rid: usize, candidates: u64, keys: u64) {
        self.stats.shadow_checks += 1;
        self.stats.shadow_candidates += candidates;
        self.stats.shadow_keys += keys;
        let entry = self.stats.shadow_by_rule.entry(rid).or_default();
        entry.0 += 1;
        entry.1 += candidates;
        entry.2 += keys;
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"variant":self.variant,"bounds":self.bounds,"compiled_entries":self.compiled_entries,"multistage_scope":self.multistage,"stats":self.stats,
            "live_receipts":self.receipts.len(),
            "live_read_ranges":self.read_ranges,
            "scope":"online complete query negatives from marker or validated multi-stage recipe entries; not baked Cell closure or static impossibility"})
    }
}

/// Does the candidate conjunction impose at least the negative's constraints?
/// Negative variables may map to candidate variables or constants, with all
/// repeated occurrences preserving correlation. A fixed negative value cannot
/// be matched by an unconstrained candidate variable.
fn entails(candidate: &[(Operator, Vec<Operand>)], negative: &[(Operator, Vec<Operand>)]) -> bool {
    if candidate.len() != negative.len() {
        return false;
    }
    let mut map = HashMap::new();
    candidate.iter().zip(negative).all(|((co, ca), (no, na))| {
        co == no
            && ca.len() == na.len()
            && ca.iter().zip(na).all(|(c, n)| match n {
                Operand::Bound(v) => matches!(c,Operand::Bound(w) if v==w),
                Operand::Variable(i) => map.entry(*i).or_insert_with(|| c.clone()) == c,
            })
    })
}

enum Token {
    Bound(Value),
    New,
    Same(usize),
}
fn tokens(key: &Key) -> Vec<Token> {
    let mut vars = HashMap::new();
    key.atoms
        .iter()
        .flat_map(|a| &a.1)
        .map(|v| match v {
            Operand::Bound(v) => Token::Bound(v.clone()),
            Operand::Variable(i) => {
                if let Some(j) = vars.get(i) {
                    Token::Same(*j)
                } else {
                    let j = vars.len();
                    vars.insert(*i, j);
                    Token::New
                }
            }
        })
        .collect()
}
#[derive(Default)]
struct ProofIndex {
    bound: HashMap<Value, ProofIndex>,
    new: Option<Box<ProofIndex>>,
    same: BTreeMap<usize, ProofIndex>,
    leaf: Option<Key>,
}
impl ProofIndex {
    fn empty(&self) -> bool {
        self.bound.is_empty() && self.new.is_none() && self.same.is_empty() && self.leaf.is_none()
    }
    fn insert(&mut self, path: &[Token], key: Key) {
        let Some((head, tail)) = path.split_first() else {
            self.leaf = Some(key);
            return;
        };
        match head {
            Token::Bound(v) => self.bound.entry(v.clone()).or_default().insert(tail, key),
            Token::New => self
                .new
                .get_or_insert_with(Default::default)
                .insert(tail, key),
            Token::Same(i) => self.same.entry(*i).or_default().insert(tail, key),
        }
    }
    fn remove(&mut self, path: &[Token]) {
        let Some((head, tail)) = path.split_first() else {
            self.leaf = None;
            return;
        };
        match head {
            Token::Bound(v) => {
                if let Some(c) = self.bound.get_mut(v) {
                    c.remove(tail);
                    if c.empty() {
                        self.bound.remove(v);
                    }
                }
            }
            Token::New => {
                if let Some(c) = self.new.as_mut() {
                    c.remove(tail);
                    if c.empty() {
                        self.new = None;
                    }
                }
            }
            Token::Same(i) => {
                if let Some(c) = self.same.get_mut(i) {
                    c.remove(tail);
                    if c.empty() {
                        self.same.remove(i);
                    }
                }
            }
        }
    }
    fn find<'a>(
        &'a self,
        values: &[&Operand],
        i: usize,
        vars: &mut Vec<Operand>,
        nodes: &mut u64,
        valid: &mut impl FnMut(&Key) -> bool,
    ) -> Option<&'a Key> {
        *nodes += 1;
        if i == values.len() {
            return self.leaf.as_ref().filter(|k| valid(k));
        }
        if let Operand::Bound(v) = values[i] {
            if let Some(c) = self.bound.get(v) {
                if let Some(k) = c.find(values, i + 1, vars, nodes, valid) {
                    return Some(k);
                }
            }
        }
        for (j, c) in &self.same {
            if vars.get(*j) == Some(values[i]) {
                if let Some(k) = c.find(values, i + 1, vars, nodes, valid) {
                    return Some(k);
                }
            }
        }
        if let Some(c) = &self.new {
            vars.push(values[i].clone());
            let result = c.find(values, i + 1, vars, nodes, valid);
            vars.pop();
            if result.is_some() {
                return result;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn atom(t: usize, args: Vec<Operand>) -> (Operator, Vec<Operand>) {
        (Operator::Table(t), args)
    }
    fn b(v: i64) -> Operand {
        Operand::Bound(Value::Int(v))
    }
    fn v(i: usize) -> Operand {
        Operand::Variable(i)
    }
    #[test]
    fn integer_guard_normalization_is_exact_at_signed_extremes() {
        for op in ["<", "<=", ">", ">="] {
            for mirror in [false, true] {
                for t in [i64::MIN, -2, 0, 2, i64::MAX] {
                    let args = if mirror {
                        vec![b(t), v(7), v(8)]
                    } else {
                        vec![v(7), b(t), v(8)]
                    };
                    let key = Key {
                        atoms: vec![
                            atom(0, vec![b(42), v(7)]),
                            (Operator::Primitive(op.into(), "Unit".into()), args),
                        ],
                    };
                    let (group, bound) = guard_form(&key.atoms).unwrap();
                    for x in [i64::MIN, -3, -2, 0, 2, 3, i64::MAX] {
                        let (a, c) = if mirror { (t, x) } else { (x, t) };
                        let actual = match op {
                            "<" => a < c,
                            "<=" => a <= c,
                            ">" => a > c,
                            _ => a >= c,
                        };
                        assert_eq!(
                            actual,
                            if group.lower {
                                (x as i128) >= bound
                            } else {
                                (x as i128) <= bound
                            }
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn slice_keeps_aliases_and_rejects_guard_result_aliases() {
        let key = Key {
            atoms: vec![
                atom(0, vec![v(9), v(5)]),
                atom(1, vec![v(5), v(9)]),
                atom(2, vec![b(2)]),
            ],
        };
        let k = key.project(&BTreeSet::from([0, 1]));
        assert_eq!(
            k.atoms,
            vec![atom(0, vec![v(0), v(1)]), atom(1, vec![v(1), v(0)])]
        );
        let bad = Key {
            atoms: vec![
                atom(0, vec![v(0)]),
                (
                    Operator::Primitive(">".into(), "Unit".into()),
                    vec![v(0), b(3), v(0)],
                ),
            ],
        };
        assert!(guard_form(&bad.atoms).is_none());
    }
    #[test]
    fn stronger_bindings_and_aliases_preserve_negative_implication() {
        let no = vec![atom(0, vec![b(7), v(0)]), atom(1, vec![v(0), v(1)])];
        assert!(entails(
            &vec![atom(0, vec![b(7), b(3)]), atom(1, vec![b(3), v(9)])],
            &no
        ));
        assert!(entails(
            &vec![atom(0, vec![b(7), v(5)]), atom(1, vec![v(5), v(5)])],
            &no
        ));
        assert!(!entails(
            &vec![atom(0, vec![b(8), v(5)]), atom(1, vec![v(5), v(6)])],
            &no
        ));
        assert!(!entails(
            &vec![atom(0, vec![b(7), v(5)]), atom(1, vec![v(6), v(7)])],
            &no
        ));
        assert!(!entails(
            &vec![atom(0, vec![v(7), v(5)]), atom(1, vec![v(5), v(6)])],
            &no
        ));
    }
    #[test]
    fn proof_index_matches_exact_containment_and_removal() {
        let choices = [b(0), b(1), v(0), v(1)];
        let mut keys = vec![];
        for a in &choices {
            for c in &choices {
                keys.push(Key {
                    atoms: vec![
                        atom(0, vec![a.clone(), v(2)]),
                        atom(1, vec![v(2), c.clone()]),
                    ],
                });
            }
        }
        let mut index = ProofIndex::default();
        for key in &keys {
            index.insert(&tokens(key), key.clone());
        }
        // Duplicate alpha-renamings denote the same pattern; validity concerns
        // semantic containment, not which equivalent stored key is returned.
        for mask in 0..256 {
            let args = (0..4)
                .map(|i| choices[(mask >> (i * 2)) & 3].clone())
                .collect::<Vec<_>>();
            let candidate = vec![atom(0, args[..2].to_vec()), atom(1, args[2..].to_vec())];
            let flat = candidate
                .iter()
                .flat_map(|a| a.1.iter())
                .collect::<Vec<_>>();
            let found = index.find(&flat, 0, &mut vec![], &mut 0, &mut |_| true);
            assert_eq!(
                found.is_some(),
                keys.iter().any(|k| entails(&candidate, &k.atoms))
            );
            if let Some(k) = found {
                assert!(entails(&candidate, &k.atoms));
            }
        }
        for key in &keys {
            index.remove(&tokens(key));
        }
        assert!(index.empty());
    }
}
