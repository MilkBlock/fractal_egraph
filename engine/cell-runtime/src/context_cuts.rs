//! Choose a connected prefix and leave one terminal table for scheduled deltas.
use super::*;
use std::cmp::Reverse;

pub(crate) type Cuts = Vec<Vec<Rc<Plan>>>;
/// The artifact needs only atom order. Slot wiring is derived from the checked IR.
pub(crate) fn layout(cuts: &Cuts) -> Vec<Vec<(usize, &[usize], usize, &[usize])>> {
    cuts.iter()
        .map(|rule| {
            rule.iter()
                .map(|p| (p.root, p.ports.as_slice(), p.tail, p.post.as_slice()))
                .collect()
        })
        .collect()
}
const MAX_TABLES: usize = 8;

struct Shape {
    vars: Vec<BTreeSet<usize>>,
    keys: Vec<BTreeSet<usize>>,
    tables: Vec<usize>,
    guards: Vec<usize>,
}

impl Shape {
    fn new(r: &Rule) -> Self {
        let (tables, guards) =
            (0..r.query.len()).partition(|&i| matches!(r.query[i].call, Call::Table(_)));
        Self {
            vars: r.query.iter().map(|a| slots(&a.args)).collect(),
            keys: r
                .query
                .iter()
                .map(|a| slots(&a.args[..a.args.len() - 1]))
                .collect(),
            tables,
            guards,
        }
    }
    fn ready_guards(
        &self,
        known: &mut BTreeSet<usize>,
        pending: &mut Vec<usize>,
        out: &mut Vec<usize>,
    ) {
        while let Some(k) = pending.iter().position(|&i| self.keys[i].is_subset(known)) {
            let i = pending.remove(k);
            known.extend(&self.vars[i]);
            out.push(i);
        }
    }

    // All partial reads retain their exact row checks. Missing key slots
    // estimate intermediate fan-out; they never justify excluding an answer.
    fn cut(
        &self,
        root: usize,
        tail: usize,
    ) -> Option<(
        (usize, usize, Reverse<usize>, usize),
        Vec<usize>,
        Vec<usize>,
    )> {
        let mut todo = self
            .tables
            .iter()
            .copied()
            .filter(|&i| i != root && i != tail)
            .collect::<Vec<_>>();
        let mut known = self.vars[root].clone();
        let mut guards = self.guards.clone();
        let mut ports = vec![];
        let mut width = 0;
        let mut expansions = 0;
        loop {
            self.ready_guards(&mut known, &mut guards, &mut ports);
            if todo.is_empty() {
                break;
            }
            let (missing, _, i, k) = todo
                .iter()
                .enumerate()
                .filter_map(|(k, &i)| {
                    let shared = self.vars[i].intersection(&known).count();
                    (shared > 0).then(|| {
                        (
                            self.keys[i].difference(&known).count(),
                            Reverse(shared),
                            i,
                            k,
                        )
                    })
                })
                .min()?;
            width += missing;
            expansions += usize::from(missing > 0);
            known.extend(&self.vars[i]);
            ports.push(todo.remove(k));
        }
        let boundary = self.vars[tail].intersection(&known).count();
        if boundary == 0 {
            return None;
        }
        known.extend(&self.vars[tail]);
        let mut post = vec![];
        self.ready_guards(&mut known, &mut guards, &mut post);
        guards
            .is_empty()
            .then_some(((width, expansions, Reverse(boundary), tail), ports, post))
    }
}
pub(crate) fn derive_cuts(p: &Program) -> Cuts {
    p.rules
        .iter()
        .map(|r| {
            if r.query.iter().any(|a| a.args.is_empty()) {
                return vec![];
            }
            let s = Shape::new(r);
            if !r.seminaive || !(2..=MAX_TABLES).contains(&s.tables.len()) {
                return vec![];
            }
            s.tables
                .iter()
                .filter_map(|&root| {
                    let ((_, _, _, tail), ports, post) = s
                        .tables
                        .iter()
                        .filter(|&&tail| tail != root)
                        .filter_map(|&tail| s.cut(root, tail))
                        .min_by_key(|(score, _, _)| *score)?;
                    project(r, root, ports, tail, post).map(Rc::new)
                })
                .collect()
        })
        .collect()
}
