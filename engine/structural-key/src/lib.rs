//! Exact structural witness key semantics shared by native and standalone engines.
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    hash::{Hash, Hasher},
    sync::Arc,
};
pub const SORT: &str = "CellStructuralKey";
#[derive(Clone)]
pub struct Key(pub Arc<Node>);
// Native graph export prints base values through Debug, not reconstruct_termdag.
// Recursively printing an Arc DAG unfolds repeated children exponentially.
// This is a diagnostic label only; exact equality/order and KeyGraph snapshots
// still inspect the complete shared structure.
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CellStructuralKey(anchor={}, height={}, hash={:016x})",
            self.0.anchor, self.0.height, self.0.hash
        )
    }
}
#[derive(Debug)]
pub struct Node {
    pub anchor: bool,
    pub height: usize,
    pub label: Arc<str>,
    pub children: Vec<Key>,
    pub hash: u64,
}
impl Key {
    pub fn new(label: String, children: Vec<Key>) -> Self {
        Self::with_label(label.into(), children)
    }
    pub fn with_label(label: Arc<str>, children: Vec<Key>) -> Self {
        let height = children.iter().map(|k| k.0.height + 1).max().unwrap_or(0);
        let mut hash = 0xcbf29ce484222325u64;
        for byte in label
            .as_bytes()
            .iter()
            .copied()
            .chain(children.iter().flat_map(|k| k.0.hash.to_le_bytes()))
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        Self(Arc::new(Node {
            anchor: false,
            height,
            label,
            children,
            hash,
        }))
    }
    pub fn anchor(self) -> Self {
        if self.0.anchor {
            return self;
        }
        let mut key = Self::new("\0input-anchor".into(), vec![self]);
        let node = Arc::get_mut(&mut key.0).unwrap();
        node.anchor = true;
        node.height = 0;
        key
    }
}
impl Hash for Key {
    fn hash<H: Hasher>(&self, s: &mut H) {
        self.0.hash.hash(s)
    }
}
impl PartialEq for Key {
    fn eq(&self, b: &Self) -> bool {
        Arc::ptr_eq(&self.0, &b.0) || (self.0.hash == b.0.hash && self.cmp(b) == Ordering::Equal)
    }
}
impl Eq for Key {}
impl Ord for Key {
    fn cmp(&self, b: &Self) -> Ordering {
        if Arc::ptr_eq(&self.0, &b.0) {
            return Ordering::Equal;
        }
        let direct = |a: &Key, b: &Key| {
            b.0.anchor
                .cmp(&a.0.anchor)
                .then_with(|| a.0.height.cmp(&b.0.height))
                .then_with(|| a.0.label.cmp(&b.0.label))
        };
        let first = direct(self, b);
        if first != Ordering::Equal {
            return first;
        }
        let mut equal = std::collections::HashSet::new();
        let mut work = vec![(self, b, false)];
        while let Some((a, b, done)) = work.pop() {
            if Arc::ptr_eq(&a.0, &b.0) {
                continue;
            }
            let pair = (Arc::as_ptr(&a.0) as usize, Arc::as_ptr(&b.0) as usize);
            if equal.contains(&pair) {
                continue;
            }
            if done {
                let c = a.0.children.len().cmp(&b.0.children.len());
                if c != Ordering::Equal {
                    return c;
                }
                equal.insert(pair);
                continue;
            }
            let c = direct(a, b);
            if c != Ordering::Equal {
                return c;
            }
            work.push((a, b, true));
            for (x, y) in a.0.children.iter().zip(&b.0.children).rev() {
                work.push((x, y, false));
            }
        }
        Ordering::Equal
    }
}
impl PartialOrd for Key {
    fn partial_cmp(&self, b: &Self) -> Option<Ordering> {
        Some(self.cmp(b))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireNode {
    pub anchor: bool,
    pub label: String,
    pub children: Vec<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireKey {
    pub nodes: Vec<WireNode>,
    pub root: usize,
}
impl Key {
    pub fn to_wire(&self) -> WireKey {
        let mut ids = std::collections::HashMap::new();
        let mut nodes = vec![];
        let mut shared = std::collections::HashMap::new();
        let mut work = vec![(self.clone(), false)];
        while let Some((k, ready)) = work.pop() {
            let ptr = Arc::as_ptr(&k.0) as usize;
            if ids.contains_key(&ptr) {
                continue;
            }
            if !ready {
                work.push((k.clone(), true));
                for c in k.0.children.iter().rev() {
                    work.push((c.clone(), false));
                }
            } else {
                let children: Vec<usize> =
                    k.0.children
                        .iter()
                        .map(|c| ids[&(Arc::as_ptr(&c.0) as usize)])
                        .collect();
                let signature = (k.0.anchor, k.0.label.to_string(), children);
                let at = *shared.entry(signature.clone()).or_insert_with(|| {
                    let i = nodes.len();
                    nodes.push(WireNode {
                        anchor: signature.0,
                        label: signature.1.clone(),
                        children: signature.2.clone(),
                    });
                    i
                });
                ids.insert(ptr, at);
            }
        }
        WireKey {
            root: ids[&(Arc::as_ptr(&self.0) as usize)],
            nodes,
        }
    }
    pub fn from_wire(w: WireKey) -> Result<Self, String> {
        let mut keys: Vec<Key> = vec![];
        for n in w.nodes {
            if n.children.iter().any(|i| *i >= keys.len()) {
                return Err("invalid key DAG order".into());
            }
            let mut k = Key::new(
                n.label,
                n.children.into_iter().map(|i| keys[i].clone()).collect(),
            );
            if n.anchor {
                if k.0.label.as_ref() != "\0input-anchor" || k.0.children.len() != 1 {
                    return Err("invalid key anchor".into());
                }
                let x = Arc::get_mut(&mut k.0).unwrap();
                x.anchor = true;
                x.height = 0;
            }
            keys.push(k);
        }
        keys.get(w.root).cloned().ok_or("invalid key root".into())
    }
    pub fn base_i64(x: i64) -> Self {
        Self::new(format!("i64:{x}"), vec![])
    }
    pub fn base_string(x: &str) -> Self {
        Self::new(format!("String:{x:?}"), vec![])
    }
    pub fn base_f64(bits: u64) -> Self {
        let x = f64::from_bits(bits);
        let b = if x.is_nan() {
            f64::NAN.to_bits()
        } else if x == 0.0 {
            0
        } else {
            bits
        };
        Self::new(format!("f64:{b:016x}"), vec![])
    }
}
impl Serialize for Key {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_wire().serialize(s)
    }
}
impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::from_wire(WireKey::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl Key {
    pub fn decode_legacy(text: &str) -> Option<Self> {
        fn go(v: &serde_json::Value) -> Option<Key> {
            let a = v.as_array()?;
            if a.len() != 3 {
                return None;
            }
            let anchor = a[0].as_bool()?;
            let label = a[1].as_str()?;
            let children = a[2]
                .as_array()?
                .iter()
                .map(go)
                .collect::<Option<Vec<_>>>()?;
            let mut k = Key::new(label.into(), children);
            if anchor {
                if label != "\0input-anchor" || k.0.children.len() != 1 {
                    return None;
                }
                let n = Arc::get_mut(&mut k.0)?;
                n.anchor = true;
                n.height = 0;
            }
            Some(k)
        }
        go(&serde_json::from_str::<serde_json::Value>(text).ok()?)
    }
}

pub const ABI: &str = include_str!("../abi.json");
pub fn signature(call: &serde_json::Value, spec: &serde_json::Value) -> bool {
    let Some(name) = call["primitive"].as_str() else {
        return false;
    };
    let Some(inputs) = call["inputs"].as_array() else {
        return false;
    };
    let abi: serde_json::Value = serde_json::from_str(ABI).unwrap();
    if let Some(s) = abi["primitives"].get(name) {
        let expected = s[0].as_array().unwrap();
        if inputs.len() != expected.len() {
            return false;
        }
        let ty = if s[1] == "$T" {
            call["output"].clone()
        } else {
            s[1].clone()
        };
        return call["output"] == ty
            && inputs
                .iter()
                .zip(expected)
                .all(|(x, y)| *x == if *y == "$T" { ty.clone() } else { y.clone() });
    }
    spec["nodes"].as_array().is_some_and(|ns| {
        ns.iter().any(|n| {
            n["primitive"] == name
                && n["arity"].as_u64() == Some(inputs.len() as u64)
                && inputs.iter().all(|s| *s == SORT)
                && call["output"] == SORT
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_dag_wire_is_linear() {
        let mut k = Key::new("leaf".into(), vec![]);
        for _ in 0..60 {
            k = Key::new("pair".into(), vec![k.clone(), k]);
        }
        let w = k.to_wire();
        assert_eq!(w.nodes.len(), 61);
        assert_eq!(Key::from_wire(w).unwrap(), k);
    }
    #[test]
    fn order_matches_literal_labels_not_ids() {
        assert!(Key::base_i64(10) < Key::base_i64(2));
        assert_eq!(
            Key::base_f64((-0.0f64).to_bits()),
            Key::base_f64(0.0f64.to_bits())
        );
        assert_eq!(
            Key::base_f64(0x7ff0000000000001),
            Key::base_f64(f64::NAN.to_bits())
        );
    }
    #[test]
    fn anchors_and_hash_collisions_are_exact() {
        let a = Key::new("a".into(), vec![]);
        let mut b = Key::new("b".into(), vec![]);
        Arc::get_mut(&mut b.0).unwrap().hash = a.0.hash;
        assert_ne!(a, b);
        assert!(b.clone().anchor() < a);
        assert_eq!(b.clone().anchor().anchor(), b.anchor());
    }
    #[test]
    fn malformed_dag_and_signature_rejected() {
        assert!(
            Key::from_wire(WireKey {
                root: 0,
                nodes: vec![WireNode {
                    anchor: false,
                    label: "x".into(),
                    children: vec![0]
                }]
            })
            .is_err()
        );
        assert!(!signature(
            &serde_json::json!({"primitive":"cell-key-select","inputs":["E","E","i64"],"output":"E"}),
            &serde_json::json!({})
        ));
    }
}
