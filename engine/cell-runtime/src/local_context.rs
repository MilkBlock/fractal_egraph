//! Retain a connected prefix and expand its final table at the scheduled Run.
//! Only correlated prefixes survive Run boundaries; terminal answers do not.
use crate::{
    entry_schedule::Scheduler,
    graph::{Dependency, Graph, Row},
    program::*,
    query::{QueryIndexes, ReadStamp},
    retained::{Answer, bind, operand},
    value::{Value, canon},
};
use rustc_hash::FxHashMap as Map;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Plan {
    root: usize,
    ports: Vec<usize>,
    tail: usize,
    columns: Vec<usize>,
    versions: Vec<usize>,
    identity_literal: bool,
    output: Vec<Field>,
    checks: Vec<(usize, Field)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    post: Vec<usize>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
enum Field {
    Prefix(usize),
    Tail(usize),
    Literal(Value),
}
impl Field {
    fn read(&self, p: &Prefix, t: &Row, g: &Graph) -> Value {
        match self {
            Self::Prefix(s) => p.values[*s].as_ref().unwrap().clone(),
            Self::Tail(col) => t.values[*col].clone(),
            Self::Literal(v) => canon(v, &g.parents),
        }
    }
    fn agrees(&self, col: usize, p: &Prefix, t: &Row, g: &Graph) -> bool {
        match self {
            Self::Prefix(s) => p.values[*s].as_ref() == Some(&t.values[col]),
            Self::Tail(c) => t.values[*c] == t.values[col],
            Self::Literal(v) => canon(v, &g.parents) == t.values[col],
        }
    }
}
fn slots(args: &[Arg]) -> BTreeSet<usize> {
    args.iter()
        .filter_map(|a| match a {
            Arg::Slot(i) => Some(*i),
            _ => None,
        })
        .collect()
}
fn table(a: &Atom) -> usize {
    match a.call {
        Call::Table(t) => t,
        _ => unreachable!("checked context atom"),
    }
}
fn identity(v: &Value) -> bool {
    match v {
        Value::Eq(_) => true,
        Value::Set(s) => s.iter().any(identity),
        _ => false,
    }
}
pub(crate) fn derive(p: &Program, entries: &serde_json::Value) -> Vec<Option<Rc<Plan>>> {
    let spine = entries
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["rule"].as_u64().map(|r| r as usize))
        .collect::<BTreeSet<_>>();
    p.rules
        .iter()
        .enumerate()
        .map(|(rid, r)| {
            let q = &r.query;
            if !r.seminaive
                || spine.contains(&rid)
                || q.len() < 4
                || q.iter().any(|a| !matches!(a.call, Call::Table(_)))
            {
                return None;
            }
            let mut chosen = None;
            for (root, a) in q.iter().enumerate() {
                if !p.tables[table(a)].constructor {
                    continue;
                }
                let keys = slots(&a.args[..a.args.len() - 1]);
                let anchor = slots(&a.args);
                if keys.len() < 2 {
                    continue;
                }
                for (tail, b) in q.iter().enumerate() {
                    let tail_slots = slots(&b.args);
                    if root == tail || !tail_slots.is_disjoint(&anchor) {
                        continue;
                    }
                    let ports = (0..q.len())
                        .filter(|&i| i != root && i != tail)
                        .collect::<Vec<_>>();
                    let mut used = BTreeSet::<usize>::new();
                    let mut known = anchor.clone();
                    if !ports.iter().all(|&i| {
                        let args = &q[i].args;
                        let k = slots(&args[..args.len() - 1]);
                        let link = k.intersection(&keys).copied().collect::<Vec<_>>();
                        used.extend(link.iter().copied());
                        known.extend(slots(args));
                        !link.is_empty()
                    }) || used.len() < 2
                    {
                        continue;
                    }
                    let tk = slots(&b.args[..b.args.len() - 1]);
                    if tk.is_disjoint(&known) || tk.is_subset(&known) {
                        continue;
                    }
                    if chosen.is_some() {
                        return None;
                    }
                    chosen = Some(project(r, root, ports, tail, vec![])?);
                }
            }
            chosen.map(Rc::new)
        })
        .collect()
}

