//! Read-only execution-boundary export. Parsing, desugaring, type checking and
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
                    if s.is_eq_sort() {
                        let v = self.get_canonical_value(*v, s);
                        anchors.insert((s.name().to_string(), format!("{v:?}")), (s.clone(), v));
                    }
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
    pub fn device_snapshot(&self) -> Result<Json> {
        let mut rows = vec![];
        for name in self.get_function_names() {
            let f = self.get_function(&name).unwrap();
            let mut error = None;
            self.function_for_each(&name,|r|{
                let vals=r.vals.iter().zip(f.schema.input.iter().chain([&f.schema.output])).map(|(v,s)|{
                    sort(s)?;
                    if s.is_eq_sort(){Ok(json!({"eq":format!("{:?}",self.get_canonical_value(*v,s)),"sort":s.name()}))}
                    else if s.name()=="i64"{Ok(json!({"value":self.value_to_base::<i64>(*v),"sort":"i64"}))}
                    else if s.name()=="f64"{Ok(json!({"value":float_bits(**self.value_to_base::<crate::sort::F>(*v)),"sort":"f64"}))}
                    else if s.name()=="bool"{Ok(json!({"value":i64::from(self.value_to_base::<bool>(*v)),"sort":"bool"}))}
                    else if s.name()=="String"{Ok(json!({"value":self.value_to_base::<crate::sort::S>(*v).to_string(),"sort":"String"}))}
                    else{Ok(json!({"value":0,"sort":"Unit"}))}
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
