//! Execution-boundary export and terminal snapshot loading for extraction. Parsing, desugaring, type checking and
//! rule installation stay in egglog. Device execution never calls this per match.
use crate::core::{GenericAtomTerm as Term, GenericCoreAction as Action, ResolvedCoreRule};
use crate::*;
use serde_json::{Value as Json, json};

type Result<T> = std::result::Result<T, String>;
// Match OrderedFloat identity: signed zeros and all NaNs compare equal.
fn float_bits(v: f64) -> i64 {
    (if v == 0.0 { 0.0f64.to_bits() } else if v.is_nan() { f64::NAN.to_bits() } else { v.to_bits() }) as i64
}
pub struct DeviceAnchors(Vec<(ArcSort, Value)>);
fn sort(s: &ArcSort) -> Result<Json> {
    if s.is_eq_sort() {
        Ok(json!({"name":s.name(),"kind":"eq"}))
    } else if s.value_type() == Some(std::any::TypeId::of::<crate::sort::SetContainer>()) {
        let inner=s.inner_sorts();
        if inner.len()!=1 || !inner[0].is_eq_sort() {return Err("device Set currently requires equality-sort elements".into());}
        Ok(json!({"name":s.name(),"kind":"set","element":sort(&inner[0])?}))
    } else if matches!(s.name(), "i64" | "Unit" | "String" | "bool" | "f64") {
        Ok(json!({"name":s.name(),"kind":s.name()}))
    } else {
        Err(format!("device backend: unsupported sort {}", s.name()))
    }
}
fn term(t: &ResolvedAtomTerm) -> Result<Json> {
    match t {
        Term::Var(_, v) => Ok(json!({"var":v.name,"sort":sort(&v.sort)?})),
        Term::Literal(_, Literal::Int(n)) => {
            Ok(json!({"literal":n,"sort":{"name":"i64","kind":"i64"}}))
        }
        Term::Literal(_, Literal::String(s)) => {
            Ok(json!({"literal":s,"sort":{"name":"String","kind":"String"}}))
        }
        Term::Literal(_, Literal::Float(f)) => Ok(json!({"literal":float_bits(f.0),"sort":{"name":"f64","kind":"f64"}})),
        Term::Literal(_, Literal::Bool(b)) => Ok(json!({"literal":i64::from(*b),"sort":{"name":"bool","kind":"bool"}})),
        Term::Literal(_, Literal::Unit) => {
            Ok(json!({"literal":0,"sort":{"name":"Unit","kind":"Unit"}}))
        }
        _ => Err("device backend: unsupported literal or unresolved global".into()),
    }
}
fn call(c: &ResolvedCall) -> Result<Json> {
    Ok(match c {
        ResolvedCall::Func(f) => json!({"table":f.name}),
        ResolvedCall::Primitive(p) => {
            for s in p.input() {
                sort(s)?;
            }
            sort(p.output())?;
            json!({"primitive":p.name(),"inputs":p.input().iter().map(|s|s.name()).collect::<Vec<_>>(),"output":p.output().name()})
        }
    })
}
fn merge_expr(e: &ResolvedExpr) -> Result<Json> {
    Ok(match e {
        GenericExpr::Var(_,v)=>json!({"var":v.name}),
        GenericExpr::Call(_,c,args)=>json!({"call":call(c)?,"args":args.iter().map(merge_expr).collect::<Result<Vec<_>>>()?}),
        GenericExpr::Lit(span,l)=>term(&Term::Literal(span.clone(),l.clone()))?,
    })
}
fn rule(r: &ResolvedCoreRule) -> Result<Json> {
    let body=r.body.atoms.iter().map(|a|Ok(json!({"call":call(&a.head)?,"args":a.args.iter().map(term).collect::<Result<Vec<_>>>()?}))).collect::<Result<Vec<_>>>()?;
    let head=r.head.0.iter().map(|a|Ok(match a {
        Action::Let(_,v,c,args)=>json!({"op":"let","dst":v.name,"sort":sort(&v.sort)?,"call":call(c)?,"args":args.iter().map(term).collect::<Result<Vec<_>>>()?}),
        Action::LetAtomTerm(_,v,x)=>json!({"op":"copy","dst":v.name,"sort":sort(&v.sort)?,"arg":term(x)?}),
        Action::Set(_,c,args,v)=>json!({"op":"set","call":call(c)?,"args":args.iter().map(term).collect::<Result<Vec<_>>>()?,"value":term(v)?}),
        Action::Union(_,a,b)=>json!({"op":"union","args":[term(a)?,term(b)?]}),
        Action::Change(_,ch,c,args)=>json!({"op":match ch {Change::Subsume=>"subsume",Change::Delete=>"delete"},"call":call(c)?,"args":args.iter().map(term).collect::<Result<Vec<_>>>()?}),
        Action::Panic(..)=>json!({"op":"panic"}),
    })).collect::<Result<Vec<_>>>()?;
    Ok(json!({"body":body,"head":head}))
}
impl EGraph {
    /// Inspect every installed rule without failing at the first unsupported sort.
    pub fn device_inventory(&self) -> Json {
        let mut tables = Vec::new();
        for name in self.get_function_names() {
            let f = self.get_function(&name).unwrap();
            tables.push(json!({"name":name,"schema":f.schema.input.iter().chain([&f.schema.output]).map(|s|s.name()).collect::<Vec<_>>(),"merge":f.decl.merge.as_ref().map(|m|m.to_string())}));
        }
        tables.sort_by_key(|t|t["name"].as_str().unwrap().to_owned());
        let mut rules = Vec::new();
        for (set, rs) in &self.rulesets {
            if let Ruleset::Rules(rs) = rs {
                for (name, (r, _)) in rs {
                    let mut primitives = Vec::new();
                    for c in r.body.atoms.iter().map(|a|&a.head).chain(r.head.0.iter().filter_map(|a|if let Action::Let(_,_,c,_) = a {Some(c)} else {None})) {
                        if let ResolvedCall::Primitive(p) = c {
                            primitives.push(json!({"name":p.name(),"inputs":p.input().iter().map(|s|s.name()).collect::<Vec<_>>(),"output":p.output().name()}));
                        }
                    }
                    rules.push(json!({"ruleset":set,"name":name,"primitives":primitives,"export_error":rule(r).err()}));
                }
            }
        }
        rules.sort_by_key(|r|(r["ruleset"].as_str().unwrap().to_owned(),r["name"].as_str().unwrap().to_owned()));
        json!({"tables":tables,"rules":rules})
    }

