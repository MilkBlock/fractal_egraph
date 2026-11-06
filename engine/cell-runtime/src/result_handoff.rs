//! Symbolic SSA keys on one finite, witnessed block path. Only constructor
//! facts are donors. Acquisition of a live fact remains a runtime obligation.
use crate::{
    graph::{Dependency, FactRef},
    known_results::Source,
    program::*,
    value::Value,
};
use serde::Serialize;
use std::{collections::BTreeMap, rc::Rc};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub(crate) enum Donor {
    Query(usize),
    Effect(usize),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Reuse {
    pub equal_output: bool,
    pub op: usize,
    pub back: usize,
    pub rule: usize,
    pub donor: Donor,
}
#[derive(Clone, Copy)]
pub(crate) struct Input {
    pub op: usize,
    pub back: usize,
    pub fact: Option<FactRef>,
    pub equal_output: bool,
}
#[derive(Clone)]
struct Fact {
    stage: usize,
    rule: usize,
    source: Donor,
    table: usize,
    key: Vec<usize>,
    result: usize,
}
#[derive(Default)]
struct Symbols {
    parents: Vec<usize>,
    literals: BTreeMap<Value, usize>,
}
impl Symbols {
    fn fresh(&mut self) -> usize {
        let i = self.parents.len();
        self.parents.push(i);
        i
    }
    fn root(&self, mut i: usize) -> usize {
        while self.parents[i] != i {
            i = self.parents[i]
        }
        i
    }
    fn equal(&mut self, a: usize, b: usize) {
        let a = self.root(a);
        let b = self.root(b);
        self.parents[a.max(b)] = a.min(b);
    }
    fn arg(&mut self, a: &Arg, slots: &[usize]) -> usize {
        match a {
            Arg::Slot(s) => slots[*s],
            Arg::Literal(v) => {
                if let Some(&i) = self.literals.get(v) {
                    return i;
                }
                let i = self.fresh();
                self.literals.insert(v.clone(), i);
                i
            }
        }
    }
    fn same_key(&self, a: &[usize], b: &[usize]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| self.root(x) == self.root(y))
    }
}
pub(crate) fn derive(p: &Program, path: &[(Option<usize>, (usize, usize))]) -> Vec<Reuse> {
    derive_both(p, path).0
}
pub(crate) fn derive_lhs(p: &Program, path: &[(Option<usize>, (usize, usize))]) -> Vec<Reuse> {
    derive_both(p, path).1
}
fn derive_both(p: &Program, path: &[(Option<usize>, (usize, usize))]) -> (Vec<Reuse>, Vec<Reuse>) {
    if path.len() < 2 {
        return (vec![], vec![]);
    }
    let mut symbols = Symbols::default();
    let mut facts = Vec::<Fact>::new();
    let mut read_facts = Vec::<Fact>::new();
    let mut published = Vec::new();
    let mut program = vec![];
    let mut lhs = vec![];
    for (stage, &(_, (rid, pivot))) in path.iter().enumerate() {
        let r = &p.rules[rid];
        let t = &p.templates[r.template];
        let mut slots = (0..t.sorts.len())
            .map(|_| symbols.fresh())
            .collect::<Vec<_>>();
        for (arg, col) in r.query[pivot].args.iter().zip(published.drain(..)) {
            let into = symbols.arg(arg, &slots);
            symbols.equal(into, col);
        }
        for (atom, a) in r.query.iter().enumerate() {
            let Call::Table(table) = a.call else { continue };
            let (value, keys) = a.args.split_last().unwrap();
            let result = symbols.arg(value, &slots);
            let key = keys
                .iter()
                .map(|a| symbols.arg(a, &slots))
                .collect::<Vec<_>>();
            if let Some(f) = read_facts
                .iter()
                .rev()
                .find(|f| f.table == table && symbols.same_key(&f.key, &key))
            {
                if stage + 1 == path.len() && atom != pivot && f.stage < stage {
                    lhs.push(Reuse {
                        equal_output: false,
                        op: atom,
                        back: stage - f.stage,
                        rule: f.rule,
                        donor: f.source,
                    });
                }
            }
            read_facts.push(Fact {
                stage,
                rule: rid,
                source: Donor::Query(atom),
                table,
                key: key.clone(),
                result,
            });
            // Mutable values are read at activation; they never unify historical
            // result symbols or become constructor RHS completion certificates.
            if !p.tables[table].constructor {
                continue;
            }
            if let Some(f) = facts
                .iter()
                .rev()
                .find(|f| f.table == table && symbols.same_key(&f.key, &key))
            {
                symbols.equal(result, f.result)
            }
            facts.push(Fact {
                stage,
                rule: rid,
                source: Donor::Query(atom),
                table,
                key,
                result,
            });
        }
        let publication = path.get(stage + 1).and_then(|s| s.0);
        let mut unions = vec![];
        for (op, ins) in t.ops.iter().enumerate() {
            let (table, keys, result_arg) = match ins {
                Op::Copy(dst, a) => {
                    slots[*dst] = symbols.arg(a, &slots);
                    continue;
                }
                Op::Union(a, b) => {
                    unions.push((symbols.arg(a, &slots), symbols.arg(b, &slots)));
                    continue;
                }
                Op::Hide(t, ..) => {
                    facts.retain(|f| f.table != *t);
                    read_facts.retain(|f| f.table != *t);
                    continue;
                }
                Op::Let(dst, Call::Table(t), a) => (*t, a, Arg::Slot(*dst)),
                Op::Set(t, a, v) => (*t, a, v.clone()),
                _ => continue,
            };
            let key = keys
                .iter()
                .map(|a| symbols.arg(a, &slots))
                .collect::<Vec<_>>();
            let value = symbols.arg(&result_arg, &slots);
            if Some(op) == publication {
                published = key.clone();
                // A mutable function's published result can change by the next Run.
                if p.tables[table].virtual_fact {
                    published.push(value);
                }
            }
            let source = match p.known_results.static_sources.get(rid).and_then(|s| s[op]) {
                Some(Source::Query(i)) => Donor::Query(i),
                Some(Source::Effect(i)) => Donor::Effect(i),
                None => Donor::Effect(op),
            };
            read_facts.push(Fact {
                stage,
                rule: rid,
                source,
                table,
                key: key.clone(),
                result: value,
            });
            if !p.tables[table].constructor || !p.tables[table].virtual_fact {
                continue;
            }
            if let Some(f) = facts
                .iter()
                .rev()
                .find(|f| f.table == table && symbols.same_key(&f.key, &key))
            {
                if f.stage != stage
                    && stage + 1 == path.len()
                    && !p.known_results.static_skip.get(rid).is_some_and(|s| s[op])
                {
                    program.push(Reuse {
                        equal_output: matches!(ins, Op::Set(..))
                            && symbols.root(value) == symbols.root(f.result),
                        op,
                        back: stage - f.stage,
                        rule: f.rule,
                        donor: f.source,
                    });
                }
                if matches!(ins, Op::Set(..)) {
                    unions.push((value, f.result));
                } else {
                    symbols.equal(value, f.result);
                }
            }
            let source = match p.known_results.static_sources.get(rid).and_then(|s| s[op]) {
                Some(Source::Query(i)) => Donor::Query(i),
                Some(Source::Effect(i)) => Donor::Effect(i),
                None => Donor::Effect(op),
            };
            facts.push(Fact {
                stage,
                rule: rid,
                source,
                table,
                key,
                result: value,
            });
        }
        for (a, b) in unions {
            symbols.equal(a, b);
        }
    }
    (program, lhs)
}
#[derive(Clone, Debug)]
struct Exports {
    rule: usize,
    rows: smallvec::SmallVec<[(Donor, Receipt); 2]>,
}
/// Cell IDs start at one. The unused zero owner encodes a function row index,
/// keeping constructor history at its original eight bytes per reference.
/// An unencodable function index is omitted and the reader falls back.
#[derive(Clone, Copy, Debug)]
struct Receipt(u64);
impl Receipt {
    fn encode(w: Dependency) -> Option<Self> {
        Some(Self(match w {
            Dependency::Fact(f) if f.cell != 0 => (u64::from(f.cell) << 32) | u64::from(f.op),
            Dependency::Function(i) => u64::from(u32::try_from(i).ok()?.checked_add(1)?),
            _ => return None,
        }))
    }
    fn decode(self) -> Dependency {
        let cell = (self.0 >> 32) as u32;
        let op = self.0 as u32;
        if cell == 0 {
            Dependency::Function((op - 1) as usize)
        } else {
            Dependency::Fact(FactRef { cell, op })
        }
    }
}
/// Shared by copied/published cursors. Empty stages maintain real offsets;
/// trimming at the largest compiled distance bounds the history.
#[derive(Debug)]
enum Trail {
    One(Exports),
    Many(Vec<Option<Rc<Exports>>>),
}
#[derive(Clone, Debug, Default)]
pub(crate) struct History(Option<Rc<Trail>>);
impl History {
    pub(crate) fn capture(
        &mut self,
        limit: usize,
        rid: usize,
        wanted: &[Donor],
        read: impl Fn(usize) -> Option<Dependency>,
        effects: impl Fn(usize) -> Option<Dependency>,
    ) -> (usize, usize, usize) {
        #[cfg(feature = "allocation-census")]
        let _allocation_scope =
            crate::allocation_census::scope(crate::allocation_census::Tag::History);
        if limit == 0 || (wanted.is_empty() && self.0.is_none()) {
            return (0, 0, 0);
        }
        let mut rows = smallvec::SmallVec::new();
        for &d in wanted {
            let w = match d {
                Donor::Query(i) => read(i),
                Donor::Effect(i) => effects(i),
            };
            if let Some(w) = w.and_then(Receipt::encode) {
                rows.push((d, w));
            }
        }
        let kept = rows.len();
        let frame = (!rows.is_empty()).then_some(Exports { rule: rid, rows });
        if limit == 1 {
            self.0 = frame.map(|f| Rc::new(Trail::One(f)));
            return (wanted.len(), kept, 0);
        }
        let prev = match self.0.as_deref() {
            Some(Trail::Many(v)) => v.as_slice(),
            _ => &[],
        };
        let prev = &prev[prev.len().saturating_sub(limit - 1)..];
        if frame.is_none() && !prev.iter().any(Option::is_some) {
            self.0 = None;
            return (wanted.len(), kept, 0);
        }
        let copied = prev.len();
        let mut v = Vec::with_capacity(prev.len() + 1);
        v.extend_from_slice(prev);
        v.push(frame.map(Rc::new));
        self.0 = Some(Rc::new(Trail::Many(v)));
        (wanted.len(), kept, copied)
    }
    pub(crate) fn get(&self, r: &Reuse) -> Option<FactRef> {
        match self.dependency(r)? {
            Dependency::Fact(f) => Some(f),
            _ => None,
        }
    }
    pub(crate) fn dependency(&self, r: &Reuse) -> Option<Dependency> {
        let e = match self.0.as_deref()? {
            Trail::One(e) if r.back == 1 => e,
            Trail::Many(v) => v.get(v.len().checked_sub(r.back)?)?.as_deref()?,
            _ => return None,
        };
        (e.rule == r.rule)
            .then(|| e.rows.iter().find(|x| x.0 == r.donor).map(|x| x.1.decode()))
            .flatten()
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Terminal {
    pub reuse: Vec<Reuse>,
    pub complete: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lhs: Vec<Reuse>,
}
pub(crate) fn terminal(p: &Program, path: &[(Option<usize>, (usize, usize))]) -> Terminal {
    let reuse = derive(p, path);
    let r = path.last().unwrap().1.0;
    let complete = is_complete(p, r, &reuse);
    Terminal {
        reuse,
        complete,
        lhs: vec![],
    }
}

type Entry = (usize, usize);
#[derive(Clone, Debug, Default)]
pub(crate) struct Switch {
    reuse_results: bool,
    nodes: Vec<Node>,
    by_rule: Vec<Vec<Option<usize>>>,
    initial: Vec<usize>,
    leaves: BTreeMap<Entry, usize>,
    pub exports: Vec<Vec<Donor>>,
    pub depth: usize,
}
#[derive(Clone, Debug, Serialize)]
struct Node {
    unit: usize,
    failure: usize,
    program: Terminal,
    next: Vec<((usize, usize), usize)>,
    supply: Vec<Donor>,
}
impl Default for Terminal {
    fn default() -> Self {
        Self {
            reuse: vec![],
            complete: false,
            lhs: vec![],
        }
    }
}
impl Switch {
    /// Retain only prefixes which can supply a terminal operation. Compile
    /// inherited suffix jumps once, so a cold entry is a direct leaf reset.
    pub(crate) fn build<'a>(
        p: &Program,
        paths: impl Iterator<Item = &'a [(Option<usize>, Entry)]>,
        leaves: &BTreeMap<Entry, usize>,
        reuse_results: bool,
    ) -> Self {
        Self::build_mode(p, paths, leaves, reuse_results, false)
    }
    pub(crate) fn build_lhs<'a>(
        p: &Program,
        paths: impl Iterator<Item = &'a [(Option<usize>, Entry)]>,
        leaves: &BTreeMap<Entry, usize>,
        reuse_results: bool,
    ) -> Self {
        Self::build_mode(p, paths, leaves, reuse_results, true)
    }
    fn build_mode<'a>(
        p: &Program,
        paths: impl Iterator<Item = &'a [(Option<usize>, Entry)]>,
        leaves: &BTreeMap<Entry, usize>,
        reuse_results: bool,
        lhs: bool,
    ) -> Self {
        let mut prefixes = BTreeMap::new();
        let mut units = BTreeMap::new();
        for (id, path) in paths.enumerate() {
            units.insert(path.to_vec(), id);
            for i in 1..=path.len() {
                prefixes.entry(path[..i].to_vec()).or_insert_with(|| {
                    let mut terminal = if reuse_results {
                        terminal(p, &path[..i])
                    } else {
                        Terminal::default()
                    };
                    if lhs {
                        terminal.lhs = derive_lhs(p, &path[..i]);
                    }
                    terminal
                });
            }
        }
        let mut paths = prefixes.into_iter().collect::<Vec<_>>();
        paths.sort_by_key(|(p, _)| p.len());
        let blank = || Node {
            unit: 0,
            failure: 0,
            program: Default::default(),
            next: vec![],
            supply: vec![],
        };
        let mut nodes = vec![blank()];
        let mut ids = BTreeMap::new();
        let mut initial = vec![0; leaves.len()];
        let mut exports = vec![vec![]; p.rules.len()];
        let mut depth = 0;
        for (path, mut program) in paths {
            let id = nodes.len();
            let &(op, entry) = path.last().unwrap();
            let leaf = leaves[&entry];
            let failure = if path.len() == 1 {
                initial[leaf] = id;
                0
            } else {
                let parent: usize = ids[&path[..path.len() - 1]];
                nodes[parent].next.push(((op.unwrap(), leaf), id));
                (1..path.len())
                    .find_map(|i| {
                        let mut suffix = path[i..].to_vec();
                        suffix[0].0 = None;
                        ids.get(&suffix).copied()
                    })
                    .unwrap_or(0)
            };
            for rule in &nodes[failure].program.reuse {
                if !program.reuse.iter().any(|r| r.op == rule.op) {
                    program.reuse.push(rule.clone());
                }
            }
            program.reuse.sort_by_key(|r| r.op);
            for rule in &nodes[failure].program.lhs {
                if !program.lhs.iter().any(|r| r.op == rule.op) {
                    program.lhs.push(rule.clone());
                }
            }
            program.lhs.sort_by_key(|r| r.op);
            program.complete = is_complete(p, entry.0, &program.reuse);
            for r in program.reuse.iter().chain(&program.lhs) {
                let donor: usize = ids[&path[..path.len() - r.back]];
                nodes[donor].supply.push(r.donor);
                exports[r.rule].push(r.donor);
                depth = depth.max(r.back);
            }
            let unit = units.get(&path).copied().unwrap_or(nodes[failure].unit);
            ids.insert(path, id);
            nodes.push(Node {
                unit,
                failure,
                program,
                ..blank()
            });
        }
        for id in 1..nodes.len() {
            let (earlier, rest) = nodes.split_at_mut(id);
            let n = &mut rest[0];
            let inherited = &earlier[n.failure];
            for &(edge, next) in &inherited.next {
                if !n.next.iter().any(|&(e, _)| e == edge) {
                    n.next.push((edge, next));
                }
            }
            n.supply.extend_from_slice(&inherited.supply);
            n.supply.sort_unstable();
            n.supply.dedup();
            n.next.sort_unstable_by_key(|x| x.0);
        }
        for s in &mut exports {
            s.sort_unstable();
            s.dedup();
        }
        let mut by_rule = vec![vec![]; p.rules.len()];
        for (&(rid, pivot), &leaf) in leaves {
            let entries = &mut by_rule[rid];
            entries.resize(entries.len().max(pivot + 1), None);
            entries[pivot] = Some(leaf);
        }
        Self {
            reuse_results,
            nodes,
            by_rule,
            initial,
            leaves: leaves.clone(),
            exports,
            depth,
        }
    }
    pub(crate) fn leaf(&self, r: usize, p: usize) -> Option<usize> {
        self.by_rule.get(r)?.get(p).copied().flatten()
    }
    pub(crate) fn active(&self, pc: usize) -> usize {
        self.nodes[pc].unit
    }
    pub(crate) fn advance(&self, state: usize, op: usize, leaf: usize) -> usize {
        let jumps = &self.nodes[state].next;
        jumps
            .iter()
            .find(|(k, _)| *k == (op, leaf))
            .map_or(self.initial[leaf], |x| x.1)
    }
    pub(crate) fn supply(&self, id: usize) -> &[Donor] {
        self.nodes.get(id).map_or(&[], |n| n.supply.as_slice())
    }
    pub(crate) fn program(&self, id: usize) -> Option<&Terminal> {
        self.nodes.get(id).map(|n| &n.program)
    }
    pub(crate) fn artifact(&self) -> serde_json::Value {
        serde_json::json!({"schema":"eggfuse-result-switch/v1","reuse_results":self.reuse_results,"initial_entries":self.initial,"states":self.nodes,"max_back":self.depth})
    }
    pub(crate) fn enabled(&self) -> bool {
        !self.nodes.is_empty()
    }
    pub(crate) fn replay(&self, path: &[(Option<usize>, Entry)]) -> usize {
        path.iter().fold(0, |s, (op, e)| {
            self.advance(s, op.unwrap_or(0), self.leaves[e])
        })
    }
}

