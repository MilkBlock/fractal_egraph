//! Persistent finite packet sequences. Hashes accelerate indexing; Eq authorizes sharing.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Effect {
    Ensure {
        op: String,
        args: Vec<usize>,
        result: usize,
    },
    Equate(usize, usize),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Template {
    pub rule: String,
    pub effects: Vec<Effect>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Packet {
    pub template: Template,
    pub binding: Vec<usize>,
}
impl Packet {
    pub fn normalize(rule: String, effects: Vec<Effect>) -> Self {
        let mut ids = HashMap::new();
        let mut binding = vec![];
        let mut var = |v: usize| {
            *ids.entry(v).or_insert_with(|| {
                let i = binding.len();
                binding.push(v);
                i
            })
        };
        let effects = effects
            .into_iter()
            .map(|e| match e {
                Effect::Ensure { op, args, result } => Effect::Ensure {
                    op,
                    args: args.into_iter().map(&mut var).collect(),
                    result: var(result),
                },
                Effect::Equate(a, b) => Effect::Equate(var(a), var(b)),
            })
            .collect();
        Self {
            template: Template { rule, effects },
            binding,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    Apply {
        template: usize,
        binding: Vec<usize>,
    },
    Concat {
        left: usize,
        right: usize,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub kind: NodeKind,
    pub len: usize,
    pub hash: u64,
    pub power: u64,
}
#[derive(Serialize)]
pub struct Library {
    pub version: usize,
    pub templates: Vec<Template>,
    pub nodes: Vec<Node>,
    pub evidence: Vec<serde_json::Value>,
    pub semantic_compositions: Vec<serde_json::Value>,
    #[serde(skip)]
    templates_by_key: HashMap<Template, usize>,
    #[serde(skip)]
    nodes_by_key: HashMap<NodeKind, usize>,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            version: 2,
            templates: vec![],
            nodes: vec![],
            evidence: vec![],
            semantic_compositions: vec![],
            templates_by_key: HashMap::new(),
            nodes_by_key: HashMap::new(),
        }
    }
}
impl Library {
    pub fn packet(&mut self, p: Packet) -> usize {
        let next = self.templates.len();
        let t = *self
            .templates_by_key
            .entry(p.template.clone())
            .or_insert_with(|| {
                self.templates.push(p.template);
                next
            });
        self.intern(NodeKind::Apply {
            template: t,
            binding: p.binding,
        })
    }
    fn intern(&mut self, kind: NodeKind) -> usize {
        if let Some(&id) = self.nodes_by_key.get(&kind) {
            return id;
        }
        let (len, hash, power) = match &kind {
            NodeKind::Apply { .. } => {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                kind.hash(&mut h);
                (1, h.finish(), 0x9e3779b185ebca87u64)
            }
            NodeKind::Concat { left, right } => {
                let a = &self.nodes[*left];
                let b = &self.nodes[*right];
                (
                    a.len + b.len,
                    a.hash.wrapping_mul(b.power).wrapping_add(b.hash),
                    a.power.wrapping_mul(b.power),
                )
            }
        };
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind: kind.clone(),
            len,
            hash,
            power,
        });
        self.nodes_by_key.insert(kind, id);
        id
    }
    pub fn concat(&mut self, left: usize, right: usize) -> usize {
        self.intern(NodeKind::Concat { left, right })
    }
    /// Equal-level binary carries: fewer than n joins before the final forest fold.
    pub fn build(&mut self, packets: impl IntoIterator<Item = Packet>) -> Option<usize> {
        let mut stack = vec![];
        for p in packets {
            let mut root = self.packet(p);
            while stack
                .last()
                .is_some_and(|i: &usize| self.nodes[*i].len == self.nodes[root].len)
            {
                root = self.concat(stack.pop().unwrap(), root);
            }
            stack.push(root);
        }
        let mut root = stack.pop()?;
        while let Some(left) = stack.pop() {
            root = self.concat(left, root);
        }
        Some(root)
    }
    pub fn slice(&mut self, root: usize, start: usize, end: usize) -> Option<usize> {
        assert!(start <= end && end <= self.nodes[root].len);
        if start == end {
            return None;
        }
        if start == 0 && end == self.nodes[root].len {
            return Some(root);
        }
        let NodeKind::Concat { left, right } = self.nodes[root].kind else {
            unreachable!()
        };
        let n = self.nodes[left].len;
        if end <= n {
            return self.slice(left, start, end);
        }
        if start >= n {
            return self.slice(right, start - n, end - n);
        }
        let a = self.slice(left, start, n).unwrap();
        let b = self.slice(right, 0, end - n).unwrap();
        Some(self.concat(a, b))
    }
    /// Derive sound root-position candidates along existing concat nodes, without
    /// claiming that a recorded application actually used that root position.
    pub fn learn_semantics(&mut self, root: usize, source: &str) -> Vec<usize> {
        let Ok(book) = crate::semantic_compose::import(source) else {
            return vec![];
        };
        let mut pending = vec![(root, false)];
        let mut summaries: HashMap<usize, Option<crate::semantic_compose::Rule>> = HashMap::new();
        let mut learned = vec![];
        while let Some((id, visited)) = pending.pop() {
            if summaries.contains_key(&id) {
                continue;
            }
            match &self.nodes[id].kind {
                NodeKind::Apply { template, .. } => {
                    summaries.insert(id, book.get(&self.templates[*template].rule).cloned());
                }
                NodeKind::Concat { left, right } => {
                    if !visited {
                        pending.push((id, true));
                        pending.push((*right, false));
                        pending.push((*left, false));
                        continue;
                    }
                    let result = match (&summaries[left], &summaries[right]) {
                        (Some(a), Some(b)) if self.nodes[id].len <= 16 => {
                            crate::semantic_compose::compose(a, b, &[]).ok()
                        }
                        _ => None,
                    };
                    if let Some(c) = result {
                        let index = self.semantic_compositions.len();
                        self.semantic_compositions.push(serde_json::json!({"node":id,"status":"symbolically_valid_root_candidate","observed_position_verified":false,"composition":c,"egg":c.rule.egg()}));
                        learned.push(index);
                        summaries.insert(id, Some(c.rule));
                    } else {
                        summaries.insert(id, None);
                    }
                }
            }
        }
        learned
    }
    /// Debug expansion only; production slicing and composition keep shared nodes.
    pub fn expand(&self, root: usize) -> Vec<Packet> {
        let mut stack = vec![root];
        let mut packets = vec![];
        while let Some(id) = stack.pop() {
            match &self.nodes[id].kind {
                NodeKind::Apply { template, binding } => packets.push(Packet {
                    template: self.templates[*template].clone(),
                    binding: binding.clone(),
                }),
                NodeKind::Concat { left, right } => {
                    stack.push(*right);
                    stack.push(*left);
                }
            }
        }
        packets
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"templates":self.templates.len(),"nodes":self.nodes.len(),"evidence_objects":self.evidence.len(),"semantic_compositions":self.semantic_compositions.len()})
    }
}
