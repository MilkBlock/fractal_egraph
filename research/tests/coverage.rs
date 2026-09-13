use egg_layout::{
    coverage::{CoverageLedger, CoverageState},
    dependency_blocks::{DependencyBlockStore, RuleSpec},
};
use egglog::{EGraph, TraceSession};
fn fixture() -> (EGraph, DependencyBlockStore, CoverageLedger, TraceSession) {
    let specs = vec![
        RuleSpec::rewrite("a", "(A x)", "(B x)"),
        RuleSpec::rewrite("b", "(B x)", "(C x)"),
        RuleSpec::rewrite("c", "(C x)", "(D x)"),
    ];
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (Var i64) (A E) (B E) (C E) (D E) (Cold E)) (A (Var 7)) (Cold (Var 99))",
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
    let ledger = CoverageLedger::new(
        ["Var", "A", "B", "C", "D", "Cold"]
            .into_iter()
            .map(str::to_owned),
    );
    (
        eg,
        DependencyBlockStore::new(specs),
        ledger,
        TraceSession::with_dependencies(),
    )
}
fn step(
    eg: &mut EGraph,
    s: &mut DependencyBlockStore,
    c: &mut CoverageLedger,
    t: &TraceSession,
    r: &str,
) {
    eg.step_rules_with_trace(r, t).unwrap();
    s.ingest_committed(t).unwrap();
    c.sync_blocks(eg, s, t).unwrap();
}
#[test]
fn inventory_includes_initial_and_cold_rows() {
    let (eg, _, mut c, _) = fixture();
    c.refresh(&eg).unwrap();
    let s = c.stats();
    assert_eq!(s.live_facts, 4);
    assert_eq!(s.covered_facts, 0);
    assert_eq!(s.states[&CoverageState::Unobserved], 4);
}
#[test]
fn a_single_application_is_seed_only() {
    let (mut eg, mut s, mut c, t) = fixture();
    step(&mut eg, &mut s, &mut c, &t, "a");
    assert!(c.views().is_empty());
    assert_eq!(c.stats().states[&CoverageState::SeedOnly], 2);
    assert_eq!(c.stats().live_facts, 5);
}
#[test]
fn overlap_counts_union_not_sum_and_preserves_native_owner() {
    let (mut eg, mut s, mut c, t) = fixture();
    for r in ["a", "b", "c"] {
        step(&mut eg, &mut s, &mut c, &t, r);
    }
    assert_eq!(c.views().len(), 2);
    let stats = c.stats();
    assert_eq!(stats.live_facts, 7);
    assert_eq!(stats.covered_facts, 4);
    assert_eq!(stats.active_memberships, 7);
    assert_eq!(stats.duplicated_memberships, 3);
    let b = c
        .facts()
        .iter()
        .find(|f| f.live && f.key.table == "B")
        .unwrap();
    assert_eq!(c.memberships(b.id), vec![0, 1]);
    assert_eq!(b.native_owner, "B");
    assert_eq!(c.marginal_new_facts(1, &[0]), 1);
    assert_eq!(c.marginal_new_facts(0, &[1]), 0);
    c.sync_blocks(&eg, &s, &t).unwrap();
    assert_eq!(c.stats().active_memberships, 7);
    assert_eq!(c.views().len(), 2);
}
#[test]
fn invalidating_one_view_does_not_remove_other_coverage() {
    let (mut eg, mut s, mut c, t) = fixture();
    for r in ["a", "b", "c"] {
        step(&mut eg, &mut s, &mut c, &t, r);
    }
    c.invalidate_view(1, "selection withdrawn");
    assert_eq!(c.stats().covered_facts, 3);
    let b = c
        .facts()
        .iter()
        .find(|f| f.live && f.key.table == "B")
        .unwrap();
    assert_eq!(c.state(b.id), CoverageState::Covered);
    let d = c
        .facts()
        .iter()
        .find(|f| f.live && f.key.table == "D")
        .unwrap();
    assert_eq!(c.state(d.id), CoverageState::Invalidated);
}
#[test]
fn real_row_removal_retires_fact_and_invalidates_views() {
    let (mut eg, mut s, mut c, t) = fixture();
    for r in ["a", "b", "c"] {
        step(&mut eg, &mut s, &mut c, &t, r);
    }
    let b = c
        .facts()
        .iter()
        .find(|f| f.live && f.key.table == "B")
        .unwrap()
        .id;
    eg.parse_and_run_program(None, "(delete (B (Var 7)))")
        .unwrap();
    s.ingest_committed(&t).unwrap();
    c.sync_blocks(&eg, &s, &t).unwrap();
    assert_eq!(c.state(b), CoverageState::Retired);
    assert_eq!(c.stats().covered_facts, 0);
    assert!(c.views().iter().all(|v| !v.active));
}
#[test]
fn unknown_rule_reads_are_reported_as_unsupported() {
    let (mut eg, _, mut c, t) = fixture();
    let mut s = DependencyBlockStore::new([]);
    eg.step_rules_with_trace("a", &t).unwrap();
    s.ingest_committed(&t).unwrap();
    c.sync_blocks(&eg, &s, &t).unwrap();
    assert_eq!(c.stats().states[&CoverageState::Unsupported], 2);
    assert!(c.views().is_empty());
}
#[test]
fn a_reinserted_row_cannot_resurrect_an_old_view() {
    let (mut eg, mut s, mut c, t) = fixture();
    for r in ["a", "b", "c"] {
        step(&mut eg, &mut s, &mut c, &t, r);
    }
    let old = c
        .facts()
        .iter()
        .find(|f| f.live && f.key.table == "B")
        .unwrap()
        .id;
    eg.parse_and_run_program(None, "(delete (B (Var 7)))")
        .unwrap();
    s.ingest_committed(&t).unwrap();
    c.sync_blocks(&eg, &s, &t).unwrap();
    eg.parse_and_run_program(None, "(B (Var 7))").unwrap();
    s.ingest_committed(&t).unwrap();
    c.sync_blocks(&eg, &s, &t).unwrap();
    let new = c
        .facts()
        .iter()
        .find(|f| f.live && f.key.table == "B")
        .unwrap()
        .id;
    assert_ne!(old, new);
    assert_eq!(c.state(old), CoverageState::Retired);
    assert!(c.memberships(new).is_empty());
    assert_eq!(c.stats().covered_facts, 0);
}
