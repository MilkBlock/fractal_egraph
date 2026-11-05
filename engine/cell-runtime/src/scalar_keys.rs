//! Column types in a validated table make a single scalar a complete index
//! key. Unusual representations still use the ordinary value-vector index.
use super::*;
pub(super) struct FunctionKeys {
    scalar: Option<Kind>,
    words: HashMap<u64, usize>,
    other: HashMap<Vec<Value>, usize>,
}
#[derive(Clone, Copy)]
enum Kind {
    Eq,
    Int,
    Float,
}
impl FunctionKeys {
    pub fn new(t: &Table) -> Self {
        let scalar = if t.schema.len() == 2 {
            match t.schema[0]["kind"].as_str() {
                Some("eq") => Some(Kind::Eq),
                Some("i64") => Some(Kind::Int),
                Some("f64") => Some(Kind::Float),
                _ => None,
            }
        } else {
            None
        };
        Self {
            scalar,
            words: Default::default(),
            other: Default::default(),
        }
    }
    #[inline]
    fn word(&self, key: &[Value]) -> Option<u64> {
        match (self.scalar, key) {
            (Some(Kind::Eq), [Value::Eq(x)]) => Some(*x),
            (Some(Kind::Int), [Value::Int(x)]) | (Some(Kind::Float), [Value::Float(x)]) => {
                Some(*x as u64)
            }
            _ => None,
        }
    }
    #[inline]
    pub fn get(&self, key: &[Value]) -> Option<&usize> {
        match self.word(key) {
            Some(x) => self.words.get(&x),
            None => self.other.get(key),
        }
    }
    pub fn insert(&mut self, key: &[Value], v: usize) {
        match self.word(key) {
            Some(x) => {
                self.words.insert(x, v);
            }
            None => {
                self.other.insert(key.to_vec(), v);
            }
        }
    }
    pub fn remove(&mut self, key: &[Value]) {
        match self.word(key) {
            Some(x) => {
                self.words.remove(&x);
            }
            None => {
                self.other.remove(key);
            }
        }
    }
}