fn project(
    r: &Rule,
    root: usize,
    ports: Vec<usize>,
    tail: usize,
    post: Vec<usize>,
) -> Option<Plan> {
    let q = &r.query;
    let prefix = std::iter::once(root).chain(ports.iter().copied());
    let known: BTreeSet<_> = prefix.clone().flat_map(|i| slots(&q[i].args)).collect();
    let versions = prefix
        .filter_map(|i| match q[i].call {
            Call::Table(t) => Some(t),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let columns = q[tail]
        .args
        .iter()
        .enumerate()
        .filter_map(|(i, a)| matches!(a,Arg::Slot(s) if known.contains(s)).then_some(i))
        .collect();
    let identity_literal = q.iter().any(|a| {
        a.args
            .iter()
            .any(|v| matches!(v,Arg::Literal(v) if identity(v)))
    });
    let mut fields: BTreeMap<_, _> = known.into_iter().map(|s| (s, Field::Prefix(s))).collect();
    let mut checks = vec![];
    for (col, arg) in q[tail].args.iter().enumerate() {
        match arg {
            Arg::Literal(v) => checks.push((col, Field::Literal(v.clone()))),
            Arg::Slot(s) => match fields.entry(*s) {
                std::collections::btree_map::Entry::Occupied(e) => {
                    checks.push((col, e.get().clone()))
                }
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(Field::Tail(col));
                }
            },
        }
    }
    if !post.is_empty() {
        let supplied: BTreeSet<_> = q.iter().flat_map(|a| slots(&a.args)).collect();
        if !r.inputs.iter().all(|s| supplied.contains(s)) {
            return None;
        }
    }
    let output = if post.is_empty() {
        r.inputs
            .iter()
            .map(|s| fields.get(s).cloned())
            .collect::<Option<_>>()?
    } else {
        vec![]
    };
    Some(Plan {
        root,
        ports,
        tail,
        columns,
        versions,
        identity_literal,
        output,
        checks,
        post,
    })
}

#[path = "context_cuts.rs"]
mod cuts;
pub(crate) use cuts::{Cuts, derive_cuts, layout};

const ROOT_LIMIT: usize = 256;
const PREFIX_LIMIT: usize = 4096;
#[derive(Clone)]
struct Prefix {
    values: Vec<Option<Value>>,
    witnesses: Vec<Option<Dependency>>,
    epoch: u64,
}
struct Root {
    epoch: u64,
    leases: Vec<ReadStamp>,
    outputs: Vec<Rc<Prefix>>,
}
#[derive(Default)]
struct Group {
    rows: Vec<Rc<Prefix>>,
}
#[derive(Default)]
struct Context {
    orientation: Option<(usize, usize)>,
    versions: Vec<u64>,
    unions: u64,
    roots: Map<Dependency, Root>,
    groups: Map<Vec<Value>, Group>,
    blocked: bool,
    oversized_at: Vec<usize>,
}
#[derive(Default, Serialize)]
pub(crate) struct Stats {
    calls: u64,
    orientations: BTreeMap<usize, u64>,
    spine_deferrals: u64,
    fallbacks: u64,
    root_rows: u64,
    root_rebuilds: u64,
    root_reuses: u64,
    port_probes: u64,
    port_candidates: u64,
    full_terminal_rows: u64,
    delta_terminal_rows: u64,
    reverse_reads: u64,
    reverse_groups: u64,
    bindings: u64,
    terminal_pairs: u64,
    peak_roots: usize,
    peak_prefix_rows: usize,
}
#[derive(Default)]
pub(crate) struct Contexts {
    enabled: bool,
    general: bool,
    states: Map<usize, Context>,
    stats: BTreeMap<usize, Stats>,
}
impl Contexts {
    pub fn new() -> Self {
        let mode = std::env::var("CELL_CPU_LOCAL_CONTEXT");
        Self {
            enabled: matches!(mode.as_deref(), Ok("1" | "general")),
            general: mode.as_deref() == Ok("general"),
            ..Default::default()
        }
    }
    pub fn general(&self) -> bool {
        self.general
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn accepts(&self, p: &Program, rid: usize) -> bool {
        self.enabled()
            && (p.local_contexts[rid].is_some() || self.general && !p.context_cuts[rid].is_empty())
    }
    pub fn report(&self, program: &Program) -> serde_json::Value {
        let (cut_rules, cut_orientations) = program.context_cut_counts();
        let plans = program.local_contexts.iter().enumerate().filter_map(|(rid,p)| p.as_ref().map(|p| serde_json::json!({"rule":rid,"root":p.root,"ports":p.ports,"terminal":p.tail}))).collect::<Vec<_>>();
        serde_json::json!({"enabled":self.enabled,"root_limit":ROOT_LIMIT,"prefix_limit":PREFIX_LIMIT,"plans":plans,"general":self.general,"cut_rules":cut_rules,"cut_orientations":cut_orientations,"rules":self.stats})
    }
    pub fn matches(
        &mut self,
        rid: usize,
        watermark: u64,
        save_witnesses: bool,
        spine_active: bool,
        g: &mut Graph,
        ix: &mut QueryIndexes,
        entries: &mut Scheduler,
    ) -> Result<Option<Vec<Answer>>> {
        let rule = g.program.rules[rid].clone();
        let stats = self.stats.entry(rid).or_default();
        if spine_active {
            stats.spine_deferrals += 1;
            return Ok(None);
        }
        let plan = if !self.general || g.program.context_cuts[rid].is_empty() {
            let p = g.program.local_contexts[rid].as_ref().unwrap();
            p.clone()
        } else {
            let best = g.program.context_cuts[rid]
                .iter()
                .filter_map(|p| {
                    let roots = g.witness_count(table(&rule.query[p.root]));
                    let tails = g.witness_count(table(&rule.query[p.tail]));
                    (roots <= ROOT_LIMIT && roots <= tails)
                        .then_some(((roots, std::cmp::Reverse(tails), p.root), p))
                })
                .min_by_key(|(score, _)| *score);
            let Some((_, p)) = best else {
                self.states.remove(&rid);
                stats.fallbacks += 1;
                return Ok(None);
            };
            p.clone()
        };
        let root_table = table(&rule.query[plan.root]);
        let tail = &rule.query[plan.tail];
        let terminal = table(tail);
        let roots = g.witness_count(root_table);
        if roots > ROOT_LIMIT || roots > g.witness_count(terminal) {
            self.states.remove(&rid);
            stats.fallbacks += 1;
            return Ok(None);
        }
        let c = self.states.entry(rid).or_default();
        let orientation = Some((plan.root, plan.tail));
        if c.orientation != orientation {
            *c = Context {
                orientation,
                ..Default::default()
            };
        }
        // An oversized cut is a performance miss, never a negative query answer.
        // Probe it again only after a prefix table shrinks. Original search
        // still observes same-size replacements while this cut is deferred.
        if self.general
            && c.blocked
            && !plan
                .versions
                .iter()
                .zip(&c.oversized_at)
                .any(|(&t, &old)| g.witness_count(t) < old)
        {
            stats.fallbacks += 1;
            return Ok(None);
        }
        let versions = plan
            .versions
            .iter()
            .map(|&t| g.query_versions[t])
            .collect::<Vec<_>>();
        if c.versions != versions || (plan.identity_literal && c.unions != g.stats.unions) {
            let reset = plan.identity_literal && c.unions != g.stats.unions;
            c.versions = versions;
            c.unions = g.stats.unions;
            c.blocked = false;
            let mut changed = reset;
            let mut previous = std::mem::take(&mut c.roots);
            let mut size = 0;
            let mut work_left = if self.general {
                PREFIX_LIMIT
            } else {
                usize::MAX
            };
            for w in g.witnesses(root_table) {
                let Some(epoch) = g.visible_epoch(w) else {
                    continue;
                };
                let cached = previous.remove(&w);
                let root = if let Some(old) = cached.filter(|r| {
                    !reset && r.epoch == epoch && r.leases.iter().all(|s| ix.valid_read(g, s))
                }) {
                    stats.root_reuses += 1;
                    old
                } else {
                    changed = true;
                    stats.root_rows += 1;
                    g.stats.residual_candidates += 1;
                    let row = g.row(w)?.ok_or("missing visible root")?;
                    let mut p = Prefix {
                        values: vec![None; g.program.templates[rule.template].sorts.len()],
                        witnesses: vec![None; rule.query.len()],
                        epoch,
                    };
                    let mut root = Root {
                        epoch,
                        leases: vec![],
                        outputs: vec![],
                    };
                    if bind_row(g, &rule.query[plan.root], &row, &mut p.values) {
                        p.witnesses[plan.root] = Some(w);
                        stats.root_rebuilds += 1;
                        if !extend(
                            g,
                            ix,
                            &rule,
                            &plan.ports,
                            p,
                            &mut root,
                            size,
                            &mut work_left,
                            stats,
                        )? {
                            c.blocked = true;
                            break;
                        }
                    }
                    root
                };
                size += root.outputs.len() + root.leases.len();
                if size > PREFIX_LIMIT {
                    c.blocked = true;
                    break;
                }
                c.roots.insert(w, root);
            }
            if c.blocked {
                c.oversized_at = plan.versions.iter().map(|&t| g.witness_count(t)).collect();
                c.roots.clear();
                c.groups.clear();
            } else if changed || !previous.is_empty() {
                c.groups.clear();
                for p in c.roots.values().flat_map(|r| &r.outputs) {
                    let key = plan
                        .columns
                        .iter()
                        .map(|&col| operand(&tail.args[col], &p.values, g).unwrap())
                        .collect();
                    let group = c.groups.entry(key).or_default();
                    group.rows.push(p.clone());
                }
                for group in c.groups.values_mut() {
                    group.rows.sort_unstable_by_key(|p| p.epoch);
                }
                stats.peak_roots = stats.peak_roots.max(c.roots.len());
                stats.peak_prefix_rows = stats
                    .peak_prefix_rows
                    .max(c.groups.values().map(|g| g.rows.len()).sum());
            }
        }
        if c.blocked {
            stats.fallbacks += 1;
            return Ok(None);
        }
        stats.calls += 1;
        *stats.orientations.entry(plan.root).or_default() += 1;
        let mut answers = Vec::new();
        let pairs_before = stats.terminal_pairs;
        // New prefix facts can match old terminal rows. Stable prefixes only
        // see the current terminal delta. Every answer has at least one new row.
        for group in c
            .groups
            .values()
            .filter(|b| b.rows.last().unwrap().epoch >= watermark)
        {
            let fresh = group.rows.partition_point(|p| p.epoch < watermark);
            let bound = tail
                .args
                .iter()
                .map(|a| operand(a, &group.rows[0].values, g))
                .collect::<Vec<_>>();
            for &w in ix.candidates(g, terminal, &bound)?.iter() {
                stats.full_terminal_rows += 1;
                g.stats.residual_candidates += 1;
                if let Some(row) = g.row(w)?
                    && !row.hidden
                {
                    stats.terminal_pairs += emit(
                        g,
                        &plan,
                        &rule,
                        if row.epoch >= watermark {
                            &group.rows
                        } else {
                            &group.rows[fresh..]
                        },
                        &row,
                        save_witnesses,
                        &mut answers,
                    )?;
                }
            }
        }
        if c.groups
            .values()
            .any(|b| b.rows.last().unwrap().epoch < watermark)
        {
            let before = stats.delta_terminal_rows;
            let batch = entries.terminal_batch(g, rid, plan.tail, watermark)?;
            let budget = self
                .general
                .then(|| batch.delta_len())
                .flatten()
                .unwrap_or(0);
            let mut cost = c.groups.len();
            let mut domains = vec![];
            if cost < budget {
                for group in c
                    .groups
                    .values()
                    .filter(|b| b.rows.last().unwrap().epoch < watermark)
                {
                    let bound = tail
                        .args
                        .iter()
                        .map(|a| operand(a, &group.rows[0].values, g))
                        .collect::<Vec<_>>();
                    stats.reverse_groups += 1;
                    cost = cost.saturating_add(ix.count(g, terminal, &bound)?);
                    if cost >= budget {
                        break;
                    }
                    domains.push((group, bound));
                }
            }
            if cost < budget {
                stats.reverse_reads += 1;
                for (group, bound) in domains {
                    for &w in ix.candidates(g, terminal, &bound)?.iter() {
                        stats.delta_terminal_rows += 1;
                        if let Some(packet) = batch.packet(w) {
                            stats.terminal_pairs += emit(
                                g,
                                &plan,
                                &rule,
                                &group.rows,
                                &packet.row,
                                save_witnesses,
                                &mut answers,
                            )?;
                        }
                    }
                }
            } else {
                for &(_, w) in batch.delta() {
                    let row = &batch.packet(w).unwrap().row;
                    stats.delta_terminal_rows += 1;
                    let key = plan
                        .columns
                        .iter()
                        .map(|&col| row.values[col].clone())
                        .collect::<Vec<_>>();
                    if let Some(group) = c.groups.get(&key)
                        && group.rows.last().unwrap().epoch < watermark
                    {
                        stats.terminal_pairs += emit(
                            g,
                            &plan,
                            &rule,
                            &group.rows,
                            row,
                            save_witnesses,
                            &mut answers,
                        )?;
                    }
                }
            }
            g.stats.residual_candidates += stats.delta_terminal_rows - before;
        }
        g.stats.residual_candidates += stats.terminal_pairs - pairs_before;
        answers.sort_by(|a, b| a.values.cmp(&b.values));
        answers.dedup_by(|a, b| a.values == b.values);
        stats.bindings += answers.len() as u64;
        Ok(Some(answers))
    }
}
fn bind_row(g: &Graph, atom: &Atom, row: &Row, values: &mut [Option<Value>]) -> bool {
    !row.hidden
        && row.table == table(atom)
        && atom
            .args
            .iter()
            .zip(row.values.iter())
            .all(|(arg, v)| bind(arg, v, values, g))
}
fn extend(
    g: &mut Graph,
    ix: &mut QueryIndexes,
    r: &Rule,
    ports: &[usize],
    p: Prefix,
    root: &mut Root,
    size: usize,
    work_left: &mut usize,
    stats: &mut Stats,
) -> Result<bool> {
    if size + root.leases.len() + root.outputs.len() >= PREFIX_LIMIT {
        return Ok(false);
    }
    let Some((&atom, rest)) = ports.split_first() else {
        root.outputs.push(Rc::new(p));
        return Ok(true);
    };
    let a = &r.query[atom];
    if matches!(a.call, Call::Primitive(..)) {
        let mut p = p;
        if !finish_guards(g, r, &[atom], &mut p.values)? {
            return Ok(true);
        }
        return extend(g, ix, r, rest, p, root, size, work_left, stats);
    }
    let t = table(a);
    let bound = a
        .args
        .iter()
        .map(|v| operand(v, &p.values, g))
        .collect::<Vec<_>>();
    if ix.count(g, t, &bound)? > PREFIX_LIMIT {
        return Ok(false);
    }
    root.leases.push(ix.read_stamp(g, t, &bound)?);
    stats.port_probes += 1;
    for &w in ix.candidates(g, t, &bound)?.iter() {
        if *work_left == 0 {
            return Ok(false);
        }
        *work_left -= 1;
        stats.port_candidates += 1;
        g.stats.residual_candidates += 1;
        if let Some(row) = g.row(w)? {
            let mut child = p.clone();
            if bind_row(g, a, &row, &mut child.values) {
                child.witnesses[atom] = Some(w);
                child.epoch = child.epoch.max(row.epoch);
                if !extend(g, ix, r, rest, child, root, size, work_left, stats)? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}
fn emit(
    g: &mut Graph,
    plan: &Plan,
    rule: &Rule,
    prefixes: &[Rc<Prefix>],
    row: &Row,
    save_witnesses: bool,
    out: &mut Vec<Answer>,
) -> Result<u64> {
    if row.hidden
        || !plan
            .checks
            .iter()
            .all(|(col, f)| f.agrees(*col, &prefixes[0], row, g))
    {
        return Ok(0);
    }
    for p in prefixes {
        let epoch = p.epoch.max(row.epoch);
        let values = if plan.post.is_empty() {
            plan.output.iter().map(|f| f.read(p, row, g)).collect()
        } else {
            let mut slots = p.values.clone();
            if !bind_row(g, &rule.query[plan.tail], row, &mut slots) {
                continue;
            }
            if !finish_guards(g, rule, &plan.post, &mut slots)? {
                continue;
            }
            rule.inputs
                .iter()
                .map(|s| slots[*s].as_ref().unwrap().clone())
                .collect()
        };
        let witnesses = if save_witnesses {
            let mut w = p.witnesses.clone();
            w[plan.tail] = Some(row.witness);
            w
        } else {
            vec![]
        };
        out.push(Answer {
            values,
            witnesses,
            epoch,
        });
    }
    Ok(prefixes.len() as u64)
}

fn finish_guards(
    g: &mut Graph,
    r: &Rule,
    post: &[usize],
    values: &mut [Option<Value>],
) -> Result<bool> {
    for &i in post {
        let a = &r.query[i];
        let Call::Primitive(name, sort) = &a.call else {
            unreachable!()
        };
        let (dst, args) = a.args.split_last().unwrap();
        let inputs = args
            .iter()
            .map(|arg| operand(arg, values, g).unwrap())
            .collect::<Vec<_>>();
        if name.starts_with("cell-key-") {
            g.stats.key_operations += 1;
        }
        let Some(v) = crate::value::primitive(
            name,
            &inputs,
            sort,
            &g.program.artifact["source_core"]["structural_key"]["nodes"],
        )?
        else {
            return Ok(false);
        };
        if !bind(dst, &v, values, g) {
            return Ok(false);
        }
    }
    Ok(true)
}
