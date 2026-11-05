//! Ordered witness buckets. A singleton is common for an exact key and
//! costs no tree node. Larger buckets retain the same deterministic order.
use std::{collections::BTreeSet, slice};
#[derive(Clone, Default)]
pub(crate) enum TinySet<T> {
    #[default]
    Empty,
    One(T),
    Tree(Box<BTreeSet<T>>),
}
pub(crate) enum Iter<'a, T> {
    Slice(slice::Iter<'a, T>),
    Tree(std::collections::btree_set::Iter<'a, T>),
}
impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Slice(xs) => xs.next(),
            Self::Tree(xs) => xs.next(),
        }
    }
}
impl<T: Ord + Copy> TinySet<T> {
    pub fn iter(&self) -> Iter<'_, T> {
        match self {
            Self::Empty => Iter::Slice([].iter()),
            Self::One(x) => Iter::Slice(slice::from_ref(x).iter()),
            Self::Tree(xs) => Iter::Tree(xs.iter()),
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::One(_) => 1,
            Self::Tree(xs) => xs.len(),
        }
    }
    pub fn first(&self) -> Option<&T> {
        match self {
            Self::Empty => None,
            Self::One(x) => Some(x),
            Self::Tree(xs) => xs.first(),
        }
    }
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
    pub fn insert(&mut self, v: T) -> bool {
        match self {
            Self::Empty => *self = Self::One(v),
            Self::One(x) if *x == v => return false,
            Self::One(x) => *self = Self::Tree(Box::new(BTreeSet::from([*x, v]))),
            Self::Tree(xs) => return xs.insert(v),
        }
        true
    }
    pub fn remove(&mut self, v: &T) -> bool {
        match self {
            Self::Empty => return false,
            Self::One(x) if x != v => return false,
            Self::One(_) => *self = Self::Empty,
            Self::Tree(xs) => {
                if !xs.remove(v) {
                    return false;
                }
                if xs.len() == 1 {
                    *self = Self::One(*xs.first().unwrap());
                }
            }
        }
        true
    }
}
impl<'a, T: Copy + Ord> IntoIterator for &'a TinySet<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn singleton_promotes_and_shrinks_in_order() {
        let mut s = TinySet::default();
        assert!(s.insert(8));
        assert!(!s.insert(8));
        s.insert(2);
        s.insert(9);
        assert_eq!(s.iter().copied().collect::<Vec<_>>(), [2, 8, 9]);
        assert!(s.remove(&2));
        s.remove(&9);
        assert!(matches!(s, TinySet::One(8)));
        s.remove(&8);
        assert!(s.is_empty());
    }
}
