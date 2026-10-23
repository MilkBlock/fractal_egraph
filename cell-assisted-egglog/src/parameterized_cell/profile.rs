//! Offline, read-only plan trials at original native Run frontiers.
//! No trial executes an RHS. Counters are compiled out of the normal runtime.
use super::*;
use egglog::cell_assist::CellQuery;
use std::io::Write;

#[derive(Clone,Default,Serialize)]
pub(super) struct StageWork { entries:u64, rows:u64, prefixes:u64, lookups:u64, fresh_prefixes:u64 }
impl Kernel {
    pub(super) fn stage(&self,step:usize,field:usize) {
        let mut work=self.stage_work.borrow_mut();
        if let Some(w)=work.get_mut(step) {
            match field {0=>w.entries+=1,1=>w.rows+=1,2=>w.prefixes+=1,3=>w.lookups+=1,4=>w.fresh_prefixes+=1,_=>unreachable!()}
        }
    }
}
struct QueryTrial { set:String, name:String, reference:CellQuery, variants:Vec<usize> }
pub struct Profiler {
    runtime:Runtime,
    queries:Vec<QueryTrial>,
    combined:BTreeMap<String,Vec<String>>,
    plans:Vec<Json>,
    pub header:Json,
    round:usize,
    live_prefixes:Vec<FxHashMap<Key,Vec<u64>>>,
    prefix_totals:Vec<Vec<u64>>,
}
impl Profiler {
    pub fn new(mut eg:EGraph,commands:&[Command])->Result<Self> {
        let sets=commands.iter().filter_map(|c|if let Command::AddRuleset(_,s)=c {Some(s.clone())} else {None})
            .chain(std::iter::once(String::new())).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
        let combined=commands.iter().filter_map(|c|if let Command::UnstableCombinedRuleset(_,s,children)=c {Some((s.clone(),children.clone()))}else{None}).collect();
        let report=boundary::inspect(&mut eg,&sets,&BTreeSet::new())?;
        let mut admitted=vec![];let mut rejected=vec![];
        for q in report["queries"].as_array().unwrap() {
            let valid=q.get("plans").is_some() && q["shape"]["atoms"].as_array().unwrap().iter().all(|a|
                a["sorts"].as_array().unwrap().iter().all(|s|eg.cell_sort_is_equality(s.as_str().unwrap()).is_ok()));
            if valid {admitted.push(q.clone());}else{rejected.push(q.clone());}
        }
        let primary=admitted.iter().map(|q|{
            let plan:boundary::Plan=serde_json::from_value(q["plans"][0].clone()).unwrap();
            boundary::Spec{ruleset:q["ruleset"].as_str().unwrap().into(),rule:q["rule"].as_str().unwrap().into(),
                root:q["shape"]["atoms"][plan.seed]["table"].as_str().unwrap().into(),plan:Some(plan)}
        }).collect::<Vec<_>>();
        let mut r=Runtime::new_with_boundaries(eg,&[],&BTreeMap::new(),vec![],None,&primary)?;
        // Reuse the one committed-table mirror. Each alternative owns only its
        // own subscriptions; common prefixes share the existing prefix DAG.
        r.kernels.clear();let mut plans=vec![];let mut queries=vec![];
        for (query,q) in admitted.iter().enumerate() {
            let set=q["ruleset"].as_str().unwrap();let name=q["rule"].as_str().unwrap();
            let sink=boundary::prepare(&mut r.eg,set,name)?;
            let reference=r.eg.prepare_cell_query(&sink)?;
            let mut variants=vec![];
            for raw in q["plans"].as_array().unwrap() {
                let plan:boundary::Plan=serde_json::from_value(raw.clone())?;
                for shared in [false] {
                    let sink=boundary::prepare(&mut r.eg,set,name)?;
                    let tables=sink.atoms.iter().map(|a|r.names.iter().position(|n|n==&a.function).unwrap()).collect();
                    let mut k=boundary::compile_seed(&r.eg,sink,tables,plan.seed)?;
                    if shared {k.payload_layout=Some((k.steps.len(),(0..k.slot_count).collect()));k.share_prefix=true;}
                    else {*k.stage_work.borrow_mut()=vec![StageWork::default();k.steps.len()];}
                    let prefixes=(0..=k.steps.len()).map(|d|shared_marker::normalize(&k,d).0).collect::<Vec<_>>();
                    variants.push(r.kernels.len());
                    plans.push(json!({"id":r.kernels.len(),"query":query,"shared":shared,"plan":raw,"prefix_keys":prefixes}));
                    r.kernels.push(k);
                }
            }
            queries.push(QueryTrial{set:set.into(),name:name.into(),reference,variants});
        }
        r.wake_tables=vec![vec![];r.names.len()];
        let mut reverse=BTreeSet::new();let mut projections=BTreeSet::new();
        for (id,k) in r.kernels.iter().enumerate() {
            for t in k.tables.iter().copied().collect::<BTreeSet<_>>() {r.wake_tables[t].push(id);}
            for (i,access) in &k.steps {
                match access {Access::Output=>{reverse.insert(k.tables[*i]);},Access::Columns(mask)=>{projections.insert((k.tables[*i],*mask));},_=>{}}
            }
        }
        r.rows=Rows{reverse,projections,..Default::default()};
        (r.shared_markers,r.marker_watch)=shared_marker::build(&mut r.kernels);
        let (feed,initial)=r.eg.subscribe_cell_commits(&r.names)?;r.feed=feed;r.ingest(initial)?;
        let live_prefixes=(0..r.kernels.len()).map(|_|FxHashMap::default()).collect();
        let prefix_totals=r.kernels.iter().map(|k|vec![0;k.steps.len()+1]).collect();
        let header=json!({"schema":"marker-round-profile/v1","markers":r.shared_markers.iter().map(|m|m.report()).collect::<Vec<_>>(),"queries":admitted,"rejected":rejected,"plans":plans,"tables":r.names,
            "scope":"read-only alternative CPU query plans; streaming prefix census retains counts per root, never intermediate bindings; native RHS and original Run boundaries; not timing or GPU credit",
            "fields":{"markers":["lookups","candidate_rows","root_refreshes","cache_hits","invalidations","published_bindings","decoded_rows","live_bindings","cached_roots","subscribed_keys"],"plans":["id","root_evaluations","index_reads","candidate_rows","bindings","shared_prefix_uses"],
                "stages":["entries","rows","prefixes","lookups","fresh_prefixes"]}});
        Ok(Self{runtime:r,queries,combined,plans,header,round:0,live_prefixes,prefix_totals})
    }
    fn members(&self,set:&str)->BTreeSet<String> {
        let mut out=BTreeSet::from([set.to_owned()]);
        loop {let before=out.len();for s in out.clone(){if let Some(xs)=self.combined.get(&s){out.extend(xs.iter().cloned());}}
            if before==out.len(){return out}}
    }
    pub fn before_run(&mut self,set:&str,path:&str)->Result<Json> {
        let r=&mut self.runtime;let batch=r.eg.drain_cell_commits(&mut r.feed)?;
        let mut updates=vec![0usize;r.names.len()];for row in &batch.rows{updates[row.table]+=1;}
        let unions=batch.unions.len();r.ingest(batch)?;
        let tables=r.names.iter().enumerate().map(|(i,_)|json!([i,r.rows.keys.table(i).count(),updates[i]])).collect::<Vec<_>>();
        let members=self.members(set);let mut rows=vec![];let mut stages=vec![];
        for q in &self.queries {
            if !members.contains(&q.set){continue}
            let r=&mut self.runtime;
            let mut oracle=r.eg.run_cell_queries(&[&q.reference])?.pop().unwrap();oracle.sort_unstable();oracle.dedup();
            for id in &q.variants {
                let dirty=r.kernels[*id].dirty.iter().cloned().collect::<Vec<_>>();
                let k=&r.kernels[*id];let before=[k.executions,k.index_reads,k.rows_examined.get(),k.bindings,k.shared_prefix_uses];
                let mut answer=r.boundary_matches(*id)?;answer.sort_unstable();answer.dedup();
                if answer!=oracle {return Err(format!("plan {} diverged at {} Run {}: {}",id,path,self.round,q.name).into())}
                let k=&r.kernels[*id];let after=[k.executions,k.index_reads,k.rows_examined.get(),k.bindings,k.shared_prefix_uses];
                let mut values=vec![*id];values.extend(after.into_iter().zip(before).map(|(a,b)|a-b));rows.push(values);
                let steps=std::mem::replace(&mut *k.stage_work.borrow_mut(),vec![StageWork::default();k.steps.len()]);
                k.census.set(true);
                let result=(||->Result<()>{
                    for root in dirty {
                        if let Some(old)=self.live_prefixes[*id].remove(&root){for(t,n)in self.prefix_totals[*id].iter_mut().zip(old){*t-=n;}}
                        let Some(row)=r.rows.keys.get(&(k.tables[k.seed],root.clone()))else{continue};
                        let mut b=smallvec![None;k.slot_count];
                        if !unify(&k.sink.atoms[k.seed].columns,&row.values,&mut b){continue}
                        let prior=k.stage_work.borrow().iter().map(|s|s.prefixes).collect::<Vec<_>>();
                        k.read_program(&r.eg,&r.rows,0,b,row.stamp,r.eg.cell_query_watermark(&k.sink),&mut Reads::new(),&mut vec![],&mut 0,&mut 0)?;
                        let now=k.stage_work.borrow();let mut counts=vec![1];
                        counts.extend(now.iter().zip(prior).map(|(s,n)|s.prefixes-n));
                        for(t,n)in self.prefix_totals[*id].iter_mut().zip(&counts){*t+=n;}
                        self.live_prefixes[*id].insert(root,counts);
                    }Ok(())})();
                k.census.set(false);result?;
                let census=std::mem::replace(&mut *k.stage_work.borrow_mut(),vec![StageWork::default();k.steps.len()]);
                stages.push(json!({"plan":id,"steps":steps,"prefix_census":census,"live_prefixes":self.prefix_totals[*id]}));
            }
        }
        self.round+=1;
        Ok(json!({"round":self.round,"path":path,"ruleset":set,"tables":tables,"unions":unions,"plans":rows,"stages":stages,"verified":true}))
    }
    pub fn schedule(&mut self,s:&Schedule,path:&str,out:&mut impl Write)->Result<bool> {
        match s {
            GenericSchedule::Run(_,run)=>{
                if run.until.is_none(){let row=self.before_run(&run.ruleset,path)?;serde_json::to_writer(&mut *out,&row)?;writeln!(out)?;}
                else {serde_json::to_writer(&mut *out,&json!({"path":path,"unmeasured":"native until boundary","ruleset":run.ruleset}))?;writeln!(out)?;}
                let outputs=self.runtime.eg.run_program(vec![Command::RunSchedule(s.clone())])?;
                for q in &self.queries {for id in q.variants.first(){self.runtime.eg.sync_cell_query_after_native_run(&self.runtime.kernels[*id].sink,&q.reference);}}
                Ok(outputs.into_iter().any(|o|matches!(o,egglog::CommandOutput::RunSchedule(r) if r.updated)))
            }
            GenericSchedule::Sequence(_,xs)=>{let mut change=false;for(i,x)in xs.iter().enumerate(){change|=self.schedule(x,&format!("{path}/{i}"),out)?;}Ok(change)}
            GenericSchedule::Repeat(_,n,x)=>{let mut any=false;for i in 0..*n {let c=self.schedule(x,&format!("{path}/repeat{i}"),out)?;any|=c;if !c{break}}Ok(any)}
            GenericSchedule::Saturate(_,x)=>{let mut any=false;for i in 0usize.. {let c=self.schedule(x,&format!("{path}/sat{i}"),out)?;any|=c;if !c{break}}Ok(any)}
        }
    }
    pub fn input(&mut self,commands:Vec<Command>)->Result<()> {self.runtime.eg.run_program(commands)?;Ok(())}
    pub fn engine(&self)->&EGraph{&self.runtime.eg}
    pub fn into_engine(self)->EGraph{self.runtime.eg}
}
