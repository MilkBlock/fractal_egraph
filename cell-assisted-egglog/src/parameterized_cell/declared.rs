//! Offline-discovered closure roles and fixed wiring carried by executable source.
//! Attachment validates the declaration; it never searches for another scheme.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Closures {
    pub selection: Option<contract::Selection>,
    pub scalars: Vec<scalar::Descriptor>,
    pub programs: Vec<artifact::Program>,
}
impl Closures {
    pub fn discover(eg: EGraph, commands: &[Command]) -> Result<Option<Self>> {
        let selection = contract::Selection::discover(&eg, commands).ok();
        let excluded = selection.as_ref().map(|s| s.sets.to_vec()).unwrap_or_default();
        let scalar = scalar::discover(&eg, commands, &excluded);
        if selection.is_none() && scalar.is_empty() { return Ok(None); }
        let scalars = scalar.iter().map(scalar::Spec::descriptor).collect();
        let r = Runtime::declared_engine(eg, commands, selection.clone(), scalar, None, &[])?;
        let programs = r.kernels.iter().enumerate().map(|(i,k)| {
            let set = r.sets.iter().find_map(|(s,ids)| ids.contains(&i).then_some(s))
                .or_else(|| r.scalar.iter().find_map(|(s,g)|g.plans.iter().any(|(id,_)|*id==i).then_some(s)))
                .expect("closure kernel has an owner");
            artifact::Program::from_kernel(set, k)
        }).collect();
        Ok(Some(Self { selection, scalars, programs }))
    }
    pub fn protected_queries(&self) -> BTreeSet<(String,String)> {
        self.programs.iter().map(|p|(p.ruleset.clone(),p.rule.clone())).collect()
    }
    pub fn attach(&self, eg:EGraph, commands:&[Command], queries:&[boundary::Spec]) -> Result<Runtime> {
        let ids=self.protected_queries();
        if self.programs.is_empty() || ids.len()!=self.programs.len() {
            return Err("missing or duplicate declared closure program".into());
        }
        let excluded=self.selection.as_ref().map(|s|s.sets.to_vec()).unwrap_or_default();
        let scalars=scalar::restore(&eg,commands,&excluded,&self.scalars)?;
        Runtime::declared_engine(eg,commands,self.selection.clone(),scalars,Some(&self.programs),queries)
    }
}
