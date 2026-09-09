//! Readable snapshots of dependency blocks as actual egglog steps complete.
use egg_layout::dependency_blocks::{DependencyBlockStore, RuleSpec};
use egglog::{EGraph, TraceSession};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn snapshot(s: &DependencyBlockStore, t: &TraceSession, phase: &str) -> Value {
    let names: BTreeMap<_, _> = t
        .matches()
        .into_iter()
        .map(|m| (m.event_id, m.rule.to_string()))
        .collect();
    json!({"phase":phase,"blocks":s.blocks().iter().map(|b|json!({"id":b.id,"version":b.version,"active":b.active,"member_capacity":b.capacity,"entry_lhs":b.entry.lhs,"entry_rule":b.entry.rule,"entry_bindings":b.entry.bindings.iter().map(|(k,v)|(k.clone(),format!("{v:?}"))).collect::<BTreeMap<_,_>>(),"members":b.members.iter().map(|m|json!({"match":m,"rule":names[m]})).collect::<Vec<_>>(),"potential_exports":b.potential_exports,"boundary":b.boundary.iter().map(|r|json!({"table":r.table,"key":format!("{:?}",r.key),"read":r.read_id,"source_block":r.source_block})).collect::<Vec<_>>(),"combined_prefixes":b.prefixes.iter().map(|p|json!({"lhs":p.lhs,"rhs":p.rhs,"rules":p.members.iter().map(|m|names[m].clone()).collect::<Vec<_>>(),"bindings":p.bindings.iter().map(|(k,v)|(k.clone(),format!("{v:?}"))).collect::<BTreeMap<_,_>>(),"supporting_reads":p.reads,"usable":p.usable})).collect::<Vec<_>>(),"invalidations":b.invalidations})).collect::<Vec<_>>(),"interactions":s.interactions().iter().map(|e|json!({"id":e.id,"parents":e.parents,"target":e.target,"rule":e.rule,"supporting_reads":e.reads})).collect::<Vec<_>>(),"rejected":s.rejected()})
}
fn main() {
    let mut outputs = Vec::new();
    let specs = vec![
        RuleSpec::rewrite("a", "(A x)", "(B x)"),
        RuleSpec::rewrite("b", "(B x)", "(C x)"),
        RuleSpec::rewrite("c", "(C x)", "(D x)"),
    ];
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (Var i64) (A E) (B E) (C E) (D E)) (A (Var 7))",
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
    let mut plain = eg.clone();
    let mut s = DependencyBlockStore::new(specs);
    let t = TraceSession::with_dependencies();
    let mut phases = Vec::new();
    for r in ["a", "b", "c"] {
        assert_eq!(
            eg.step_rules_with_trace(r, &t).unwrap().updated,
            plain.step_rules(r).unwrap().updated
        );
        s.ingest_committed(&t).unwrap();
        phases.push(snapshot(&s, &t, r));
    }
    for g in [&mut eg, &mut plain] {
        g.parse_and_run_program(None, "(check (= (A (Var 7)) (D (Var 7))))")
            .unwrap();
    }
    assert_eq!(s.blocks().len(), 1);
    assert_eq!(s.blocks()[0].members.len(), 3);
    assert!(
        s.blocks()[0]
            .prefixes
            .iter()
            .any(|p| p.rhs == "(D v0)" && p.members.len() == 3)
    );
    outputs.push(json!({"scenario":"chain_growth","phases":phases}));
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (relation LS (i64)) (relation LM (i64)) (relation LL (i64))
 (relation RS (i64)) (relation RM (i64)) (relation RR (i64)) (relation Out (i64))
 (ruleset la) (ruleset lb) (ruleset ra) (ruleset rb) (ruleset join)
 (rule ((LS x)) ((LM x)) :ruleset la :name "la")
 (rule ((LM x)) ((LL x)) :ruleset lb :name "lb")
 (rule ((RS x)) ((RM x)) :ruleset ra :name "ra")
 (rule ((RM x)) ((RR x)) :ruleset rb :name "rb")
 (rule ((LL x) (RR x)) ((Out x)) :ruleset join :name "join") (LS 7) (RS 7)
 "#,
    )
    .unwrap();
    let specs = [
        ("la", "(LS x)", "(LM x)"),
        ("lb", "(LM x)", "(LL x)"),
        ("ra", "(RS x)", "(RM x)"),
        ("rb", "(RM x)", "(RR x)"),
        ("join", "((LL x) (RR x))", "(Out x)"),
    ]
    .into_iter()
    .map(|(n, l, r)| RuleSpec::opaque(n, l, r));
    let mut s = DependencyBlockStore::new(specs);
    let t = TraceSession::with_dependencies();
    let mut phases = Vec::new();
    for r in ["la", "lb", "ra", "rb", "join"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
        phases.push(snapshot(&s, &t, r));
    }
    eg.parse_and_run_program(None, "(check (Out 7)) (delete (LL 7))")
        .unwrap();
    s.ingest_committed(&t).unwrap();
    phases.push(snapshot(&s, &t, "delete_LL"));
    assert_eq!(s.interactions()[0].parents, vec![0, 1]);
    assert!(!s.blocks()[0].active);
    assert!(s.blocks()[1].active);
    assert!(!s.blocks()[2].active);
    outputs.push(json!({"scenario":"cross_block_and_invalidation","phases":phases}));
    println!("{}",serde_json::to_string_pretty(&json!({"scope":"diagnostic dependency block index","local_shortcuts_installed":false,"physical_nodes_moved":false,"cases":outputs})).unwrap());
}
