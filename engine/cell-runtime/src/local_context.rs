//! A small structural root, root-keyed ports and a wide terminal expansion.
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
                    let columns = b
                        .args
                        .iter()
                        .enumerate()
                        .filter_map(|(i, a)| {
                            matches!(a,Arg::Slot(s) if known.contains(s)).then_some(i)
                        })
                        .collect();
                    let versions = std::iter::once(root)
                        .chain(ports.iter().copied())
                        .map(|i| table(&q[i]))
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    let identity_literal = q.iter().any(|a| {
                        a.args
                            .iter()
                            .any(|v| matches!(v,Arg::Literal(v) if identity(v)))
                    });
                    // Two orientations lack a unique compiler-owned root.
                    if chosen.is_some() {
                        return None;
                    }
                    let mut fields = known
                        .iter()
                        .map(|s| (*s, Field::Prefix(*s)))
                        .collect::<BTreeMap<_, _>>();
                    let mut checks = vec![];
                    for (col, v) in b.args.iter().enumerate() {
                        match v {
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
                    let output = r
                        .inputs
                        .iter()
                        .map(|s| fields.get(s).cloned())
                        .collect::<Option<Vec<_>>>()?;
                    chosen = Some(Plan {
                        root,
                        ports,
                        tail,
                        columns,
                        versions,
                        identity_literal,
                        output,
                        checks,
                    });
                }
            }
            chosen.map(Rc::new)
        })
        .collect()
}

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
    versions: Vec<u64>,
    unions: u64,
    roots: Map<Dependency, Root>,
    groups: Map<Vec<Value>, Group>,
    blocked: bool,
}
#[derive(Default, Serialize)]
pub(crate) struct Stats {
    calls: u64,
    fallbacks: u64,
    root_rows: u64,
    root_rebuilds: u64,
    root_reuses: u64,
    port_probes: u64,
    port_candidates: u64,
    full_terminal_rows: u64,
    delta_terminal_rows: u64,
    bindings: u64,
    terminal_pairs: u64,
    peak_roots: usize,
    peak_prefix_rows: usize,
}
#[derive(Default)]
pub(crate) struct Contexts {
    enabled: bool,
    states: Map<usize, Context>,
    stats: BTreeMap<usize, Stats>,
}
impl Contexts {
    pub fn new() -> Self {
        Self {
            enabled: std::env::var("CELL_CPU_LOCAL_CONTEXT").as_deref() == Ok("1"),
            ..Default::default()
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn report(&self, program: &Program) -> serde_json::Value {
        let plans = program.local_contexts.iter().enumerate().filter_map(|(rid,p)| p.as_ref().map(|p| serde_json::json!({"rule":rid,"root":p.root,"ports":p.ports,"terminal":p.tail}))).collect::<Vec<_>>();
        serde_json::json!({"enabled":self.enabled,"root_limit":ROOT_LIMIT,"prefix_limit":PREFIX_LIMIT,"plans":plans,"rules":self.stats})
    }
    pub fn matches(
        &mut self,
        rid: usize,
        watermark: u64,
        save_witnesses: bool,
        g: &mut Graph,
        ix: &mut QueryIndexes,
        entries: &mut Scheduler,
    ) -> Result<Option<Vec<Answer>>> {
        let Some(plan) = g.program.local_contexts[rid].clone() else {
            return Ok(None);
        };
        let rule = g.program.rules[rid].clone();
        let root_table = table(&rule.query[plan.root]);
        let tail = &rule.query[plan.tail];
        let terminal = table(tail);
        let stats = self.stats.entry(rid).or_default();
        let roots = g.witness_count(root_table);
        if roots > ROOT_LIMIT || roots > g.witness_count(terminal) {
            self.states.remove(&rid);
            stats.fallbacks += 1;
            return Ok(None);
        }
        let c = self.states.entry(rid).or_default();
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
                        if !extend(g, ix, &rule, &plan.ports, p, &mut root, size, stats)? {
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
                        if row.epoch >= watermark {
                            &group.rows
                        } else {
                            &group.rows[fresh..]
                        },
                        &row,
                        save_witnesses,
                        &mut answers,
                    );
                }
            }
        }
        if c.groups
            .values()
            .any(|b| b.rows.last().unwrap().epoch < watermark)
        {
            let before = stats.delta_terminal_rows;
            entries.terminal_delta(g, rid, plan.tail, watermark, |row| {
                stats.delta_terminal_rows += 1;
                let key = plan
                    .columns
                    .iter()
                    .map(|&col| row.values[col].clone())
                    .collect::<Vec<_>>();
                if let Some(group) = c.groups.get(&key)
                    && group.rows.last().unwrap().epoch < watermark
                {
                    stats.terminal_pairs +=
                        emit(g, &plan, &group.rows, row, save_witnesses, &mut answers);
                }
            })?;
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
        stats.port_candidates += 1;
        g.stats.residual_candidates += 1;
        if let Some(row) = g.row(w)? {
            let mut child = p.clone();
            if bind_row(g, a, &row, &mut child.values) {
                child.witnesses[atom] = Some(w);
                child.epoch = child.epoch.max(row.epoch);
                if !extend(g, ix, r, rest, child, root, size, stats)? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}
fn emit(
    g: &Graph,
    plan: &Plan,
    prefixes: &[Rc<Prefix>],
    row: &Row,
    save_witnesses: bool,
    out: &mut Vec<Answer>,
) -> u64 {
    if row.hidden
        || !plan
            .checks
            .iter()
            .all(|(col, f)| f.agrees(*col, &prefixes[0], row, g))
    {
        return 0;
    }
    for p in prefixes {
        let epoch = p.epoch.max(row.epoch);
        let values = plan.output.iter().map(|f| f.read(p, row, g)).collect();
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
    prefixes.len() as u64
}
