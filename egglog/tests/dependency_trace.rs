use egglog::{EGraph, TraceSession, WriteOutcome};
fn graph(preexisting: bool) -> EGraph {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (relation Seed (i64)) (relation Mid (i64)) (relation Out (i64))
 (ruleset a) (ruleset b)
 (rule ((Seed x)) ((Mid x)) :ruleset a :name "A")
 (rule ((Mid x)) ((Out x)) :ruleset b :name "B")
 (Seed 7)
 "#,
    )
    .unwrap();
    if preexisting {
        eg.parse_and_run_program(None, "(Mid 7)").unwrap();
    }
    eg
}
#[test]
fn committed_insert_is_consumed_by_later_match() {
    let mut eg = graph(false);
    let trace = TraceSession::with_dependencies();
    eg.step_rules_with_trace("a", &trace).unwrap();
    eg.step_rules_with_trace("b", &trace).unwrap();
    let ms = trace.matches();
    let a = ms.iter().find(|m| m.rule.as_ref() == "A").unwrap();
    let b = ms.iter().find(|m| m.rule.as_ref() == "B").unwrap();
    assert!(b.physical_witness_complete, "{ms:?}");
    assert!(
        trace
            .write_events()
            .iter()
            .any(|w| w.match_event_id == a.event_id && w.outcome == WriteOutcome::Inserted)
    );
    assert!(
        trace.row_reads().iter().any(
            |r| r.match_event_id == b.event_id && r.producer_match_event_id == Some(a.event_id)
        ),
        "{:?}",
        trace.row_reads()
    );
    eg.parse_and_run_program(None, "(check (Out 7))").unwrap();
}
#[test]
fn preexisting_result_does_not_become_a_dependency() {
    let mut eg = graph(true);
    let trace = TraceSession::with_dependencies();
    assert!(!eg.step_rules_with_trace("a", &trace).unwrap().updated);
    eg.step_rules_with_trace("b", &trace).unwrap();
    let ms = trace.matches();
    let a = ms.iter().find(|m| m.rule.as_ref() == "A").unwrap();
    let b = ms.iter().find(|m| m.rule.as_ref() == "B").unwrap();
    assert!(
        !trace
            .write_events()
            .iter()
            .any(|w| w.match_event_id == a.event_id && w.outcome == WriteOutcome::Inserted)
    );
    assert!(
        trace
            .row_reads()
            .iter()
            .filter(|r| r.match_event_id == b.event_id)
            .all(|r| r.producer_match_event_id.is_none())
    );
}
#[test]
fn competing_producers_credit_only_the_committed_winner() {
    let mut eg = graph(false);
    eg.parse_and_run_program(None, r#"(rule ((Seed x)) ((Mid x)) :ruleset a :name "A2")"#)
        .unwrap();
    let trace = TraceSession::with_dependencies();
    eg.step_rules_with_trace("a", &trace).unwrap();
    eg.step_rules_with_trace("b", &trace).unwrap();
    let ms = trace.matches();
    let b = ms.iter().find(|m| m.rule.as_ref() == "B").unwrap();
    let ids: Vec<_> = ms
        .iter()
        .filter(|m| m.rule.as_ref() == "A" || m.rule.as_ref() == "A2")
        .map(|m| m.event_id)
        .collect();
    let writes = trace.write_events();
    let winners: Vec<_> = writes
        .iter()
        .filter(|w| ids.contains(&w.match_event_id) && w.outcome == WriteOutcome::Inserted)
        .collect();
    assert_eq!(winners.len(), 1, "{writes:?}");
    assert!(
        trace
            .row_reads()
            .iter()
            .any(|r| r.match_event_id == b.event_id
                && r.producer_match_event_id == Some(winners[0].match_event_id))
    );
}
#[test]
fn traced_and_untraced_results_agree_across_batches() {
    let mut a = graph(false);
    for i in 0..300 {
        a.parse_and_run_program(None, &format!("(Seed {i})"))
            .unwrap();
    }
    let mut b = a.clone();
    let trace = TraceSession::with_dependencies();
    for rule in ["a", "b"] {
        assert_eq!(
            a.step_rules_with_trace(rule, &trace).unwrap().updated,
            b.step_rules(rule).unwrap().updated
        );
    }
    let collect = |eg: &EGraph| {
        let mut rows = Vec::new();
        eg.function_for_each("Out", |r| rows.push(eg.value_to_base::<i64>(r.vals[0])))
            .unwrap();
        rows.sort();
        rows
    };
    assert_eq!(collect(&a), collect(&b));
    assert_eq!(collect(&a).len(), 300);
    let mids: std::collections::HashSet<_> = trace
        .matches()
        .iter()
        .filter(|m| m.rule.as_ref() == "B")
        .map(|m| m.event_id)
        .collect();
    assert_eq!(
        trace
            .row_reads()
            .iter()
            .filter(|r| mids.contains(&r.match_event_id) && r.producer_match_event_id.is_some())
            .count(),
        300
    );
}
#[test]
fn default_trace_does_not_enable_dependency_mode() {
    let mut eg = graph(false);
    let trace = TraceSession::new();
    eg.step_rules_with_trace("a", &trace).unwrap();
    assert!(trace.row_reads().is_empty());
    assert!(trace.write_events().is_empty());
    assert!(trace.matches().iter().all(|m| !m.physical_witness_complete));
}
#[test]
fn overwrite_invalidates_old_producer() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (relation Seed (i64)) (function F (i64) i64 :merge new) (relation Out (i64 i64))
 (ruleset a) (ruleset c) (ruleset b)
 (rule ((Seed x)) ((set (F x) 1)) :ruleset a :name "A")
 (rule ((Seed x)) ((set (F x) 2)) :ruleset c :name "C")
 (rule ((Seed x) (= y (F x))) ((Out x y)) :ruleset b :name "B")
 (Seed 7)
 "#,
    )
    .unwrap();
    let t = TraceSession::with_dependencies();
    for r in ["a", "c", "b"] {
        eg.step_rules_with_trace(r, &t).unwrap();
    }
    let ms = t.matches();
    let b = ms.iter().find(|m| m.rule.as_ref() == "B").unwrap();
    assert!(
        t.write_events()
            .iter()
            .any(|w| w.outcome == WriteOutcome::Updated)
    );
    assert!(
        t.row_reads()
            .iter()
            .filter(|r| r.match_event_id == b.event_id)
            .all(|r| r.producer_match_event_id.is_none())
    );
    eg.parse_and_run_program(None, "(check (Out 7 2))").unwrap();
}
#[test]
fn delete_and_untraced_reinsert_do_not_reuse_a_producer() {
    let mut eg = graph(false);
    let t = TraceSession::with_dependencies();
    eg.step_rules_with_trace("a", &t).unwrap();
    eg.parse_and_run_program(None, "(delete (Mid 7)) (Mid 7)")
        .unwrap();
    eg.step_rules_with_trace("b", &t).unwrap();
    let b = t
        .matches()
        .into_iter()
        .find(|m| m.rule.as_ref() == "B")
        .unwrap();
    assert!(
        t.row_reads()
            .iter()
            .filter(|r| r.match_event_id == b.event_id)
            .all(|r| r.producer_match_event_id.is_none())
    );
}
#[test]
fn new_constructor_row_has_a_real_rewrite_dependency() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (datatype E (Var i64) (A E) (B E) (C E))
 (ruleset a) (ruleset b)
 (rewrite (A x) (B x) :ruleset a :name "A-to-B")
 (rewrite (B x) (C x) :ruleset b :name "B-to-C")
 (A (Var 7))
 "#,
    )
    .unwrap();
    let t = TraceSession::with_dependencies();
    eg.step_rules_with_trace("a", &t).unwrap();
    eg.step_rules_with_trace("b", &t).unwrap();
    let ms = t.matches();
    let a = ms.iter().find(|m| m.rule.as_ref() == "A-to-B").unwrap();
    let b = ms.iter().find(|m| m.rule.as_ref() == "B-to-C").unwrap();
    let reads = t.row_reads();
    let edge = reads
        .iter()
        .find(|r| r.match_event_id == b.event_id && r.producer_match_event_id == Some(a.event_id))
        .expect("B reads the constructor committed by A");
    assert_eq!(edge.table_name.as_deref(), Some("B"));
    let writes = t.write_events();
    let commit = writes
        .iter()
        .find(|w| Some(w.event_id) == edge.producer_write_event_id)
        .unwrap();
    assert_eq!(commit.outcome, WriteOutcome::Inserted);
    assert_eq!(commit.actual, edge.row);
    eg.parse_and_run_program(None, "(check (= (A (Var 7)) (C (Var 7))))")
        .unwrap();
}
#[test]
fn constant_key_reads_have_exact_witnesses() {
    let mut eg = graph(false);
    eg.parse_and_run_program(None,r#"(relation Done ()) (ruleset fixed) (rule ((Mid 7)) ((Done)) :ruleset fixed :name "fixed-B")"#).unwrap();
    let t = TraceSession::with_dependencies();
    eg.step_rules_with_trace("a", &t).unwrap();
    eg.step_rules_with_trace("fixed", &t).unwrap();
    let b = t
        .matches()
        .into_iter()
        .find(|m| m.rule.as_ref() == "fixed-B")
        .unwrap();
    assert!(b.physical_witness_complete);
    assert!(
        t.row_reads()
            .iter()
            .any(|r| r.match_event_id == b.event_id && r.producer_match_event_id.is_some())
    );
}
#[test]
fn eliminated_key_variables_do_not_create_guessed_witnesses() {
    let mut eg = graph(false);
    eg.parse_and_run_program(None,r#"(relation Done ()) (ruleset exists) (rule ((Mid x)) ((Done)) :ruleset exists :name "exists-B")"#).unwrap();
    let t = TraceSession::with_dependencies();
    eg.step_rules_with_trace("a", &t).unwrap();
    eg.step_rules_with_trace("exists", &t).unwrap();
    for b in t.matches().iter().filter(|m| m.rule.as_ref() == "exists-B") {
        if !b.bindings.iter().any(|b| b.name.as_deref() == Some("x")) {
            assert!(!b.physical_witness_complete);
            assert!(
                !t.row_reads()
                    .iter()
                    .any(|r| r.match_event_id == b.event_id && r.producer_match_event_id.is_some())
            );
        }
    }
}
#[test]
fn equality_enabled_matches_are_not_claimed_as_row_dependencies() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (datatype E (Atom i64) (Wrap E)) (relation Target (E)) (relation Result (E E))
 (let a (Atom 1)) (let b (Atom 2)) (Target a) (Wrap b)
 (ruleset merge) (ruleset consume)
 (rule ((= x (Atom 1)) (= y (Atom 2))) ((union x y)) :ruleset merge :name "merge-A")
 (rule ((Target x) (= n (Wrap x))) ((Result x n)) :ruleset consume :name "consume-B")
 "#,
    )
    .unwrap();
    assert!(!eg.step_rules("consume").unwrap().updated);
    let t = TraceSession::with_dependencies();
    eg.step_rules_with_trace("merge", &t).unwrap();
    eg.step_rules_with_trace("consume", &t).unwrap();
    let ms = t.matches();
    let a = ms.iter().find(|m| m.rule.as_ref() == "merge-A").unwrap();
    assert!(ms.iter().any(|m| m.rule.as_ref() == "consume-B"));
    assert!(
        !t.row_reads()
            .iter()
            .any(|r| r.producer_match_event_id == Some(a.event_id)),
        "union lineage is deliberately unsupported"
    );
    eg.parse_and_run_program(None, "(check (Result (Atom 1) (Wrap (Atom 2))))")
        .unwrap();
}

