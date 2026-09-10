use egg_layout::dependency_blocks::{DependencyBlockStore, RuleSpec};
use egglog::{EGraph, TraceSession};
fn chain() -> (EGraph, DependencyBlockStore, TraceSession) {
    let specs = vec![
        RuleSpec::rewrite("a", "(A x)", "(B x)"),
        RuleSpec::rewrite("b", "(B x)", "(C x)"),
        RuleSpec::rewrite("c", "(C x)", "(D x)"),
        RuleSpec::rewrite("fork", "(B x)", "(F x)"),
    ];
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (Var i64) (A E) (B E) (C E) (D E) (F E)) (A (Var 7))",
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
    (
        eg,
        DependencyBlockStore::new(specs),
        TraceSession::with_dependencies(),
    )
}
#[test]
fn grows_a_b_c_without_replacing_block_handle() {
    let (mut eg, mut store, t) = chain();
    eg.step_rules_with_trace("a", &t).unwrap();
    store.ingest_committed(&t).unwrap();
    assert!(store.blocks().is_empty());
    eg.step_rules_with_trace("b", &t).unwrap();
    store.ingest_committed(&t).unwrap();
    assert_eq!(store.blocks().len(), 1);
    assert_eq!(store.blocks()[0].members.len(), 2);
    let id = store.blocks()[0].id;
    eg.step_rules_with_trace("c", &t).unwrap();
    store.ingest_committed(&t).unwrap();
    let b = &store.blocks()[0];
    assert_eq!(b.id, id);
    assert_eq!(b.members.len(), 3);
    assert_eq!(b.capacity, 4);
    assert!(b.active);
    assert!(
        b.prefixes
            .iter()
            .any(|p| p.lhs == "(A v0)" && p.rhs == "(D v0)" && p.members.len() == 3 && p.usable)
    );
    assert_eq!(b.boundary.len(), 1);
    assert_eq!(b.potential_exports.len(), 3);
    for w in &b.potential_exports {
        assert_eq!(store.active_block_for_fact(*w), Some(id));
    }
    let r = store.ingest_committed(&t).unwrap();
    assert_eq!(r.new_members, 0);
    assert_eq!(r.new_blocks, 0);
}
#[test]
fn a_fork_does_not_invent_a_linear_dependency() {
    let (mut eg, mut store, t) = chain();
    for r in ["a", "b", "c", "fork"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        store.ingest_committed(&t).unwrap();
    }
    let b = &store.blocks()[0];
    assert_eq!(b.members.len(), 4);
    assert!(
        b.prefixes
            .iter()
            .any(|p| p.rhs == "(F v0)" && p.members.len() == 2)
    );
    assert!(
        !b.prefixes
            .iter()
            .any(|p| p.rhs == "(F v0)" && p.members.len() == 4)
    );
}
fn joined() -> (EGraph, DependencyBlockStore, TraceSession) {
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
 (rule ((LL x) (RR x)) ((Out x)) :ruleset join :name "join")
 (LS 7) (RS 7)
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
    (
        eg,
        DependencyBlockStore::new(specs),
        TraceSession::with_dependencies(),
    )
}
#[test]
fn join_records_a_hyperedge_instead_of_merging_sources() {
    let (mut eg, mut s, t) = joined();
    for r in ["la", "lb", "ra", "rb", "join"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
    }
    assert_eq!(s.blocks().len(), 3);
    assert_eq!(s.blocks()[0].members.len(), 2);
    assert_eq!(s.blocks()[1].members.len(), 2);
    assert_eq!(s.blocks()[2].members.len(), 1);
    let e = &s.interactions()[0];
    assert_eq!(e.parents, vec![0, 1]);
    assert_eq!(e.target, 2);
    assert_eq!(e.reads.len(), 2);
    assert_eq!(s.interactions_for_rule("join"), &[0]);
    for b in [0, 1, 2] {
        assert_eq!(s.interactions_for_block(b), &[0]);
    }
    assert_eq!(s.blocks()[2].boundary.len(), 2);
    assert!(s.blocks()[2].prefixes.is_empty());
}
#[test]
fn real_removal_invalidates_blocks_and_downstream_interfaces() {
    let (mut eg, mut s, t) = joined();
    for r in ["la", "lb", "ra", "rb", "join"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
    }
    eg.parse_and_run_program(None, "(delete (LL 7))").unwrap();
    assert!(!t.origin_invalidations().is_empty());
    s.ingest_committed(&t).unwrap();
    assert!(!s.blocks()[0].active);
    assert!(s.blocks()[1].active);
    assert!(!s.blocks()[2].active);
}
#[test]
fn explicit_unknown_mutation_invalidates_concrete_prefixes() {
    let (mut eg, mut s, t) = chain();
    for r in ["a", "b", "c"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
    }
    s.invalidate_all("untracked union");
    assert!(!s.blocks()[0].active);
    assert!(s.blocks()[0].prefixes.iter().all(|p| !p.usable));
}
#[test]
fn unrelated_trace_sessions_are_rejected() {
    let (_, mut s, t) = chain();
    s.ingest_committed(&t).unwrap();
    assert!(
        s.ingest_committed(&TraceSession::with_dependencies())
            .is_err()
    );
}
#[test]
fn preexisting_results_do_not_seed_a_dependency_block() {
    let (mut eg, mut s, t) = chain();
    eg.parse_and_run_program(None, "(B (Var 7))").unwrap();
    for r in ["a", "b"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
    }
    assert!(s.blocks().is_empty());
}
#[test]
fn unregistered_prefixes_are_not_absorbed() {
    let (mut eg, _, t) = chain();
    let mut s = DependencyBlockStore::new([RuleSpec::rewrite("b", "(B x)", "(C x)")]);
    for r in ["a", "b"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
    }
    assert!(s.blocks().is_empty());
    assert!(!s.rejected().is_empty());
}
#[test]
fn rejected_join_does_not_promote_an_unrelated_seed() {
    let (mut eg, mut s, t) = joined();
    s.register_rule(RuleSpec::opaque("join_new", "((LL x) (RM x))", "(Out x)"))
        .unwrap();
    eg.parse_and_run_program(
        None,
        r#"(ruleset join_new) (rule ((LL x) (RM x)) ((Out x)) :ruleset join_new :name "join_new")"#,
    )
    .unwrap();
    for r in ["la", "lb", "ra"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
    }
    assert_eq!(s.blocks().len(), 1);
    s.invalidate_all("untracked union");
    eg.step_rules_with_trace("join_new", &t).unwrap();
    s.ingest_committed(&t).unwrap();
    assert_eq!(s.blocks().len(), 1);
    assert!(!s.rejected().is_empty());
}
#[test]
fn body_only_variables_can_still_have_complete_keyed_witnesses() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (relation Seed (i64)) (relation Mid (i64)) (relation Done ())
      (ruleset a) (ruleset b)
      (rule ((Seed x)) ((Mid x)) :ruleset a :name "a")
      (rule ((Mid x)) ((Done)) :ruleset b :name "b") (Seed 7)
    "#,
    )
    .unwrap();
    let mut s = DependencyBlockStore::new([
        RuleSpec::opaque("a", "(Seed x)", "(Mid x)"),
        RuleSpec::opaque("b", "(Mid x)", "(Done)"),
    ]);
    let t = TraceSession::with_dependencies();
    for r in ["a", "b"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
    }
    let b = t
        .matches()
        .into_iter()
        .find(|m| m.rule.as_ref() == "b")
        .unwrap();
    assert!(b.physical_witness_complete);
    assert_eq!(s.blocks().len(), 1);
    assert_eq!(s.block_for_match(b.event_id), Some(0));
    assert!(!s.rejected().contains_key(&b.event_id));
}
#[test]
fn an_actual_row_update_invalidates_the_concrete_block() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (relation Seed (i64)) (function F (i64) i64 :merge new) (relation Out (i64 i64))
      (ruleset a) (ruleset b)
      (rule ((Seed x)) ((set (F x) 1)) :ruleset a :name "a")
      (rule ((Seed x) (= y (F x))) ((Out x y)) :ruleset b :name "b") (Seed 7)
    "#,
    )
    .unwrap();
    let mut s = DependencyBlockStore::new([
        RuleSpec::opaque("a", "(Seed x)", "(set (F x) 1)"),
        RuleSpec::opaque("b", "((Seed x) (= y (F x)))", "(Out x y)"),
    ]);
    let t = TraceSession::with_dependencies();
    for r in ["a", "b"] {
        eg.step_rules_with_trace(r, &t).unwrap();
        s.ingest_committed(&t).unwrap();
    }
    assert_eq!(s.blocks().len(), 1);
    assert!(s.blocks()[0].active);
    eg.parse_and_run_program(None, "(set (F 7) 2)").unwrap();
    assert!(
        t.origin_invalidations()
            .iter()
            .any(|i| i.reason == egglog::OriginInvalidationReason::Updated)
    );
    s.ingest_committed(&t).unwrap();
    assert!(!s.blocks()[0].active);
}

#[test]
fn revalidation_checks_internal_rows_not_only_the_entry() {
    let (mut eg, mut store, trace) = chain();
    for rule in ["a", "b", "c"] {
        eg.step_rules_with_trace(rule, &trace).unwrap();
        store.ingest_committed(&trace).unwrap();
    }
    store.invalidate_all("request full validation");
    assert_eq!(store.revalidate_after_union(&eg), 1);
    eg.parse_and_run_program(None, "(delete (B (Var 7)))")
        .unwrap();
    store.ingest_committed(&trace).unwrap();
    assert_eq!(store.revalidate_after_union(&eg), 0);
    assert!(!store.blocks()[0].active);
    eg.parse_and_run_program(None, "(check (= (A (Var 7)) (D (Var 7))))")
        .unwrap();
    eg.parse_and_run_program(None, "(B (Var 7))").unwrap();
    assert_eq!(
        store.revalidate_after_union(&eg),
        0,
        "untraced reinsertion cannot revive a certificate"
    );
}
