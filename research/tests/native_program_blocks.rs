use egg_layout::dependency_blocks::{DependencyBlockStore, RuleSpec};
use egglog::{EGraph, TraceSession};
use std::collections::BTreeSet;
#[test]
fn cyk_keeps_valid_outputs_and_retires_blocks_on_pop() {
    let mut eg = EGraph::default();
    let commands = eg
        .parse_program(None, include_str!("../../egglog/tests/web-demo/cyk.egg"))
        .unwrap();
    let t = TraceSession::with_dependencies();
    let mut store = DependencyBlockStore::default();
    let mut names = BTreeSet::new();
    let mut saw_active = false;
    for command in commands {
        let resets = t.scope_resets().len();
        eg.run_program_with_trace(vec![command], &t).unwrap();
        for m in t.matches() {
            if names.insert(m.rule.to_string()) {
                store
                    .register_rule(RuleSpec::opaque(&m.rule, &m.rule, ""))
                    .unwrap();
            }
        }
        store.ingest_committed(&t).unwrap();
        assert!(store.blocks().iter().all(|b| b.prefixes.is_empty()));
        if t.scope_resets().len() != resets {
            assert!(store.blocks().iter().all(|b| !b.active));
        } else {
            saw_active |= store.blocks().iter().any(|b| b.active);
        }
    }
    assert!(
        saw_active,
        "CYK must have real committed producer-consumer blocks"
    );
}

#[test]
fn union_to_rebuild_to_read_creates_an_equality_interaction() {
    let mut eg = EGraph::default();
    let commands = eg
        .parse_program(
            None,
            include_str!("../../experiments/native_programs/union.egg"),
        )
        .unwrap();
    let trace = TraceSession::with_dependencies();
    eg.run_program_with_trace(commands, &trace).unwrap();
    let names: BTreeSet<_> = trace.matches().iter().map(|m| m.rule.to_string()).collect();
    let mut store = DependencyBlockStore::new(names.iter().map(|n| RuleSpec::opaque(n, n, "")));
    store.ingest_committed(&trace).unwrap();
    assert!(
        store
            .interactions()
            .iter()
            .any(|e| e.rule.starts_with("union #"))
    );
    assert!(store.blocks().iter().any(|b| b.entry.rule == "merge"));
    assert!(store.blocks().iter().all(|b| b.prefixes.is_empty()));
    let count = store.interactions().len();
    store.ingest_committed(&trace).unwrap();
    assert_eq!(store.interactions().len(), count);
}
