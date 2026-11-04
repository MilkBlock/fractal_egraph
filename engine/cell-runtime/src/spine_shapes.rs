//! Diagnostic-only lineage for the winning origin of an applied consumer.
//! A shared root may fork. Each applied occurrence tracks its own children.
use crate::{graph::FactRef, known_results::Source, program::*};
use serde::Serialize;
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

type Edge = [usize; 4];

#[derive(Clone, Copy, Default)]
pub(super) struct Trace {
    prefix: usize,
    occurrence: Option<usize>,
    pub source: Option<FactRef>,
    pub pivot: usize,
    pub same_fact: bool,
}
#[derive(Serialize)]
struct Node {
    parent: usize,
    edge: Edge,
    visits: u64,
    leaves: u64,
    same_fact: u64,
}
#[derive(Default)]
pub(super) struct Profile {
    prefixes: HashMap<(usize, Edge), usize>,
    nodes: Vec<Node>,
    has_child: Vec<bool>,
    static_counts: J,
}
impl Profile {
    pub(super) fn catalog(&mut self, p: &Program, anchors: &[Vec<(usize, Option<Source>)>]) {
        let mut consumers = BTreeMap::<usize, Vec<(usize, usize)>>::new();
        for (r, pivot, _) in &p.multi_stage.stage_entries {
            let Call::Table(t) = p.rules[*r].query[*pivot].call else {
                unreachable!()
            };
            consumers.entry(t).or_default().push((*r, *pivot));
        }
        let mut pairs = BTreeSet::new();
        let (mut edges, mut self_edges, mut certified, mut outputs) =
            (0usize, 0usize, 0usize, 0usize);
        for (producer, ops) in anchors.iter().enumerate() {
            let template = p.rules[producer].template;
            for &(op, _) in ops {
                let table = match &p.templates[template].ops[op] {
                    Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => *t,
                    _ => unreachable!(),
                };
                outputs += 1;
                for &(consumer, pivot) in &consumers[&table] {
                    edges += 1;
                    self_edges += usize::from(producer == consumer);
                    pairs.insert((producer, consumer));
                    certified += usize::from(
                        p.multi_stage
                            .routes
                            .contains_key(&(template, op, consumer, pivot)),
                    );
                }
            }
        }
        self.static_counts = json!({"producer_operations":outputs,"consumer_entries":p.multi_stage.stage_entries.len(),
            "type_compatible_edges":edges,"rule_pairs":pairs.len(),"self_edges":self_edges,"multi_stage_certified_edges":certified});
    }
    pub(super) fn resume(&mut self, trace: &mut Trace, producer: usize, consumer: usize) {
        let edge = [
            producer,
            trace.source.expect("published cursor").op as usize,
            consumer,
            trace.pivot,
        ];
        if let Some(previous) = trace.occurrence
            && !self.has_child[previous]
        {
            self.has_child[previous] = true;
            self.nodes[trace.prefix - 1].leaves -= 1;
        }
        let prefix = *self
            .prefixes
            .entry((trace.prefix, edge))
            .or_insert_with(|| {
                self.nodes.push(Node {
                    parent: trace.prefix,
                    edge,
                    visits: 0,
                    leaves: 0,
                    same_fact: 0,
                });
                self.nodes.len()
            });
        let node = &mut self.nodes[prefix - 1];
        node.visits += 1;
        node.leaves += 1;
        node.same_fact += u64::from(trace.same_fact);
        trace.occurrence = Some(self.has_child.len());
        self.has_child.push(false);
        trace.prefix = prefix;
    }
    pub(super) fn report(&self) -> J {
        json!({"schema":"eggfuse-spine-shapes/v1","static":self.static_counts,"prefixes":self.nodes})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn branching_marks_each_parent_occurrence_only_once() {
        let mut p = Profile::default();
        let root = Trace {
            source: Some(FactRef { cell: 9, op: 2 }),
            pivot: 1,
            ..Default::default()
        };
        let (mut a, mut b) = (root, root);
        p.resume(&mut a, 3, 5);
        p.resume(&mut b, 3, 5);
        let mut other = a;
        p.resume(&mut a, 5, 7);
        p.resume(&mut other, 5, 8);
        assert_eq!((p.nodes[0].visits, p.nodes[0].leaves), (2, 1));
        assert_eq!(p.nodes.iter().map(|n| n.leaves).sum::<u64>(), 3);
    }
}
