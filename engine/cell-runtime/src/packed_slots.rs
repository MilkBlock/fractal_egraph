//! A shared SSA slot number needs only one occupancy bit in a compact Cell.
//! Values stay in logical slot order. Unbounded/wide templates keep the
//! ordinary sorted map and use exactly the same checkpoint representation.
use super::FrameMap;
use crate::value::Value;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{cmp::Ordering, collections::BTreeMap, ops::Index};
#[derive(Clone, Debug)]
pub struct Slots(Repr);
#[derive(Clone, Debug)]
enum Repr {
    Packed { bits: u64, values: Vec<Value> },
    Wide(FrameMap<usize, Value>),
}
const IDS: [usize; 64] = {
    let mut a = [0; 64];
    let mut i = 0;
    while i < 64 {
        a[i] = i;
        i += 1;
    }
    a
};
#[derive(Clone, Copy)]
struct Occupied(u64);
impl Iterator for Occupied {
    type Item = &'static usize;
    fn next(&mut self) -> Option<Self::Item> {
        if self.0 == 0 {
            return None;
        }
        let i = self.0.trailing_zeros() as usize;
        self.0 &= self.0 - 1;
        Some(&IDS[i])
    }
}
enum Entries<'a> {
    Packed(std::iter::Zip<Occupied, std::slice::Iter<'a, Value>>),
    Wide(std::slice::Iter<'a, (usize, Value)>),
}
impl<'a> Iterator for Entries<'a> {
    type Item = (&'a usize, &'a Value);
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Packed(rows) => rows.next().map(|(i, v)| (i as &'a usize, v)),
            Self::Wide(rows) => rows.next().map(|(i, v)| (i, v)),
        }
    }
}
impl Default for Slots {
    fn default() -> Self {
        Self(Repr::Packed {
            bits: 0,
            values: vec![],
        })
    }
}
impl Slots {
    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::Packed { values, .. } => values.len(),
            Repr::Wide(m) => m.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    #[inline]
    fn bit(k: usize) -> Option<u64> {
        (k < 64).then(|| 1u64 << k)
    }
    #[inline]
    fn offset(bits: u64, bit: u64) -> usize {
        (bits & (bit - 1)).count_ones() as usize
    }
    pub fn contains_key(&self, k: &usize) -> bool {
        match &self.0 {
            Repr::Packed { bits, .. } => Self::bit(*k).is_some_and(|b| bits & b != 0),
            Repr::Wide(m) => m.contains_key(k),
        }
    }
    #[inline]
    pub fn get(&self, k: &usize) -> Option<&Value> {
        match &self.0 {
            Repr::Packed { bits, values } => {
                let b = Self::bit(*k)?;
                if *bits & b == 0 {
                    None
                } else {
                    values.get(Self::offset(*bits, b))
                }
            }
            Repr::Wide(m) => m.get(k),
        }
    }
    pub fn get_mut(&mut self, k: &usize) -> Option<&mut Value> {
        match &mut self.0 {
            Repr::Packed { bits, values } => {
                let b = Self::bit(*k)?;
                if *bits & b == 0 {
                    None
                } else {
                    values.get_mut(Self::offset(*bits, b))
                }
            }
            Repr::Wide(m) => m.get_mut(k),
        }
    }
    pub fn insert(&mut self, k: usize, v: Value) -> Option<Value> {
        if let Repr::Wide(m) = &mut self.0 {
            return m.insert(k, v);
        }
        let Repr::Packed { bits, values } = &mut self.0 else {
            unreachable!()
        };
        if let Some(b) = Self::bit(k) {
            let i = Self::offset(*bits, b);
            if *bits & b != 0 {
                return Some(std::mem::replace(&mut values[i], v));
            }
            *bits |= b;
            if i == values.len() {
                values.push(v)
            } else {
                values.insert(i, v)
            };
            return None;
        }
        let mut m: FrameMap<_, _> = Occupied(*bits)
            .copied()
            .zip(std::mem::take(values))
            .collect();
        m.insert(k, v);
        self.0 = Repr::Wide(m);
        None
    }
    pub fn remove(&mut self, k: &usize) -> Option<Value> {
        match &mut self.0 {
            Repr::Packed { bits, values } => {
                let b = Self::bit(*k)?;
                if *bits & b == 0 {
                    return None;
                }
                let i = Self::offset(*bits, b);
                *bits &= !b;
                Some(values.remove(i))
            }
            Repr::Wide(m) => m.remove(k),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&usize, &Value)> {
        match &self.0 {
            Repr::Packed { bits, values } => Entries::Packed(Occupied(*bits).zip(values.iter())),
            Repr::Wide(m) => Entries::Wide(m.0.iter()),
        }
    }
    pub fn keys(&self) -> impl Iterator<Item = &usize> {
        self.iter().map(|(k, _)| k)
    }
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.iter().map(|(_, v)| v)
    }
    pub(crate) fn projection_mask(keys: &[usize]) -> Option<u64> {
        if !keys.windows(2).all(|x| x[0] < x[1]) {
            return None;
        }
        keys.iter()
            .try_fold(0, |mask, &k| Some(mask | Self::bit(k)?))
    }
    pub(crate) fn slice(&self, mask: u64) -> Option<&[Value]> {
        match &self.0 {
            Repr::Packed { bits, values } if *bits == mask => Some(values),
            _ => None,
        }
    }
}
impl Extend<(usize, Value)> for Slots {
    fn extend<I: IntoIterator<Item = (usize, Value)>>(&mut self, i: I) {
        for (k, v) in i {
            self.insert(k, v);
        }
    }
}
impl FromIterator<(usize, Value)> for Slots {
    fn from_iter<I: IntoIterator<Item = (usize, Value)>>(i: I) -> Self {
        let it = i.into_iter();
        let mut s = Self(Repr::Packed {
            bits: 0,
            values: Vec::with_capacity(it.size_hint().0),
        });
        s.extend(it);
        s
    }
}
impl Index<&usize> for Slots {
    type Output = Value;
    fn index(&self, k: &usize) -> &Value {
        self.get(k).expect("missing Cell slot")
    }
}
impl PartialEq for Slots {
    fn eq(&self, b: &Self) -> bool {
        self.iter().eq(b.iter())
    }
}
impl Eq for Slots {}
impl PartialOrd for Slots {
    fn partial_cmp(&self, b: &Self) -> Option<Ordering> {
        Some(self.cmp(b))
    }
}
impl Ord for Slots {
    fn cmp(&self, b: &Self) -> Ordering {
        self.iter().cmp(b.iter())
    }
}
impl Serialize for Slots {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.iter())
    }
}
impl<'de> Deserialize<'de> for Slots {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(BTreeMap::<usize, Value>::deserialize(d)?
            .into_iter()
            .collect())
    }
}
