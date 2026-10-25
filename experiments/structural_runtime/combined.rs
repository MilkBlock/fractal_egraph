//! Compose existing closure services and ridge without consuming a native
//! journal twice. The first reader forwards overlapping rows to the ridge view;
//! the outer matcher receives each drained row batch once, under one namespace.
use super::*;

pub struct Combined {
    joint:Box<dyn SaturationHook>,ridge:Hook,
    joint_names:Vec<String>,ridge_names:Vec<String>,names:Vec<String>,
    joint_to_ridge:Vec<Option<usize>>,joint_to_outer:Vec<Option<usize>>,ridge_to_outer:Vec<Option<usize>>,
    forwarded_rows:usize,joint_bindings:usize,ridge_bindings:usize,
}
impl Combined {
    pub fn new(joint:Box<dyn SaturationHook>,ridge:Hook)->Self{
        Self{joint,ridge,joint_names:vec![],ridge_names:vec![],names:vec![],joint_to_ridge:vec![],joint_to_outer:vec![],ridge_to_outer:vec![],forwarded_rows:0,joint_bindings:0,ridge_bindings:0}
    }
    fn mappings(&mut self,joint:&[String],ridge:&[String]) {
        if joint==self.joint_names && ridge==self.ridge_names{return;}
        self.joint_names=joint.to_vec();self.ridge_names=ridge.to_vec();
        self.names=joint.iter().chain(ridge).cloned().collect::<BTreeSet<_>>().into_iter().collect();
        self.joint_to_ridge=joint.iter().map(|n|ridge.iter().position(|r|r==n)).collect();
        self.joint_to_outer=joint.iter().map(|n|self.names.iter().position(|r|r==n)).collect();
        self.ridge_to_outer=ridge.iter().map(|n|self.names.iter().position(|r|r==n)).collect();
    }
}
fn projected(b:&CellCommitBatch,map:&[Option<usize>])->CellCommitBatch{
    CellCommitBatch{
        rows:b.rows.iter().filter_map(|r|map[r.table].map(|id|{let mut row=r.clone();row.table=id;row})).collect(),
        clears:b.clears.iter().filter_map(|t|map[*t]).collect(),
        // Every native subscriber has its own union cursor. Do not forward
        // another subscriber's union stream and invalidate coordinates twice.
        unions:vec![],
    }
}
fn remap(mut b:CellCommitBatch,map:&[Option<usize>])->CellCommitBatch{
    b.rows.retain_mut(|r|if let Some(id)=map[r.table]{r.table=id;true}else{false});
    b.clears.retain_mut(|t|if let Some(id)=map[*t]{*t=id;true}else{false});
    b.unions.clear();b
}
impl SaturationHook for Combined {
    fn binding_connections(&self)->cell_assisted_egglog::cell_connection::Library{self.joint.binding_connections()}
    fn query_cells(&mut self,eg:&mut EGraph,set:&str,sinks:&[&CellActionSink])->Result<Vec<(usize,Vec<Vec<Value>>)>>{
        let mut answers=self.joint.query_cells(eg,set,sinks)?;
        let mut claimed=BTreeSet::new();
        for (id,b) in &answers{if *id>=sinks.len() || !claimed.insert(*id){return Err("invalid joint query ownership".into());}self.joint_bindings+=b.len();}
        let remaining=sinks.iter().enumerate().filter(|(i,_)|!claimed.contains(i)).map(|(i,s)|(i,*s)).collect::<Vec<_>>();
        if remaining.iter().any(|(_,s)|self.ridge.lib.plans.iter().any(|p|p.name==s.name && p.ruleset==set)) {
            let subset=remaining.iter().map(|(_,s)|*s).collect::<Vec<_>>();
            for (id,b) in self.ridge.query_cells(eg,set,&subset)? {
                let original=remaining.get(id).ok_or("foreign ridge query")?.0;
                if !claimed.insert(original){return Err("duplicate query provider".into());}
                self.ridge_bindings+=b.len();answers.push((original,b));
            }
        }
        Ok(answers)
    }
    fn handles(&self,s:&Schedule)->bool{self.joint.handles(s)}
    fn try_run(&mut self,eg:&mut EGraph,set:&str)->Result<Option<egglog_reports::RunReport>>{self.joint.try_run(eg,set)}
    fn try_boundary(&mut self,eg:&mut EGraph,s:&Schedule)->Result<Option<Json>>{self.joint.try_boundary(eg,s)}
    fn after_boundary(&mut self,eg:&mut EGraph,s:&Schedule)->Result<()>{self.joint.after_boundary(eg,s)}
    fn take_commits(&mut self,eg:&mut EGraph)->Result<(Vec<String>,Vec<CellCommitBatch>)>{
        let (jn,jb)=self.joint.take_commits(eg)?;
        let rn=self.ridge.names.clone();self.mappings(&jn,&rn);
        if self.ridge.feed.is_some(){
            for b in &jb{let selected=projected(b,&self.joint_to_ridge);self.forwarded_rows+=selected.rows.len();self.ridge.ingest(selected,false);}
        }
        // Before first use, the ridge view bootstraps from current native
        // rows, so discarded pre-bootstrap forwarding is already represented.
        let (rn,rb)=self.ridge.take_commits(eg)?;self.mappings(&jn,&rn);
        let batches=jb.into_iter().map(|b|remap(b,&self.joint_to_outer))
            .chain(rb.into_iter().map(|b|remap(b,&self.ridge_to_outer))).collect();
        Ok((self.names.clone(),batches))
    }
    fn diagnostics(&self)->Json{
        let mut d=self.ridge.diagnostics();d["joint_service"]=self.joint.diagnostics();
        d["combined_services"]=json!({"forwarded_overlapping_rows":self.forwarded_rows,"joint_bindings":self.joint_bindings,"ridge_bindings":self.ridge_bindings,
            "scope":"Existing joint/loop providers and ridge completion coexist. A query has one owner; shared journal rows fan out before ordinary GPU consumption."});d
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use egglog::cell_events::CellRowDelta;
    #[test]
    fn forwarding_preserves_removal_visibility_stamp_and_clears_without_duplicate_unions(){
        let b=CellCommitBatch{rows:vec![CellRowDelta{table:0,key:vec![Value::new_const(1)],row:None,visible:false,timestamp:None},
            CellRowDelta{table:1,key:vec![],row:Some(vec![Value::new_const(2)]),visible:true,timestamp:Some(9)}],clears:vec![0,1],unions:vec![(Value::new_const(1),Value::new_const(2))]};
        let out=projected(&b,&[Some(3),None]);assert_eq!(out.rows.len(),1);assert_eq!(out.rows[0].table,3);assert!(out.rows[0].row.is_none());assert_eq!(out.clears,vec![3]);assert!(out.unions.is_empty());
        let out=remap(b,&[Some(2),Some(0)]);assert_eq!(out.rows[1].timestamp,Some(9));assert!(out.rows[1].visible);assert_eq!(out.rows[1].table,0);
    }
}
