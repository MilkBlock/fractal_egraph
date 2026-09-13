//! Prepare contextual shortcuts without changing the input program's schedule.
#[allow(dead_code)]
#[path = "rule_combine/compose.rs"]
mod compose;
use egglog::{EGraph, ast::Command};
use serde_json::json;
use std::{collections::BTreeMap, error::Error};
fn main() -> Result<(), Box<dyn Error>> {
    let source = std::env::args()
        .nth(1)
        .ok_or("usage: rule_prepare FILE.egg [OUTPUT.json]")?;
    let mut eg = EGraph::default();
    let commands = eg.parse_program(Some(source.clone()), &std::fs::read_to_string(&source)?)?;
    let mut active: BTreeMap<String, Vec<compose::Rule>> = BTreeMap::new();
    let mut stack = Vec::new();
    let mut batches = Vec::new();
    let mut unsupported = Vec::new();
    for (index, command) in commands.into_iter().enumerate() {
        match command {
            Command::Push(n) => {for _ in 0..n {stack.push(active.clone());}}
            Command::Pop(_,n) => {for _ in 0..n {active=stack.pop().ok_or("unbalanced pop")?;}}
            Command::Rewrite(ruleset, rewrite, _) => {
                let name=if rewrite.name.is_empty(){format!("source_rule_{index}")}else{rewrite.name};
                let mut rule=compose::Rule::new(&name,&rewrite.lhs.to_string(),&rewrite.rhs.to_string());
                rule.conditions=rewrite.conditions.iter().map(|f|compose::Pat::parse(&f.to_string())).collect();
                if rule.conditions.iter().any(|p|!p.vars().is_subset(&rule.lhs.vars())) {
                    unsupported.push(json!({"command":index,"rule":name,"reason":"guard binds additional variables; needs relational composition"}));
                    continue;
                }
                let rules=active.entry(ruleset.clone()).or_default();rules.push(rule);
                let newest=rules.len()-1;
                let candidates=compose::candidates(rules).into_iter().chain(compose::contextual_candidates(rules));
                let prepared:Vec<_>=candidates.filter(|c|c.first==newest || c.second==newest).enumerate().map(|(i,mut c)| {
                    c.rule.name=format!("prepared_{index}_{i}");
                    json!({"producer":rules[c.first].name,"consumer":rules[c.second].name,"lhs":c.rule.lhs.to_string(),"rhs":c.rule.rhs.to_string(),"command":c.rule.command(&ruleset)})
                }).collect();
                batches.push(json!({"after_command":index,"ruleset":ruleset,"candidates":prepared}));
            }
            Command::Include(..) | Command::BiRewrite(..) | Command::Rule{..} => unsupported.push(json!({"command":index,"reason":"not a direct rewrite in this file; native runner still executes it"})),
            _ => {}
        }
    }
    let output = serde_json::to_string_pretty(
        &json!({"source":source,"scope":"prepared symbolic candidates; native typechecking and a concrete witness are required before activation; original rules must remain", "batches":batches,"unsupported":unsupported}),
    )?;
    if let Some(path) = std::env::args().nth(2) {
        std::fs::write(path, output)?;
    } else {
        println!("{output}");
    }
    Ok(())
}
