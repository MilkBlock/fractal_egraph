//! Export existing source-witness classes through egglog's native AST parser.
use egglog::{
    EGraph,
    ast::{Action, Command, Expr, Fact, Rule},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn expr(e: &Expr, ports: &mut BTreeMap<String, String>) -> Value {
    match e {
        Expr::Var(_, v) => {
            let n = ports.len();
            let p = ports
                .entry(v.clone())
                .or_insert_with(|| format!("local:{n}"));
            json!({"var":p})
        }
        Expr::Lit(_, v) => json!({"literal":v.to_string()}),
        Expr::Call(_, op, args) => {
            json!({"call":op,"args":args.iter().map(|e|expr(e,ports)).collect::<Vec<_>>()})
        }
    }
}
fn contract(r: &Rule, mut ports: BTreeMap<String, String>) -> Value {
    let body: Vec<_> = r
        .body
        .iter()
        .map(|f| match f {
            Fact::Eq(_, a, b) => json!({"require_eq":[expr(a,&mut ports),expr(b,&mut ports)]}),
            Fact::Fact(e) => json!({"require":expr(e,&mut ports)}),
        })
        .collect();
    let head: Vec<_> = r
        .head
        .0
        .iter()
        .map(|a| match a {
            Action::Union(_, a, b) => json!({"union":[expr(a,&mut ports),expr(b,&mut ports)]}),
            Action::Expr(_, e) => json!({"evaluate":expr(e,&mut ports)}),
            // Preserve unsupported syntax verbatim; never manufacture a positive effect.
            _ => json!({"opaque_action":a.to_string()}),
        })
        .collect();
    json!({"body":body,"head":head,"naive":r.naive})
}
fn main() {
    let a: Vec<_> = std::env::args().collect();
    assert_eq!(
        a.len(),
        4,
        "babble_corpus binding-report.json profile.json output.json"
    );
    let read =
        |p: &str| serde_json::from_str::<Value>(&std::fs::read_to_string(p).unwrap()).unwrap();
    let bindings = read(&a[1]);
    let profile = read(&a[2]);
    let mut eg = EGraph::default();
    let rules: BTreeMap<_, _> = profile["rule_labels"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            let cmds = eg
                .parse_program(None, v["definition"].as_str().unwrap())
                .unwrap();
            let Command::Rule { rule } = cmds.into_iter().next().unwrap() else {
                panic!("expected normalized rule")
            };
            (k.clone(), rule)
        })
        .collect();
    let entries:Vec<_>=bindings["classes"].as_array().unwrap().iter().enumerate().map(|(i,c)|{
        let inputs=c["producer_port_labels"].as_array().unwrap();
        let producers: BTreeMap<_,_>=c["contracts"]["producer_rules"].as_object().unwrap().iter().map(|(stage,id)|{
            let prefix=format!("{stage}.");
            let ports=inputs.iter().enumerate().filter_map(|(i,v)|v.as_str().unwrap().strip_prefix(&prefix).map(|v|(v.to_owned(),format!("in:{i}")))).collect();
            (stage,contract(&rules[id.as_str().unwrap()],ports))
        }).collect();
        let outputs=c["consumer_port_labels"].as_array().unwrap().iter().enumerate().map(|(i,v)|(v.as_str().unwrap().to_owned(),format!("out:{i}"))).collect();
        json!({"class":c["shape"],"occurrences":c["occurrences"],"split":if i%5==0 {"test"}else{"train"},"program":{
            "wiring":c["normalized"],"producers":producers,"consumer":contract(&rules[c["contracts"]["consumer_rule"].as_str().unwrap()],outputs),
            "boundary_consumer_ports":c["boundary_consumer_ports"],"source_connection":c["shape"]
        }})
    }).collect();
    let result = json!({"schema":"combine-babble-v1","scope":"source-witness classes with normalized wiring and native-parsed rule contracts; source connection retained verbatim; clause effects are syntax, not inferred semantic summaries","split":"deterministic every fifth class held out within one trace; unweighted unique classes","entries":entries});
    std::fs::write(&a[3], serde_json::to_string_pretty(&result).unwrap()).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parsed(s: &str) -> Rule {
        let mut eg = EGraph::default();
        let Command::Rule { rule } = eg.parse_program(None, s).unwrap().remove(0) else {
            panic!()
        };
        rule
    }
    #[test]
    fn rename_ports_but_retain_union_and_guard() {
        let a = parsed("(rule ((= root (Add x y)) (= x y)) ((union root (Add y x))))");
        let b = parsed("(rule ((= r (Add a b)) (= a b)) ((union r (Add b a))))");
        let ports = |a: &str, b: &str| {
            BTreeMap::from([(a.into(), "in:0".into()), (b.into(), "in:1".into())])
        };
        let x = contract(&a, ports("x", "y"));
        assert_eq!(x, contract(&b, ports("a", "b")));
        assert!(x["head"][0].get("union").is_some());
        assert_eq!(x["body"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn unsupported_actions_remain_explicit() {
        let r = parsed("(rule ((P x)) ((set (Q x) 1)))");
        assert!(
            contract(&r, BTreeMap::new())["head"][0]["opaque_action"]
                .as_str()
                .unwrap()
                .contains("set")
        );
    }
}
