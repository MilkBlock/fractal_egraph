//! Reverse dependencies are unordered until consumed by a matcher or inserted
//! into the ordered repair queue. Store singleton subscriptions inline.
use super::Dependency;
use rustc_hash::FxHashSet;

#[derive(Default)]
pub(super) enum UseSet {
    #[default]
    Empty,
    One(Dependency),
    Many(Box<FxHashSet<Dependency>>),
}

impl UseSet {
    pub fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::One(_) => 1,
            Self::Many(rows) => rows.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Dependency> {
        let (one, many) = match self {
            Self::Empty => (None, None),
            Self::One(row) => (Some(row), None),
            Self::Many(rows) => (None, Some(rows.iter())),
        };
        one.into_iter().chain(many.into_iter().flatten())
    }

    pub fn insert(&mut self, row: Dependency) {
        match self {
            Self::Empty => *self = Self::One(row),
            Self::One(old) if *old != row => {
                *self = Self::Many(Box::new([*old, row].into_iter().collect()));
            }
            Self::Many(rows) => {
                rows.insert(row);
            }
            Self::One(_) => (),
        }
    }

    pub fn remove(&mut self, row: &Dependency) {
        match self {
            Self::One(old) if old == row => *self = Self::Empty,
            Self::Many(rows) if rows.contains(row) => {
                rows.remove(row);
                if rows.len() == 1 {
                    *self = Self::One(*rows.iter().next().unwrap());
                }
            }
            _ => (),
        }
    }
}
