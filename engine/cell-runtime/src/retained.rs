//! Exact, resumable query contexts. A branch retains correlated values and
//! physical witnesses from several producers. Only read domains are cached;
//! RHS effects and source Run visibility remain with the ordinary executor.
use crate::{
    graph::{Dependency, Graph},
    program::{Arg, Atom, Call, Program, Result},
    query::{QueryIndexes, ReadKey},
    value::{self, Value, canon},
};
use serde::Serialize;
use serde_json::{Value as J, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
};

#[derive(Clone)]
pub struct Answer {
    pub values: Vec<Value>,
    pub witnesses: Vec<Option<Dependency>>,
    pub epoch: u64,
}
#[derive(Default)]
struct Node {
    id: u64,
    ready: bool,
    input: Vec<Option<Value>>,
    unions: u64,
    chosen: Option<usize>,
    branches: Vec<Branch>,
    primitive: Option<Box<Node>>,
    matches: usize,
    latest: u64,
}
struct Branch {
    values: Vec<Value>,
    witness: Dependency,
    epoch: u64,
    child: Box<Node>,
}
#[derive(Default, Serialize)]
pub struct Stats {
    published_row_notifications: u64,
    invalidated_contexts: u64,
    delta_domain_refreshes: u64,
    full_domain_refreshes: u64,
    publication_seconds: f64,
    calls: u64,
    root_hits: u64,
    subtree_hits: u64,
    domains_reused: u64,
    domains_refreshed: u64,
    function_domains_refreshed: u64,
    union_refreshes: u64,
    rebound_contexts: u64,
    contexts_created: u64,
    retired_contexts: u64,
    candidate_rows: u64,
    key_reads: u64,
    guard_checks: u64,
    waiting_domains: u64,
    emitted_bindings: u64,
    old_answers_filtered: u64,
    multi_producer_bindings: u64,
    calls_by_rule: BTreeMap<usize, u64>,
    rows_by_rule: BTreeMap<usize, u64>,
    live_contexts: usize,
    peak_contexts: usize,
    live_branches: usize,
    peak_branches: usize,
}
pub struct Contexts {
    pub enabled: bool,
    rules: BTreeSet<usize>,
    roots: BTreeMap<usize, Node>,
    anchors: BTreeMap<usize, usize>,
    signals: Signals,
    literal_rules: BTreeSet<usize>,
    unions: u64,
    pub stats: Stats,
}
#[derive(Default)]
struct Signal {
    parent: Option<u64>,
    dirty: bool,
    full: bool,
    live: bool,
    read: Option<ReadKey>,
    pending: BTreeSet<Dependency>,
}
#[derive(Default)]
struct Signals {
    slots: Vec<Signal>,
    free: Vec<usize>,
    watchers: HashMap<ReadKey, BTreeSet<u64>>,
}
impl Signals {
    fn allocate(&mut self, parent: Option<u64>) -> u64 {
        if self.slots.is_empty() {
            self.slots.push(Signal::default());
        }
        let i = self.free.pop().unwrap_or_else(|| {
            let i = self.slots.len();
            self.slots.push(Signal::default());
            i
        });
        self.slots[i] = Signal {
            parent,
            live: true,
            ..Signal::default()
        };
        i as u64
    }
    fn watch(&mut self, id: u64, key: ReadKey) {
        if self.slots[id as usize].read == Some(key) {
            return;
        }
        self.unwatch(id);
        self.slots[id as usize].read = Some(key);
        self.watchers.entry(key).or_default().insert(id);
    }
    fn unwatch(&mut self, id: u64) {
        if let Some(key) = self.slots[id as usize].read.take() {
            let ids = self.watchers.get_mut(&key).unwrap();
            ids.remove(&id);
            if ids.is_empty() {
                self.watchers.remove(&key);
            }
        }
    }
    fn remove(&mut self, id: u64) {
        if id == 0 {
            return;
        }
        self.unwatch(id);
        let i = id as usize;
        assert!(self.slots[i].live);
        self.slots[i] = Signal::default();
        self.free.push(i);
    }
    fn notify(&mut self, mut id: u64) -> u64 {
        let mut count = 0;
        loop {
            let slot = &mut self.slots[id as usize];
            assert!(slot.live);
            if slot.dirty {
                break;
            }
            slot.dirty = true;
            count += 1;
            match slot.parent {
                Some(p) => id = p,
                None => break,
            }
        }
        count
    }
    fn dirty(&self, id: u64) -> bool {
        self.slots[id as usize].dirty
    }
    fn full(&self, id: u64) -> bool {
        self.slots[id as usize].full
    }
    fn done(&mut self, id: u64) {
        let s = &mut self.slots[id as usize];
        s.dirty = false;
        s.full = false;
    }
    fn take_pending(&mut self, id: u64) -> Option<BTreeSet<Dependency>> {
        let p = &mut self.slots[id as usize].pending;
        (!p.is_empty()).then(|| std::mem::take(p))
    }
    fn reset(&mut self) {
        for s in &mut self.slots {
            if s.live {
                s.dirty = true;
                s.full = true;
                s.pending.clear();
            }
        }
    }
}
impl Contexts {
    pub fn new(p: &Program) -> Self {
        let enabled = std::env::var("CELL_CPU_RETAINED_CONTEXTS").as_deref() == Ok("1");
        let names = std::env::var("CELL_CPU_RETAINED_SETS")
            .unwrap_or_else(|_| "always-run,type-analysis".into());
        let rules: BTreeSet<usize> = if enabled {
            names
                .split(',')
                .filter_map(|s| p.sets.get(s))
                .flatten()
                .copied()
                .filter(|r| {
                    p.rules[*r]
                        .query
                        .iter()
                        .filter(|a| matches!(a.call, Call::Table(_)))
                        .count()
                        > 1
                })
                .collect()
        } else {
            BTreeSet::new()
        };
        // Own a context by an existing structural marker, or by a constructor
        // binding the most equality coordinates. Mutable port values must not
        // decide whether their structural owner exists in the first place.
        let anchors = rules
            .iter()
            .filter_map(|rid| {
                p.rules[*rid]
                    .query
                    .iter()
                    .enumerate()
                    .filter_map(|(i, a)| {
                        let Call::Table(t) = a.call else { return None };
                        let marker = p.views.markers.contains(&t);
                        if !marker && !p.tables[t].constructor {
                            return None;
                        }
                        let eq = a
                            .args
                            .iter()
                            .zip(&p.tables[t].schema)
                            .filter_map(|(a, s)| match a {
                                Arg::Slot(i) if s["kind"] == "eq" => Some(*i),
                                _ => None,
                            })
                            .collect::<BTreeSet<_>>()
                            .len();
                        Some(((!marker, usize::MAX - eq, i), i))
                    })
                    .min_by_key(|(key, _)| *key)
                    .map(|(_, i)| (*rid, i))
            })
            .collect();
        fn identity(v: &Value) -> bool {
            match v {
                Value::Eq(_) => true,
                Value::Set(v) => v.iter().any(identity),
                _ => false,
            }
        }
        let literal_rules = rules
            .iter()
            .filter(|r| {
                p.rules[**r]
                    .query
                    .iter()
                    .flat_map(|a| &a.args)
                    .any(|a| matches!(a,Arg::Literal(v) if identity(v)))
            })
            .copied()
            .collect();
        Self {
            enabled,
            rules,
            roots: BTreeMap::new(),
            anchors,
            signals: Signals::default(),
            literal_rules,
            unions: 0,
            stats: Stats::default(),
        }
    }
    pub fn accepts(&self, rid: usize) -> bool {
        self.rules.contains(&rid)
    }
    pub fn publish(&mut self, g: &Graph, ix: &mut QueryIndexes) {
        if !self.enabled {
            return;
        }
        let clock = std::time::Instant::now();
        if std::mem::take(&mut ix.read_reset) {
            self.signals.reset();
        }
        let mut changes = std::mem::take(&mut ix.read_changes);
        changes.sort_unstable();
        changes.dedup();
        for (key, w) in changes {
            let ids = self
                .signals
                .watchers
                .get(&key)
                .into_iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            for id in ids {
                self.signals.slots[id as usize].pending.insert(w);
                self.stats.published_row_notifications += 1;
                self.stats.invalidated_contexts += self.signals.notify(id);
            }
        }
        if self.unions != g.stats.unions && !self.literal_rules.is_empty() {
            fn mark(n: &Node, s: &mut Signals) {
                if n.id != 0 {
                    s.slots[n.id as usize].full = true;
                    s.notify(n.id);
                }
                for b in &n.branches {
                    mark(&b.child, s)
                }
                if let Some(c) = &n.primitive {
                    mark(c, s)
                }
            }
            for rid in &self.literal_rules {
                if let Some(n) = self.roots.get(rid) {
                    mark(n, &mut self.signals);
                }
            }
        }
        self.unions = g.stats.unions;
        self.stats.publication_seconds += clock.elapsed().as_secs_f64();
    }
    pub fn matches(
        &mut self,
        g: &mut Graph,
        ix: &mut QueryIndexes,
        rid: usize,
        watermark: u64,
    ) -> Result<Vec<Answer>> {
        self.stats.calls += 1;
        *self.stats.calls_by_rule.entry(rid).or_default() += 1;
        let before = self.stats.candidate_rows;
        let rule = g.program.rules[rid].clone();
        let mut remaining = vec![true; rule.query.len()];
        // Only query coordinates belong in retained contexts. RHS temporaries
        // are allocated by the existing effect frame when a binding is emitted.
        let input = vec![None; rule.inputs.iter().max().map_or(0, |i| i + 1)];
        let node = self.roots.entry(rid).or_default();
        if !node.ready {
            node.chosen = self.anchors.get(&rid).copied();
        }
        if node.ready && !self.signals.dirty(node.id) {
            self.stats.root_hits += 1;
            if rule.seminaive {
                self.stats.old_answers_filtered += node.matches as u64;
                return Ok(vec![]);
            }
        }
        evaluate(
            node,
            &input,
            &mut remaining,
            &rule.query,
            &rule.inputs,
            g,
            ix,
            &mut self.stats,
            &mut self.signals,
            None,
        )?;
        *self.stats.rows_by_rule.entry(rid).or_default() += self.stats.candidate_rows - before;
        let mut unique = BTreeMap::new();
        collect(
            node,
            &rule.inputs,
            &mut vec![None; rule.query.len()],
            0,
            rule.seminaive.then_some(watermark),
            &mut unique,
            &mut self.stats,
        )?;
        self.stats.emitted_bindings += unique.len() as u64;
        for a in unique.values() {
            let owners = a
                .witnesses
                .iter()
                .filter_map(|w| match w {
                    Some(Dependency::Fact(f)) => Some(f.cell),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            self.stats.multi_producer_bindings += u64::from(owners.len() > 1);
        }
        Ok(unique.into_values().collect())
    }
    pub fn report(&self) -> J {
        let mut contexts = 0;
        let mut branches = 0;
        let mut slots = 0;
        for n in self.roots.values() {
            let (a, b, c) = storage(n);
            contexts += a;
            branches += b;
            slots += c;
        }
        json!({"enabled":self.enabled,"selected_rules":self.rules,"structural_anchors":self.anchors,"stats":self.stats,
            "live_contexts":contexts,"live_branches":branches,"live_value_slots":slots,"subscriptions":self.signals.slots.iter().filter(|s|s.read.is_some()).count(),"pending_contexts":self.signals.slots.iter().filter(|s|!s.pending.is_empty()).count(),"context_slots":self.signals.slots.len().saturating_sub(1),"free_context_slots":self.signals.free.len(),
            "scope":"exact typed query continuations across source Runs; partial/empty domains retained; not a certified RipSat closure or permission to reorder effects"})
    }
}
fn storage(n: &Node) -> (usize, usize, usize) {
    let mut result = (
        usize::from(n.ready),
        n.branches.len(),
        n.input.len() + n.branches.iter().map(|b| b.values.len()).sum::<usize>(),
    );
    for c in n
        .branches
        .iter()
        .map(|b| b.child.as_ref())
        .chain(n.primitive.as_deref())
    {
        let (a, b, c) = storage(c);
        result.0 += a;
        result.1 += b;
        result.2 += c;
    }
    result
}
fn operand(a: &Arg, input: &[Option<Value>], g: &Graph) -> Option<Value> {
    match a {
        Arg::Literal(v) => Some(canon(v, &g.parents)),
        Arg::Slot(i) => input[*i].clone(),
    }
}
fn bind(a: &Arg, v: &Value, input: &mut [Option<Value>], g: &Graph) -> bool {
    match a {
        Arg::Literal(x) => canon(x, &g.parents) == *v,
        Arg::Slot(i) => match &input[*i] {
            Some(old) => old == v,
            None => {
                input[*i] = Some(v.clone());
                true
            }
        },
    }
}
fn retire(n: &Node, stats: &mut Stats, signals: &mut Signals) {
    let (nodes, branches, _) = storage(n);
    stats.retired_contexts += nodes as u64;
    stats.live_contexts -= nodes;
    stats.live_branches -= branches;
    fn forget(n: &Node, s: &mut Signals) {
        s.remove(n.id);
        for b in &n.branches {
            forget(&b.child, s)
        }
        if let Some(c) = &n.primitive {
            forget(c, s)
        }
    }
    forget(n, signals);
}
fn evaluate(
    n: &mut Node,
    input: &[Option<Value>],
    remaining: &mut [bool],
    query: &[Atom],
    inputs: &[usize],
    g: &mut Graph,
    ix: &mut QueryIndexes,
    stats: &mut Stats,
    signals: &mut Signals,
    parent: Option<u64>,
) -> Result<()> {
    if n.ready && n.input == input && !signals.dirty(n.id) {
        stats.subtree_hits += 1;
        return Ok(());
    }
    if n.ready && n.input != input {
        stats.rebound_contexts += 1;
        retire(n, stats, signals);
        *n = Node::default();
    }
    if !n.ready {
        n.id = signals.allocate(parent);
        n.input = input.to_vec();
        stats.contexts_created += 1;
        stats.live_contexts += 1;
        stats.peak_contexts = stats.peak_contexts.max(stats.live_contexts);
    } else if n.unions != g.stats.unions {
        stats.union_refreshes += 1;
    }
    if !remaining.iter().any(|x| *x) {
        if inputs.iter().any(|i| input[*i].is_none()) {
            return Err("incomplete retained binding".into());
        }
        n.matches = 1;
        n.latest = 0;
        n.ready = true;
        n.unions = g.stats.unions;
        signals.done(n.id);
        return Ok(());
    }
    let chosen = if let Some(i) = n.chosen {
        i
    } else {
        let mut best = None;
        for (i, a) in query.iter().enumerate().filter(|(i, _)| remaining[*i]) {
            let bound = a
                .args
                .iter()
                .map(|v| operand(v, input, g))
                .collect::<Vec<_>>();
            let keyed = bound[..bound.len() - 1].iter().all(Option::is_some);
            let cost = match a.call {
                Call::Primitive(..) if keyed => 0,
                Call::Primitive(..) => continue,
                Call::Table(_) if keyed => 1,
                Call::Table(t) => 2 + ix.count(g, t, &bound)?,
            };
            if best.is_none_or(|b| (cost, i) < b) {
                best = Some((cost, i));
            }
        }
        let i = best.ok_or("unbound retained primitive cycle")?.1;
        n.chosen = Some(i);
        i
    };
    remaining[chosen] = false;
    let a = &query[chosen];
    n.matches = 0;
    n.latest = 0;
    match &a.call {
        Call::Primitive(name, output) => {
            stats.guard_checks += 1;
            let mut passed = false;
            let args = a.args[..a.args.len() - 1]
                .iter()
                .map(|a| operand(a, input, g).ok_or("unbound retained guard".into()))
                .collect::<Result<Vec<_>>>()?;
            if name.starts_with("cell-key-") {
                g.stats.key_operations += 1;
            }
            if let Some(v) = value::primitive(
                name,
                &args,
                output,
                &g.program.artifact["source_core"]["structural_key"]["nodes"],
            )? {
                let mut next = input.to_vec();
                if bind(a.args.last().unwrap(), &v, &mut next, g) {
                    passed = true;
                    let child = n.primitive.get_or_insert_with(Default::default);
                    evaluate(
                        child,
                        &next,
                        remaining,
                        query,
                        inputs,
                        g,
                        ix,
                        stats,
                        signals,
                        Some(n.id),
                    )?;
                    n.matches = child.matches;
                    n.latest = child.latest;
                }
            }
            if !passed {
                if let Some(c) = n.primitive.take() {
                    retire(&c, stats, signals);
                }
            }
        }
        Call::Table(t) => {
            let bound = a
                .args
                .iter()
                .map(|v| operand(v, input, g))
                .collect::<Vec<_>>();
            let pending = signals.take_pending(n.id);
            // Input changes rebuild this node; equality literals and index
            // resets set full. Otherwise subscribed row deltas are exhaustive.
            let registered = signals.slots[n.id as usize].read.is_some();
            let incremental = registered && pending.is_some() && !signals.full(n.id);
            let valid = registered && pending.is_none() && !signals.full(n.id);
            if valid {
                stats.domains_reused += 1;
            } else {
                stats.domains_refreshed += 1;
                stats.function_domains_refreshed += u64::from(!g.program.tables[*t].virtual_fact);
                let old = std::mem::take(&mut n.branches);
                stats.live_branches -= old.len();
                let mut previous = old
                    .into_iter()
                    .map(|b| (b.witness, b))
                    .collect::<BTreeMap<_, _>>();
                let refs = if incremental {
                    stats.delta_domain_refreshes += 1;
                    Rc::new(pending.unwrap().into_iter().collect::<Vec<_>>())
                } else {
                    stats.full_domain_refreshes += 1;
                    if let Some(key) = bound[..bound.len() - 1]
                        .iter()
                        .cloned()
                        .collect::<Option<Vec<_>>>()
                    {
                        stats.key_reads += 1;
                        Rc::new(
                            g.lookup(*t, &key)?
                                .into_iter()
                                .map(|r| r.witness)
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        ix.candidates(g, *t, &bound)?
                    }
                };
                let mut refreshed = BTreeMap::new();
                for w in refs.iter().copied() {
                    stats.candidate_rows += 1;
                    g.stats.residual_candidates += 1;
                    g.stats.decoded_rows += 1;
                    let old = previous.remove(&w);
                    let mut next = input.to_vec();
                    let row = g.row(w)?.filter(|r| {
                        r.table == *t
                            && !r.hidden
                            && a.args
                                .iter()
                                .zip(&r.values)
                                .all(|(a, v)| bind(a, v, &mut next, g))
                    });
                    if let Some(row) = row {
                        let mut b = old.unwrap_or_else(|| Branch {
                            values: vec![],
                            witness: w,
                            epoch: 0,
                            child: Box::default(),
                        });
                        if b.child.ready && b.values != row.values {
                            signals.notify(b.child.id);
                        }
                        b.values = row.values;
                        b.witness = row.witness;
                        b.epoch = row.epoch;
                        refreshed.insert(w, b);
                    } else if let Some(b) = old {
                        retire(&b.child, stats, signals);
                    }
                }
                if incremental {
                    previous.extend(refreshed);
                    n.branches = previous.into_values().collect();
                } else {
                    for b in previous.values() {
                        retire(&b.child, stats, signals);
                    }
                    n.branches = refreshed.into_values().collect();
                }
                stats.live_branches += n.branches.len();
                stats.peak_branches = stats.peak_branches.max(stats.live_branches);
                stats.waiting_domains += u64::from(n.branches.is_empty());
                signals.watch(n.id, ix.read_key(g, *t, &bound)?);
            }
            for b in &mut n.branches {
                if b.child.ready && !signals.dirty(b.child.id) {
                    stats.subtree_hits += 1;
                } else {
                    let mut next = input.to_vec();
                    if !a
                        .args
                        .iter()
                        .zip(&b.values)
                        .all(|(a, v)| bind(a, v, &mut next, g))
                    {
                        return Err(
                            "retained row binding changed without a row notification".into()
                        );
                    }
                    evaluate(
                        &mut b.child,
                        &next,
                        remaining,
                        query,
                        inputs,
                        g,
                        ix,
                        stats,
                        signals,
                        Some(n.id),
                    )?;
                }
                n.matches += b.child.matches;
                if b.child.matches > 0 {
                    n.latest = n.latest.max(b.epoch.max(b.child.latest));
                }
            }
        }
    }
    remaining[chosen] = true;
    n.ready = true;
    n.unions = g.stats.unions;
    signals.done(n.id);
    Ok(())
}

/// Materialize only current delta paths. Old complete bindings remain factored
/// in the tree instead of being copied into every ancestor's answer vector.
fn collect(
    n: &Node,
    inputs: &[usize],
    witnesses: &mut [Option<Dependency>],
    epoch: u64,
    watermark: Option<u64>,
    out: &mut BTreeMap<Vec<Value>, Answer>,
    stats: &mut Stats,
) -> Result<()> {
    if n.matches == 0 {
        return Ok(());
    }
    if watermark.is_some_and(|w| epoch.max(n.latest) < w) {
        stats.old_answers_filtered += n.matches as u64;
        return Ok(());
    }
    if let Some(i) = n.chosen {
        if let Some(child) = &n.primitive {
            collect(child, inputs, witnesses, epoch, watermark, out, stats)?;
        } else {
            for b in &n.branches {
                witnesses[i] = Some(b.witness);
                collect(
                    &b.child,
                    inputs,
                    witnesses,
                    epoch.max(b.epoch),
                    watermark,
                    out,
                    stats,
                )?;
            }
            witnesses[i] = None;
        }
    } else {
        let values = inputs
            .iter()
            .map(|i| {
                n.input[*i]
                    .clone()
                    .ok_or("incomplete retained output".into())
            })
            .collect::<Result<Vec<_>>>()?;
        out.entry(values.clone()).or_insert_with(|| Answer {
            values,
            witnesses: witnesses.to_vec(),
            epoch,
        });
    }
    Ok(())
}
