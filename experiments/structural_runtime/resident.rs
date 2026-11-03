//! Sparse updates to persistent typed context rows. Opaque native identities
//! select rows; the ridge model sees only the existing Analyze feature tokens.
use super::{Library, Map, Result};
use std::collections::{BTreeMap, BTreeSet};
/// Counts actual anchor-to-condition connections, not every row x every plan.
/// Removing one publisher does not remove other consumers of the same context.
pub struct Scopes {
    model_sorts:Vec<u32>,
    live:Map<(u32,u32),BTreeMap<usize,usize>>,
}
impl Scopes {
    pub fn new(lib:&Library)->Self{Self{model_sorts:Client::new(lib).model_sorts,live:Map::default()}}
    pub fn change(&mut self,model:usize,key:u32,add:bool){
        let node=(self.model_sorts[model],key);
        if add{*self.live.entry(node).or_default().entry(model).or_default()+=1;}
        else {
            let heads=self.live.get_mut(&node).expect("missing consumer context");
            let n=heads.get_mut(&model).expect("missing consumer head");
            *n-=1;if *n==0{heads.remove(&model);}
            if heads.is_empty(){self.live.remove(&node);}
        }
    }
    pub fn heads(&self,model:usize,key:u32)->Vec<usize>{
        // Direct demand is always included, even before a new subscriber has
        // been published. Scope is scheduling, never a negative matching proof.
        let mut heads=self.live.get(&(self.model_sorts[model],key)).map(|h|h.keys().copied().collect::<BTreeSet<_>>()).unwrap_or_default();
        heads.insert(model);heads.into_iter().collect()
    }
    pub fn report(&self)->serde_json::Value{
        let mut widths=BTreeMap::<usize,usize>::new();
        for heads in self.live.values(){*widths.entry(heads.len()).or_default()+=1;}
        serde_json::json!({"live_contexts":self.live.len(),"live_condition_connections":self.live.values().map(|h|h.len()).sum::<usize>(),
            "anchor_supports":self.live.values().flat_map(|h|h.values()).sum::<usize>(),"head_count_histogram":widths,
            "scope":"Actual published anchor connections; not Cell coverage or exact predicate truth"})
    }
}
#[derive(Clone)]
struct Context { row:u32, version:u32, tokens:BTreeSet<u32> }
pub struct Client {
    contexts:Map<(u32,u32),Context>,
    model_sorts:Vec<u32>,
    vocabulary:u32,
}
impl Client {
    pub fn new(lib:&Library)->Self {
        let sorts=lib.models.iter().map(|m|m.sort.clone()).collect::<BTreeSet<_>>().into_iter().enumerate().map(|(i,s)|(s,i as u32)).collect::<BTreeMap<_,_>>();
        Self{contexts:Map::default(),model_sorts:lib.models.iter().map(|m|sorts[&m.sort]).collect(),vocabulary:lib.vocabulary.len() as u32}
    }
    /// Several condition heads consume one row/version. Only changed token
    /// assignments cross the worker boundary; unchanged rows send no features.
    pub fn packet(&mut self,requests:&[(usize,u32,Vec<usize>,Vec<usize>)])->Result<Vec<u8>> {
        // Validate the complete frozen batch before changing the sender ledger.
        let mut batch=BTreeMap::new();
        let mut scopes=BTreeMap::<(u32,u32),BTreeSet<usize>>::new();
        for (model,key,features,heads) in requests {
            let sort=*self.model_sorts.get(*model).ok_or("unknown resident model")?;
            if heads.iter().any(|m|self.model_sorts.get(*m)!=Some(&sort)){return Err("mistyped consumer head".into());}
            if !heads.is_empty(){let scope=scopes.entry((sort,*key)).or_default();scope.extend(heads);scope.insert(*model);}
            if features.iter().any(|i|*i>=self.vocabulary as usize){return Err("unknown feature token".into());}
            let mut tokens=features.iter().map(|i|*i as u32).collect::<BTreeSet<_>>();
            if tokens.len()!=features.len(){return Err("duplicate feature token".into());}
            tokens.insert(self.vocabulary+sort);
            if let Some(old)=batch.insert((sort,*key),tokens.clone()) {
                if old!=tokens{return Err("one context has conflicting features in a frozen batch".into());}
            }
        }
        let mut patches=BTreeMap::<u32,(u32,u32,Vec<(u32,u32)>)>::new();
        for ((sort,key),tokens) in batch {
            let next=u32::try_from(self.contexts.len())?;
            let ctx=self.contexts.entry((sort,key)).or_insert(Context{row:next,version:0,tokens:BTreeSet::new()});
            if ctx.tokens!=tokens {
                ctx.version=ctx.version.checked_add(1).ok_or("context version exhausted")?;
                let edits=ctx.tokens.difference(&tokens).map(|i|(*i,0)).chain(tokens.difference(&ctx.tokens).map(|i|(*i,1))).collect();
                ctx.tokens=tokens;
                patches.insert(ctx.row,(ctx.version,sort,edits));
            }
        }
        let routes=requests.iter().map(|(model,key,_,_)| {
            let c=&self.contexts[&(self.model_sorts[*model],*key)];
            (c.row,*model as u32,c.version)
        }).collect::<Vec<_>>();
        let mut words=vec![u32::try_from(routes.len())?,u32::try_from(self.contexts.len())?,u32::try_from(patches.len())?];
        for (row,(version,sort,edits)) in patches {
            words.extend([row,version,sort,u32::try_from(edits.len())?]);
            for (feature,on) in edits{words.extend([feature,on]);}
        }
        words.push(u32::try_from(scopes.len())?);
        for (node,heads) in scopes {
            words.extend([self.contexts[&node].row,u32::try_from(heads.len())?]);
            words.extend(heads.into_iter().map(|m|m as u32));
        }
        for (row,model,version) in routes{words.extend([row,model,version]);}
        Ok(words.into_iter().flat_map(u32::to_le_bytes).collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn client()->Client{Client{contexts:Map::default(),model_sorts:vec![0,0,1],vocabulary:4}}
    fn words(b:Vec<u8>)->Vec<u32>{b.chunks_exact(4).map(|x|u32::from_le_bytes(x.try_into().unwrap())).collect()}
    #[test]
    fn two_heads_share_one_version_and_updates_are_sparse(){
        let mut c=client();let b=words(c.packet(&[(0,10,vec![0,1],vec![0,1]),(1,10,vec![0,1],vec![1])]).unwrap());
        assert_eq!(&b[..3],&[2,1,1]);assert_eq!(&b[3..7],&[0,1,0,3]);
        let b=words(c.packet(&[(1,10,vec![0,1],vec![])]).unwrap());assert_eq!(b,vec![1,1,0,0,0,1,1]);
        let b=words(c.packet(&[(0,10,vec![1,2],vec![0])]).unwrap());assert_eq!(&b[3..7],&[0,2,0,2]);assert_eq!(&b[7..11],&[0,0,2,1]);
        let b=words(c.packet(&[(2,10,vec![1,2],vec![2])]).unwrap());assert_eq!(&b[..7],&[1,2,1,1,1,1,3]);
    }
    #[test]
    fn conflicting_contexts_and_unknown_features_are_errors(){
        let mut c=client();assert!(c.packet(&[(0,10,vec![0],vec![]),(1,10,vec![1],vec![])]).is_err());assert!(c.contexts.is_empty());
        assert!(client().packet(&[(0,10,vec![4],vec![])]).is_err());
        assert!(client().packet(&[(0,10,vec![1,1],vec![])]).is_err());
        assert!(client().packet(&[(0,10,vec![1],vec![2])]).is_err());
    }
    #[test]
    fn consumer_scopes_preserve_duplicate_supports_types_and_new_demand(){
        let mut s=Scopes{model_sorts:vec![0,0,1],live:Map::default()};
        s.change(0,7,true);s.change(0,7,true);s.change(1,7,true);s.change(2,7,true);
        assert_eq!(s.heads(0,7),vec![0,1]);assert_eq!(s.heads(2,7),vec![2]);
        s.change(0,7,false);assert_eq!(s.heads(1,7),vec![0,1]);
        s.change(0,7,false);assert_eq!(s.heads(1,7),vec![1]);
        s.change(1,7,false);assert_eq!(s.heads(1,7),vec![1]);
        assert_eq!(s.live.len(),1); // only the other sort remains
    }
}