#[test]
fn parallel_search_preserves_every_committed_row_dependency() {
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap()
        .install(|| {
            const N: usize = 20_001;
            let mut eg = graph(false);
            let program: String = (0..N).map(|i| format!("(Seed {i})\n")).collect();
            eg.parse_and_run_program(None, &program).unwrap();
            let mut plain = eg.clone();
            let trace = TraceSession::with_dependencies();
            for ruleset in ["a", "b"] {
                assert_eq!(
                    eg.step_rules_with_trace(ruleset, &trace).unwrap().updated,
                    plain.step_rules(ruleset).unwrap().updated
                );
            }
            let matches = trace.matches();
            let consumers: std::collections::HashSet<_> = matches
                .iter()
                .filter(|m| m.rule.as_ref() == "B")
                .map(|m| m.event_id)
                .collect();
            assert_eq!(consumers.len(), N);
            let writes = trace.write_events();
            let by_id: std::collections::HashMap<_, _> =
                writes.iter().map(|w| (w.event_id, w)).collect();
            let reads = trace.row_reads();
            let edges: Vec<_> = reads
                .iter()
                .filter(|r| consumers.contains(&r.match_event_id))
                .collect();
            assert_eq!(edges.len(), N);
            for edge in edges {
                let write = by_id[&edge.producer_write_event_id.expect("committed producer")];
                assert_eq!(write.outcome, WriteOutcome::Inserted);
                assert_eq!(Some(write.match_event_id), edge.producer_match_event_id);
                assert_eq!(write.actual, edge.row);
            }
            let collect = |g: &EGraph| {
                let mut xs = Vec::new();
                g.function_for_each("Out", |r| xs.push(g.value_to_base::<i64>(r.vals[0])))
                    .unwrap();
                xs.sort();
                xs
            };
            assert_eq!(collect(&eg), collect(&plain));
            assert_eq!(collect(&eg).len(), N);
        });
}

#[test]
fn deleting_one_key_preserves_other_row_origins() {
    let mut eg = graph(false);
    eg.parse_and_run_program(None, "(Seed 8)").unwrap();
    let t = TraceSession::with_dependencies();
    eg.step_rules_with_trace("a", &t).unwrap();
    eg.parse_and_run_program(None, "(delete (Mid 999))")
        .unwrap();
    assert!(t.origin_invalidations().is_empty());
    eg.parse_and_run_program(None, "(delete (Mid 7)) (Mid 7)")
        .unwrap();
    assert_eq!(t.origin_invalidations().len(), 1);
    eg.step_rules_with_trace("b", &t).unwrap();
    let reads = t.row_reads();
    let mid: Vec<_> = reads
        .iter()
        .filter(|r| r.table_name.as_deref() == Some("Mid"))
        .collect();
    assert_eq!(mid.len(), 2);
    assert_eq!(
        mid.iter()
            .filter(|r| r.producer_write_event_id.is_some())
            .count(),
        1
    );
    eg.parse_and_run_program(None, "(check (Out 7)) (check (Out 8))")
        .unwrap();
}
