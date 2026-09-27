#[cfg(feature="mi")]
#[global_allocator]
static ALLOCATOR:mimalloc::MiMalloc=mimalloc::MiMalloc;
// Identical public-API harness for pristine egglog and this checkout. No tracing.
use egglog::EGraph;
use egglog_numeric_id::NumericId;
use serde_json::json;
use std::{time::Instant,io::Write,collections::BTreeSet};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let a=std::env::args().skip(1).collect::<Vec<_>>();
 let text=std::fs::read_to_string(&a[0])?;let start=Instant::now();let mut eg=EGraph::default();
 let outputs=eg.parse_and_run_program(Some(a[0].clone()),&text)?;
 let seconds=start.elapsed().as_secs_f64();
 let matches=outputs.iter().filter_map(|o|if let egglog::CommandOutput::RunSchedule(r)=o{Some(r.num_matches_per_rule.values().sum::<usize>())}else{None}).sum::<usize>();
 if let Some(path)=a.get(1){
  let mut out=std::io::BufWriter::new(std::fs::File::create(path)?);write!(out,"[")?;let mut comma=false;let mut nodes=0;let mut classes=BTreeSet::new();
  for name in eg.get_function_names(){let f=eg.get_function(&name).unwrap();if f.schema().output.name()!="Math"{continue;}
   eg.function_for_each(&name,|r|{
    let args=r.vals.iter().zip(&f.schema().input).map(|(v,t)|match t.name(){"Math"=>json!(["Math",eg.get_canonical_value(*v,t).rep()]),"i64"=>json!(["i64",eg.value_to_base::<i64>(*v).to_string()]),"String"=>json!(["String",eg.value_to_base::<egglog::sort::S>(*v).to_string()]),_=>panic!("unsupported benchmark sort")}).collect::<Vec<_>>();
    let result=eg.get_canonical_value(*r.vals.last().unwrap(),&f.schema().output).rep();classes.insert(result);nodes+=1;
    if comma{write!(out,",").unwrap();}comma=true;serde_json::to_writer(&mut out,&json!({"op":name,"args":args,"result":result})).unwrap();
   })?;
  }write!(out,"]")?;out.flush()?;println!("{}",json!({"seconds":seconds,"matches":matches,"enodes":nodes,"eclasses":classes.len(),"verification":true}));
 }else{drop(outputs);drop(eg);println!("{}",json!({"seconds":seconds,"through_drop_seconds":start.elapsed().as_secs_f64(),"matches":matches,"verification":false}));}
 Ok(())
}
