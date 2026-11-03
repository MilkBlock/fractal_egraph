//! Common typed, connected table prefixes. Providers retain correlated answers;
//! consumers keep independent suffixes and semi-naive watermarks. This is query
//! common-subexpression elimination, not a permission to execute an early RHS.
use super::*;

type Key = Vec<(usize, Vec<Arg>)>;
type Rows = BTreeMap<Vec<Value>, Answer>;
// Provider-local identity of a currently known correlated binding. Never an
// eclass ID. IDs are not reused: a delayed consumer cannot confuse a removed
// binding with a later insertion. Only current rows/identities occupy storage.
type Snapshot = BTreeMap<u64, Rc<Answer>>;
#[derive(Clone)]
struct Projection {
    provider: usize,
    slots: Vec<usize>, // normalized prefix coordinate -> consumer coordinate
    atoms: Vec<usize>, // prefix witness coordinate -> consumer atom
}
struct Plan {
    query: Vec<Atom>,
    inputs: Vec<usize>,
    parent: Option<Projection>,
    literal: bool,
}
#[derive(Default)]
struct Continuation {
    // A private signal header allows a changed suffix to invalidate its owner
    // without falsely invalidating the shared provider or other consumers.
    head: Node,
    revision: u64,
    rows: BTreeMap<u64, (Rc<Answer>, Node)>,
}
#[derive(Default)]
struct Provider {
    root: Node,
    continuation: Continuation,
    rows: Rc<Snapshot>,
    identities: BTreeMap<Vec<Value>, u64>,
    next_identity: u64,
    revision: u64,
    requests: u64,
    refreshes: u64,
    hits: u64,
    cross_consumer_hits: u64,
    last_consumer: Option<usize>,
}
#[derive(Default, Serialize)]
struct PrefixStats {
    provider_requests: u64,
    provider_refreshes: u64,
    provider_hits: u64,
    cross_consumer_hits: u64,
    unchanged_outputs: u64,
    output_rows_rebuilt: u64,
    consumer_calls: u64,
    consumer_bindings: u64,
    consumer_hits: u64,
    suffix_updates: u64,
    prefix_rows_attached: u64,
    prefix_rows_retired: u64,
    provider_seconds: f64,
}
#[derive(Default)]
pub(super) struct Prefixes {
    enabled: bool,
    plans: Vec<Plan>,
    consumers: BTreeMap<usize, Projection>,
    providers: Vec<Provider>,
    suffixes: BTreeMap<usize, Continuation>,
    stats: PrefixStats,
}
fn identity(v: &Value) -> bool {
    match v {
        Value::Eq(_) => true,
        Value::Set(xs) => xs.iter().any(identity),
        _ => false,
    }
}
impl Prefixes {
    pub fn new(p: &Program, rules: &BTreeSet<usize>, anchors: &BTreeMap<usize, usize>) -> Self {
        let enabled = std::env::var("CELL_CPU_SPINE_PREFIXES").as_deref() == Ok("1");
        if !enabled {
            return Self::default();
        }
        let mut occurrences: BTreeMap<Key, Vec<(usize, Vec<usize>, Vec<usize>)>> = BTreeMap::new();
        for &rid in rules {
            let Some(&anchor) = anchors.get(&rid) else {
                continue;
            };
            let q = &p.rules[rid].query;
            let mut slots = BTreeMap::new();
            let mut inverse = vec![];
            let mut literals = BTreeSet::new();
            let mut atoms = vec![];
            let mut key = vec![];
            for i in std::iter::once(anchor).chain((0..q.len()).filter(|i| *i != anchor)) {
                let Call::Table(t) = q[i].call else { break }; // guards remain in the consumer
                if !atoms.is_empty()
                    && !q[i].args.iter().any(|a| match a {
                        Arg::Slot(s) => slots.contains_key(s),
                        Arg::Literal(v) => identity(v) && literals.contains(v),
                    })
                {
                    break;
                }
                for a in &q[i].args {
                    if let Arg::Literal(v) = a {
                        if identity(v) {
                            literals.insert(v.clone());
                        }
                    }
                }
                let args = q[i]
                    .args
                    .iter()
                    .map(|a| match a {
                        Arg::Literal(v) => Arg::Literal(v.clone()),
                        Arg::Slot(s) => Arg::Slot(*slots.entry(*s).or_insert_with(|| {
                            inverse.push(*s);
                            inverse.len() - 1
                        })),
                    })
                    .collect();
                key.push((t, args));
                atoms.push(i);
                if key.len() >= 2 {
                    occurrences.entry(key.clone()).or_default().push((
                        rid,
                        inverse.clone(),
                        atoms.clone(),
                    ));
                }
            }
        }
        let mut keys = occurrences
            .keys()
            .filter(|k| occurrences[*k].len() > 1)
            .cloned()
            .collect::<Vec<_>>();
        keys.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
        let mut ids: BTreeMap<Key, usize> = BTreeMap::new();
        let mut plans: Vec<Plan> = vec![];
        let mut consumers = BTreeMap::new();
        for key in keys {
            let id = plans.len();
            let inputs = (0..key
                .iter()
                .flat_map(|(_, args)| args)
                .filter_map(|a| {
                    if let Arg::Slot(s) = a {
                        Some(s + 1)
                    } else {
                        None
                    }
                })
                .max()
                .unwrap_or(0))
                .collect::<Vec<_>>();
            let parent = (2..key.len()).rev().find_map(|n| {
                ids.get(&key[..n]).map(|&provider| Projection {
                    provider,
                    slots: (0..plans[provider].inputs.len()).collect(),
                    atoms: (0..n).collect(),
                })
            });
            let literal = key
                .iter()
                .flat_map(|(_, a)| a)
                .any(|a| matches!(a,Arg::Literal(v) if identity(v)));
            plans.push(Plan {
                query: key
                    .iter()
                    .map(|(t, args)| Atom {
                        call: Call::Table(*t),
                        args: args.clone(),
                    })
                    .collect(),
                inputs,
                parent,
                literal,
            });
            for (rid, slots, atoms) in &occurrences[&key] {
                // The longest available prefix feeds each consumer. Its shorter
                // shared ancestors remain providers in the same DAG.
                consumers.insert(
                    *rid,
                    Projection {
                        provider: id,
                        slots: slots.clone(),
                        atoms: atoms.clone(),
                    },
                );
            }
            ids.insert(key, id);
        }
        Self {
            enabled,
            providers: (0..plans.len()).map(|_| Provider::default()).collect(),
            plans,
            consumers,
            ..Self::default()
        }
    }
    pub fn accepts(&self, rid: usize) -> bool {
        self.consumers.contains_key(&rid)
    }
    fn ensure(
        &mut self,
        id: usize,
        g: &mut Graph,
        ix: &mut QueryIndexes,
        stats: &mut Stats,
        signals: &mut Signals,
    ) -> Result<()> {
        self.stats.provider_requests += 1;
        let parent = self.plans[id].parent.clone();
        let seed = if let Some(p) = &parent {
            self.ensure(p.provider, g, ix, stats, signals)?;
            Some((
                self.providers[p.provider].revision,
                self.providers[p.provider].rows.clone(),
            ))
        } else {
            None
        };
        let plan = &self.plans[id];
        let state = &mut self.providers[id];
        state.requests += 1;
        let other = state
            .last_consumer
            .replace(stats.active_rule)
            .is_some_and(|r| r != stats.active_rule);
        let changed = if let Some((revision, rows)) = seed {
            advance(
                &mut state.continuation,
                &rows,
                revision,
                parent.as_ref().unwrap(),
                &plan.query,
                &plan.inputs,
                g,
                ix,
                stats,
                signals,
                &mut self.stats,
            )?
        } else {
            let dirty = !state.root.ready || signals.dirty(state.root.id);
            if dirty {
                if !state.root.ready {
                    state.root.chosen = Some(0);
                }
                evaluate(
                    &mut state.root,
                    &vec![None; plan.inputs.len()],
                    &mut vec![true; plan.query.len()],
                    &plan.query,
                    &plan.inputs,
                    g,
                    ix,
                    stats,
                    signals,
                    None,
                )?;
            }
            dirty
        };
        if !changed {
            self.stats.provider_hits += 1;
            state.hits += 1;
            if other {
                state.cross_consumer_hits += 1;
                self.stats.cross_consumer_hits += 1;
            }
            return Ok(());
        }
        self.stats.provider_refreshes += 1;
        state.refreshes += 1;
        let mut rows = Rows::new();
        if parent.is_some() {
            collect_suffix(
                &state.continuation,
                parent.as_ref().unwrap(),
                &plan.query,
                &plan.inputs,
                None,
                true,
                &mut rows,
                stats,
            )?;
        } else {
            collect(
                &state.root,
                &plan.inputs,
                &mut vec![None; plan.query.len()],
                0,
                None,
                &mut rows,
                stats,
                true,
            )?;
        }
        self.stats.output_rows_rebuilt += rows.len() as u64;
        if state.rows.len() == rows.len()
            && rows.iter().all(|(k, v)| {
                state
                    .identities
                    .get(k)
                    .and_then(|id| state.rows.get(id))
                    .is_some_and(|old| old.as_ref() == v)
            })
        {
            self.stats.unchanged_outputs += 1;
        } else {
            let mut next = Snapshot::new();
            let mut identities = BTreeMap::new();
            for (key, answer) in rows {
                let id = if let Some(&id) = state.identities.get(&key) {
                    id
                } else {
                    let id = state.next_identity;
                    state.next_identity = state
                        .next_identity
                        .checked_add(1)
                        .ok_or("prefix binding identity exhausted")?;
                    id
                };
                let value = state
                    .rows
                    .get(&id)
                    .filter(|old| old.as_ref() == &answer)
                    .cloned()
                    .unwrap_or_else(|| Rc::new(answer));
                identities.insert(key, id);
                next.insert(id, value);
            }
            state.identities = identities;
            state.rows = Rc::new(next);
            state.revision += 1;
        }
        Ok(())
    }
    pub fn matches(
        &mut self,
        rid: usize,
        g: &mut Graph,
        ix: &mut QueryIndexes,
        stats: &mut Stats,
        signals: &mut Signals,
        watermark: u64,
    ) -> Result<Vec<Answer>> {
        self.stats.consumer_calls += 1;
        let projection = self.consumers[&rid].clone();
        let clock = std::time::Instant::now();
        self.ensure(projection.provider, g, ix, stats, signals)?;
        self.stats.provider_seconds += clock.elapsed().as_secs_f64();
        let state = &self.providers[projection.provider];
        let rule = g.program.rules[rid].clone();
        let suffix = self.suffixes.entry(rid).or_default();
        let changed = advance(
            suffix,
            &state.rows,
            state.revision,
            &projection,
            &rule.query,
            &rule.inputs,
            g,
            ix,
            stats,
            signals,
            &mut self.stats,
        )?;
        if !changed {
            self.stats.consumer_hits += 1;
            if rule.seminaive {
                return Ok(vec![]);
            }
        }
        let mut rows = Rows::new();
        collect_suffix(
            suffix,
            &projection,
            &rule.query,
            &rule.inputs,
            rule.seminaive.then_some(watermark),
            false,
            &mut rows,
            stats,
        )?;
        self.stats.consumer_bindings += rows.len() as u64;
        Ok(rows.into_values().collect())
    }
    pub fn invalidate_literals(&mut self, rules: &BTreeSet<usize>, signals: &mut Signals) {
        for (plan, provider) in self.plans.iter().zip(&self.providers) {
            if plan.literal {
                mark_full(&provider.root, signals);
                provider.continuation.mark(signals);
            }
        }
        for (rid, suffix) in &self.suffixes {
            if rules.contains(rid) {
                suffix.mark(signals);
            }
        }
    }
    pub fn storage(&self) -> (usize, usize, usize) {
        let mut total = (0, 0, 0);
        for provider in &self.providers {
            add(&mut total, super::storage(&provider.root));
            add(&mut total, provider.continuation.storage());
        }
        for suffix in self.suffixes.values() {
            add(&mut total, suffix.storage());
        }
        total
    }
    pub fn report(&self) -> J {
        json!({"enabled":self.enabled,"stats":self.stats,"consumers":self.consumers.keys().collect::<Vec<_>>(),
            "plans":self.plans.iter().enumerate().map(|(i,p)|json!({"id":i,"atoms":p.query.len(),"slots":p.inputs.len(),"parent":p.parent.as_ref().map(|x|x.provider),"tables":p.query.iter().map(|a|match a.call{Call::Table(t)=>t,_=>unreachable!()}).collect::<Vec<_>>(),"rows":self.providers[i].rows.len(),"requests":self.providers[i].requests,"refreshes":self.providers[i].refreshes,"hits":self.providers[i].hits,"cross_consumer_hits":self.providers[i].cross_consumer_hits,"consumers":self.consumers.iter().filter(|(_,c)|c.provider==i).map(|(r,_)|*r).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "live_provider_rows":self.providers.iter().map(|p|p.rows.len()).sum::<usize>(),
            "retained_prefix_bindings":self.suffixes.values().map(|s|s.rows.len()).sum::<usize>(),
            "provider_extension_bindings":self.providers.iter().map(|s|s.continuation.rows.len()).sum::<usize>(),
            "scope":"shared connected table-prefix DAG; complete correlated bindings; consumer-specific stage/watermark/effects"})
    }
}
fn add(a: &mut (usize, usize, usize), b: (usize, usize, usize)) {
    a.0 += b.0;
    a.1 += b.1;
    a.2 += b.2;
}
impl Continuation {
    fn storage(&self) -> (usize, usize, usize) {
        let mut n = super::storage(&self.head);
        for (_, node) in self.rows.values() {
            add(&mut n, super::storage(node));
        }
        n
    }
    fn mark(&self, signals: &mut Signals) {
        mark_full(&self.head, signals);
        for (_, node) in self.rows.values() {
            mark_full(node, signals);
        }
    }
}
fn advance(
    c: &mut Continuation,
    rows: &Snapshot,
    revision: u64,
    p: &Projection,
    query: &[Atom],
    inputs: &[usize],
    g: &mut Graph,
    ix: &mut QueryIndexes,
    stats: &mut Stats,
    signals: &mut Signals,
    ps: &mut PrefixStats,
) -> Result<bool> {
    if c.head.ready && c.revision == revision && !signals.dirty(c.head.id) {
        return Ok(false);
    }
    if !c.head.ready {
        c.head.id = signals.allocate(None);
        c.head.ready = true;
        stats.contexts_created += 1;
        stats.live_contexts += 1;
        stats.peak_contexts = stats.peak_contexts.max(stats.live_contexts);
    }
    if c.revision != revision {
        let stale = c
            .rows
            .keys()
            .filter(|k| !rows.contains_key(*k))
            .cloned()
            .collect::<Vec<_>>();
        for k in stale {
            let (_, n) = c.rows.remove(&k).unwrap();
            retire(&n, stats, signals);
            ps.prefix_rows_retired += 1;
        }
        for (key, answer) in rows {
            if let Some((old, _)) = c.rows.get_mut(key) {
                if !Rc::ptr_eq(old, answer) {
                    *old = answer.clone();
                }
            } else {
                c.rows
                    .insert(key.clone(), (answer.clone(), Node::default()));
                ps.prefix_rows_attached += 1;
            }
        }
    }
    c.head.matches = 0;
    c.head.latest = 0;
    let mut remaining = vec![true; query.len()];
    for &a in &p.atoms {
        remaining[a] = false;
    }
    for (answer, node) in c.rows.values_mut() {
        if !node.ready || signals.dirty(node.id) {
            ps.suffix_updates += 1;
            let mut input = vec![None; inputs.iter().max().map_or(0, |s| s + 1)];
            for (&slot, value) in p.slots.iter().zip(&answer.values) {
                input[slot] = Some(value.clone());
            }
            evaluate(
                node,
                &input,
                &mut remaining,
                query,
                inputs,
                g,
                ix,
                stats,
                signals,
                Some(c.head.id),
            )?;
        }
        c.head.matches += node.matches;
        c.head.latest = c.head.latest.max(answer.epoch.max(node.latest));
    }
    c.revision = revision;
    signals.done(c.head.id);
    Ok(true)
}
fn collect_suffix(
    c: &Continuation,
    p: &Projection,
    query: &[Atom],
    inputs: &[usize],
    watermark: Option<u64>,
    latest: bool,
    out: &mut Rows,
    stats: &mut Stats,
) -> Result<()> {
    let mut witnesses = vec![None; query.len()];
    for (answer, node) in c.rows.values() {
        for (&a, w) in p.atoms.iter().zip(&answer.witnesses) {
            witnesses[a] = *w;
        }
        collect(
            node,
            inputs,
            &mut witnesses,
            answer.epoch,
            watermark,
            out,
            stats,
            latest,
        )?;
    }
    Ok(())
}
