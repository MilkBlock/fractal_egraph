use egglog::{EGraph, TraceSession};
#[test]
fn fibonacci_with_primitive_keys_agrees_with_normal_execution() {
    let mut traced = EGraph::default();
    traced
        .parse_and_run_program(
            None,
            r#"
 (function fib (i64) i64 :no-merge)
 (set (fib 0) 0) (set (fib 1) 1)
 (rule ((= f0 (fib x)) (= f1 (fib (+ x 1))))
       ((set (fib (+ x 2)) (+ f0 f1))) :name "fib-step")
 "#,
        )
        .unwrap();
    let mut plain = traced.clone();
    let t = TraceSession::with_dependencies();
    for _ in 0..7 {
        assert_eq!(
            traced.step_rules_with_trace("", &t).unwrap().updated,
            plain.step_rules("").unwrap().updated
        );
    }
    for g in [&mut traced, &mut plain] {
        g.parse_and_run_program(None, "(check (= (fib 7) 13))")
            .unwrap();
    }
}
#[test]
fn computed_relation_key_agrees_with_normal_execution() {
    let mut traced = EGraph::default();
    traced
        .parse_and_run_program(
            None,
            r#"
 (relation P (i64 i64)) (relation Out (i64))
 (P 0 7) (P 1 8)
 (rule ((P x a) (P (+ x 1) b)) ((Out (+ a b))) :name "adjacent")
 "#,
        )
        .unwrap();
    let mut plain = traced.clone();
    let t = TraceSession::with_dependencies();
    assert_eq!(
        traced.step_rules_with_trace("", &t).unwrap().updated,
        plain.step_rules("").unwrap().updated
    );
    for g in [&mut traced, &mut plain] {
        g.parse_and_run_program(None, "(check (Out 15))").unwrap();
    }
    assert!(t.matches().iter().all(|m| m.physical_witness_complete));
    assert!(
        t.row_reads()
            .iter()
            .all(|r| r.producer_match_event_id.is_none())
    );
}

#[test]
fn cyk_first_case_preserves_command_order_and_checks() {
    use egglog::ast::Command;
    let source = include_str!("web-demo/cyk.egg");
    let mut traced = EGraph::default();
    let commands = traced.parse_program(None, source).unwrap();
    let mut plain = EGraph::default();
    let trace = TraceSession::with_dependencies();
    for command in commands {
        if matches!(command, Command::Pop(..)) {
            break;
        }
        if matches!(command, Command::RunSchedule(..)) {
            // This fixture's first schedule is exactly (run 100).
            assert_eq!(command.to_string(), "(run-schedule (repeat 100 (run)))");
            plain.run_program(vec![command]).unwrap();
            for _ in 0..100 {
                if !traced.step_rules_with_trace("", &trace).unwrap().updated {
                    break;
                }
            }
        } else {
            plain.run_program(vec![command.clone()]).unwrap();
            traced.run_program(vec![command]).unwrap();
        }
    }
    assert!(trace.matches().iter().all(|m| m.physical_witness_complete));
    assert!(
        trace
            .row_reads()
            .iter()
            .any(|r| r.producer_write_event_id.is_some())
    );
    // Union rebuilds tree constructors, not the P relation in this fixture.
    let writes = trace.write_events();
    let names = trace.table_names();
    for inv in trace.origin_invalidations() {
        let w = writes
            .iter()
            .find(|w| w.event_id == inv.write_event_id)
            .unwrap();
        let (_, name) = names.iter().find(|(id, _)| *id == w.table).unwrap();
        assert_ne!(name.as_ref(), "P");
    }
}

#[test]
fn extending_fibonacci_to_100_steps_fails_in_both_modes() {
    let mut failures = Vec::new();
    for tracing in [false, true] {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
        (function fib (i64) i64 :no-merge)
        (set (fib 0) 0) (set (fib 1) 1)
        (rule ((= f0 (fib x)) (= f1 (fib (+ x 1))))
              ((set (fib (+ x 2)) (+ f0 f1))))
        "#,
        )
        .unwrap();
        let trace = TraceSession::with_dependencies();
        let failure = (0..100)
            .find_map(|step| {
                let result = if tracing {
                    eg.step_rules_with_trace("", &trace)
                } else {
                    eg.step_rules("")
                };
                result.err().map(|error| (step, error.to_string()))
            })
            .expect("extending the schedule must fail on i64 Fibonacci addition");
        assert!(failure.1.contains("call of primitive + failed"));
        failures.push(failure);
    }
    assert_eq!(failures[0], failures[1]);
}
