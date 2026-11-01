//! Shared DAG values/primitives used unchanged by the native and baseline-extension runners.
use crate::pipeline::Result;
use egglog::{
    ArcSort, Core, EGraph, Primitive, PurePrim, PureState, TermDag, TermId, Value,
    ast::{Literal, Span},
    constraint::{SimpleTypeConstraint, TypeConstraint},
    prelude::{BaseSort, add_base_sort},
    sort::{BaseValues, Boxed, F, S},
};
use std::{
    cmp::Ordering,
    sync::Arc,
};
pub(super) use eggfuse_structural_key::{Key, SORT};
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
pub(super) fn decode_key(text: &str) -> Option<Key> { Key::decode_legacy(text) }
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