    pub fn capture_device_anchors(&self) -> Result<DeviceAnchors> {
        let mut anchors = std::collections::BTreeMap::new();
        for name in self.get_function_names() {
            let f = self.get_function(&name).unwrap();
            self.function_for_each(&name, |r| {
                for (v, s) in r
                    .vals
                    .iter()
                    .zip(f.schema.input.iter().chain([&f.schema.output]))
                {
                    let values=if s.is_container_sort(){s.inner_values(self.backend.container_values(),*v)}else{vec![(s.clone(),*v)]};
                    for (s,v) in values {if s.is_eq_sort(){
                        let v=self.get_canonical_value(v,&s);
                        anchors.insert((s.name().to_string(),format!("{v:?}")),(s,v));
                    }}
                }
            })
            .map_err(|e| e.to_string())?;
        }
        Ok(DeviceAnchors(anchors.into_values().collect()))
    }
    pub fn device_checkpoint(&self, anchors: &DeviceAnchors) -> Result<Json> {
        Ok(
            json!({"rows":self.device_snapshot()?,"anchors":anchors.0.iter().map(|(s,v)|json!({"sort":s.name(),"old":format!("{v:?}"),"now":format!("{:?}",self.get_canonical_value(*v,s))})).collect::<Vec<_>>()}),
        )
    }
    /// Narrow observation with union lineage for all values seen by this
    /// observer. Mutates observer metadata only, never the egraph.
    pub fn device_checkpoint_tables(&self, names:&[String], anchors:&mut DeviceAnchors)->Result<Json>{
        let mut current=std::collections::BTreeMap::new();
        let mut aliases=vec![];
        for (s,v) in &anchors.0 {
            let now=self.get_canonical_value(*v,s);
            if *v!=now {aliases.push(json!({"sort":s.name(),"old":format!("{v:?}"),"now":format!("{now:?}")}));}
            current.insert((s.name().to_owned(),format!("{now:?}")),(s.clone(),now));
        }
        for name in names {
            let f=self.get_function(name).ok_or_else(||format!("unknown snapshot table {name}"))?;
            self.function_for_each(name,|row|{
                for (v,s) in row.vals.iter().zip(f.schema.input.iter().chain([&f.schema.output])) {
                    let values=if s.is_container_sort(){s.inner_values(self.backend.container_values(),*v)}else{vec![(s.clone(),*v)]};
                    for (s,v) in values {if s.is_eq_sort(){let now=self.get_canonical_value(v,&s);
                        current.insert((s.name().to_owned(),format!("{now:?}")),(s,now));}}
                }
            }).map_err(|e|e.to_string())?;
        }
        anchors.0=current.into_values().collect();
        Ok(json!({"rows":self.device_snapshot_tables(names)?,"anchors":aliases}))
    }
    /// Export typed core rules plus one initialized state. Unsupported sorts are
    /// rejected before the GPU takes ownership; this is not a fallback protocol.
    pub fn export_device_program(&self, sets: &[String]) -> Result<Json> {
        let mut tables = vec![];
        for name in self.get_function_names() {
            let f = self.get_function(&name).unwrap();
            let schema = f
                .schema
                .input
                .iter()
                .chain([&f.schema.output])
                .map(sort)
                .collect::<Result<Vec<_>>>()?;
            let merge = f.decl.merge.as_ref().map(|m| m.to_string());
            let kind = if f.is_constructor() {
                "constructor"
            } else {
                "function"
            };
            tables.push(json!({"name":name,"schema":schema,"kind":kind,"merge":merge,"merge_expr":f.decl.merge.as_ref().map(merge_expr).transpose()?,"internal":f.is_let_binding(),"can_subsume":f.can_subsume}));
        }
        tables.sort_by_key(|t| t["name"].as_str().unwrap().to_owned());
        fn collect(eg: &EGraph, set: &str, stack: &mut Vec<String>) -> Result<Vec<Json>> {
            if stack.iter().any(|s| s == set) {
                return Err("recursive combined ruleset".into());
            }
            stack.push(set.into());
            let out = match eg
                .rulesets
                .get(set)
                .ok_or_else(|| format!("missing ruleset {set}"))?
            {
                Ruleset::Rules(rs) => rs
                    .iter()
                    .map(|(name, (r, id))| {
                        let mut j = rule(r)?;
                        j["name"] = json!(name);
                        j["source_set"] = json!(set);
                        j["seminaive"] = json!(eg.backend.device_rule_is_seminaive(*id));
                        Ok(j)
                    })
                    .collect::<Result<Vec<_>>>()?,
                Ruleset::Combined(children) => {
                    let mut all = vec![];
                    for s in children {
                        all.extend(collect(eg, s, stack)?);
                    }
                    all
                }
            };
            stack.pop();
            Ok(out)
        }
        let programs = sets
            .iter()
            .map(|s| Ok(json!({"name":s,"rules":collect(self,s,&mut vec![])?})))
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"schema":"egglog-device-core/v1","tables":tables,"rulesets":programs,"initial":self.device_snapshot()?}),
        )
    }
    /// Original native tables, including visibility. Values retain explicit
    /// sorts so device IDs are never confused with numeric primitive values.
    /// Load an already-canonical typed snapshot for CPU extraction only. The
    /// returned map makes the imported identity correspondence inspectable.
    pub fn import_device_rows_for_extraction(&mut self, rows:&Json)->Result<Json>{
        use std::collections::{BTreeMap,BTreeSet};
        fn decode(eg:&mut EGraph,s:&ArcSort,x:&Json,ids:&mut BTreeMap<(String,String),Value>)->Result<Value>{
            if x["sort"].as_str()!=Some(s.name()){return Err("snapshot value sort mismatch".into());}
            if s.is_eq_sort(){
                let key=(s.name().to_owned(),x["eq"].as_str().ok_or("missing equality identity")?.to_owned());
                return Ok(*ids.entry(key).or_insert_with(||eg.backend.fresh_id()));
            }
            let desc=sort(s)?;
            if desc["kind"]=="set"{
                let inner=s.inner_sorts();let es=x["elements"].as_array().ok_or("missing set elements")?;
                let mut data=BTreeSet::new();
                for e in es{if !data.insert(decode(eg,&inner[0],e,ids)?){return Err("duplicate set element".into());}}
                return Ok(eg.container_to_value(crate::sort::SetContainer{do_rebuild:true,data}));
            }
            Ok(match s.name(){
                "i64"=>eg.base_to_value(x["value"].as_i64().ok_or("invalid i64")?),
                "f64"=>eg.base_to_value(crate::sort::F::from(crate::sort::OrderedFloat(f64::from_bits(x["value"].as_i64().ok_or("invalid f64 bits")? as u64)))),
                "String"=>eg.base_to_value(crate::sort::S::new(x["value"].as_str().ok_or("invalid string")?.to_owned())),
                "bool"=>eg.base_to_value(match x["value"].as_i64(){Some(0)=>false,Some(1)=>true,_=>return Err("invalid bool".into())}),
                "Unit"=>{if x["value"]!=0{return Err("invalid Unit".into());}eg.base_to_value(())},
                _=>return Err("unsupported snapshot value".into()),
            })
        }
        let mut ids=BTreeMap::new();let mut decoded=Vec::new();
        for row in rows.as_array().ok_or("snapshot rows must be an array")?{
            let name=row["table"].as_str().ok_or("missing table")?;
            let f=self.get_function(name).ok_or_else(||format!("unknown snapshot table {name}"))?;
            let function=f.backend_id;let schema=f.schema.input.iter().chain([&f.schema.output]).cloned().collect::<Vec<_>>();
            let vs=row["values"].as_array().ok_or("missing values")?;
            if vs.len()!=schema.len(){return Err("snapshot row arity mismatch".into());}
            let values=vs.iter().zip(schema).map(|(v,s)|decode(self,&s,v,&mut ids)).collect::<Result<Vec<_>>>()?;
            decoded.push((function,values,row["subsumed"].as_bool().ok_or("missing visibility")?));
        }
        self.backend.replace_snapshot_for_extraction(decoded).map_err(|e|e.to_string())?;
        let identities=ids.into_iter().map(|((sort,old),v)|json!({"sort":sort,"old":old,"now":format!("{v:?}")})).collect::<Vec<_>>();
        Ok(json!({"identities":identities,"rows":self.device_snapshot()?,"rules_executed":0,"rebuilds_executed":0}))
    }

    fn device_value(&self,v:Value,s:&ArcSort)->Result<Json>{
        let desc=sort(s)?;
        if s.is_eq_sort(){return Ok(json!({"eq":format!("{:?}",self.get_canonical_value(v,s)),"sort":s.name()}));}
        if desc["kind"]=="set" {
            let values=s.inner_values(self.backend.container_values(),v).into_iter().map(|(s,v)|self.device_value(v,&s)).collect::<Result<Vec<_>>>()?;
            return Ok(json!({"sort":s.name(),"elements":values}));
        }
        let value=match s.name(){
            "i64"=>json!(self.value_to_base::<i64>(v)),
            "f64"=>json!(float_bits(**self.value_to_base::<crate::sort::F>(v))),
            "bool"=>json!(i64::from(self.value_to_base::<bool>(v))),
            "String"=>json!(self.value_to_base::<crate::sort::S>(v).to_string()),
            "Unit"=>json!(0),
            _=>return Err("unsupported value".into()),
        };
        Ok(json!({"value":value,"sort":s.name()}))
    }
    pub fn device_snapshot(&self) -> Result<Json> {
        self.device_snapshot_tables(&self.get_function_names())
    }
    /// Read-only diagnostic view of explicitly selected tables. Unlike exporting
    /// the whole graph then filtering, unrelated function tables are not scanned.
    pub fn device_snapshot_tables(&self, names: &[String]) -> Result<Json> {
        let mut rows = vec![];
        for name in names {
            let f = self.get_function(name).ok_or_else(||format!("unknown snapshot table {name}"))?;
            let mut error = None;
            self.function_for_each(name,|r|{
                let vals=r.vals.iter().zip(f.schema.input.iter().chain([&f.schema.output])).map(|(v,s)|{
                    self.device_value(*v,s)
                }).collect::<Result<Vec<_>>>();
                match vals{Ok(v)=>rows.push(json!({"table":name,"values":v,"subsumed":r.subsumed})),Err(e)=>error=Some(e)}
            }).map_err(|e|e.to_string())?;
            if let Some(e) = error {
                return Err(e);
            }
        }
        Ok(json!(rows))
    }
}
