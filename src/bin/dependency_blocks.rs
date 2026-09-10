//! Readable snapshots of dependency blocks as actual egglog steps complete.
use egg_layout::block_visualization::export_snapshot as snapshot;
use egg_layout::dependency_blocks::{DependencyBlockStore, RuleSpec};
use egglog::{EGraph, TraceSession};
use serde_json::{Value, json};
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
        phases.push(snapshot(&s, &t, &eg, r));
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
        phases.push(snapshot(&s, &t, &eg, r));
    }
    eg.parse_and_run_program(None, "(check (Out 7)) (delete (LL 7))")
        .unwrap();
    s.ingest_committed(&t).unwrap();
    phases.push(snapshot(&s, &t, &eg, "delete_LL"));
    assert_eq!(s.interactions()[0].parents, vec![0, 1]);
    assert!(!s.blocks()[0].active);
    assert!(s.blocks()[1].active);
    assert!(!s.blocks()[2].active);
    outputs.push(json!({"scenario":"cross_block_and_invalidation","phases":phases}));
    if std::env::args().any(|a| a == "--include-union") {
        outputs.push(union_case());
    }
    println!("{}",serde_json::to_string_pretty(&json!({"scope":"diagnostic dependency block index","local_shortcuts_installed":false,"physical_nodes_moved":false,"cases":outputs})).unwrap());
}

fn union_case() -> Value {
    let specs = vec![
        RuleSpec::rewrite("a", "(A x)", "(B x)"),
        RuleSpec::rewrite("b", "(B x)", "(C x)"),
    ];
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (Var i64) (A E) (B E) (C E)) (A (Var 7)) (A (Var 8))",
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
    let mut s = DependencyBlockStore::new(specs);
    let t = TraceSession::with_dependencies();
    for r in ["a", "b"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
    }
    assert_eq!(s.blocks().len(), 2);
    let before = snapshot(&s, &t, &eg, "before_union");
    assert_ne!(
        before["blocks"][0]["current_root_classes"],
        before["blocks"][1]["current_root_classes"]
    );
    eg.parse_and_run_program(None, "(union (B (Var 7)) (B (Var 8)))")
        .unwrap();
    // Explicit conservative fallback: this is not an automatic union-lineage repair.
    s.invalidate_all("explicit union: concrete prefix witnesses need revalidation");
    s.ingest_committed(&t).unwrap();
    let partial = snapshot(&s, &t, &eg, "root_union");
    assert_eq!(
        partial["blocks"][0]["current_root_classes"],
        partial["blocks"][1]["current_root_classes"]
    );
    assert_ne!(
        partial["blocks"][0]["combined_prefixes"][0]["canonical_bindings"],
        partial["blocks"][1]["combined_prefixes"][0]["canonical_bindings"]
    );
    eg.parse_and_run_program(None, "(union (Var 7) (Var 8))")
        .unwrap();
    s.invalidate_all("input union: compare canonical bindings after revalidation");
    s.ingest_committed(&t).unwrap();
    let full = snapshot(&s, &t, &eg, "input_union");
    assert_eq!(
        full["blocks"][0]["combined_prefixes"][0]["canonical_bindings"],
        full["blocks"][1]["combined_prefixes"][0]["canonical_bindings"]
    );
    assert_eq!(s.blocks().len(), 2);
    assert!(s.blocks().iter().all(|b| !b.active));
    json!({"scenario":"union_overlap","explicit_union_demo":true,"automatic_block_merge":false,"automatic_prefix_revalidation":false,"phases":[before,partial,full]})
}
#[cfg(test)]
mod tests {
    #[test]
    fn union_keeps_instances_separate_until_revalidated() {
        super::union_case();
    }
}
