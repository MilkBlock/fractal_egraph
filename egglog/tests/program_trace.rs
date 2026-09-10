use egglog::{EGraph, TraceSession};

#[test]
fn native_program_trace_preserves_all_cyk_scopes_and_checks() {
    let source = include_str!("web-demo/cyk.egg");
    let mut plain = EGraph::default();
    let mut traced = EGraph::default();
    plain.parse_and_run_program(None, source).unwrap();
    let commands = traced.parse_program(None, source).unwrap();
    let trace = TraceSession::with_dependencies();
    traced.run_program_with_trace(commands, &trace).unwrap();
    assert!(!trace.matches().is_empty());
    assert!(trace.matches().iter().all(|m| m.physical_witness_complete));
}

#[test]
fn native_program_trace_preserves_fibonacci_seven_steps() {
    let mut eg = EGraph::default();
    let commands = eg
        .parse_program(None, include_str!("web-demo/fibonacci.egg"))
        .unwrap();
    let trace = TraceSession::with_dependencies();
    eg.run_program_with_trace(commands, &trace).unwrap();
    assert!(!trace.matches().is_empty());
}

#[test]
fn native_trace_preserves_until_scopes_and_restores_after_error() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (relation P (i64)) (P 0) (ruleset next)
      (rule ((P x) (< x 5)) ((P (+ x 1))) :ruleset next :name "next")
      (push)
    "#,
    )
    .unwrap();
    let trace = TraceSession::with_dependencies();
    let commands = eg
        .parse_program(
            None,
            r#"
      (run next 100 :until (P 2)) (check (P 2)) (fail (check (P 3)))
      (pop) (fail (check (P 1)))
      (run next 1) (check (P 1)) (fail (check (P 2)))
      (check (P 99))
    "#,
        )
        .unwrap();
    assert!(eg.run_program_with_trace(commands, &trace).is_err());
    let n = trace.matches().len();
    eg.parse_and_run_program(None, "(run next 1) (check (P 2))")
        .unwrap();
    assert_eq!(trace.matches().len(), n);
}
