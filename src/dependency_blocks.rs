//! Diagnostic block index over committed dependency events. Does not move e-nodes.
#[allow(dead_code)]
#[path = "bin/rule_combine/compose.rs"]
mod pattern;
use egglog::{
    RowReadEvent, RuleActionOutcome, RuleMatchEvent, TraceSession, Value, WriteEvent, WriteOutcome,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
pub type BlockId = usize;
type Env = BTreeMap<String, Value>;

#[derive(Clone, Debug)]
pub struct RuleSpec {
    pub name: String,
    pub lhs: String,
    pub rhs: String,
    rewrite: Option<pattern::Rule>,
}
impl RuleSpec {
    /// Caller must register the exact unconditional rewrite executed by the engine.
    pub fn rewrite(name: &str, lhs: &str, rhs: &str) -> Self {
        Self {
            name: name.into(),
            lhs: lhs.into(),
            rhs: rhs.into(),
            rewrite: Some(pattern::Rule::new(name, lhs, rhs)),
        }
    }
    /// An explicit multi-atom/guarded prefix. Never passed to the equality composer.
    pub fn opaque(name: &str, lhs: &str, rhs: &str) -> Self {
        Self {
            name: name.into(),
            lhs: lhs.into(),
            rhs: rhs.into(),
            rewrite: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct EntryPrefix {
    pub rule: String,
    pub lhs: String,
    pub match_id: u64,
    pub bindings: Env,
}
#[derive(Clone, Debug)]
pub struct BoundaryRead {
    pub read_id: u64,
    pub consumer: u64,
    pub producer_write: Option<u64>,
    pub source_block: Option<BlockId>,
    pub table: String,
    pub key: Vec<Value>,
}
#[derive(Clone, Debug)]
pub struct Dependency {
    pub producer: u64,
    pub consumer: u64,
    pub write: u64,
    pub read: u64,
}
#[derive(Clone, Debug)]
pub struct CombinedPrefix {
    pub lhs: String,
    pub rhs: String,
    pub bindings: Env,
    pub members: Vec<u64>,
    pub reads: Vec<u64>,
    pub usable: bool,
}
#[derive(Clone, Debug)]
pub struct BlockMeta {
    pub id: BlockId,
    pub version: u64,
    pub active: bool,
    pub capacity: usize,
    pub entry: EntryPrefix,
    pub members: Vec<u64>,
    pub potential_exports: BTreeSet<u64>,
    pub boundary: Vec<BoundaryRead>,
    pub dependencies: Vec<Dependency>,
    pub prefixes: Vec<CombinedPrefix>,
    pub invalidations: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Interaction {
    pub id: usize,
    pub rule: String,
    pub parents: Vec<BlockId>,
    pub target: BlockId,
    pub consumer: u64,
    pub reads: Vec<u64>,
}
#[derive(Clone)]
struct Recipe {
    rule: pattern::Rule,
    env: Env,
    members: Vec<u64>,
    reads: Vec<u64>,
}
#[derive(Default, Debug)]
pub struct IngestReport {
    pub invalidated_blocks: usize,
    pub new_blocks: usize,
    pub new_members: usize,
    pub rejected: usize,
    pub new_interactions: usize,
}
/// One store belongs to one trace/execution lineage. Call only after a step returns.
#[derive(Default)]
pub struct DependencyBlockStore {
    session: Option<TraceSession>,
    last_scope_reset: Option<u64>,
    seen_equality_reads: HashSet<(u64, u64)>,
    catalog: HashMap<String, RuleSpec>,
    matches: HashMap<u64, RuleMatchEvent>,
    reads: HashMap<u64, Vec<RowReadEvent>>,
    writes: HashMap<u64, WriteEvent>,
    outputs: HashMap<u64, Vec<u64>>,
    processed: HashSet<u64>,
    seen_reads: HashSet<u64>,
    seen_invalidations: HashSet<u64>,
    invalid_facts: HashSet<u64>,
    blocks: Vec<BlockMeta>,
    match_owner: HashMap<u64, BlockId>,
    fact_owner: HashMap<u64, BlockId>,
    interactions: Vec<Interaction>,
    by_block: HashMap<BlockId, Vec<usize>>,
    by_rule: HashMap<String, Vec<usize>>,
    recipes: HashMap<u64, Recipe>,
    rejected: BTreeMap<u64, String>,
}
impl DependencyBlockStore {
    pub fn new(specs: impl IntoIterator<Item = RuleSpec>) -> Self {
        let mut s = Self::default();
        for r in specs {
            assert!(s.catalog.insert(r.name.clone(), r).is_none());
        }
        s
    }
    pub fn register_rule(&mut self, spec: RuleSpec) -> Result<(), String> {
        if self.catalog.contains_key(&spec.name) {
            return Err("rule names must be unique and immutable".into());
        }
        self.catalog.insert(spec.name.clone(), spec);
        Ok(())
    }
    pub fn uses_trace(&self, trace: &TraceSession) -> bool {
        self.session.as_ref().is_some_and(|s| s.same_session(trace))
    }
    pub fn blocks(&self) -> &[BlockMeta] {
        &self.blocks
    }
    pub fn interactions(&self) -> &[Interaction] {
        &self.interactions
    }
    pub fn interactions_for_block(&self, b: BlockId) -> &[usize] {
        self.by_block.get(&b).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn interactions_for_rule(&self, r: &str) -> &[usize] {
        self.by_rule.get(r).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn rejected(&self) -> &BTreeMap<u64, String> {
        &self.rejected
    }
    pub fn block_for_match(&self, m: u64) -> Option<BlockId> {
        self.match_owner.get(&m).copied()
    }
    pub fn block_for_fact(&self, w: u64) -> Option<BlockId> {
        self.fact_owner.get(&w).copied()
    }
    pub fn active_block_for_fact(&self, w: u64) -> Option<BlockId> {
        let b = self.block_for_fact(w)?;
        (!self.invalid_facts.contains(&w) && self.blocks[b].active).then_some(b)
    }
    pub fn invalidate_all(&mut self, reason: &str) {
        for b in 0..self.blocks.len() {
            self.dirty(b, reason);
        }
    }
    /// Revalidate whole unconditional recipes against physical row versions.
    /// Never merges instances on the basis of a common canonical root.
    /// Unknown guards, scope rollback, deletion/reinsertion and missing lineage
    /// remain inactive. A present entry alone is insufficient.
    pub fn revalidate_after_union(&mut self, eg: &egglog::EGraph) -> usize {
        let Some(trace) = &self.session else {
            return 0;
        };
        let names: HashMap<_, _> = trace.table_names().into_iter().collect();
        let mut all_writes = trace.write_events();
        all_writes.sort_by_key(|w| w.event_id);
        let valid_row = |table, old: &[Value]| -> bool {
            let Some(name) = names.get(&table) else {
                return false;
            };
            let Some(function) = eg.get_function(name) else {
                return false;
            };
            let n = function.schema().input.len();
            if old.len() <= n {
                return false;
            }
            let keys: Vec<_> = old[..n]
                .iter()
                .zip(&function.schema().input)
                .map(|(&v, s)| {
                    if s.is_eq_sort() {
                        eg.get_canonical_value(v, s)
                    } else {
                        v
                    }
                })
                .collect();
            let Some(current) = eg.lookup_function_row(name, &keys) else {
                return false;
            };
            if current == old {
                return true;
            }
            let mut versions: HashSet<_> = all_writes
                .iter()
                .filter(|w| {
                    w.table == table && w.actual == old && w.outcome == WriteOutcome::Inserted
                })
                .map(|w| w.event_id)
                .collect();
            for w in &all_writes {
                if w.table != table || !w.rebuild_of.is_some_and(|id| versions.contains(&id)) {
                    continue;
                }
                if !matches!(
                    w.outcome,
                    WriteOutcome::Inserted | WriteOutcome::Deduplicated
                ) {
                    continue;
                }
                versions.insert(w.event_id);
                if w.actual == current && !self.invalid_facts.contains(&w.event_id) {
                    return true;
                }
            }
            false
        };
        let mut ready = HashSet::new();
        for b in &self.blocks {
            if b.active
                || b.prefixes.is_empty()
                || self.last_scope_reset.is_some_and(|r| b.entry.match_id < r)
            {
                continue;
            }
            let valid = b.members.iter().all(|m| {
                self.catalog
                    .get(self.matches[m].rule.as_ref())
                    .is_some_and(|s| s.rewrite.is_some())
                    && self
                        .reads
                        .get(m)
                        .is_some_and(|rs| rs.iter().all(|r| valid_row(r.table, &r.row)))
                    && self
                        .outputs
                        .get(m)
                        .into_iter()
                        .flatten()
                        .all(|w| valid_row(self.writes[w].table, &self.writes[w].actual))
            });
            if valid {
                ready.insert(b.id);
            }
        }
        loop {
            let bad: Vec<_> = ready
                .iter()
                .copied()
                .filter(|b| {
                    self.blocks[*b].boundary.iter().any(|r| {
                        r.source_block.is_some_and(|p| {
                            p != *b && !self.blocks[p].active && !ready.contains(&p)
                        })
                    })
                })
                .collect();
            if bad.is_empty() {
                break;
            }
            for b in bad {
                ready.remove(&b);
            }
        }
        for &id in &ready {
            let block = &mut self.blocks[id];
            block.active = true;
            block.version += 1;
            for prefix in &mut block.prefixes {
                prefix.usable = true;
            }
        }
        ready.len()
    }
    fn dirty(&mut self, start: BlockId, reason: &str) {
        let mut queue = VecDeque::from([start]);
        let mut seen = HashSet::new();
        while let Some(b) = queue.pop_front() {
            if !seen.insert(b) {
                continue;
            }
            let block = &mut self.blocks[b];
            if block.active {
                block.active = false;
                block.version += 1;
                block.invalidations.push(reason.into());
                for p in &mut block.prefixes {
                    p.usable = false;
                }
            }
            for id in self.by_block.get(&b).into_iter().flatten() {
                let edge = &self.interactions[*id];
                if edge.parents.contains(&b) {
                    queue.push_back(edge.target);
                }
            }
        }
    }
    fn env(&self, m: u64) -> Env {
        self.matches[&m]
            .bindings
            .iter()
            .filter_map(|b| b.name.as_ref().map(|n| (n.to_string(), b.value)))
            .collect()
    }
    fn eligible(&self, m: u64, survived: &HashSet<u64>) -> Result<(), String> {
        let x = self.matches.get(&m).ok_or("missing producer match")?;
        // Opaque observations certify individual committed outputs only. An
        // unrelated invalidated side output cannot erase a still-valid P row.
        // Composed rewrites keep the stronger whole-recipe validity gate.
        if self
            .catalog
            .get(x.rule.as_ref())
            .is_some_and(|s| s.rewrite.is_some())
            && self
                .outputs
                .get(&m)
                .into_iter()
                .flatten()
                .any(|w| self.invalid_facts.contains(w))
        {
            return Err("match has invalidated output provenance".into());
        }
        if !survived.contains(&m) {
            return Err("action lane did not survive".into());
        }
        if !x.physical_witness_complete {
            return Err("incomplete keyed LHS witnesses".into());
        }
        let spec = self
            .catalog
            .get(x.rule.as_ref())
            .ok_or("unregistered rule prefix")?;
        if let Some(r) = &spec.rewrite {
            let env = self.env(m);
            if !r.lhs.vars().iter().all(|v| env.contains_key(v)) {
                return Err("missing prefix binding".into());
            }
        }
        Ok(())
    }
    fn add_member(&mut self, b: BlockId, m: u64) {
        if self.match_owner.contains_key(&m) {
            return;
        }
        self.match_owner.insert(m, b);
        let block = &mut self.blocks[b];
        while block.members.len() >= block.capacity {
            block.capacity *= 2;
        }
        if block.members.capacity() < block.capacity {
            block
                .members
                .reserve_exact(block.capacity - block.members.len());
        }
        block.members.push(m);
        block.version += 1;
        for w in self.outputs.get(&m).into_iter().flatten() {
            if !self.invalid_facts.contains(w) {
                block.potential_exports.insert(*w);
                self.fact_owner.insert(*w, b);
            }
        }
        let reads = self.reads.get(&m).cloned().unwrap_or_default();
        for r in reads {
            let source = r
                .producer_write_event_id
                .and_then(|w| self.fact_owner.get(&w).copied());
            if source != Some(b) {
                self.blocks[b].boundary.push(BoundaryRead {
                    read_id: r.event_id,
                    consumer: m,
                    producer_write: r.producer_write_event_id,
                    source_block: source,
                    table: r
                        .table_name
                        .as_deref()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{:?}", r.table)),
                    key: r.key,
                });
            }
        }
    }
    fn allocate(&mut self, entry: u64) -> BlockId {
        let m = &self.matches[&entry];
        let spec = &self.catalog[m.rule.as_ref()];
        let id = self.blocks.len();
        let mut entry_bindings = self.env(entry);
        if let Some(rule) = &spec.rewrite {
            let vars = rule.lhs.vars();
            entry_bindings.retain(|name, _| vars.contains(name));
        }
        self.blocks.push(BlockMeta {
            id,
            version: 0,
            active: true,
            capacity: 2,
            entry: EntryPrefix {
                rule: spec.name.clone(),
                lhs: spec.lhs.clone(),
                match_id: entry,
                bindings: entry_bindings,
            },
            members: Vec::with_capacity(2),
            potential_exports: BTreeSet::new(),
            boundary: vec![],
            dependencies: vec![],
            prefixes: vec![],
            invalidations: vec![],
        });
        self.add_member(id, entry);
        id
    }
    pub fn ingest_committed(&mut self, trace: &TraceSession) -> Result<IngestReport, String> {
        if !trace.dependencies_enabled() {
            return Err("requires dependency tracing".into());
        }
        if let Some(s) = &self.session {
            if !s.same_session(trace) {
                return Err("cannot mix trace sessions".into());
            }
        } else {
            self.session = Some(trace.clone());
        }
        let active_before: Vec<_> = self
            .blocks
            .iter()
            .filter(|b| b.active)
            .map(|b| b.id)
            .collect();
        let before = (
            self.blocks.len(),
            self.match_owner.len(),
            self.rejected.len(),
            self.interactions.len(),
        );
        if let Some(reset) = trace.scope_resets().into_iter().max() {
            if self.last_scope_reset != Some(reset) {
                self.invalidate_all("scope rollback: previous block certificates retired");
                self.last_scope_reset = Some(reset);
            }
        }
        for m in trace.matches() {
            self.matches.insert(m.event_id, m);
        }
        for w in trace.write_events() {
            if !self.writes.contains_key(&w.event_id) {
                if w.outcome == WriteOutcome::Inserted {
                    self.outputs
                        .entry(w.match_event_id)
                        .or_default()
                        .push(w.event_id);
                }
                self.writes.insert(w.event_id, w);
            }
        }
        for r in trace.row_reads() {
            if self.seen_reads.insert(r.event_id) {
                self.reads.entry(r.match_event_id).or_default().push(r);
            }
        }
        for inv in trace.origin_invalidations() {
            if self.seen_invalidations.insert(inv.event_id) {
                self.invalid_facts.insert(inv.write_event_id);
                if let Some(b) = self.fact_owner.get(&inv.write_event_id).copied() {
                    self.blocks[b].potential_exports.remove(&inv.write_event_id);
                    self.dirty(
                        b,
                        &format!(
                            "origin {} invalidated: {:?}",
                            inv.write_event_id, inv.reason
                        ),
                    );
                }
            }
        }
        let survived: HashSet<_> = trace
            .action_outcomes()
            .iter()
            .filter(|o| o.outcome == RuleActionOutcome::Survived)
            .map(|o| o.match_event_id)
            .collect();
        let mut pending: Vec<_> = self
            .matches
            .keys()
            .filter(|m| !self.processed.contains(m))
            .copied()
            .collect();
        pending.sort();
        for m in pending {
            self.processed.insert(m);
            if self.last_scope_reset.is_some_and(|reset| m < reset) {
                self.rejected
                    .insert(m, "historical match before scope rollback".into());
                continue;
            }
            if let Err(reason) = self.eligible(m, &survived) {
                self.rejected.insert(m, reason);
                continue;
            }
            let spec = &self.catalog[self.matches[&m].rule.as_ref()];
            if let Some(rule) = &spec.rewrite {
                self.recipes.insert(
                    m,
                    Recipe {
                        rule: rule.clone(),
                        env: self.env(m),
                        members: vec![m],
                        reads: vec![],
                    },
                );
            }
            let reads = self.reads.get(&m).cloned().unwrap_or_default();
            let mut edges = Vec::new();
            let mut error = None;
            for r in reads {
                let (Some(p), Some(w)) = (r.producer_match_event_id, r.producer_write_event_id)
                else {
                    continue;
                };
                if self.last_scope_reset.is_some_and(|reset| w < reset) {
                    continue; // Restored inputs are boundaries, not live block producers.
                }
                let valid = self.writes.get(&w).is_some_and(|x| {
                    x.outcome == WriteOutcome::Inserted
                        && x.match_event_id == p
                        && x.table == r.table
                        && x.actual == r.row
                        && x.event_id < r.event_id
                });
                if !valid || self.invalid_facts.contains(&w) {
                    error = Some("unavailable or invalidated producer fact".to_string());
                    break;
                }
                if let Err(e) = self.eligible(p, &survived) {
                    error = Some(format!("producer prefix unavailable: {e}"));
                    break;
                }
                if self.rejected.contains_key(&p) {
                    error = Some("producer dependency coverage is incomplete".into());
                    break;
                }
                edges.push(Dependency {
                    producer: p,
                    consumer: m,
                    write: w,
                    read: r.event_id,
                });
            }
            if let Some(e) = error {
                self.rejected.insert(m, e);
                continue;
            }
            if edges.is_empty() {
                continue;
            } // An ordinary match remains a seed, not a block.
            if edges.iter().any(|e| {
                self.block_for_match(e.producer)
                    .is_some_and(|b| !self.blocks[b].active)
            }) {
                self.rejected
                    .insert(m, "producer block needs revalidation".into());
                continue;
            }
            let mut parents = BTreeSet::new();
            for e in &edges {
                let b = if let Some(b) = self.block_for_match(e.producer) {
                    b
                } else {
                    self.allocate(e.producer)
                };
                parents.insert(b);
            }
            if parents.iter().any(|b| !self.blocks[*b].active) {
                self.rejected
                    .insert(m, "producer block needs revalidation".into());
                continue;
            }
            let b = if parents.len() == 1 {
                let b = *parents.first().unwrap();
                self.add_member(b, m);
                b
            } else {
                let b = self.allocate(m);
                let id = self.interactions.len();
                let rule = self.matches[&m].rule.to_string();
                let parents: Vec<_> = parents.iter().copied().collect();
                self.interactions.push(Interaction {
                    id,
                    rule: rule.clone(),
                    parents: parents.clone(),
                    target: b,
                    consumer: m,
                    reads: edges.iter().map(|e| e.read).collect(),
                });
                for parent in parents.iter().chain(std::iter::once(&b)) {
                    self.by_block.entry(*parent).or_default().push(id);
                }
                self.by_rule.entry(rule).or_default().push(id);
                b
            };
            let producers: BTreeSet<_> = edges.iter().map(|e| e.producer).collect();
            if parents.len() == 1 && producers.len() == 1 {
                self.extend_prefix(b, *producers.first().unwrap(), m, &edges);
            }
            self.blocks[b].dependencies.extend(edges);
        }
        // Equality interactions are separate from row writes: a union is never
        // relabelled as an Inserted fact. Follow committed rebuild versions back
        // to the exact union events used by canonicalization.
        let unions: HashMap<_, _> = trace
            .union_events()
            .into_iter()
            .map(|u| (u.event_id, u))
            .collect();
        for read in trace.row_reads() {
            let Some(target) = self.block_for_match(read.match_event_id) else {
                continue;
            };
            if !self.blocks[target].active {
                continue;
            }
            let mut current = read.producer_write_event_id;
            let mut visited = HashSet::new();
            let mut equality_events = BTreeSet::new();
            while let Some(id) = current {
                if !visited.insert(id) {
                    break;
                }
                let Some(w) = self.writes.get(&id) else {
                    break;
                };
                equality_events.extend(w.union_dependencies.iter().copied());
                current = w.rebuild_of;
            }
            for id in equality_events {
                if self.seen_equality_reads.contains(&(read.event_id, id)) {
                    continue;
                }
                let Some(union) = unions.get(&id) else {
                    continue;
                };
                if self.last_scope_reset.is_some_and(|r| id < r)
                    || union.event_id >= read.event_id
                    || self.eligible(union.match_event_id, &survived).is_err()
                {
                    continue;
                }
                let source = self
                    .block_for_match(union.match_event_id)
                    .unwrap_or_else(|| self.allocate(union.match_event_id));
                if !self.blocks[source].active {
                    continue;
                }
                self.seen_equality_reads.insert((read.event_id, id));
                for prefix in &mut self.blocks[target].prefixes {
                    prefix.usable = false;
                }
                if source == target {
                    continue;
                }
                let edge_id = self.interactions.len();
                let rule = format!("union #{id}: {}", self.matches[&union.match_event_id].rule);
                self.interactions.push(Interaction {
                    id: edge_id,
                    rule: rule.clone(),
                    parents: vec![source],
                    target,
                    consumer: read.match_event_id,
                    reads: vec![read.event_id],
                });
                self.by_rule.entry(rule).or_default().push(edge_id);
                self.by_block.entry(source).or_default().push(edge_id);
                self.by_block.entry(target).or_default().push(edge_id);
            }
        }
        Ok(IngestReport {
            invalidated_blocks: active_before
                .iter()
                .filter(|b| !self.blocks[**b].active)
                .count(),
            new_blocks: self.blocks.len() - before.0,
            new_members: self.match_owner.len() - before.1,
            rejected: self.rejected.len() - before.2,
            new_interactions: self.interactions.len() - before.3,
        })
    }
    fn extend_prefix(&mut self, b: BlockId, p: u64, m: u64, edges: &[Dependency]) {
        let Some(prev) = self.recipes.get(&p).cloned() else {
            return;
        };
        let Some(next) = self.recipes.get(&m).cloned() else {
            return;
        };
        for c in pattern::candidates(&[prev.rule.clone(), next.rule.clone()])
            .into_iter()
            .filter(|c| c.first == 0 && c.second == 1)
        {
            let mut env = Env::new();
            if !align(&c.rule.lhs, &prev.rule.lhs, &prev.env, &mut env) {
                continue;
            }
            let overlap = c.middle.at(&c.path);
            if !equal_terms(overlap, &env, &next.rule.lhs, &next.env) {
                continue;
            }
            let pattern::Pat::App(op, args) = overlap else {
                continue;
            };
            let keys: Option<Vec<Value>> = args
                .iter()
                .map(|p| {
                    if let pattern::Pat::Var(v) = p {
                        env.get(v).copied()
                    } else {
                        None
                    }
                })
                .collect();
            let Some(keys) = keys else {
                continue;
            };
            let related = edges.iter().any(|e| {
                self.reads[&m].iter().any(|r| {
                    r.event_id == e.read && r.table_name.as_deref() == Some(op) && r.key == keys
                })
            });
            if !related {
                continue;
            }
            let mut members = prev.members.clone();
            members.push(m);
            let mut reads = prev.reads.clone();
            reads.extend(edges.iter().map(|e| e.read));
            self.blocks[b].prefixes.push(CombinedPrefix {
                lhs: c.rule.lhs.to_string(),
                rhs: c.rule.rhs.to_string(),
                bindings: env.clone(),
                members: members.clone(),
                reads: reads.clone(),
                usable: true,
            });
            self.recipes.insert(
                m,
                Recipe {
                    rule: c.rule,
                    env,
                    members,
                    reads,
                },
            );
            break;
        }
    }
}
fn align(new: &pattern::Pat, old: &pattern::Pat, known: &Env, out: &mut Env) -> bool {
    match (new, old) {
        (pattern::Pat::Lit(a), pattern::Pat::Lit(b)) => a == b,
        (pattern::Pat::Var(a), pattern::Pat::Var(b)) => {
            let Some(v) = known.get(b) else {
                return false;
            };
            if let Some(old) = out.get(a) {
                old == v
            } else {
                out.insert(a.clone(), *v);
                true
            }
        }
        (pattern::Pat::App(a, x), pattern::Pat::App(b, y)) => {
            a == b && x.len() == y.len() && x.iter().zip(y).all(|(a, b)| align(a, b, known, out))
        }
        _ => false,
    }
}
fn equal_terms(a: &pattern::Pat, ae: &Env, b: &pattern::Pat, be: &Env) -> bool {
    match (a, b) {
        (pattern::Pat::Lit(a), pattern::Pat::Lit(b)) => a == b,
        (pattern::Pat::Var(a), pattern::Pat::Var(b)) => {
            ae.get(a).is_some() && ae.get(a) == be.get(b)
        }
        (pattern::Pat::App(a, x), pattern::Pat::App(b, y)) => {
            a == b && x.len() == y.len() && x.iter().zip(y).all(|(a, b)| equal_terms(a, ae, b, be))
        }
        _ => false,
    }
}
