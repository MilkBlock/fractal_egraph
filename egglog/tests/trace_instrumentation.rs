use egglog::{EGraph, RuleActionOutcome, TraceSession};

#[test]
fn trace_records_rewrite_logical_substitution() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(
            None,
            r#"
                (datatype E (Atom i64) (Neg E) (Abs E))
                (ruleset traced)
                (rewrite (Neg x) (Abs x)
                    :ruleset traced
                    :name "neg-to-abs")
                (let root (Neg (Atom 1)))
            "#,
        )
        .unwrap();

    let trace = TraceSession::new();
    let report = egraph.step_rules_with_trace("traced", &trace).unwrap();
    let events = trace.drain_matches();
    let outcomes = trace.drain_action_outcomes();

    assert!(report.updated);
    assert_eq!(events.len(), 1, "{events:#?}");
    assert_eq!(&*events[0].rule, "neg-to-abs");
    let names = events[0]
        .bindings
        .iter()
        .filter_map(|binding| binding.name.as_deref())
        .collect::<Vec<_>>();
    assert!(names.contains(&"x"), "{names:?}");
    assert!(
        names.len() >= 2,
        "rewrite lowering should retain its generated root binding: {names:?}"
    );
    assert!(!events[0].physical_witness_complete);
    assert_eq!(outcomes.len(), 1, "{outcomes:#?}");
    assert_eq!(outcomes[0].match_event_id, events[0].event_id);
    assert_eq!(outcomes[0].outcome, RuleActionOutcome::Survived);
}

#[test]
fn trace_does_not_confuse_a_match_with_a_database_change() {
    let mut egraph = EGraph::default();
    egraph
        .parse_and_run_program(
            None,
            r#"
                (datatype E (Atom))
                (relation Seen (E))
                (let a (Atom))
                (Seen a)
                (ruleset traced)
                (rule ((Seen x))
                      ((Seen x))
                      :ruleset traced
                      :name "redundant")
            "#,
        )
        .unwrap();

    let trace = TraceSession::new();
    let report = egraph.step_rules_with_trace("traced", &trace).unwrap();
    let events = trace.drain_matches();
    let outcomes = trace.drain_action_outcomes();

    assert_eq!(events.len(), 1, "{events:#?}");
    assert_eq!(&*events[0].rule, "redundant");
    assert_eq!(outcomes.len(), 1, "{outcomes:#?}");
    assert_eq!(outcomes[0].match_event_id, events[0].event_id);
    assert_eq!(outcomes[0].outcome, RuleActionOutcome::Survived);
    assert!(
        !report.updated,
        "a recorded logical match must not imply an effective mutation: {report:#?}"
    );
}
