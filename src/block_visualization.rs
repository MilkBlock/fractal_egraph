//! Stable JSON export for inspecting event blocks, their prefixes and live bindings.
use crate::dependency_blocks::DependencyBlockStore;
use egglog::{EGraph, TraceSession, Value};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
fn native_fact(eg: &EGraph, table: &str, key: &[Value]) -> Json {
    let Some(f) = eg.get_function(table) else {
        return json!({"table":table,"available":false});
    };
    if key.len() != f.schema().input.len() {
        return json!({"table":table,"available":false});
    }
    let keys: Vec<_> = key
        .iter()
        .zip(&f.schema().input)
        .map(|(v, s)| {
            if s.is_eq_sort() {
                eg.get_canonical_value(*v, s)
            } else {
                *v
            }
        })
        .collect();
    let labels: Vec<_> = keys
        .iter()
        .zip(&f.schema().input)
        .map(|(v, s)| {
            if s.is_eq_sort() {
                eg.value_to_class_id(s, *v).to_string()
            } else if s.name() == "i64" {
                eg.value_to_base::<i64>(*v).to_string()
            } else {
                format!("{v:?}")
            }
        })
        .collect();
    let output = eg.lookup_function(table, &keys);
    let class = output
        .filter(|_| f.schema().output.is_eq_sort())
        .map(|v| eg.value_to_class_id(&f.schema().output, v).to_string());
    json!({"table":table,"key":labels,"label":format!("{}({})",table,labels.join(", ")),"available":output.is_some(),"eclass":class})
}
fn canonical_bindings(eg: &EGraph, bindings: &BTreeMap<String, Value>) -> BTreeMap<String, String> {
    bindings
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                eg.get_sort_by_name("E")
                    .map(|s| eg.value_to_class_id(s, *v).to_string())
                    .unwrap_or_else(|| format!("{v:?}")),
            )
        })
        .collect()
}
pub fn export_snapshot(
    s: &DependencyBlockStore,
    t: &TraceSession,
    eg: &EGraph,
    phase: &str,
) -> Json {
    let names: BTreeMap<_, _> = t
        .matches()
        .into_iter()
        .map(|m| (m.event_id, m.rule.to_string()))
        .collect();
    let writes: HashMap<_, _> = t
        .write_events()
        .into_iter()
        .map(|w| (w.event_id, w))
        .collect();
    let tables: HashMap<_, _> = t.table_names().into_iter().collect();
    let reads = t.row_reads();
    let blocks:Vec<_>=s.blocks().iter().map(|b|{
  let mut root_classes=BTreeSet::new();
  let entry_facts:Vec<_>=reads.iter().filter(|r|r.match_event_id==b.entry.match_id).filter_map(|r|r.table_name.as_deref().map(|name|native_fact(eg,name,&r.key))).inspect(|f|{if let Some(c)=f["eclass"].as_str(){root_classes.insert(c.to_string());}}).collect();
  let outputs:Vec<_>=b.potential_exports.iter().filter_map(|id|{let w=writes.get(id)?;let name=tables.get(&w.table)?;let n=eg.get_function(name)?.schema().input.len();Some(json!({"write":id,"producer_match":w.match_event_id,"current":native_fact(eg,name,&w.actual[..n])}))}).collect();
  json!({"id":b.id,"version":b.version,"active":b.active,"member_capacity":b.capacity,"entry_lhs":b.entry.lhs,"entry_rule":b.entry.rule,"entry_bindings":b.entry.bindings.iter().map(|(k,v)|(k.clone(),format!("{v:?}"))).collect::<BTreeMap<_,_>>(),"entry_facts":entry_facts,"current_root_classes":root_classes,"members":b.members.iter().map(|m|json!({"match":m,"rule":names[m]})).collect::<Vec<_>>(),"dependencies":b.dependencies.iter().map(|d|json!({"producer":d.producer,"consumer":d.consumer,"write":d.write,"read":d.read})).collect::<Vec<_>>(),"outputs":outputs,"boundary":b.boundary.iter().map(|r|json!({"table":r.table,"key":format!("{:?}",r.key),"current":native_fact(eg,&r.table,&r.key),"read":r.read_id,"consumer":r.consumer,"producer_write":r.producer_write,"source_block":r.source_block})).collect::<Vec<_>>(),"combined_prefixes":b.prefixes.iter().map(|p|json!({"lhs":p.lhs,"rhs":p.rhs,"member_matches":p.members,"rules":p.members.iter().map(|m|names[m].clone()).collect::<Vec<_>>(),"bindings":p.bindings.iter().map(|(k,v)|(k.clone(),format!("{v:?}"))).collect::<BTreeMap<_,_>>(),"canonical_bindings":canonical_bindings(eg,&p.bindings),"supporting_reads":p.reads,"usable":p.usable})).collect::<Vec<_>>(),"invalidations":b.invalidations})
 }).collect();
    json!({"phase":phase,"blocks":blocks,"union_events":t.union_events().iter().map(|u|json!({"event":u.event_id,"match":u.match_event_id,"lhs":format!("{:?}",u.lhs),"rhs":format!("{:?}",u.rhs),"canonical":format!("{:?}",u.canonical),"changed":u.displaced.is_some()})).collect::<Vec<_>>(),"rebuild_versions":t.write_events().iter().filter(|w|w.rebuild_of.is_some()).map(|w|json!({"write":w.event_id,"source_write":w.rebuild_of,"unions":w.union_dependencies,"outcome":format!("{:?}",w.outcome)})).collect::<Vec<_>>(),"interactions":s.interactions().iter().map(|e|json!({"id":e.id,"parents":e.parents,"target":e.target,"rule":e.rule,"consumer":e.consumer,"supporting_reads":e.reads})).collect::<Vec<_>>(),"rejected":s.rejected()})
}
