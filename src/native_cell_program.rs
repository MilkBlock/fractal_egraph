//! Incremental evidence for the offline executable Cell compiler. Observations
//! select templates; a separate typed closure check must justify execution.
use super::super::super::{Captured, Input};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Census {
    seen: usize,
    rules: BTreeMap<usize, Value>,
    links: BTreeMap<(usize, usize), usize>,
    source: Option<Value>,
}
impl Census {
    pub fn observe(&mut self, c: &Captured) {
        if self.source.is_none() {
            let hash = c
                .preview_source
                .bytes()
                .fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
                });
            self.source = Some(
                json!({"algorithm":"fnv1a64","bytes":c.preview_source.len(),"value":format!("{hash:016x}")}),
            );
        }
        for (index, r) in c.records.iter().enumerate().skip(self.seen) {
            let entry=self.rules.entry(r.rule).or_insert_with(|| {
                let rule=&c.rules[r.rule].rule;
                let bindings:Vec<_>=r.inputs.iter().zip(&r.wanted).filter_map(|(input,token)| {
                    let Input::Var(name)=input else{return None};let v=&c.pool.values[*token];
                    Some(json!({"name":name,"token":token,"sort":v.sort,"literal":c.pool.literals.get(token)}))
                }).collect();
                json!({"id":r.rule,"name":rule.name,"ruleset":rule.ruleset,"ordinal":c.rules[..r.rule].iter().filter(|x|x.rule.ruleset==rule.ruleset).count(),"applications":0,
                    "body_operators":c.rules[r.rule].calls.values().filter(|(path,_)|path.starts_with("body/")).filter_map(|(_,e)|if let egglog::ast::Expr::Call(_,op,_) = e {Some(op.clone())} else {None}).collect::<Vec<_>>(),
                    "example":{"event":r.id,"record":index,"bindings":bindings,"parents":r.parents,"unions":r.unions,"updated_rows":r.updated_rows}})
            });
            entry["applications"] = json!(entry["applications"].as_u64().unwrap() + 1);
            for &parent in &r.parents {
                *self
                    .links
                    .entry((c.records[parent].rule, r.rule))
                    .or_default() += 1;
            }
        }
        self.seen = c.records.len();
    }
    pub fn report(&self) -> Value {
        json!({"schema":"analyzer-cell-program-evidence/v1","source_fingerprint":self.source,
        "applications":self.seen,"rules":self.rules.values().collect::<Vec<_>>(),
        "producer_edges":self.links.iter().map(|((a,b),n)|json!({"from_rule":a,"to_rule":b,"applications":n})).collect::<Vec<_>>(),
        "scope":"committed native applications; executable closure admission requires an independent typed proof"})
    }
}
