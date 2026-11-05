use rustc_hash::FxHashSet as Set;
#[derive(Default)]
pub(super) enum Support {
    #[default]
    Empty,
    Unit,
    One(Set<u64>),
    Two(Set<[u64; 2]>),
    Many(Set<Vec<u64>>),
}
impl Support {
    pub fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Unit => 1,
            Self::One(k) => k.len(),
            Self::Two(k) => k.len(),
            Self::Many(k) => k.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn insert(&mut self, k: &[u64]) -> bool {
        if matches!(self, Self::Empty) {
            *self = match k {
                [] => {
                    *self = Self::Unit;
                    return true;
                }
                [_] => Self::One(Default::default()),
                [_, _] => Self::Two(Default::default()),
                _ => Self::Many(Default::default()),
            };
        }
        match self {
            Self::Empty => unreachable!(),
            Self::Unit => {
                debug_assert!(k.is_empty());
                false
            }
            Self::One(s) => s.insert(k[0]),
            Self::Two(s) => s.insert([k[0], k[1]]),
            Self::Many(s) => {
                if s.contains(k) {
                    false
                } else {
                    s.insert(k.to_vec())
                }
            }
        }
    }
    pub fn iter(&self) -> Iter<'_> {
        match self {
            Self::Empty => Iter::Empty,
            Self::Unit => Iter::Unit(true),
            Self::One(s) => Iter::One(s.iter()),
            Self::Two(s) => Iter::Two(s.iter()),
            Self::Many(s) => Iter::Many(s.iter()),
        }
    }
}
pub(super) enum Iter<'a> {
    Empty,
    Unit(bool),
    One(std::collections::hash_set::Iter<'a, u64>),
    Two(std::collections::hash_set::Iter<'a, [u64; 2]>),
    Many(std::collections::hash_set::Iter<'a, Vec<u64>>),
}
impl<'a> Iterator for Iter<'a> {
    type Item = &'a [u64];
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Empty => None,
            Self::Unit(v) => std::mem::take(v).then_some(&[]),
            Self::One(it) => it.next().map(std::slice::from_ref),
            Self::Two(it) => it.next().map(|k| &k[..]),
            Self::Many(it) => it.next().map(Vec::as_slice),
        }
    }
}
impl<'a> IntoIterator for &'a Support {
    type Item = &'a [u64];
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}
