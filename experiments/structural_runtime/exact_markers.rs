//! Exact shared residual conjunctions. A marker carries a factorized set of
//! WHOLE witness rows, never independent columns or an expanded Cartesian table.
use super::{Map, Set, StoredRow};
use serde::Serialize;
use std::sync::Arc;

pub type Condition = (usize, usize); // typed projection ID, stable binding handle
pub type Payload = Arc<Vec<Arc<Vec<StoredRow>>>>;
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord,Hash)]
pub enum Id { Leaf(Condition), Group(usize) }
#[derive(Clone)]
pub struct Use {
    pub id: Id,
    pub order: Vec<usize>, // canonical factor -> consumer's original step
}
struct Marker {
    factors: Vec<Condition>,
    users: usize,
    ready: bool,
    payload: Option<Payload>,
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub created: usize,
    pub reused: usize,
    pub removed: usize,
    pub activated: usize,
    pub deactivated: usize,
    pub invalidations: usize,
    pub publications: usize,
    pub truth_reads: usize,
    pub payload_builds: usize,
    pub payload_hits: usize,
    pub factor_references: usize,
    pub tuples: usize,
    pub consumer_tuple_visits: usize,
    pub leaf_truth_reads: usize,
    pub leaf_payloads: usize,
    pub query_seconds: f64,
}
#[derive(Default)]
pub struct Markers {
    // A singleton already IS an exact native condition slot. Only count its
    // users here; reuse its presence bit, version and whole-row answer buffer.
    leaves: Map<Condition,usize>,
    directory: Map<Vec<Condition>, usize>,
    nodes: Vec<Option<Marker>>,
    free: Vec<usize>,
    dependents: Map<Condition, Set<usize>>,
    dirty: Set<usize>,
    pub stats: Stats,
}
impl Markers {
    pub fn attach(&mut self, factors: Vec<Condition>) -> Use {
        if factors.len()==1 {
            let n=self.leaves.entry(factors[0]).or_default();
            if *n==0{self.stats.created+=1;}else{self.stats.reused+=1;}
            *n+=1;return Use{id:Id::Leaf(factors[0]),order:vec![0]};
        }
        let mut keyed = factors.into_iter().enumerate().map(|(i,k)|(k,i)).collect::<Vec<_>>();
        keyed.sort_unstable();
        let key=keyed.iter().map(|(k,_)|*k).collect::<Vec<_>>();
        // Multiplicity is essential: P(x,a), P(x,b) has two independent witnesses.
        let order=keyed.iter().map(|(_,i)|*i).collect();
        let id=if let Some(&id)=self.directory.get(&key) {
            let n=self.nodes[id].as_mut().unwrap();n.users+=1;
            if n.users==2{self.subscribe(id);}
            self.stats.reused+=1;id
        } else {
            let id=self.free.pop().unwrap_or_else(||{self.nodes.push(None);self.nodes.len()-1});
            self.nodes[id]=Some(Marker{factors:key.clone(),users:1,ready:false,payload:None});
            self.directory.insert(key,id);self.stats.created+=1;id
        };
        Use{id:Id::Group(id),order}
    }
    fn subscribe(&mut self,id:usize) {
        for &c in &self.nodes[id].as_ref().unwrap().factors{self.dependents.entry(c).or_default().insert(id);}
        self.dirty.insert(id);self.stats.activated+=1;
    }
    fn unsubscribe(&mut self,id:usize) {
        let n=self.nodes[id].as_mut().unwrap();n.payload=None;
        for c in &n.factors {
            if let Some(users)=self.dependents.get_mut(c) {
                users.remove(&id);if users.is_empty(){self.dependents.remove(c);}
            }
        }
        self.dirty.remove(&id);self.stats.deactivated+=1;
    }
    pub fn detach(&mut self, id:Id) {
        let Id::Group(id)=id else {
            let Id::Leaf(c)=id else{unreachable!()};let n=self.leaves.get_mut(&c).unwrap();*n-=1;
            if *n==0{self.leaves.remove(&c);self.stats.removed+=1;}return;
        };
        let node=self.nodes[id].as_mut().unwrap();node.users-=1;let users=node.users;
        if users==1{self.unsubscribe(id);}
        if users!=0{return;}
        let node=self.nodes[id].take().unwrap();self.directory.remove(&node.factors);
        self.dirty.remove(&id);self.free.push(id);self.stats.removed+=1;
    }
    pub fn changed(&mut self,c:Condition) {
        if let Some(users)=self.dependents.get(&c) {
            for &id in users {
                if self.dirty.insert(id){self.stats.invalidations+=1;}
                self.nodes[id].as_mut().unwrap().payload=None;
            }
        }
    }
    pub fn publish(&mut self,mut present:impl FnMut(Condition)->bool) {
        for id in self.dirty.drain() {
            let n=self.nodes[id].as_mut().unwrap();
            n.ready=n.factors.iter().all(|&c|{self.stats.truth_reads+=1;present(c)});
            self.stats.publications+=1;
        }
    }
    pub fn shared_ready(&mut self,id:Id,mut present:impl FnMut(Condition)->bool)->Option<bool> {
        let Id::Group(id)=id else {
            let Id::Leaf(c)=id else{unreachable!()};
            return if self.leaves[&c]>1{self.stats.leaf_truth_reads+=1;Some(present(c))}else{None};
        };
        assert!(!self.dirty.contains(&id),"unpublished exact marker");
        let n=self.nodes[id].as_ref().unwrap();(n.users>1).then_some(n.ready)
    }
    pub fn payload(&mut self,id:Id,mut load:impl FnMut(Condition)->Arc<Vec<StoredRow>>)->Payload {
        let Id::Group(id)=id else {
            let Id::Leaf(c)=id else{unreachable!()};assert!(self.leaves[&c]>1);
            self.stats.leaf_payloads+=1;let rows=load(c);assert!(!rows.is_empty());return Arc::new(vec![rows]);
        };
        assert!(!self.dirty.contains(&id));let n=self.nodes[id].as_mut().unwrap();assert!(n.ready);
        if let Some(p)=&n.payload {self.stats.payload_hits+=1;return p.clone();}
        let p=Arc::new(n.factors.iter().map(|&c|load(c)).collect::<Vec<_>>());
        assert!(p.iter().all(|r|!r.is_empty()));self.stats.payload_builds+=1;
        self.stats.factor_references+=p.len();n.payload=Some(p.clone());p
    }
    pub fn report(&self)->serde_json::Value {
        serde_json::json!({"statistics":self.stats,"live_markers":self.directory.len()+self.leaves.len(),
            "native_leaf_markers":self.leaves.len(),"conjunction_markers":self.directory.len(),
            "shared_markers":self.nodes.iter().flatten().filter(|m|m.users>1).count()+self.leaves.values().filter(|n|**n>1).count(),
            "live_anchor_uses":self.nodes.iter().flatten().map(|m|m.users).sum::<usize>()+self.leaves.values().sum::<usize>(),
            "factor_connections":self.dependents.values().map(|d|d.len()).sum::<usize>(),
            "scope":"Exact current native projection witnesses; not a new RipSat closure certificate"})
    }
}
/// Stream each factor tuple once, then let several consumers scatter it. No
/// eager Cartesian materialization, and each row's original timestamp survives.
pub fn visit<'a>(factors:&'a [Arc<Vec<StoredRow>>],rows:&mut Vec<&'a StoredRow>,stamp:u32,emit:&mut impl FnMut(&[&StoredRow],u32)) {
    let depth=rows.len();
    if depth==factors.len(){emit(rows,stamp);return;}
    for row in factors[depth].iter(){rows.push(row);visit(factors,rows,stamp.max(row.stamp),emit);rows.pop();}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::{store_row,Row,Value};
    #[test]
    fn permutations_multiple_publishers_invalidation_and_slot_reuse() {
        let mut m=Markers::default();let a=m.attach(vec![(2,4),(1,3)]);let b=m.attach(vec![(1,3),(2,4)]);
        assert_eq!(a.id,b.id);assert_eq!(a.order,vec![1,0]);assert_eq!(b.order,vec![0,1]);
        m.publish(|_|true);assert_eq!(m.shared_ready(a.id,|_|true),Some(true));
        m.changed((1,3));m.publish(|c|c!=(1,3));assert_eq!(m.shared_ready(a.id,|_|true),Some(false));
        m.detach(a.id);assert_eq!(m.shared_ready(b.id,|_|true),None);m.detach(b.id);
        assert!(m.dependents.is_empty());assert!(m.directory.is_empty());
        let c=m.attach(vec![(9,9),(9,10)]);assert_eq!(c.id,a.id);m.publish(|_|false);
        assert_eq!(m.shared_ready(c.id,|_|false),None);
    }
    #[test]
    fn unique_consumers_do_not_subscribe_or_freeze_stale_truth() {
        let mut m=Markers::default();let a=m.attach(vec![(0,1),(1,2)]);
        m.changed((0,1));m.publish(|_|panic!("unique consumer needs no maintained view"));
        assert!(m.dependents.is_empty());assert_eq!(m.shared_ready(a.id,|_|true),None);
        let b=m.attach(vec![(0,1),(1,2)]);m.publish(|_|false);assert_eq!(m.shared_ready(a.id,|_|true),Some(false));
        m.detach(b.id);m.changed((0,1));m.publish(|_|panic!("unsubscribe after sharing ends"));
        m.attach(vec![(0,1),(1,2)]);m.publish(|_|true);assert_eq!(m.shared_ready(a.id,|_|true),Some(true));
    }
    #[test]
    fn leaf_reuses_native_truth_instead_of_a_second_snapshot() {
        let mut m=Markers::default();let a=m.attach(vec![(2,3)]);let b=m.attach(vec![(2,3)]);
        m.changed((2,3));m.publish(|_|panic!("leaf truth belongs to native projection"));
        assert!(m.nodes.is_empty());assert!(m.dependents.is_empty());
        assert_eq!(m.shared_ready(a.id,|_|false),Some(false));
        assert_eq!(m.shared_ready(a.id,|_|true),Some(true));
        m.detach(a.id);assert_eq!(m.shared_ready(b.id,|_|panic!("unshared")),None);
        m.detach(b.id);assert!(m.leaves.is_empty());
    }
    #[test]
    fn whole_rows_multiplicity_and_timestamps_are_preserved() {
        let row=|a,b,stamp|store_row(Row{values:vec![Value::new_const(a),Value::new_const(b)],stamp});
        let mut m=Markers::default();let a=m.attach(vec![(0,0),(0,0)]);m.attach(vec![(0,0),(0,0)]);m.publish(|_|true);
        let rows=Arc::new(vec![row(1,10,3),row(2,20,7)]);
        let p=m.payload(a.id,|_|rows.clone());let mut seen=vec![];
        visit(&p,&mut vec![],0,&mut |rs,t|seen.push((rs[0].values.clone(),rs[1].values.clone(),t)));
        assert_eq!(seen.len(),4);assert_eq!(seen.iter().filter(|(_,_,t)|*t==7).count(),3);
        assert!(seen.iter().all(|(a,b,_)|[a,b].iter().all(|r|r[1]==Value::new_const(if r[0]==Value::new_const(1){10}else{20}))));
        assert!(Arc::ptr_eq(&p,&m.payload(a.id,|_|panic!("cached"))));
        m.changed((0,0));m.publish(|_|true);let updated=m.payload(a.id,|_|Arc::new(vec![row(3,30,9)]));
        assert!(!Arc::ptr_eq(&p,&updated));assert_eq!(p[0].len(),2);assert_eq!(updated[0].len(),1);
    }
}
