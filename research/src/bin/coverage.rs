use egg_layout::{
    coverage::CoverageLedger,
    dependency_blocks::{DependencyBlockStore, RuleSpec},
};
use egglog::{EGraph, TraceSession};
use serde_json::{Value, json};
fn snapshot(c: &CoverageLedger, phase: &str) -> Value {
    let s = c.stats();
    json!({"phase":phase,"live_facts":s.live_facts,"covered_facts":s.covered_facts,"logical_payload_bytes":s.payload_bytes,"covered_logical_payload_bytes":s.covered_payload_bytes,"active_memberships":s.active_memberships,"duplicated_memberships":s.duplicated_memberships,"states":s.states.iter().map(|(k,v)|(format!("{k:?}"),*v)).collect::<std::collections::BTreeMap<_,_>>(),"facts":c.facts().iter().map(|f|json!({"id":f.id,"table":f.key.table,"fields":format!("{:?}",f.key.fields),"owner":f.native_owner,"state":format!("{:?}",c.state(f.id)),"views":c.memberships(f.id)})).collect::<Vec<_>>(),"views":c.views().iter().map(|v|json!({"id":v.id,"block":v.block,"lhs":v.lhs,"rhs":v.rhs,"facts":v.facts,"active":v.active,"invalid_reason":v.invalid_reason})).collect::<Vec<_>>()})
}
fn main() {
    let specs = vec![
        RuleSpec::rewrite("a", "(A x)", "(B x)"),
        RuleSpec::rewrite("b", "(B x)", "(C x)"),
        RuleSpec::rewrite("c", "(C x)", "(D x)"),
    ];
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (Var i64) (A E) (B E) (C E) (D E) (Cold E)) (A (Var 7)) (Cold (Var 99))",
    )
    .unwrap();
    for r in &specs {
        eg.parse_and_run_program(
            None,
            &format!(
                "(ruleset {}) (rewrite {} {} :ruleset {} :name \"{}\")",
                r.name, r.lhs, r.rhs, r.name, r.name
            ),
        )
        .unwrap();
    }
    let mut c = CoverageLedger::new(
        ["Var", "A", "B", "C", "D", "Cold"]
            .into_iter()
            .map(str::to_owned),
    );
    let mut s = DependencyBlockStore::new(specs);
    let t = TraceSession::with_dependencies();
    let mut phases = Vec::new();
    c.refresh(&eg).unwrap();
    phases.push(snapshot(&c, "initial_inventory"));
    for r in ["a", "b", "c"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
        c.sync_blocks(&eg, &s, &t).unwrap();
        phases.push(snapshot(&c, r));
    }
    assert_eq!(c.stats().covered_facts, 4);
    assert_eq!(c.stats().active_memberships, 7);
    assert_eq!(c.stats().live_facts, 7);
    assert_eq!(c.marginal_new_facts(1, &[0]), 1);
    c.invalidate_view(1, "explicitly withdraw the longer view");
    assert_eq!(c.stats().covered_facts, 3);
    phases.push(snapshot(&c, "withdraw_longer_view"));
    eg.parse_and_run_program(None, "(delete (B (Var 7)))")
        .unwrap();
    s.ingest_committed(&t).unwrap();
    c.sync_blocks(&eg, &s, &t).unwrap();
    assert_eq!(c.stats().covered_facts, 0);
    phases.push(snapshot(&c, "remove_B"));
    println!("{}",serde_json::to_string_pretty(&json!({"scope":"constructor-row coverage ledger","native_execution_changed":false,"payload_bytes_are_not_allocated_memory":true,"phases":phases})).unwrap());
}
