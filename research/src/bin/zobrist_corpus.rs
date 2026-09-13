//! Generate controlled but real egglog rewrite snapshots for a storage experiment.
use egglog::{EGraph, SerializeConfig};
use serde_json::json;
use std::{collections::BTreeMap, io::Write};
fn main() {
    let path = std::env::args().nth(1).expect("output JSONL");
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    for (arity, instances, rounds) in [(4, 16, 16), (6, 4, 32)] {
        let vars: Vec<_> = (0..arity).map(|i| format!("x{i}")).collect();
        let args = vars.join(" ");
        let mut rotate = vars.clone();
        rotate.rotate_left(1);
        let mut swap = vars.clone();
        swap.swap(0, 1);
        let types = vec!["E"; arity].join(" ");
        let mut program = format!(
            "(datatype E (Var i64) (Sum {types}))\n(ruleset r)\n(rewrite (Sum {args}) (Sum {}) :ruleset r)\n(rewrite (Sum {args}) (Sum {}) :ruleset r)\n",
            rotate.join(" "),
            swap.join(" ")
        );
        for i in 0..instances {
            let leaves: Vec<_> = (0..arity)
                .map(|j| format!("(Var {})", i * arity + j))
                .collect();
            program.push_str(&format!("(let root{i} (Sum {}))\n", leaves.join(" ")));
        }
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &program).unwrap();
        let sort = eg.get_sort_by_name("E").unwrap().clone();
        let roots: Vec<_> = (0..instances)
            .map(|i| eg.lookup_function(&format!("root{i}"), &[]).unwrap())
            .collect();
        for round in 0..=rounds {
            let s = eg.serialize(SerializeConfig::default());
            assert!(s.is_complete());
            let g = s.egraph;
            let mut groups = BTreeMap::<String, Vec<_>>::new();
            for node in g.nodes.values() {
                groups.entry(node.eclass.to_string()).or_default().push(json!({"op":node.op,"children":node.children.iter().map(|c|g.nodes[c].eclass.to_string()).collect::<Vec<_>>()}));
            }
            for (instance, root) in roots.iter().enumerate() {
                let id = eg.value_to_class_id(&sort, *root).to_string();
                let nodes = &groups[&id];
                writeln!(out,"{}",json!({"family":format!("sum{arity}"),"instance":instance,"round":round,"root":id,"nodes":nodes})).unwrap();
            }
            if round == rounds {
                panic!("expected finite permutation saturation");
            }
            if !eg.step_rules("r").unwrap().updated {
                let expected = (1..=arity).product::<usize>();
                for root in &roots {
                    let id = eg.value_to_class_id(&sort, *root).to_string();
                    assert_eq!(groups[&id].len(), expected);
                }
                eprintln!(
                    "sum{arity}: {instances} roots; {expected} nodes/root; saturated after {round} rounds"
                );
                break;
            }
        }
    }
}
