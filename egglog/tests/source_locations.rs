use egglog::{EGraph, TraceSession, WriteOutcome};
fn slice<'a>(source: &'a str, id: &str) -> &'a str {
    let mut parts = id.rsplit(':');
    let end: usize = parts.next().unwrap().parse().unwrap();
    let start: usize = parts.next().unwrap().parse().unwrap();
    &source[start..end]
}
#[test]
fn nested_rhs_and_lhs_occurrences_survive_lowering_and_planning() {
    let source = r#"
 (datatype E (Var i64) (Add E E))
 (ruleset assoc-step) (ruleset comm-step)
 (rewrite (Add a (Add b c)) (Add (Add a b) c) :ruleset assoc-step :name "assoc")
 (rewrite (Add a b) (Add b a) :ruleset comm-step :name "comm")
 (Add (Var 1) (Add (Var 2) (Var 3)))
 "#;
    let mut eg = EGraph::default();
    eg.parse_and_run_program(Some("test.egg".into()), source)
        .unwrap();
    let t = TraceSession::with_dependencies();
    eg.step_rules_with_trace("assoc-step", &t).unwrap();
    eg.step_rules_with_trace("comm-step", &t).unwrap();
    let ms = t.matches();
    let a = ms.iter().find(|m| m.rule.as_ref() == "assoc").unwrap();
    let names: std::collections::BTreeMap<_, _> = t.table_names().into_iter().collect();
    let writes: Vec<_> = t
        .write_events()
        .into_iter()
        .filter(|w| {
            w.match_event_id == a.event_id
                && w.rebuild_of.is_none()
                && w.outcome == WriteOutcome::Inserted
                && names[&w.table].as_ref() == "Add"
        })
        .collect();
    let sites: std::collections::BTreeSet<_> = writes
        .iter()
        .map(|w| slice(source, w.source_span.as_deref().unwrap()))
        .collect();
    assert_eq!(
        sites,
        std::collections::BTreeSet::from(["(Add a b)", "(Add (Add a b) c)"])
    );
    let reads = t.row_reads();
    let consumer_reads: Vec<_> = reads
        .iter()
        .filter(|r| {
            r.producer_match_event_id == Some(a.event_id) && r.table_name.as_deref() == Some("Add")
        })
        .collect();
    assert_eq!(consumer_reads.len(), 2);
    for r in consumer_reads {
        assert_eq!(
            slice(source, r.source_span.as_deref().unwrap()),
            "(Add a b)"
        );
        let w = writes
            .iter()
            .find(|w| Some(w.event_id) == r.producer_write_event_id)
            .unwrap();
        assert_eq!(w.actual, r.row);
    }
    let assoc_read_sites: std::collections::BTreeSet<_> = reads
        .iter()
        .filter(|r| r.match_event_id == a.event_id && r.table_name.as_deref() == Some("Add"))
        .map(|r| slice(source, r.source_span.as_deref().unwrap()))
        .collect();
    assert_eq!(
        assoc_read_sites,
        std::collections::BTreeSet::from(["(Add b c)", "(Add a (Add b c))"])
    );
}
#[test]
fn source_locations_do_not_bleed_between_lanes() {
    let source =
        r#"(relation P (i64)) (relation Q (i64)) (P 1) (P 2) (rule ((P x)) ((Q x)) :name "copy")"#;
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, source).unwrap();
    let t = TraceSession::with_dependencies();
    eg.step_rules_with_trace("", &t).unwrap();
    assert_eq!(t.matches().len(), 2);
    for read in t.row_reads() {
        assert_eq!(slice(source, read.source_span.as_deref().unwrap()), "(P x)");
    }
    for write in t.write_events() {
        if write.outcome == WriteOutcome::Inserted {
            assert_eq!(
                slice(source, write.source_span.as_deref().unwrap()),
                "(Q x)"
            );
        }
    }
}
