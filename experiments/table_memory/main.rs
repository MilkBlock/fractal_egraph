use egglog::EGraph;
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};
fn snapshot(eg:&mut EGraph)->Value {
 let mut schema=BTreeMap::new();let mut classes=BTreeSet::new();let mut enodes=0;
 for name in eg.get_function_names(){let f=eg.get_function(&name).unwrap();let mut count=0;
  let math=f.schema().output.name()=="Math" && !f.is_let_binding();
  eg.function_for_each(&name,|r|{count+=1;if math{enodes+=1;classes.insert(eg.value_to_class_id(&f.schema().output,*r.vals.last().unwrap()).to_string());}}).unwrap();
  schema.insert(name,json!({"visible_rows":count,"input_sorts":f.schema().input.iter().map(|s|s.name()).collect::<Vec<_>>(),"output_sort":f.schema().output.name()}));
 }
 let mut shared_global=BTreeMap::new();let mut tables=vec![];let mut local_total=0;
 for (name,fields,shared) in eg.audit_tables(){
  let metrics=fields.into_iter().collect::<BTreeMap<_,_>>();
  let local:usize=metrics.iter().filter(|(k,_)|k.ends_with("_bytes")).map(|(_,v)|*v).sum();
  let referenced:usize=shared.iter().map(|(_,_,b)|*b).sum();
  for (id,kind,bytes) in shared{if let Some(old)=shared_global.insert((kind,id),bytes){assert_eq!(old,bytes);}}
  local_total+=local;
  tables.push(json!({"name":name,"schema":schema.get(&name),"metrics":metrics,"local_known_bytes":local,"shared_referenced_bytes":referenced,"inclusive_known_bytes":local+referenced}));
 }
 let mut kinds=BTreeMap::new();for ((kind,_),bytes) in &shared_global{*kinds.entry(*kind).or_insert(0usize)+=*bytes;}
 json!({"math_enodes":enodes,"math_eclasses":classes.len(),"tables":tables,"shared_pool_bytes":kinds,"local_known_bytes":local_total,"total_known_unique_bytes":local_total+shared_global.values().sum::<usize>(),"unmapped_functions":schema.keys().filter(|n|!tables.iter().any(|t|t["name"]==n.as_str())).collect::<Vec<_>>()})
}
#[cfg(feature="pack")]
fn pack(eg:&mut EGraph, file:&str)->Result<Value,Box<dyn std::error::Error>>{
 use egglog::{VirtualFactRow,ast::{Expr,Parser}};
 let catalog:Value=serde_json::from_slice(&std::fs::read(file)?)?;let mut outcomes=vec![];
 for (id,entry) in catalog.as_array().unwrap().iter().enumerate(){
  let attempt=(||->Result<Value,Box<dyn std::error::Error>>{
   let q=&entry[0];let arities=entry[1].as_array().unwrap();let mut parser=Parser::default();
   let values=q["values"].as_array().unwrap().iter().map(|v|->Result<_,Box<dyn std::error::Error>>{
    Ok((v[0].as_str().unwrap().to_owned(),if v[1].is_null(){None}else{match parser.get_expr_from_string(None,v[1].as_str().unwrap())?{Expr::Lit(_,v)=>Some(v),_=>return Err("nonliteral".into())}}))
   }).collect::<Result<Vec<_>,_>>()?;
   let atoms:Vec<(String,Vec<usize>)>=serde_json::from_value(q["atoms"].clone())?;
   let projection:Vec<usize>=serde_json::from_value(q["projection"].clone())?;
   let query=eg.compile_fact_query(&values,&atoms,&projection)?;let found=eg.run_fact_query(&query);eg.free_fact_query(query);
   let found=found?.into_iter().collect::<BTreeSet<_>>();if found.is_empty(){return Ok(json!({"pattern":id,"matches":0}));}
   let slots=projection.iter().enumerate().map(|(i,v)|(*v,i)).collect::<BTreeMap<_,_>>();let mut next=slots.len();let mut missing=vec![];let mut rows=vec![];
   for ((op,cols),arity) in atoms.iter().zip(arities){let mut c=cols.iter().map(|v|slots[v]).collect::<Vec<_>>();if cols.len()==arity.as_u64().unwrap() as usize{missing.push((op.clone(),c.clone()));c.push(next);next+=1;}rows.push(VirtualFactRow{function:op.clone(),columns:c});}
   let template=eg.virtual_fact_template(rows)?;let mut bindings=vec![];
   for mut b in found{for (op,cols) in &missing{let key=cols.iter().map(|i|b[*i]).collect::<Vec<_>>();b.push(eg.lookup_function(op,&key).ok_or("row vanished")?);}bindings.push(b);}
   eg.virtualize_existing_facts(&template,&bindings)?;Ok(json!({"pattern":id,"matches":bindings.len()}))
  })();outcomes.push(attempt.unwrap_or_else(|e|json!({"pattern":id,"rejected":e.to_string()})));
 }Ok(json!(outcomes))
}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args=std::env::args().skip(1).collect::<Vec<_>>();let mut eg=EGraph::default();
 eg.parse_and_run_program(Some(args[0].clone()),&std::fs::read_to_string(&args[0])?)?;
 let before=snapshot(&mut eg);
 #[cfg(feature="pack")] let packing=if let Some(plans)=args.get(1){pack(&mut eg,plans)?}else{Value::Null};
 #[cfg(not(feature="pack"))] let packing=Value::Null;
 let after=snapshot(&mut eg);
 assert_eq!(before["math_enodes"],after["math_enodes"]);assert_eq!(before["math_eclasses"],after["math_eclasses"]);
 println!("{}",serde_json::to_string_pretty(&json!({"before":before,"after":after,"packing":packing}))?);Ok(())
}
