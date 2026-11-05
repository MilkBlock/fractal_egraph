//! Reverse demand for witnessed structural ports. A path proposes a receipt;
//! only the target's complete key and current visibility authorize a read.
use crate::program::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(crate) const MAX_DEPTH: usize = 3;
pub(crate) const BUDGET: usize = 8;
const MAX_NODES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) enum Source {
    Query(usize),
    Effect(usize),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct Path {
    pub len: u8,
    pub edges: [u32; MAX_DEPTH],
}
impl Path {
    pub fn then(self, other: Self) -> Option<Self> {
        let n = self.len as usize;
        let m = other.len as usize;
        if n + m > MAX_DEPTH {
            return None;
        }
        let mut p = self;
        p.edges[n..n + m].copy_from_slice(&other.edges[..m]);
        p.len = (n + m) as u8;
        Some(p)
    }
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Export {
    pub src: Source,
    pub table: usize,
    pub path: Path,
    pub via: Vec<Source>,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Output {
    pub op: usize,
    pub table: usize,
    pub donors: Vec<Export>,
}
#[derive(Default, Debug, Serialize)]
pub(crate) struct Recipe {
    pub outputs: Vec<Output>,
    /// Keyed atoms in original order of availability. A strict prefix can still supply ports.
    pub orders: Vec<Vec<usize>>,
    pub ctor_ops: usize,
    pub closed_shape: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct Edge(pub usize, pub usize, pub usize, pub usize);
#[derive(Default, Debug, Serialize)]
pub(crate) struct Plan {
    pub recipes: Vec<Recipe>,
    pub edges: Vec<Edge>,
    pub capped_recipes: usize,
}
fn root(ps: &[usize], mut n: usize) -> usize {
    while ps[n] != n {
        n = ps[n]
    }
    n
}
fn unify(ps: &mut [usize], a: usize, b: usize) -> bool {
    let (a, b) = (root(ps, a), root(ps, b));
    ps[a.max(b)] = a.min(b);
    a != b
}
struct Node {
    table: usize,
    src: Source,
    fields: Vec<Arg>,
}
fn keyed(rule: &Rule, pivot: usize) -> Vec<usize> {
    let mut known = BTreeSet::new();
    let add = |set: &mut BTreeSet<usize>, args: &[Arg]| {
        for a in args {
            if let Arg::Slot(i) = a {
                set.insert(*i);
            }
        }
    };
    add(&mut known, &rule.query[pivot].args);
    let mut pending: BTreeSet<_> = (0..rule.query.len()).filter(|&i| i != pivot).collect();
    let mut order = vec![];
    while let Some(i) = pending
        .iter()
        .copied()
        .filter(|i| {
            let a = &rule.query[*i];
            a.args[..a.args.len() - 1].iter().all(|a| match a {
                Arg::Slot(s) => known.contains(s),
                Arg::Literal(_) => true,
            })
        })
        .min_by_key(|i| (matches!(rule.query[*i].call, Call::Table(_)), *i))
    {
        order.push(i);
        pending.remove(&i);
        add(&mut known, &rule.query[i].args);
    }
    order
}
impl Plan {
    pub fn derive(p: &Program) -> Self {
        let mut plan = Self::default();
        let mut ids = BTreeMap::new();
        let mut needs = vec![BTreeSet::new(); p.tables.len()];
        for r in &p.rules {
            let reads: Vec<_> = r
                .query
                .iter()
                .filter_map(|a| {
                    if let Call::Table(t) = a.call {
                        Some(t)
                    } else {
                        None
                    }
                })
                .collect();
            for &t in &reads {
                needs[t].extend(&reads);
                for op in p.templates[r.template].ops.iter() {
                    if let Op::Let(_, Call::Table(o), _) | Op::Set(o, _, _) = op
                        && p.tables[*o].constructor
                    {
                        needs[t].insert(*o);
                    }
                }
            }
        }
        for r in &p.rules {
            let tpl = &p.templates[r.template];
            let mut recipe = Recipe::default();
            recipe.orders = (0..r.query.len())
                .map(|i| {
                    if matches!(r.query[i].call, Call::Table(_)) {
                        keyed(r, i)
                    } else {
                        vec![]
                    }
                })
                .collect();
            let is_ctor = |t: usize| p.tables[t].constructor && p.tables[t].virtual_fact;
            recipe.ctor_ops = tpl
                .ops
                .iter()
                .filter(|op| matches!(op,Op::Let(_,Call::Table(t),_)|Op::Set(t,_,_) if is_ctor(*t)))
                .count();
            recipe.closed_shape = recipe.ctor_ops > 0
                && tpl.ops.iter().all(|op| match op {
                    Op::Copy(..) | Op::Union(..) => true,
                    Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => is_ctor(*t),
                    _ => false,
                });
            let mut nodes: Vec<_> = r
                .query
                .iter()
                .enumerate()
                .filter_map(|(i, a)| {
                    if let Call::Table(t) = a.call {
                        Some(Node {
                            table: t,
                            src: Source::Query(i),
                            fields: a.args.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect();
            if tpl.ops.len() > MAX_NODES * 3 || nodes.len() + recipe.ctor_ops > MAX_NODES {
                plan.capped_recipes += 1;
                plan.recipes.push(recipe);
                continue;
            }
            let mut parents: Vec<_> = (0..tpl.sorts.len()).collect();
            for (i, op) in tpl.ops.iter().enumerate() {
                let (t, args, result) = match op {
                    Op::Copy(dst, Arg::Slot(src)) => {
                        unify(&mut parents, *dst, *src);
                        continue;
                    }
                    Op::Union(Arg::Slot(a), Arg::Slot(b)) => {
                        unify(&mut parents, *a, *b);
                        continue;
                    }
                    Op::Let(dst, Call::Table(t), aa) => (*t, aa, Arg::Slot(*dst)),
                    Op::Set(t, aa, v) => (*t, aa, v.clone()),
                    _ => continue,
                };
                let mut fields = args.clone();
                fields.push(result);
                nodes.push(Node {
                    table: t,
                    src: Source::Effect(i),
                    fields,
                });
            }
            if nodes.len() > MAX_NODES {
                plan.capped_recipes += 1;
                plan.recipes.push(recipe);
                continue;
            }
            // Congruence on the finite producer fragment. Mutable table values never create equalities.
            loop {
                let mut keys = BTreeMap::new();
                let mut changed = false;
                for n in nodes.iter().filter(|n| is_ctor(n.table)) {
                    let (out, key) = n.fields.split_last().unwrap();
                    let Arg::Slot(dst) = out else { continue };
                    let key: Vec<_> = key
                        .iter()
                        .map(|a| match a {
                            Arg::Slot(i) => Arg::Slot(root(&parents, *i)),
                            Arg::Literal(v) => Arg::Literal(v.clone()),
                        })
                        .collect();
                    if let Some(prev) = keys.insert((n.table, key), *dst) {
                        changed |= unify(&mut parents, prev, *dst);
                    }
                }
                if !changed {
                    break;
                }
            }
            let mut adj = vec![vec![]; nodes.len()];
            for (i, a) in nodes.iter().enumerate() {
                for (j, b) in nodes.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let edge=a.fields.iter().enumerate().find_map(|(ac,x)|{
                    let Arg::Slot(x)=x else{return None};if tpl.sorts[*x]["kind"]!="eq"{return None}
                    b.fields.iter().position(|y|matches!(y,Arg::Slot(y) if root(&parents,*x)==root(&parents,*y))).map(|bc|Edge(a.table,ac,b.table,bc))
                });
                    if let Some(e) = edge {
                        let next = ids.len() as u32;
                        let id = *ids.entry(e.clone()).or_insert_with(|| {
                            plan.edges.push(e);
                            next
                        });
                        adj[i].push((j, id));
                    }
                }
            }
            for (i, n) in nodes.iter().enumerate() {
                let Source::Effect(op) = n.src else { continue };
                if needs[n.table].is_empty() {
                    continue;
                }
                let mut seen = BTreeSet::from([i]);
                let mut queue = VecDeque::from([(i, Path::default(), Vec::<Source>::new())]);
                let mut donors = vec![];
                while let Some((from, path, via)) = queue.pop_front() {
                    if path.len as usize == MAX_DEPTH {
                        continue;
                    }
                    for &(j, edge) in &adj[from] {
                        if seen.insert(j) {
                            let mut path = path;
                            path.edges[path.len as usize] = edge;
                            path.len += 1;
                            donors.push(Export {
                                src: nodes[j].src,
                                table: nodes[j].table,
                                path,
                                via: via.clone(),
                            });
                            let mut through = via.clone();
                            through.push(nodes[j].src);
                            queue.push_back((j, path, through));
                        }
                    }
                }
                donors.sort_by_key(|d| (d.path.len, !needs[n.table].contains(&d.table), d.src));
                if !donors.is_empty() {
                    recipe.outputs.push(Output {
                        op,
                        table: n.table,
                        donors,
                    });
                }
            }
            plan.recipes.push(recipe);
        }
        plan
    }
}
