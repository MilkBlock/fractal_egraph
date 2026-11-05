//! Counterfactual replay on the sealed Run snapshot. Its streams, indexes and
//! timers are isolated from the executor and never drive a real RHS.
use super::{Binding, StagePatch};
use crate::{
    graph::{Dependency, Graph, Row},
    program::{Arg, Call, Program, Result},
    slots::Slots,
    value::{self, Value},
};
use rustc_hash::FxHashMap as Map;
use serde::Serialize;
use smallvec::SmallVec;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

const PAIR_LIMIT: usize = 250_000;
const TABLE_LIMIT: usize = 250_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
struct Axis(usize, Option<usize>);
#[derive(Default, Serialize)]
struct Cost {
    cold_ns: u64,
    update_ns: u64,
    union_run_ns: u64,
    changes: u64,
    retirements: u64,
    rekeys: u64,
    changed_epoch: u64,
    peak: usize,
}
#[derive(Default)]
struct Directory {
    rows: Map<Dependency, Row>,
    epochs: BTreeSet<(u64, Dependency)>,
    keys: Map<Value, BTreeSet<Dependency>>,
    ready: bool,
    cost: Cost,
}
impl Directory {
    fn put(&mut self, col: Option<usize>, w: Dependency, row: Option<Row>) {
        let next = row.filter(|r| !r.hidden);
        if let Some(old) = self.rows.remove(&w) {
            self.epochs.remove(&(old.epoch, w));
            if next.is_none() {
                self.cost.retirements += 1
            }
            if let Some(c) = col {
                let k = &old.values[c];
                if next.as_ref().is_none_or(|r| r.values[c] != *k) {
                    let bucket = self.keys.get_mut(k).expect("indexed witness");
                    bucket.remove(&w);
                    if bucket.is_empty() {
                        self.keys.remove(k);
                    }
                    self.cost.rekeys += u64::from(next.is_some());
                }
            }
        }
        if let Some(row) = next {
            if let Some(c) = col {
                self.keys
                    .entry(row.values[c].clone())
                    .or_default()
                    .insert(w);
            }
            self.epochs.insert((row.epoch, w));
            self.rows.insert(w, row);
        }
    }
    fn build(&mut self, g: &Graph, axis: Axis) -> Result<()> {
        if !self.ready {
            let start = Instant::now();
            for w in g.witnesses(axis.0) {
                self.put(axis.1, w, g.row(w)?);
            }
            self.cost.peak = self.cost.peak.max(self.rows.len());
            self.ready = true;
            self.cost.cold_ns += start.elapsed().as_nanos() as u64;
        }
        Ok(())
    }
}
#[derive(Clone)]
enum Field {
    Row(usize, usize),
    Const(Value),
    Temp(usize),
}
impl Field {
    fn get<'a>(&'a self, rows: &[&'a Row], temp: &'a [Value]) -> &'a Value {
        match self {
            Self::Row(i, c) => &rows[*i].values[*c],
            Self::Const(v) => v,
            Self::Temp(i) => &temp[*i],
        }
    }
    fn lower(a: &Arg, slots: &BTreeMap<usize, Field>) -> Option<Self> {
        match a {
            Arg::Slot(s) => slots.get(s).cloned(),
            Arg::Literal(v) if !matches!(v, Value::Eq(_) | Value::Set(_)) => {
                Some(Self::Const(v.clone()))
            }
            _ => None,
        }
    }
}
struct Primitive {
    name: String,
    output: String,
    args: Vec<Field>,
    check: Option<Field>,
}
struct Member {
    rule: usize,
    eq: Vec<(Field, Field)>,
    ops: Vec<Primitive>,
    output: Vec<(usize, Field)>,
}
impl Member {
    fn new(p: &Program, rid: usize, atoms: &[usize]) -> Option<Self> {
        let r = &p.rules[rid];
        if !r.seminaive {
            return None;
        }
        let mut slots = BTreeMap::new();
        let mut eq = vec![];
        let mut ops = vec![];
        for (side, &atom) in atoms.iter().enumerate() {
            for (col, a) in r.query[atom].args.iter().enumerate() {
                let source = Field::Row(side, col);
                if let Some(other) = Field::lower(a, &slots) {
                    eq.push((source, other))
                } else if let Arg::Slot(s) = a {
                    slots.insert(*s, source);
                } else {
                    return None;
                }
            }
        }
        let mut remaining = r
            .query
            .iter()
            .filter(|a| matches!(a.call, Call::Primitive(..)))
            .collect::<Vec<_>>();
        while !remaining.is_empty() {
            let pos = remaining.iter().position(|a| {
                a.args[..a.args.len() - 1]
                    .iter()
                    .all(|v| Field::lower(v, &slots).is_some())
            })?;
            let atom = remaining.remove(pos);
            let Call::Primitive(name, sort) = &atom.call else {
                return None;
            };
            // Keep this speculative interpreter total, even on pairs filtered by a later guard.
            if !matches!(
                name.as_str(),
                "<" | ">" | "<=" | ">=" | "=" | "!=" | "+" | "-" | "*" | "min" | "max"
            ) {
                return None;
            }
            let (last, inputs) = atom.args.split_last()?;
            let args = inputs
                .iter()
                .map(|a| Field::lower(a, &slots))
                .collect::<Option<Vec<_>>>()?;
            let check = Field::lower(last, &slots);
            if check.is_none() {
                let Arg::Slot(s) = last else { return None };
                slots.insert(*s, Field::Temp(ops.len()));
            }
            ops.push(Primitive {
                name: name.clone(),
                output: sort.clone(),
                args,
                check,
            });
        }
        Some(Self {
            rule: rid,
            eq,
            ops,
            output: r
                .inputs
                .iter()
                .map(|s| Some((*s, slots.get(s)?.clone())))
                .collect::<Option<_>>()?,
        })
    }
    fn emit(&self, rows: &[&Row], out: &mut Vec<Slots>) -> Result<()> {
        if self
            .eq
            .iter()
            .any(|(a, b)| a.get(rows, &[]) != b.get(rows, &[]))
        {
            return Ok(());
        }
        let mut temps = SmallVec::<[Value; 4]>::new();
        for op in &self.ops {
            let args: SmallVec<[Value; 4]> = op
                .args
                .iter()
                .map(|f| f.get(rows, &temps).clone())
                .collect();
            let Some(v) = value::primitive(&op.name, &args, &op.output, &serde_json::Value::Null)?
            else {
                return Ok(());
            };
            if op.check.as_ref().is_some_and(|c| c.get(rows, &temps) != &v) {
                return Ok(());
            }
            temps.push(v);
        }
        out.push(
            self.output
                .iter()
                .map(|(s, f)| (*s, f.get(rows, &temps).clone()))
                .collect(),
        );
        Ok(())
    }
}

