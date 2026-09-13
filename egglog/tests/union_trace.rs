use egglog::{EGraph, TraceSession};
#[test]
fn committed_and_redundant_unions_have_distinct_events() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype E (Atom i64)) (Atom 1) (Atom 2)
      (ruleset a) (ruleset b)
      (rule ((= x (Atom 1)) (= y (Atom 2))) ((union x y)) :ruleset a :name "merge")
      (rule ((= x (Atom 1)) (= y (Atom 2))) ((union x y)) :ruleset b :name "redundant")
    "#,
    )
    .unwrap();
    let trace = TraceSession::with_dependencies();
    eg.step_rules_with_trace("a", &trace).unwrap();
    eg.step_rules_with_trace("b", &trace).unwrap();
    let events = trace.union_events();
    assert_eq!(events.iter().filter(|e| e.displaced.is_some()).count(), 1);
    assert!(events.iter().any(|e| e.displaced.is_none()));
    let matches = trace.matches();
    for e in &events {
        assert!(matches.iter().any(|m| m.event_id == e.match_event_id));
    }
    eg.parse_and_run_program(None, "(check (= (Atom 1) (Atom 2)))")
        .unwrap();
}

#[test]
fn rebuild_transports_a_row_only_with_a_committed_equality_path() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype E (Atom i64)) (Atom 1) (Atom 2)
      (relation Seed (E)) (relation Mid (E)) (relation Out (E))
      (Seed (Atom 2))
      (ruleset produce) (ruleset merge) (ruleset consume)
      (rule ((Seed x)) ((Mid x)) :ruleset produce :name "produce")
      (rule ((= x (Atom 1)) (= y (Atom 2))) ((union x y)) :ruleset merge :name "merge")
      (rule ((Mid x)) ((Out x)) :ruleset consume :name "consume")
    "#,
    )
    .unwrap();
    let trace = TraceSession::with_dependencies();
    eg.step_rules_with_trace("produce", &trace).unwrap();
    eg.step_rules_with_trace("merge", &trace).unwrap();
    eg.step_rules_with_trace("consume", &trace).unwrap();
    let writes = trace.write_events();
    let rebuilt: Vec<_> = writes.iter().filter(|w| w.rebuild_of.is_some()).collect();
    assert!(
        !rebuilt.is_empty(),
        "expected a changed-key Mid version: {writes:?}"
    );
    for w in &rebuilt {
        let old = writes
            .iter()
            .find(|old| Some(old.event_id) == w.rebuild_of)
            .unwrap();
        assert!(old.source_span.is_some());
        assert_eq!(
            w.source_span, old.source_span,
            "rebuild must retain the originating source site"
        );
        assert!(writes.iter().any(|old| Some(old.event_id) == w.rebuild_of));
        assert!(!w.union_dependencies.is_empty());
        assert!(w.union_dependencies.iter().all(|id| {
            trace
                .union_events()
                .iter()
                .any(|u| u.event_id == *id && u.displaced.is_some())
        }));
    }
    assert!(trace.row_reads().iter().any(|r| {
        rebuilt
            .iter()
            .any(|w| Some(w.event_id) == r.producer_write_event_id)
    }));
    eg.parse_and_run_program(None, "(check (Out (Atom 1)))")
        .unwrap();
    let e = trace
        .union_events()
        .into_iter()
        .find(|e| e.displaced.is_some())
        .unwrap();
    assert!(trace.equality_path(e.lhs, e.rhs).is_some());
    trace.record_scope_reset();
    assert!(trace.equality_path(e.lhs, e.rhs).is_none());
}

#[test]
fn untraced_union_does_not_invent_a_rebuild_proof() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype E (Atom i64)) (Atom 1) (Atom 2)
      (relation Mid (E)) (ruleset produce)
      (rule ((= x (Atom 2))) ((Mid x)) :ruleset produce :name "produce")
    "#,
    )
    .unwrap();
    let t = TraceSession::with_dependencies();
    eg.step_rules_with_trace("produce", &t).unwrap();
    eg.parse_and_run_program(None, "(union (Atom 1) (Atom 2))")
        .unwrap();
    assert!(t.union_events().is_empty());
    assert!(t.write_events().iter().all(|w| w.rebuild_of.is_none()));
}

#[test]
fn draining_unions_keeps_equality_paths_until_scope_reset() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype E (Atom i64)) (Atom 1) (Atom 2)
      (rule ((= x (Atom 1)) (= y (Atom 2))) ((union x y)) :name "merge")
    "#,
    )
    .unwrap();
    let trace = TraceSession::with_dependencies();
    eg.step_rules_with_trace("", &trace).unwrap();
    let batch = trace.drain_completed();
    let e = batch.unions.iter().find(|e| e.displaced.is_some()).unwrap();
    let (lhs, rhs, id) = (e.lhs, e.rhs, e.event_id);
    drop(batch);
    assert!(trace.union_events().is_empty());
    assert_eq!(trace.equality_path(lhs, rhs), Some(vec![id]));
    assert!(trace.drain_completed().is_empty());
    assert_eq!(trace.equality_path(lhs, rhs), Some(vec![id]));
    trace.record_scope_reset();
    trace.drain_completed();
    assert_eq!(trace.equality_path(lhs, rhs), None);
}
