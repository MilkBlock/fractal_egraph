// Transfer ranked leaves by verified rule signature; remap reachable binary units.
use eggfuse_cell_runtime::{checksum::artifact_digest, compiler::bake_spine_blocks, iterative::Catalog, program::Program};
use serde_json::{Value as J,json};
use std::{collections::BTreeMap,env,fs};
fn main()->Result<(),Box<dyn std::error::Error>> {
    let args=env::args().skip(1).collect::<Vec<_>>();
    let [source,target,out]=args.as_slice() else {return Err("expected donor target new-output".into())};
    let load=|p:&str|->Result<Program,Box<dyn std::error::Error>>{Ok(Program::parse(serde_json::from_slice(&fs::read(p)?)?)?)};
    let donor=load(source)?; let target=load(target)?;
    let entries=donor.artifact["spine_blocks"]["entries"].as_array().ok_or("missing leaves")?;
    let prefixes=entries.iter().map(|e|json!({"edge":[0,0,e["consumer"],e["pivot"]],"visits":e["training_visits"]})).collect::<Vec<_>>();
    let profile=json!({"schema":"eggfuse-spine-shapes/v1","program_sha256":donor.artifact["sha256"],"prefixes":prefixes});
    let leaves=bake_spine_blocks(&donor,&profile,&target,entries.len())?;
    let bound=leaves["entries"].as_array().unwrap();
    let mut ids=BTreeMap::new();
    for (id,e) in bound.iter().enumerate() {
        let original=entries.iter().position(|o|o["consumer"]==e["training_rule"] && o["pivot"]==e["pivot"]).unwrap();
        ids.insert(original,id);
    }
    let mut raw=target.artifact;
    let mut count=0;
    if !bound.is_empty() {
        let n=bound.len(); raw["spine_blocks"]=leaves;
        let p=Program::parse(raw)?;
        let mut plan=Catalog::seed(&p,serde_json::from_value(donor.artifact["iterative_bake"]["limits"].clone())?)?;
        let mut joins=Vec::new();
        for (i,j) in donor.artifact["iterative_bake"]["joins"].as_array().unwrap().iter().enumerate(){
            let get=|key:&str|ids.get(&(j[key].as_u64()? as usize)).copied();
            if let (Some(l),Some(r))=(get("left"),get("right")){
                ids.insert(entries.len()+i,n+joins.len());joins.push(json!({"left":l,"right":r,"output":j["output"]}));
            }
        }
        count=n+joins.len();plan["joins"]=json!(joins);Catalog::emit_handoffs(&p,&mut plan)?;
        raw=p.artifact;raw["iterative_bake"]=plan;
    }
    raw.as_object_mut().unwrap().remove("sha256");raw["sha256"]=artifact_digest(&raw).into();
    let checked=Program::parse(raw)?;fs::write(out,serde_json::to_vec(&checked.artifact)?)?;println!("{count}");Ok(())
}
