//! Complete *functional* interfaces extracted from parameterized Cell programs.
//! A full native function key determines zero or one visible row. This is an
//! upper bound on bindings, unlike positive observations in a closure catalog.
//! The GPU instantiates these interfaces from current rows and watches every
//! read, including the first missing key. Reverse/non-functional reads fall back.
use egglog::cell_assist::{CellActionSink, CellOperand};
use egglog_numeric_id::NumericId;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Clone)]
pub struct Library {
    shapes: BTreeMap<String, BTreeSet<String>>,
    ranges: BTreeMap<(String,usize),BTreeSet<String>>,
}
fn key(q: &CellActionSink, parts: &[usize]) -> String {
    let mut names = BTreeMap::new();
    let atoms = parts.iter().map(|i| {
        let a = &q.atoms[*i];
        let args = a.columns.iter().map(|o| match o {
            CellOperand::Slot(s) => {
                let next=names.len(); json!([0,*names.entry(*s).or_insert(next)])
            },
            CellOperand::Constant{value,..} => json!([1,value.rep()]),
        }).collect::<Vec<_>>();
        json!([a.function,a.sorts,args])
    }).collect::<Vec<_>>();
    json!(atoms).to_string()
}
fn functional(q: &CellActionSink, parts: &[usize]) -> bool {
    let mut bound=BTreeSet::new();
    for (i,part) in parts.iter().enumerate() {
        let a=&q.atoms[*part];
        if i>0 && !a.columns[..a.columns.len()-1].iter().all(|o|match o {
            CellOperand::Slot(s)=>bound.contains(s), _=>true,
        }) {return false}
        bound.extend(a.columns.iter().filter_map(|o|match o{CellOperand::Slot(s)=>Some(*s),_=>None}));
    }
    true
}
impl Library {
    pub fn from_cell_programs(programs:&[CellActionSink])->Self {
        let mut lib=Self::default();for q in programs{lib.add_program(q)}lib
    }
    /// Every source is an installed Cell transfer program, not an arbitrary
    /// consumer rule or a list of observed positive bindings. Extract also
    /// non-root interfaces; retain constants, sorts, aliases and slot wiring.
    pub(crate) fn add_program(&mut self, q: &CellActionSink) {
        for a in &q.atoms {for (col,sort) in a.sorts.iter().enumerate(){if sort=="i64" {
            self.ranges.entry((a.function.clone(),col)).or_default().insert(q.name.clone());
        }}}

        for a in 0..q.atoms.len() {for b in 0..q.atoms.len() {
            if a==b || !functional(q,&[a,b]) {continue}
            // Disconnected nullary/constant reads aren't a Cell connection.
            if !q.atoms[b].columns[..q.atoms[b].columns.len()-1].iter().any(|o|
                matches!(o,CellOperand::Slot(s) if q.atoms[a].columns.iter().any(|x|matches!(x,CellOperand::Slot(t) if s==t)))) {continue}
            self.shapes.entry(key(q,&[a,b])).or_default().insert(q.name.clone());
        }}
    }
    pub(crate) fn evidence(&self,q:&CellActionSink,parts:&[usize])->Option<&BTreeSet<String>> {
        if parts.len()!=2 || !functional(q,parts){return None}
        self.shapes.get(&key(q,parts))
    }
    pub(crate) fn range_sources(&self,table:&str,column:usize)->Option<&BTreeSet<String>> {self.ranges.get(&(table.to_string(),column))}
    pub fn len(&self)->usize {self.shapes.len()}
    pub fn is_empty(&self)->bool {self.shapes.is_empty() && self.ranges.is_empty()}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_names_share_but_aliases_and_constants_do_not() {
        let mut eg=egglog::EGraph::default();
        eg.parse_and_run_program(None,r#"
          (datatype E (N i64) (Pair E E)) (function f (E) i64 :merge (min old new))
          (relation Hit (E))
          (rule ((= p (Pair x y)) (= k (f x))) ((Hit p)) :name "a")
          (rule ((= z (Pair u v)) (= n (f u)) (> n 3)) ((Hit z)) :name "b")
          (rule ((= p (Pair x x)) (= k (f x))) ((Hit p)) :name "alias")
        "#).unwrap();
        let a=eg.prepare_gpu_cell_action("","a").unwrap();
        let b=eg.prepare_gpu_cell_action("","b").unwrap();
        let alias=eg.prepare_gpu_cell_action("","alias").unwrap();
        let parts=|q:&CellActionSink|["Pair","f"].map(|n|q.atoms.iter().position(|a|a.function==n).unwrap());
        let mut lib=Library::default();lib.add_program(&a);
        assert!(lib.evidence(&b,&parts(&b)).is_some());
        assert!(lib.evidence(&alias,&parts(&alias)).is_none());
        assert!(lib.evidence(&b,&[parts(&b)[1],parts(&b)[0]]).is_none());
    }
}