type Shape = (Axis, Option<Axis>);
struct Family {
    set: String,
    axes: Vec<usize>,
    tree: Option<Tree>,
    members: Vec<Member>,
    stats: Totals,
    windows: Vec<(u64, u64, Vec<usize>, u64, u64)>,
}
#[derive(Default, Serialize)]
struct Totals {
    runs: u64,
    fused_ns: u64,
    split_ns: u64,
    failed_ns: u64,
    skips: u64,
    mismatches: u64,
    answers: u64,
    first_delta: u64,
    late_peer: u64,
    both_fresh: u64,
}
#[derive(Default)]
struct Work {
    pairs: usize,
    first: u64,
    late: u64,
    both: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    a: usize,
    b: usize,
    ca: usize,
    cb: usize,
}
struct Hop {
    from: usize,
    to: usize,
    col: usize,
    axis: usize,
}
struct Tree {
    paths: Vec<Vec<Hop>>,
    roots: Vec<usize>,
}

fn common_tree(p: &Program, rules: &[(usize, Vec<usize>)]) -> Option<Vec<Edge>> {
    let mut common: Option<BTreeSet<Edge>> = None;
    for (r, atoms) in rules {
        let q = &p.rules[*r].query;
        let mut edges = BTreeSet::new();
        for a in 0..atoms.len() {
            for b in a + 1..atoms.len() {
                for (ca, v) in q[atoms[a]]
                    .args
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| matches!(v, Arg::Slot(_)))
                {
                    for (cb, _) in q[atoms[b]].args.iter().enumerate().filter(|(_, w)| *w == v) {
                        edges.insert(Edge { a, b, ca, cb });
                    }
                }
            }
        }
        if let Some(shared) = &mut common {
            shared.retain(|e| edges.contains(e))
        } else {
            common = Some(edges)
        }
    }
    let n = rules.first()?.1.len();
    let mut part: Vec<_> = (0..n).collect();
    let mut tree = vec![];
    for e in common? {
        let (a, b) = (part[e.a], part[e.b]);
        if a != b {
            for x in &mut part {
                if *x == b {
                    *x = a;
                }
            }
            tree.push(e);
        }
    }
    (tree.len() + 1 == n).then_some(tree)
}

