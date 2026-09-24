// Synthetic API workload, not evidence of native saturation or planner speedup.
use egg_layout::{
    coarse_smooth::{Comb, CombKind, RelativeBinding},
    reuse_graph::*,
    saturated_rule_composition::{Row, SaturatedRuleComposition, Vertex},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub fn state() -> SaturatedRuleComposition {
    SaturatedRuleComposition {
        version: 2,
        local_ids: vec![],
        scope: json!({"rules":["add-mul"],"datatype":"E"}),
        values: vec![
            Vertex {
                sort: "E".into(),
                literal: None
            };
            3
        ],
        rows: vec![
            Row {
                op: "Add".into(),
                args: vec![0, 1],
                result: 2,
            },
            Row {
                op: "Mul".into(),
                args: vec![1, 0],
                result: 2,
            },
        ],
        subsumed_rows: vec![],
        ports: BTreeMap::new(),
    }
}
pub fn report(g: &SaturatedRuleComposition) -> Value {
    json!({"ripen":{"state":"Saturated","updated":false},"checks":"passed","native_stop_reason":null,"saturated_rule_composition":{"status":"exported","values":g.values.len(),"facts":g.rows.len()}})
}
pub fn body() -> RipSatEgraph {
    let g = state();
    let r = report(&g);
    RipSatEgraph::from_native_export(g, &r).unwrap()
}
pub fn origin(rule: &str) -> Comb {
    Comb {
        kind: CombKind::CoarseRuleComposition,
        rule: rule.into(),
        input_roles: vec!["root".into()],
        output_roles: vec!["root".into()],
        parents: vec![],
        binding: vec![RelativeBinding::External { slot: 0 }],
        aliases: vec![0, 0, 0],
        input_count: 1,
        output_count: 1,
        external_count: 1,
        requirements: vec![],
        effects: vec![],
        external_facts: vec![],
    }
}
pub fn descriptor(body: RipSatId, rule: &str) -> EnclosureDescriptor {
    EnclosureDescriptor {
        body,
        interface: Interface {
            upper: vec![2],
            lower: vec![0, 1],
        },
        origin: vec![origin(rule)],
    }
}
pub fn value(id: u64) -> RuntimeValue {
    RuntimeValue {
        sort: "E".into(),
        id,
    }
}
pub fn fact(table: &str, id: u64) -> RuntimeFact {
    RuntimeFact {
        run: 10,
        table: table.into(),
        id,
        version: 0,
    }
}
pub fn bind_reads(r: &mut ReuseGraph, i: InstanceId, rev: u64) {
    r.bind_fact_slot(i, rev, 0, fact("Add", 1)).unwrap();
    r.bind_fact_slot(i, rev, 1, fact("Mul", 2)).unwrap();
}
pub fn setup() -> (ReuseGraph, EnclosureId, InstanceId) {
    let mut graph = ReuseGraph::default();
    let b = graph.intern_body(body()).unwrap().id;
    let e = graph
        .register_enclosure(descriptor(b, "Add-to-Mul"), Admission::InitialBake)
        .unwrap();
    let i = graph
        .instantiate(e, 10, vec![value(100)], vec![value(101), value(102)])
        .unwrap();
    bind_reads(&mut graph, i, 0);
    (graph, e, i)
}
