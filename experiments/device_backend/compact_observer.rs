//! Diagnostic table deltas. No matching, inference, or mutation is performed.
use egglog::{device_program::DeviceAnchors,EGraph};
use serde_json::{Value,json};
use std::collections::{BTreeSet,HashMap};

pub struct Observer {
    tables:Vec<String>,
    anchors:DeviceAnchors,
    values:HashMap<String,u32>,
    previous:BTreeSet<Vec<u32>>,
    revision:u64,
}
impl Observer {
    pub fn new(eg:&EGraph,mut tables:Vec<String>)->Result<Self,String>{
        tables.sort();tables.dedup();
        for t in &tables {if eg.get_function(t).is_none(){return Err(format!("unknown observed table {t}"));}}
        Ok(Self{tables,anchors:eg.capture_device_anchors()?,values:HashMap::new(),previous:BTreeSet::new(),revision:0})
    }
    pub fn snapshot(&mut self,eg:&EGraph)->Result<Value,String>{
        let snap=eg.device_checkpoint_tables(&self.tables,&mut self.anchors)?;
        let mut added_values=vec![];let mut next=BTreeSet::new();
        for row in snap["rows"].as_array().ok_or("rows")? {
            let table=self.tables.binary_search(&row["table"].as_str().ok_or("table")?.to_owned()).map_err(|_|"table")?;
            let mut record=vec![table as u32,u32::from(row["subsumed"].as_bool().ok_or("visibility")?)];
            for value in row["values"].as_array().ok_or("values")? {
                let key=value.to_string();let n=self.values.len() as u32;
                let id=*self.values.entry(key).or_insert_with(||{added_values.push(json!([n,value]));n});record.push(id);
            }
            next.insert(record);
        }
        let added=next.difference(&self.previous).collect::<Vec<_>>();
        let removed=self.previous.difference(&next).collect::<Vec<_>>();
        if !added.is_empty() || !removed.is_empty() || !snap["anchors"].as_array().unwrap().is_empty(){self.revision+=1;}
        let result=json!({"schema":"native-compact-observer/v1","tables":self.tables,
            "values":added_values,"insert":added,"remove":removed,"aliases":snap["anchors"],
            "rows":next.len(),"revision":self.revision});
        self.previous=next;Ok(result)
    }
}
