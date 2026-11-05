//! Checked register programs for connected queries with at most one index
//! expansion. Every other read is a complete key or a bound primitive.
use crate::program::*;
use serde::Serialize;
use std::{collections::BTreeSet, rc::Rc};
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) enum Access {
    Key,
    Expand,
    Primitive,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Step {
    pub atom: usize,
    pub access: Access,
    pub assign: Vec<(usize, usize)>,
    pub check: Vec<(usize, Arg)>,
    pub bound: Vec<(usize, Arg)>,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Kernel {
    pub pivot: usize,
    pub seed: Vec<(usize, usize)>,
    pub steps: Vec<Step>,
}
pub(crate) type Kernels = Vec<Vec<Option<Rc<Kernel>>>>;
pub(crate) fn derive(rules: &[Rule]) -> Kernels {
    rules
        .iter()
        .map(|r| {
            (0..r.query.len())
                .map(|pivot| plan(r, pivot).map(Rc::new))
                .collect()
        })
        .collect()
}
fn plan(r: &Rule, pivot: usize) -> Option<Kernel> {
    if !r.seminaive || r.direct.is_some() || !matches!(r.query[pivot].call, Call::Table(_)) {
        return None;
    }
    let mut known = BTreeSet::new();
    let mut seed = vec![];
    for (col, a) in r.query[pivot].args.iter().enumerate() {
        if let Arg::Slot(s) = a
            && known.insert(*s)
        {
            seed.push((col, *s));
        }
    }
    let mut remaining = (0..r.query.len())
        .filter(|&i| i != pivot)
        .collect::<BTreeSet<_>>();
    let mut expanded = false;
    let mut steps = vec![];
    while !remaining.is_empty() {
        let available =
            |v: &Arg| matches!(v, Arg::Literal(_)) || matches!(v,Arg::Slot(s) if known.contains(s));
        let (rank, atom) = remaining
            .iter()
            .filter_map(|&i| {
                let a = &r.query[i];
                let (_, keys) = a.args.split_last()?;
                if keys.iter().all(available) {
                    Some((usize::from(matches!(a.call, Call::Table(_))), i))
                } else if !expanded
                    && matches!(a.call, Call::Table(_))
                    && a.args.iter().any(available)
                {
                    Some((2, i))
                } else {
                    None
                }
            })
            .min()?;
        let a = &r.query[atom];
        let access = match (&a.call, rank) {
            (Call::Primitive(..), _) => Access::Primitive,
            (_, 2) => {
                expanded = true;
                Access::Expand
            }
            _ => Access::Key,
        };
        let bound = a
            .args
            .iter()
            .enumerate()
            .filter(|(_, v)| available(v))
            .map(|(c, v)| (c, v.clone()))
            .collect();
        let mut assign = vec![];
        let mut check = vec![];
        let last = a.args.len() - 1;
        for (col, v) in a.args.iter().enumerate() {
            if let Arg::Slot(s) = v
                && known.insert(*s)
            {
                assign.push((col, *s));
            } else if col == last || matches!(access, Access::Expand) {
                check.push((col, v.clone()));
            }
        }
        steps.push(Step {
            atom,
            access,
            assign,
            check,
            bound,
        });
        remaining.remove(&atom);
    }
    Some(Kernel { pivot, seed, steps })
}

/// A single functional write with a pure operand prefix. Its committed tuple
/// is also the only possible published anchor of this RHS.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Write {
    #[cfg(feature="sparse-cells")]
    #[serde(skip)] pub(crate) ports: Option<PortWrite>,
    pub op: usize,
    pub table: usize,
    pub key: Vec<Arg>,
    pub value: Arg,
    pub prefix: Vec<(usize, String, String, Vec<Arg>)>,
}
#[cfg(feature="sparse-cells")]
#[derive(Clone,Debug)]
pub(crate) struct PortWrite {
    key: usize, value: Arg, add: Option<(usize,Arg,Arg)>,
}
#[cfg(feature="sparse-cells")]
impl PortWrite {
    fn derive(p:&Program,r:&Rule,table:usize,keys:&[Arg],value:&Arg,ops:&[(usize,String,String,Vec<Arg>)])->Option<Self>{
        let [Arg::Slot(key)]=keys else{return None};
        if p.tables[table].schema[0]["kind"]!="eq" || p.tables[table].schema.last()?["kind"]!="i64" || !r.inputs.contains(key){return None}
        let input=|a:&Arg|match a {Arg::Slot(s)=>r.inputs.contains(s) && p.templates[r.template].sorts[*s]["kind"]=="i64",Arg::Literal(crate::value::Value::Int(_))=>true,_=>false};
        let add=match ops {
            [] if input(value)=>None,
            [(dst,op,sort,args)] if op=="+" && sort=="i64" && value==&Arg::Slot(*dst)=>{
                let [a,b]=args.as_slice() else{return None};
                if !input(a)||!input(b){return None}
                Some((*dst,a.clone(),b.clone()))
            },
            _=>return None,
        };
        Some(Self{key:*key,value:value.clone(),add})
    }
    pub(crate) fn project(&self,slots:&crate::slots::Slots,parents:&crate::value::Parents)->Result<(crate::value::Value,i64,Option<usize>)>{
        use crate::value::Value;
        let Value::Eq(key)=slots[&self.key] else{return Err("invalid producer expression port".into())};
        let int=|a:&Arg|match a {Arg::Literal(Value::Int(v))=>Ok(*v),Arg::Slot(s)=>match &slots[s]{Value::Int(v)=>Ok(*v),_=>Err("invalid integer port".to_string())},_=>Err("invalid integer operand".to_string())};
        let (v,dst)=if let Some((dst,a,b))=&self.add{(int(a)?.checked_add(int(b)?).ok_or("RHS primitive failed: +")?,Some(*dst))}else{(int(&self.value)?,None)};
        Ok((Value::Eq(crate::value::root(parents,key)),v,dst))
    }
}
pub(crate) fn writes(p: &Program) -> Vec<Option<Rc<Write>>> {
    p.rules
        .iter()
        .map(|r| {
            let t = &p.templates[r.template];
            let (Op::Set(table, key, value), prefix) = t.ops.split_last()? else {
                return None;
            };
            if p.tables[*table].virtual_fact
                || (!p.tables[*table].merge.is_null() && p.physical.merges[*table].is_none())
            {
                return None;
            }
            let norm = |a: &Arg| crate::multistage::normalize(t, a);
            let mut calc = vec![];
            for ins in prefix {
                match ins {
                    Op::Copy(..) => {}
                    Op::Let(dst, Call::Primitive(name, sort), args)
                        if !name.starts_with("cell-key-") =>
                    {
                        calc.push((
                            *dst,
                            name.clone(),
                            sort.clone(),
                            args.iter().map(norm).collect(),
                        ))
                    }
                    _ => return None,
                }
            }
            let key=key.iter().map(norm).collect::<Vec<_>>();
            let value=norm(value);
            Some(Rc::new(Write {
                #[cfg(feature="sparse-cells")]
                ports:PortWrite::derive(p,r,*table,&key,&value,&calc),
                op: prefix.len(),
                table: *table,
                key,
                value,
                prefix: calc,
            }))
        })
        .collect()
}