fn is_complete(p: &Program, r: usize, reuse: &[Reuse]) -> bool {
    !reuse.is_empty()
        && p.templates[p.rules[r].template]
            .ops
            .iter()
            .enumerate()
            .all(|(i, o)| matches!(o, Op::Copy(..)) || reuse.iter().any(|x| x.op == i))
}

#[cfg(test)]
mod receipt_tests {
    use super::*;
    #[test]
    fn mixed_query_and_effect_receipts_keep_kind_and_stage_distance() {
        let mut h=History::default();
        let f=FactRef{cell:7,op:3};
        assert_eq!(h.capture(3,4,&[Donor::Query(0),Donor::Effect(1)],
            |_|Some(Dependency::Function(0)),|_|Some(Dependency::Fact(f))),(2,2,0));
        h.capture(3,5,&[],|_|None,|_|None);
        let r=Reuse{equal_output:false,op:0,back:2,rule:4,donor:Donor::Query(0)};
        assert_eq!(h.dependency(&r),Some(Dependency::Function(0)));
        assert_eq!(h.get(&r),None,"function receipt cannot become a constructor RHS proof");
        assert_eq!(h.get(&Reuse{donor:Donor::Effect(1),..r.clone()}),Some(f));
        assert_eq!(h.dependency(&Reuse{rule:9,..r}),None);
        assert!(Receipt::encode(Dependency::Function(u32::MAX as usize)).is_none());
    }
}
