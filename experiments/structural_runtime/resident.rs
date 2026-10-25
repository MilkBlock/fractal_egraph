//! Sparse updates to persistent typed context rows. Opaque native identities
//! select rows; the ridge model sees only the existing Analyze feature tokens.
use super::{Library, Map, Result};
use std::collections::{BTreeMap, BTreeSet};
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
    pub fn packet(&mut self,requests:&[(usize,u32,Vec<usize>)])->Result<Vec<u8>> {
        // Validate the complete frozen batch before changing the sender ledger.
        let mut batch=BTreeMap::new();
        for (model,key,features) in requests {
            let sort=*self.model_sorts.get(*model).ok_or("unknown resident model")?;
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
        let routes=requests.iter().map(|(model,key,_)| {
            let c=&self.contexts[&(self.model_sorts[*model],*key)];
            (c.row,*model as u32,c.version)
        }).collect::<Vec<_>>();
        let mut words=vec![u32::try_from(routes.len())?,u32::try_from(self.contexts.len())?,u32::try_from(patches.len())?];
        for (row,(version,sort,edits)) in patches {
            words.extend([row,version,sort,u32::try_from(edits.len())?]);
            for (feature,on) in edits{words.extend([feature,on]);}
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
        let mut c=client();let b=words(c.packet(&[(0,10,vec![0,1]),(1,10,vec![0,1])]).unwrap());
        assert_eq!(&b[..3],&[2,1,1]);assert_eq!(&b[3..7],&[0,1,0,3]);
        let b=words(c.packet(&[(1,10,vec![0,1])]).unwrap());assert_eq!(b,vec![1,1,0,0,1,1]);
        let b=words(c.packet(&[(0,10,vec![1,2])]).unwrap());assert_eq!(&b[3..7],&[0,2,0,2]);assert_eq!(&b[7..11],&[0,0,2,1]);
        let b=words(c.packet(&[(2,10,vec![1,2])]).unwrap());assert_eq!(&b[..7],&[1,2,1,1,1,1,3]);
    }
    #[test]
    fn conflicting_contexts_and_unknown_features_are_errors(){
        let mut c=client();assert!(c.packet(&[(0,10,vec![0]),(1,10,vec![1])]).is_err());assert!(c.contexts.is_empty());
        assert!(client().packet(&[(0,10,vec![4])]).is_err());
        assert!(client().packet(&[(0,10,vec![1,1])]).is_err());
    }
}
