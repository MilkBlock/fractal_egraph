use egglog::{EGraph, TraceSession};
use std::time::Duration;
fn setup() -> EGraph {
    let mut eg = EGraph::default();
    let mut source = String::from(
        "(relation A (i64)) (relation Pair (i64 i64)) (rule ((A x) (A y)) ((Pair x y)))",
    );
    for i in 0..30 {
        source += &format!("(A {i})");
    }
    eg.parse_and_run_program(None, &source).unwrap();
    eg
}
fn count(eg: &EGraph) -> usize {
    let mut n = 0;
    eg.function_for_each("Pair", |_| n += 1).unwrap();
    n
}
#[test]
fn interrupted_query_commits_prefix_and_resume_does_not_skip_rows() {
    let mut eg = setup();
    let trace = TraceSession::with_dependencies();
    trace.set_execution_limits(None, Some(40));
    eg.step_rules_with_trace("", &trace).unwrap();
    assert_eq!(
        trace.execution_stop_reason(),
        Some("pending_fact_reservations")
    );
    assert!(count(&eg) < 900);
    let batch = trace.drain_completed();
    assert!(batch.matches.len() <= 40);
    trace.clear_execution_limits();
    for _ in 0..3 {
        eg.step_rules_with_trace("", &trace).unwrap();
        trace.drain_completed();
    }
    assert_eq!(count(&eg), 900);
}
#[test]
fn expired_deadline_allocates_no_match_trace_and_can_resume() {
    let mut eg = setup();
    let trace = TraceSession::with_dependencies();
    trace.set_execution_limits(Some(Duration::ZERO), None);
    eg.step_rules_with_trace("", &trace).unwrap();
    assert_eq!(trace.execution_stop_reason(), Some("time"));
    assert!(trace.drain_completed().matches.is_empty());
    trace.clear_execution_limits();
    eg.step_rules_with_trace("", &trace).unwrap();
    assert_eq!(count(&eg), 900);
}
#[test]
fn interrupted_union_still_rebuilds_and_resumes() {
    let mut eg = EGraph::default();
    let mut text =
        String::from("(datatype T (X i64) (Y i64)) (rule ((= a (X i)) (= b (Y i))) ((union a b)))");
    for i in 0..30 {
        text += &format!("(X {i}) (Y {i})");
    }
    eg.parse_and_run_program(None, &text).unwrap();
    let trace = TraceSession::with_dependencies();
    trace.set_execution_limits(None, Some(40));
    eg.step_rules_with_trace("", &trace).unwrap();
    assert!(trace.execution_was_interrupted());
    assert!(!trace.union_events().is_empty());
    trace.clear_execution_limits();
    eg.step_rules_with_trace("", &trace).unwrap();
    for i in 0..30 {
        eg.parse_and_run_program(None, &format!("(check (= (X {i}) (Y {i})))"))
            .unwrap();
    }
}
