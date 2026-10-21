//! Grounded action receipts -> candidate spine witnesses. No native query or
//! graph scan. Receipts remain untrusted until the post-commit row directory and
//! then the device's generations/typed wiring agree.
use crate::Result;
use egglog::{Value, cell_assist::{CellOperand,CellQueryAtom,CellActionSink}};
use serde::Serialize;
use serde_json::Value as Json;
use std::collections::{BTreeMap,BTreeSet};
#[derive(Default,Serialize)]
pub(crate) struct Stats {
    pub publisher_rules:usize, pub publisher_recipes:usize,
    pub applications:usize, pub receipt_rows:usize, pub directory_resolutions:usize,
    pub rejected_rows:usize, pub prefix_extensions:usize, pub emitted_hints:usize,
    pub uploaded_words:usize, pub seconds:f64,
}
pub(crate) struct GroundRow {pub table:usize,pub slot:u32,pub generation:u32,pub values:Vec<Value>}
struct Plan {id:u32,atoms:Vec<(usize,CellQueryAtom)>}
struct Recipe {plan:usize,anchor:usize,steps:Vec<Vec<usize>>}
pub(crate) struct Compiler {
    plans:Vec<Plan>, recipes:Vec<Vec<Recipe>>,
    pending:BTreeMap<(u32,u32),Vec<u32>>, pub stats:Stats,
}
impl Compiler {
    pub fn new(config:&Json,sinks:&[CellActionSink],tables:&[String])->Result<Self>{
        let mut plans=vec![];
        for p in config["cell_evidence"].as_array().ok_or("marker plan metadata")? {
            let q=&sinks[p["query"].as_u64().ok_or("marker query")? as usize];
            let atoms=p["atoms"].as_array().ok_or("marker atoms")?.iter().map(|i| {
                let a=q.atoms[i.as_u64().unwrap() as usize].clone();
                (tables.iter().position(|n|*n==a.function).unwrap(),a)
            }).collect::<Vec<_>>();
            plans.push(Plan{id:p["plan"].as_u64().unwrap() as u32,atoms});
        }
        let recipes=sinks.iter().map(|sink| {
            let mut out=vec![];
            if let Some(tap)=&sink.receipts {
                for (pi,p) in plans.iter().enumerate(){
                    let positions=p.atoms.iter().map(|(_,atom)|tap.rows.iter().enumerate().filter_map(|(i,(name,types))|
                        (*name==atom.function && *types==atom.sorts).then_some(i)).collect::<Vec<_>>()).collect::<Vec<_>>();
                    if positions.len()<2 || positions[1].is_empty(){continue}
                    for anchor in &positions[0]{out.push(Recipe{plan:pi,anchor:*anchor,steps:positions[1..].to_vec()});}
                }
                tap.set_enabled(!out.is_empty());
            }
            out
        }).collect::<Vec<_>>();
        let stats=Stats{publisher_rules:recipes.iter().filter(|r|!r.is_empty()).count(),publisher_recipes:recipes.iter().map(Vec::len).sum(),..Default::default()};
        Ok(Self{plans,recipes,pending:BTreeMap::new(),stats})
    }
    /// Programs address fixed positions in this one application, not the graph.
    pub fn observe(&mut self,rule:usize,rows:Vec<Option<GroundRow>>){
        let mut seen=BTreeSet::new();
        for recipe in &self.recipes[rule] {
            let plan=&self.plans[recipe.plan];let Some(row)=&rows[recipe.anchor] else{continue};
            let mut binding=BTreeMap::new();if !bind(&plan.atoms[0].1,&row.values,&mut binding){continue}
            let mut witnesses=vec![];
            for ((table,atom),positions) in plan.atoms[1..].iter().zip(&recipe.steps) {
                let mut selected=None;
                for pos in positions {
                    let Some(next)=&rows[*pos] else{continue};if next.table!=*table {continue}
                    let mut extended=binding.clone();
                    if bind(atom,&next.values,&mut extended){selected=Some((next,extended));break}
                }
                let Some((next,extended))=selected else{break};binding=extended;
                witnesses.extend([next.slot,next.generation]);self.stats.prefix_extensions+=1;
            }
            if !witnesses.is_empty() && seen.insert((plan.id,row.slot,witnesses.clone())) {
                let mut hint=vec![plan.id,row.slot,row.generation,(witnesses.len()/2) as u32];hint.extend(witnesses);
                let entry=self.pending.entry((row.slot,plan.id)).or_default();
                if hint.len()>=entry.len(){*entry=hint;}
            }
        }
    }
    pub fn drain(&mut self,parts:usize)->Vec<u32>{
        let pending=std::mem::take(&mut self.pending);let mut words=vec![pending.len() as u32];
        for (_,mut hint) in pending {hint.resize(4+2*parts,u32::MAX);words.extend(hint);self.stats.emitted_hints+=1;}
        self.stats.uploaded_words+=words.len();words
    }
}
fn value(o:&CellOperand,b:&BTreeMap<usize,Value>)->Option<Value>{match o{CellOperand::Slot(s)=>b.get(s).copied(),CellOperand::Constant{value,..}=>Some(*value)}}
fn bind(a:&CellQueryAtom,row:&[Value],b:&mut BTreeMap<usize,Value>)->bool{
    if a.columns.len()!=row.len(){return false}
    for (o,v) in a.columns.iter().zip(row){if let Some(old)=value(o,b){if old!=*v{return false}}else if let CellOperand::Slot(s)=o{b.insert(*s,*v);}}
    true
}
