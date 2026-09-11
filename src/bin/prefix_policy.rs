//! JSON-lines bridge to the same policy used by DependencyBlockStore.
use egg_layout::prefix_policy::{Candidate, Policy};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
fn main() {
    let mut policy = None;
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if v.get("reset").is_some() {
            policy = Some(Policy::new(
                v["epoch"].as_u64().unwrap(),
                v["period"].as_u64().unwrap(),
                v["max_wait"].as_u64().unwrap(),
            ));
            println!("{{\"reset\":true}}");
        } else {
            let candidates: Vec<_> = v["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| Candidate {
                    id: c["id"].as_u64().unwrap(),
                    marginal_bytes: c["gain"].as_i64().unwrap(),
                    coarse: c["coarse"].as_bool().unwrap(),
                })
                .collect();
            let p = policy.as_mut().unwrap();
            let epoch = v["epoch"].as_u64().unwrap();
            let d = if let Some(id) = v.get("reserved") {
                Some(
                    p.record_reserved(epoch, &candidates, id.as_u64().unwrap())
                        .unwrap(),
                )
            } else {
                p.choose(epoch, &candidates).unwrap()
            };
            println!("{}",d.map(|d|json!({"candidate":d.candidate,"reason":d.reason,"age":d.age,"gain":d.marginal_bytes})).unwrap_or(Value::Null));
        }
        io::stdout().flush().unwrap();
    }
}
