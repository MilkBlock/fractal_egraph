//! Shared DAG values/primitives used unchanged by the native and baseline-extension runners.
use crate::Result;
use egglog::{
    ArcSort, Core, EGraph, Primitive, PurePrim, PureState, TermDag, TermId, Value,
    ast::{Literal, Span},
    constraint::{SimpleTypeConstraint, TypeConstraint},
    prelude::{BaseSort, add_base_sort},
    sort::{BaseValues, Boxed, F, S},
};
use serde_json::Value as Json;
use std::{
    cmp::Ordering,
    hash::{Hash, Hasher},
    sync::Arc,
};
pub(super) const SORT: &str = "CellStructuralKey";
#[derive(Clone)]
pub(super) struct Key(pub(super) Arc<Node>);
// Native graph export prints base values through Debug, not reconstruct_termdag.
// Recursively printing an Arc DAG unfolds repeated children exponentially.
// This is a diagnostic label only; exact equality/order and KeyGraph snapshots
// still inspect the complete shared structure.
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"CellStructuralKey(anchor={}, height={}, hash={:016x})",self.0.anchor,self.0.height,self.0.hash)
    }
}
#[derive(Debug)]
pub(super) struct Node {
    pub(super) anchor: bool,
    pub(super) height: usize,
    pub(super) label: Arc<str>,
    pub(super) children: Vec<Key>,
    pub(super) hash: u64,
}
impl Key {
    pub(super) fn new(label: String, children: Vec<Key>) -> Self {
        Self::with_label(label.into(), children)
    }
    pub(super) fn with_label(label: Arc<str>, children: Vec<Key>) -> Self {
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
    pub(super) fn anchor(self) -> Self {
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
        b.0.anchor
            .cmp(&self.0.anchor)
            .then_with(|| self.0.height.cmp(&b.0.height))
            .then_with(|| self.0.label.cmp(&b.0.label))
            .then_with(|| self.0.children.cmp(&b.0.children))
    }
}
impl PartialOrd for Key {
    fn partial_cmp(&self, b: &Self) -> Option<Ordering> {
        Some(self.cmp(b))
    }
}
pub(super) type Stored = Boxed<Key>;
#[derive(Debug)]
pub(super) struct KeySort;
impl BaseSort for KeySort {
    type Base = Stored;
    fn name(&self) -> &str {
        SORT
    }
    fn reconstruct_termdag(&self, b: &BaseValues, v: Value, d: &mut TermDag) -> TermId {
        let k = b.unwrap::<Stored>(v);
        // Only a diagnostic rendering. Equality/order always uses the full DAG.
        d.lit(Literal::String(format!(
            "key:{}:{:016x}",
            k.0.0.height, k.0.0.hash
        )))
    }
}
#[derive(Clone)]
pub(super) enum Op {
    Min,
    Compare,
    Decode,
    Choose,
    PreferLeft,
    Anchor,
    Node(Arc<str>),
    Literal(String),
}
#[derive(Clone)]
pub(super) struct Prim {
    pub(super) name: String,
    pub(super) types: Vec<ArcSort>,
    pub(super) op: Op,
}
impl Primitive for Prim {
    fn name(&self) -> &str {
        &self.name
    }
    fn get_type_constraints(&self, s: &Span) -> Box<dyn TypeConstraint> {
        SimpleTypeConstraint::new(self.name(), self.types.clone(), s.clone()).into_box()
    }
}
impl PurePrim for Prim {
    fn apply<'a, 'db>(&self, s: PureState<'a, 'db>, a: &[Value]) -> Option<Value> {
        let b = s.base_values();
        let key = |v| b.unwrap::<Stored>(v);
        match &self.op {
            Op::Decode => decode_key(b.unwrap::<S>(a[0]).as_str()).map(|k| b.get(Stored::new(k))),
            Op::Compare => Some(b.get(match key(a[0]).cmp(&key(a[1])) {
                Ordering::Less => -1i64,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            })),
            Op::Min => Some(if a[0] == a[1] || key(a[0]) <= key(a[1]) {
                a[0]
            } else {
                a[1]
            }),
            Op::Anchor => Some(b.get(Stored::new((*key(a[0])).clone().anchor()))),
            Op::PreferLeft => Some(b.get(
                b.unwrap::<i64>(a[0]) > b.unwrap::<i64>(a[1])
                    || (b.unwrap::<i64>(a[0]) == b.unwrap::<i64>(a[1]) && key(a[2]) <= key(a[3])),
            )),
            Op::Choose => {
                let c0 = b.unwrap::<i64>(a[2]);
                let c1 = b.unwrap::<i64>(a[3]);
                Some(if c0 > c1 || (c0 == c1 && key(a[4]) <= key(a[5])) {
                    a[0]
                } else {
                    a[1]
                })
            }
            Op::Node(label) => Some(b.get(Stored::new(Key::with_label(
                label.clone(),
                a.iter().map(|v| (*key(*v)).clone()).collect(),
            )))),
            Op::Literal(sort) => {
                let text = match sort.as_str() {
                    "i64" => b.unwrap::<i64>(a[0]).to_string(),
                    "f64" => {
                        let value = b.unwrap::<F>(a[0]);
                        let bits = if value.is_nan() {
                            f64::NAN.to_bits()
                        } else if value.to_bits() & 0x7fff_ffff_ffff_ffff == 0 {
                            0
                        } else {
                            value.to_bits()
                        };
                        format!("{bits:016x}")
                    }
                    "String" => format!("{:?}", b.unwrap::<S>(a[0]).as_str()),
                    "bool" => b.unwrap::<bool>(a[0]).to_string(),
                    "Unit" => "()".into(),
                    _ => unreachable!(),
                };
                Some(b.get(Stored::new(Key::new(format!("{sort}:{text}"), vec![]))))
            }
        }
    }
}
pub(super) fn decode_key(text: &str) -> Option<Key> {
    fn decode(j: &Json) -> Option<Key> {
        let xs = j.as_array()?;
        if xs.len() != 3 {
            return None;
        }
        let anchor = xs[0].as_bool()?;
        let label = xs[1].as_str()?.to_owned();
        let children = xs[2]
            .as_array()?
            .iter()
            .map(decode)
            .collect::<Option<Vec<_>>>()?;
        if anchor && (label != "\0input-anchor" || children.len() != 1) {
            return None;
        }
        let mut key = Key::new(label, children);
        if anchor {
            let n = Arc::get_mut(&mut key.0)?;
            n.anchor = true;
            n.height = 0;
        }
        Some(key)
    }
    decode(&serde_json::from_str::<Json>(text).ok()?)
}
pub(super) fn install_base(eg: &mut EGraph) -> Result<()> {
    add_base_sort(eg, KeySort, Span::Panic)?;
    let key = eg.get_sort_by_name(SORT).unwrap().clone();
    let int = eg.get_sort_by_name("i64").unwrap().clone();
    let string = eg.get_sort_by_name("String").unwrap().clone();
    for (name, op, types) in [
        ("cell-key-min", Op::Min, vec![key.clone(); 3]),
        ("cell-key-anchor", Op::Anchor, vec![key.clone(); 2]),
        (
            "cell-key-cmp",
            Op::Compare,
            vec![key.clone(), key.clone(), int],
        ),
        ("cell-key-decode", Op::Decode, vec![string, key]),
    ] {
        eg.add_pure_primitive(
            Prim {
                name: name.into(),
                types,
                op,
            },
            None,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_printing_does_not_unfold_a_shared_dag() {
        let mut key=Key::new("leaf".into(),vec![]);
        for _ in 0..60 {key=Key::new("pair".into(),vec![key.clone(),key]);}
        let text=format!("{:?}",Stored::new(key));
        assert!(text.len()<160,"{text}");
        assert!(text.contains("height=60"));
    }
}
