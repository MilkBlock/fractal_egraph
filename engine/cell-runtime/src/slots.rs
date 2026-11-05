//! Ordered, compact storage for small integer-indexed maps in Cell frames.
//! Dense slots use direct indexing. Sparse SSA and fact IDs use binary search.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::BTreeMap, ops::Index};
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameMap<K, V>(Vec<(K, V)>);
pub type Slots = FrameMap<usize, crate::value::Value>;
impl<K, V> Default for FrameMap<K, V> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<K: Ord + Copy + TryInto<usize>, V> FrameMap<K, V> {
    fn position(&self, k: &K) -> Option<usize> {
        if let Ok(i) = (*k).try_into()
            && self.0.get(i).is_some_and(|(old, _)| old == k)
        {
            return Some(i);
        }
        self.0.binary_search_by_key(k, |x| x.0).ok()
    }
    pub fn get(&self, k: &K) -> Option<&V> {
        self.position(k).map(|i| &self.0[i].1)
    }
    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        self.position(k).map(|i| &mut self.0[i].1)
    }
    pub fn contains_key(&self, k: &K) -> bool {
        self.position(k).is_some()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    #[cfg(feature = "sparse-cells")]
    pub(crate) fn reserve_first(&mut self) {
        if self.0.capacity() == 0 {
            self.0.reserve_exact(1);
        }
    }
    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        if self.0.last().is_none_or(|x| x.0 < k) {
            self.0.push((k, v));
            return None;
        }
        match self.0.binary_search_by_key(&k, |x| x.0) {
            Ok(i) => Some(std::mem::replace(&mut self.0[i].1, v)),
            Err(i) => {
                self.0.insert(i, (k, v));
                None
            }
        }
    }
    pub fn remove(&mut self, k: &K) -> Option<V> {
        self.position(k).map(|i| self.0.remove(i).1)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.0.iter().map(|(k, _)| k)
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.iter().map(|(_, v)| v)
    }
}
impl<K: Ord + Copy + TryInto<usize>, V> FromIterator<(K, V)> for FrameMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(i: I) -> Self {
        let it = i.into_iter();
        let mut m = Self(Vec::with_capacity(it.size_hint().0));
        m.extend(it);
        m
    }
}
impl<K: Ord + Copy + TryInto<usize>, V> Extend<(K, V)> for FrameMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, i: I) {
        for (k, v) in i {
            self.insert(k, v);
        }
    }
}
impl<K: Ord + Copy + TryInto<usize>, V> Index<&K> for FrameMap<K, V> {
    type Output = V;
    fn index(&self, k: &K) -> &V {
        self.get(k).expect("missing Cell slot")
    }
}
impl<K: Serialize, V: Serialize> Serialize for FrameMap<K, V> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().map(|(k, v)| (k, v)))
    }
}
impl<'de, K: Deserialize<'de> + Ord + Copy + TryInto<usize>, V: Deserialize<'de>> Deserialize<'de>
    for FrameMap<K, V>
{
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(BTreeMap::<K, V>::deserialize(d)?.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_checkpoint_slots_round_trip_in_order() {
        let mut slots: FrameMap<usize, u64> = [(0, 1), (20, 3), (2, 2)].into_iter().collect();
        assert_eq!(slots.insert(2, 4), Some(2));
        assert_eq!(slots.remove(&20), Some(3));
        assert_eq!(slots.keys().copied().collect::<Vec<_>>(), [0, 2]);
        let encoded = serde_json::to_string(&slots).unwrap();
        assert_eq!(encoded, r#"{"0":1,"2":4}"#);
        assert_eq!(
            serde_json::from_str::<FrameMap<usize, u64>>(&encoded).unwrap(),
            slots
        );
    }
}