fn visit_tree(
    dirs: &[Directory],
    tree: &Tree,
    wm: u64,
    mut send: impl FnMut(&[&Row]) -> Result<()>,
) -> Result<Option<Work>> {
    struct Walk<'a, F> {
        dirs: &'a [Directory],
        hops: &'a [Hop],
        pivot: usize,
        wm: u64,
        rows: Vec<Option<&'a Row>>,
        work: &'a mut Work,
        send: &'a mut F,
    }
    impl<'a, F: FnMut(&[&Row]) -> Result<()>> Walk<'a, F> {
        fn next(&mut self, i: usize) -> Result<bool> {
            if i == self.hops.len() {
                let rr: SmallVec<[&Row; 8]> = self.rows.iter().map(|r| r.unwrap()).collect();
                if self.pivot == 0 {
                    self.work.first += 1;
                } else {
                    self.work.late += 1;
                }
                self.work.both += u64::from(rr.iter().filter(|r| r.epoch >= self.wm).count() > 1);
                (self.send)(&rr)?;
                return Ok(true);
            }
            let hop = &self.hops[i];
            let key = &self.rows[hop.from].unwrap().values[hop.col];
            let dir = &self.dirs[hop.axis];
            if let Some(bucket) = dir.keys.get(key) {
                for w in bucket {
                    self.work.pairs += 1;
                    if self.work.pairs > PAIR_LIMIT {
                        return Ok(false);
                    }
                    let row = &dir.rows[w];
                    if hop.to < self.pivot && row.epoch >= self.wm {
                        continue;
                    }
                    self.rows[hop.to] = Some(row);
                    if !self.next(i + 1)? {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        }
    }
    let low = (
        wm,
        Dependency::Fact(crate::graph::FactRef { cell: 0, op: 0 }),
    );
    let mut work = Work::default();
    for (pivot, path) in tree.paths.iter().enumerate() {
        let dir = &dirs[tree.roots[pivot]];
        let mut walk = Walk {
            dirs,
            hops: path,
            pivot,
            wm,
            rows: vec![None; path.len() + 1],
            work: &mut work,
            send: &mut send,
        };
        for &(_, w) in dir.epochs.range(low..) {
            walk.rows[pivot] = Some(&dir.rows[&w]);
            if !walk.next(0)? {
                return Ok(None);
            }
        }
    }
    Ok(Some(work))
}

fn visit(
    dirs: &[Directory],
    f: &Family,
    axes: &[Axis],
    wm: u64,
    mut send: impl FnMut(&[&Row]) -> Result<()>,
) -> Result<Option<Work>> {
    if let Some(tree) = &f.tree {
        return visit_tree(dirs, tree, wm, send);
    }
    let mut work = Work::default();
    let low = (
        wm,
        Dependency::Fact(crate::graph::FactRef { cell: 0, op: 0 }),
    );
    if f.axes.len() == 1 {
        let d = &dirs[f.axes[0]];
        for &(_, w) in d.epochs.range(low..) {
            work.pairs += 1;
            if work.pairs > PAIR_LIMIT {
                return Ok(None);
            }
            work.first += 1;
            send(&[&d.rows[&w]])?;
        }
    } else {
        for side in [0, 1] {
            let a = f.axes[side];
            let b = f.axes[1 - side];
            let driver = &dirs[a];
            let peer = &dirs[b];
            for &(_, w) in driver.epochs.range(low..) {
                let row = &driver.rows[&w];
                let key = &row.values[axes[a].1.unwrap()];
                if let Some(bucket) = peer.keys.get(key) {
                    for v in bucket {
                        work.pairs += 1;
                        if work.pairs > PAIR_LIMIT {
                            return Ok(None);
                        }
                        let other = &peer.rows[v];
                        if side == 1 && other.epoch >= wm {
                            continue;
                        }
                        if side == 0 {
                            work.first += 1;
                            work.both += u64::from(other.epoch >= wm);
                            send(&[row, other])?;
                        } else {
                            work.late += 1;
                            send(&[other, row])?;
                        }
                    }
                }
            }
        }
    }
    Ok(Some(work))
}
pub(super) struct Replay {
    axes: Vec<Axis>,
    dirs: Vec<Directory>,
    watched: Vec<Vec<usize>>,
    by_set: BTreeMap<String, Vec<usize>>,
    families: Vec<Family>,
}
impl Replay {
    pub fn new(p: &Program) -> Self {
        let mut this = Self {
            axes: vec![],
            dirs: vec![],
            watched: vec![vec![]; p.tables.len()],
            by_set: BTreeMap::new(),
            families: vec![],
        };
        let mut axis_ids = BTreeMap::new();
        for (set, rules) in &p.sets {
            let mut groups = BTreeMap::<Shape, BTreeMap<usize, Vec<usize>>>::new();
            let mut wide = BTreeMap::<Vec<usize>, Vec<(usize, Vec<usize>)>>::new();
            for &rid in rules {
                let r = &p.rules[rid];
                let atoms = r
                    .query
                    .iter()
                    .enumerate()
                    .filter_map(|(i, a)| match a.call {
                        Call::Table(t) => Some((t, i)),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                match atoms.as_slice() {
                    &[(t, i)] => {
                        groups
                            .entry((Axis(t, None), None))
                            .or_default()
                            .insert(rid, vec![i]);
                    }
                    &[a, b] => {
                        for ((x, i), (y, j)) in [(a, b), (b, a)] {
                            if x > y {
                                continue;
                            }
                            for (c, arg) in r.query[i].args.iter().enumerate() {
                                if !matches!(arg, Arg::Slot(_)) {
                                    continue;
                                }
                                for (k, _) in r.query[j]
                                    .args
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, v)| *v == arg)
                                {
                                    if x == y && c > k {
                                        continue;
                                    }
                                    groups
                                        .entry((Axis(x, Some(c)), Some(Axis(y, Some(k)))))
                                        .or_default()
                                        .entry(rid)
                                        .or_insert_with(|| vec![i, j]);
                                }
                            }
                        }
                    }
                    many if many.len() >= 3 => {
                        let mut sorted = many.to_vec();
                        sorted.sort();
                        wide.entry(sorted.iter().map(|x| x.0).collect())
                            .or_default()
                            .push((rid, sorted.iter().map(|x| x.1).collect()));
                    }
                    _ => {}
                }
            }
            // Assign each rule once per ruleset. Wider agreement wins, then a stable column order.
            let mut groups: Vec<_> = groups.into_iter().filter(|(_, r)| r.len() > 1).collect();
            groups.sort_by_key(|(shape, r)| (std::cmp::Reverse(r.len()), *shape));
            let mut used = BTreeSet::new();
            for ((a, b), rules) in groups {
                let members: Vec<_> = rules
                    .into_iter()
                    .filter(|(r, _)| !used.contains(r))
                    .filter_map(|(r, atoms)| Member::new(p, r, &atoms))
                    .collect();
                if members.len() < 2 {
                    continue;
                }
                used.extend(members.iter().map(|m| m.rule));
                let mut axes = vec![];
                for axis in std::iter::once(a).chain(b) {
                    let id = *axis_ids.entry(axis).or_insert_with(|| {
                        let id = this.axes.len();
                        this.axes.push(axis);
                        this.dirs.push(Directory::default());
                        this.watched[axis.0].push(id);
                        id
                    });
                    axes.push(id);
                }
                this.by_set
                    .entry(set.clone())
                    .or_default()
                    .push(this.families.len());
                this.families.push(Family {
                    set: set.clone(),
                    axes,
                    tree: None,
                    members,
                    stats: Default::default(),
                    windows: vec![],
                });
            }
            for (tables, candidates) in wide {
                if candidates.len() < 2 {
                    continue;
                }
                let accepted: Vec<_> = candidates
                    .into_iter()
                    .filter_map(|(r, a)| Some((Member::new(p, r, &a)?, (r, a))))
                    .collect();
                if accepted.len() < 2 {
                    continue;
                }
                let (members, rules): (Vec<_>, Vec<_>) = accepted.into_iter().unzip();
                let Some(edges) = common_tree(p, &rules) else {
                    continue;
                };
                let mut axes = BTreeSet::new();
                let mut get_axis = |t, c| {
                    let axis = Axis(t, Some(c));
                    let id = *axis_ids.entry(axis).or_insert_with(|| {
                        let id = this.axes.len();
                        this.axes.push(axis);
                        this.dirs.push(Directory::default());
                        this.watched[t].push(id);
                        id
                    });
                    axes.insert(id);
                    id
                };
                let n = tables.len();
                let mut paths = vec![];
                let mut roots = vec![];
                for pivot in 0..n {
                    let mut joined = BTreeSet::from([pivot]);
                    let mut path = vec![];
                    while joined.len() < n {
                        let e = edges
                            .iter()
                            .find(|e| joined.contains(&e.a) != joined.contains(&e.b))
                            .unwrap();
                        let (from, to, c, k) = if joined.contains(&e.a) {
                            (e.a, e.b, e.ca, e.cb)
                        } else {
                            (e.b, e.a, e.cb, e.ca)
                        };
                        if path.is_empty() {
                            roots.push(get_axis(tables[from], c));
                        }
                        path.push(Hop {
                            from,
                            to,
                            col: c,
                            axis: get_axis(tables[to], k),
                        });
                        joined.insert(to);
                    }
                    paths.push(path);
                }
                this.by_set
                    .entry(set.clone())
                    .or_default()
                    .push(this.families.len());
                this.families.push(Family {
                    set: set.clone(),
                    axes: axes.into_iter().collect(),
                    tree: Some(Tree { paths, roots }),
                    members,
                    stats: Default::default(),
                    windows: vec![],
                });
            }
        }
        this
    }
    pub fn run(
        &mut self,
        g: &Graph,
        set: &str,
        patch: &StagePatch,
        samples: &[[u64; 8]],
    ) -> Result<()> {
        let Some(ids) = self.by_set.get(set) else {
            return Ok(());
        };
        let active: Map<usize, u64> = samples.iter().map(|a| (a[2] as usize, a[1])).collect();
        let mut ranges = Map::default();
        let mut i = 0;
        while i < patch.bindings.len() {
            let rid = patch.bindings[i].0;
            let start = i;
            while i < patch.bindings.len() && patch.bindings[i].0 == rid {
                i += 1
            }
            ranges.insert(rid, start..i);
        }
        for &id in ids {
            let f = &mut self.families[id];
            let mut windows = BTreeMap::<u64, Vec<usize>>::new();
            for (m, member) in f.members.iter().enumerate() {
                if let Some(&wm) = active.get(&member.rule) {
                    windows.entry(wm).or_default().push(m);
                }
            }
            for (wm, members) in windows {
                if members.len() < 2 {
                    continue;
                }
                if f.axes
                    .iter()
                    .any(|&a| g.witness_count(self.axes[a].0) > TABLE_LIMIT)
                {
                    f.stats.skips += 1;
                    continue;
                }
                for &a in &f.axes {
                    self.dirs[a].build(g, self.axes[a])?;
                }
                let replay = |which: &[usize]| -> Result<_> {
                    let mut out = vec![Vec::new(); which.len()];
                    let w = visit(&self.dirs, f, &self.axes, wm, |rows| {
                        for (&m, o) in which.iter().zip(&mut out) {
                            f.members[m].emit(rows, o)?;
                        }
                        Ok(())
                    })?;
                    for o in &mut out {
                        o.sort();
                        o.dedup();
                    }
                    Ok((out, w))
                };
                let split_replay = || -> Result<_> {
                    let clock = Instant::now();
                    let rows = members
                        .iter()
                        .map(|&m| replay(&[m]))
                        .collect::<Result<Vec<_>>>()?;
                    Ok((rows, clock.elapsed().as_nanos() as u64))
                };
                let fused_replay = || -> Result<_> {
                    let clock = Instant::now();
                    let (out, w) = replay(&members)?;
                    Ok((out, w, clock.elapsed().as_nanos() as u64))
                };
                let ((out, w, fused), (split_rows, split)) =
                    if (f.stats.runs + f.stats.skips + f.stats.mismatches) % 2 == 0 {
                        (fused_replay()?, split_replay()?)
                    } else {
                        let split = split_replay()?;
                        (fused_replay()?, split)
                    };
                let Some(w) = w else {
                    f.stats.skips += 1;
                    f.stats.failed_ns += fused;
                    continue;
                };
                if !out
                    .iter()
                    .zip(&split_rows)
                    .all(|(a, (b, w))| w.is_some() && *a == b[0])
                {
                    return Err("split replay differs from fused replay".into());
                }
                let correct = members.iter().zip(&out).all(|(&m, o)| {
                    let rr = ranges.get(&f.members[m].rule).cloned().unwrap_or(0..0);
                    let ordinary: &[(usize, Binding)] = &patch.bindings[rr];
                    o.len() == ordinary.len()
                        && o.iter().zip(ordinary).all(|(v, (_, b))| *v == b.values)
                });
                if !correct {
                    f.stats.mismatches += 1;
                    f.stats.failed_ns += fused;
                    continue;
                }
                f.stats.runs += 1;
                f.stats.fused_ns += fused;
                f.stats.split_ns += split;
                f.stats.answers += out.iter().map(Vec::len).sum::<usize>() as u64;
                f.stats.first_delta += w.first;
                f.stats.late_peer += w.late;
                f.stats.both_fresh += w.both;
                f.windows.push((
                    g.epoch,
                    wm,
                    members.iter().map(|&m| f.members[m].rule).collect(),
                    fused,
                    split,
                ));
            }
        }
        Ok(())
    }
    pub fn publish(&mut self, g: &Graph, with_union: bool) -> Result<()> {
        for (t, changes) in g.changes.tables() {
            for &a in &self.watched[t] {
                let dir = &mut self.dirs[a];
                if !dir.ready {
                    continue;
                }
                let start = Instant::now();
                if g.witness_count(t) > TABLE_LIMIT {
                    dir.rows = Default::default();
                    dir.epochs = Default::default();
                    dir.keys = Default::default();
                    dir.ready = false;
                    let ns = start.elapsed().as_nanos() as u64;
                    dir.cost.update_ns += ns;
                    if with_union {
                        dir.cost.union_run_ns += ns
                    }
                    continue;
                }
                for (&w, kind) in changes {
                    dir.put(self.axes[a].1, w, kind.row(g, w)?);
                }
                let ns = start.elapsed().as_nanos() as u64;
                dir.cost.update_ns += ns;
                if with_union {
                    dir.cost.union_run_ns += ns
                }
                dir.cost.peak = dir.cost.peak.max(dir.rows.len());
                dir.cost.changes += changes.len() as u64;
                dir.cost.changed_epoch += 1;
            }
        }
        Ok(())
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"pair_limit":PAIR_LIMIT,"table_limit":TABLE_LIMIT,"axes":self.axes.iter().zip(&self.dirs).map(|(a,d)|(a,&d.cost)).collect::<Vec<_>>(),
            "families":self.families.iter().map(|f|serde_json::json!({"set":f.set,"axes":f.axes,"members":f.members.iter().map(|m|m.rule).collect::<Vec<_>>(),"stats":f.stats,"windows":f.windows})).collect::<Vec<_>>()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn any_fresh_tree_pivot_emits_the_tuple_once() {
        let make = |i, e| Row {
            table: i,
            witness: Dependency::Function(i),
            epoch: e,
            hidden: false,
            values: crate::graph::row_fields(vec![Value::Int(7)]),
        };
        for mask in 0..8 {
            let mut dirs: Vec<_> = (0..3).map(|_| Directory::default()).collect();
            for (i, dir) in dirs.iter_mut().enumerate() {
                let r = make(i, if mask & (1 << i) != 0 { 2 } else { 1 });
                dir.put(Some(0), r.witness, Some(r));
            }
            let hop = |from, to| Hop {
                from,
                to,
                col: 0,
                axis: to,
            };
            let tree = Tree {
                roots: vec![0, 1, 2],
                paths: vec![
                    vec![hop(0, 1), hop(1, 2)],
                    vec![hop(1, 0), hop(1, 2)],
                    vec![hop(2, 1), hop(1, 0)],
                ],
            };
            let mut count = 0;
            visit_tree(&dirs, &tree, 2, |rows| {
                assert_eq!(rows.len(), 3);
                count += 1;
                Ok(())
            })
            .unwrap();
            assert_eq!(count, usize::from(mask != 0));
        }
    }
    #[test]
    fn directory_rekeys_and_retires_the_same_witness() {
        let mut d = Directory::default();
        let w = Dependency::Function(0);
        for n in [1, 2] {
            let row = Row {
                table: 0,
                witness: w,
                epoch: n,
                hidden: false,
                values: crate::graph::row_fields(vec![Value::Int(n as i64)]),
            };
            d.put(Some(0), w, Some(row));
        }
        assert_eq!(d.keys.len(), 1);
        assert_eq!(d.epochs.len(), 1);
        assert!(d.keys.contains_key(&Value::Int(2)));
        d.put(Some(0), w, None);
        assert!(d.rows.is_empty() && d.keys.is_empty() && d.epochs.is_empty());
    }
}
