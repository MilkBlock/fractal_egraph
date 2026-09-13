//! Injective, non-induced embedding of finite labelled DAGs. Roots need not align.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub label: Value,
    #[serde(default)]
    pub constraints: Value,
    #[serde(default)]
    pub ports: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Interface {
    pub name: String,
    pub node: usize,
    pub port: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dag {
    pub root: usize,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub interface: Vec<Interface>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Found {
        node_map: Vec<usize>,
        root_image: usize,
        states: usize,
    },
    Absent {
        states: usize,
    },
    UnknownBudget {
        states: usize,
    },
}
impl Dag {
    pub fn validate(&self) -> Result<(), String> {
        if self.nodes.is_empty() || self.root >= self.nodes.len() {
            return Err("invalid DAG root".into());
        }
        let mut degree = vec![0; self.nodes.len()];
        let mut children = vec![vec![]; self.nodes.len()];
        for e in &self.edges {
            if e.from >= self.nodes.len() || e.to >= self.nodes.len() {
                return Err("invalid edge endpoint".into());
            }
            degree[e.to] += 1;
            children[e.from].push(e.to);
        }
        let mut ready: Vec<_> = degree
            .iter()
            .enumerate()
            .filter(|(_, d)| **d == 0)
            .map(|(i, _)| i)
            .collect();
        let mut seen = 0;
        while let Some(i) = ready.pop() {
            seen += 1;
            for j in &children[i] {
                degree[*j] -= 1;
                if degree[*j] == 0 {
                    ready.push(*j)
                }
            }
        }
        if seen != self.nodes.len() {
            return Err("input contains a cycle; unfold recursive grammar explicitly".into());
        }
        for p in &self.interface {
            if p.node >= self.nodes.len() || !self.nodes[p.node].ports.contains(&p.port) {
                return Err("invalid interface port".into());
            }
        }
        Ok(())
    }
}
fn entails(big: &Value, small: &Value) -> bool {
    if small.is_null() {
        return true;
    }
    match (big, small) {
        (Value::Object(b), Value::Object(s)) => s
            .iter()
            .all(|(k, v)| b.get(k).is_some_and(|x| entails(x, v))),
        _ => big == small,
    }
}
fn compatible(a: &Node, b: &Node) -> bool {
    a.label == b.label
        && entails(&b.constraints, &a.constraints)
        && a.ports.iter().all(|p| b.ports.contains(p))
}
fn edge_counts(g: &Dag) -> BTreeMap<(usize, usize, String), usize> {
    let mut counts = BTreeMap::new();
    for e in &g.edges {
        *counts
            .entry((e.from, e.to, e.label.to_string()))
            .or_default() += 1;
    }
    counts
}
fn consistent(
    small: &Dag,
    big: &BTreeMap<(usize, usize, String), usize>,
    map: &[Option<usize>],
) -> bool {
    let mut needs = BTreeMap::new();
    for e in &small.edges {
        if let (Some(a), Some(b)) = (map[e.from], map[e.to]) {
            *needs.entry((a, b, e.label.to_string())).or_insert(0usize) += 1;
        }
    }
    needs
        .iter()
        .all(|(key, n)| big.get(key).is_some_and(|available| available >= n))
}
/// Exhaustive under these labels/constraints unless the explicit state budget is hit.
/// Finds one certificate, not all possible embeddings. Extra big edges are allowed.
pub fn embed(small: &Dag, big: &Dag, budget: usize) -> Result<Outcome, String> {
    small.validate()?;
    big.validate()?;
    if small.nodes.len() > big.nodes.len() || small.edges.len() > big.edges.len() {
        return Ok(Outcome::Absent { states: 0 });
    }
    let candidates: Vec<Vec<usize>> = small
        .nodes
        .iter()
        .map(|n| {
            big.nodes
                .iter()
                .enumerate()
                .filter(|(_, b)| compatible(n, b))
                .map(|(i, _)| i)
                .collect()
        })
        .collect();
    if candidates.iter().any(Vec::is_empty) {
        return Ok(Outcome::Absent { states: 0 });
    }
    let mut order: Vec<_> = (0..small.nodes.len()).collect();
    order.sort_by_key(|i| (*i != small.root, candidates[*i].len(), *i));
    struct Search<'a> {
        s: &'a Dag,
        edges: BTreeMap<(usize, usize, String), usize>,
        candidates: Vec<Vec<usize>>,
        order: Vec<usize>,
        states: usize,
        budget: usize,
        exhausted: bool,
    }
    impl Search<'_> {
        fn go(
            &mut self,
            depth: usize,
            map: &mut [Option<usize>],
            used: &mut BTreeSet<usize>,
        ) -> bool {
            if depth == self.order.len() {
                return true;
            }
            let i = self.order[depth];
            for n in self.candidates[i].clone() {
                if used.contains(&n) {
                    continue;
                }
                if self.states >= self.budget {
                    self.exhausted = true;
                    return false;
                }
                self.states += 1;
                map[i] = Some(n);
                used.insert(n);
                if consistent(self.s, &self.edges, map) && self.go(depth + 1, map, used) {
                    return true;
                }
                used.remove(&n);
                map[i] = None;
                if self.exhausted {
                    return false;
                }
            }
            false
        }
    }
    let mut search = Search {
        s: small,
        edges: edge_counts(big),
        candidates,
        order,
        states: 0,
        budget,
        exhausted: false,
    };
    let mut map = vec![None; small.nodes.len()];
    if search.go(0, &mut map, &mut BTreeSet::new()) {
        let node_map: Vec<_> = map.into_iter().map(Option::unwrap).collect();
        Ok(Outcome::Found {
            root_image: node_map[small.root],
            node_map,
            states: search.states,
        })
    } else if search.exhausted {
        Ok(Outcome::UnknownBudget {
            states: search.states,
        })
    } else {
        Ok(Outcome::Absent {
            states: search.states,
        })
    }
}
