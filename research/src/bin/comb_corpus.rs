//! Encode complete generated comb classes, with atomic rule references and wiring.
use serde_json::{Value, json};
fn term(sort: &str, op: impl AsRef<str>, args: Vec<Value>) -> Value {
    json!({"sort":sort,"op":op.as_ref(),"args":args})
}
fn path(s: &str) -> Value {
    term(
        "Position",
        "path",
        s.split('/')
            .map(|s| term("PositionPart", s, vec![]))
            .collect(),
    )
}
fn unpath(p: &Value) -> String {
    p["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["op"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("/")
}
fn encode(c: &Value) -> Value {
    let shape = c["shape"].as_str().unwrap();
    let (body, consumer) = shape
        .strip_prefix('[')
        .unwrap()
        .split_once("] -> ")
        .unwrap();
    let edges = body
        .split(" + ")
        .map(|s| {
            let (producer, s) = s.split_once('@').unwrap();
            let (stage, s) = s.split_once(':').unwrap();
            let (table, s) = s.split_once('#').unwrap();
            let (slot, s) = s.split_once('{').unwrap();
            let (from, to) = s.strip_suffix('}').unwrap().split_once("=>").unwrap();
            assert!(
                !from.contains('|') && !to.contains('|'),
                "ambiguous sites need an explicit alternatives node"
            );
            term(
                "CombEdge",
                format!("use:{producer}->{consumer}:{table}"),
                vec![
                    term("ProducerRef", stage, vec![]),
                    term("ReadSlot", slot, vec![]),
                    path(from),
                    path(to),
                ],
            )
        })
        .collect();
    let labels = |key: &str| {
        term(
            "PortLabels",
            key,
            c[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| term("PortLabel", s.as_str().unwrap(), vec![]))
                .collect(),
        )
    };
    term(
        "RuleComb",
        format!("consumer:{consumer}"),
        vec![
            term("CombSupport", "supports", edges),
            c["normalized"].clone(),
            labels("producer_port_labels"),
            labels("consumer_port_labels"),
            term(
                "Boundary",
                "consumer-ports",
                c["boundary_consumer_ports"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| term("PortIndex", i.to_string(), vec![]))
                    .collect(),
            ),
        ],
    )
}
fn shape(p: &Value) -> String {
    let consumer = p["op"].as_str().unwrap().strip_prefix("consumer:").unwrap();
    let edges = p["args"][0]["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            let name = e["op"].as_str().unwrap().strip_prefix("use:").unwrap();
            let (producer, rest) = name.split_once("->").unwrap();
            let (target, table) = rest.split_once(':').unwrap();
            assert_eq!(target, consumer);
            let a = e["args"].as_array().unwrap();
            format!(
                "{producer}@{}:{table}#{}{{{}=>{}}}",
                a[0]["op"].as_str().unwrap(),
                a[1]["op"].as_str().unwrap(),
                unpath(&a[2]),
                unpath(&a[3])
            )
        })
        .collect::<Vec<_>>()
        .join(" + ");
    format!("[{edges}] -> {consumer}")
}
fn main() {
    let a: Vec<_> = std::env::args().collect();
    assert_eq!(
        a.len(),
        4,
        "comb_corpus binding.json profile.json output.json"
    );
    let read =
        |p: &str| serde_json::from_str::<Value>(&std::fs::read_to_string(p).unwrap()).unwrap();
    let bindings = read(&a[1]);
    let profile = read(&a[2]);
    let entries:Vec<_>=bindings["classes"].as_array().unwrap().iter().enumerate().map(|(i,c)|{
        let p=encode(c);assert_eq!(shape(&p),c["shape"].as_str().unwrap());assert_eq!(p["args"][1],c["normalized"]);
        json!({"class":c["shape"],"occurrences":c["occurrences"],"split":if i%5==0{"test"}else{"train"},"program":p})
    }).collect();
    let output = json!({"schema":"connected-rule-combs-v1","rule_dictionary":profile["rule_labels"],"entries":entries,
        "scope":"existing generated comb classes, not newly synthesized recursive chains; source positions, shared producer references, typed wiring and boundary ports retained; atomic rules stored once"});
    std::fs::write(&a[3], serde_json::to_string_pretty(&output).unwrap()).unwrap();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_edges_can_reference_the_same_producer() {
        let c = json!({"shape":"[R1@p0:Mul#0{head/0/expr/1=>body/0/expr/1/args/0} + R1@p0:Mul#1{head/0/expr/1=>body/0/expr/1/args/1}] -> R10","normalized":{},"producer_port_labels":[],"consumer_port_labels":[],"boundary_consumer_ports":[2]});
        let p = encode(&c);
        assert_eq!(shape(&p), c["shape"]);
        assert_eq!(
            p["args"][0]["args"][0]["args"][0],
            p["args"][0]["args"][1]["args"][0]
        );
        assert_ne!(
            p["args"][0]["args"][0]["args"][3],
            p["args"][0]["args"][1]["args"][3]
        );
    }
}
