use cell_assisted_egglog::{Part, Session, resident::ResidentSession};
use egglog::EGraph;
use std::path::Path;
fn session() -> Option<ResidentSession> {
    let worker = std::env::var_os("CELL_RESIDENT_WORKER")?;
    let mut s = Session::new(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze"),
        "",
        "route-get",
        vec!["single-get".into()],
        vec![
            Part {
                atoms: vec![0],
                cell: true,
            },
            Part {
                atoms: vec![1],
                cell: false,
            },
        ],
    )
    .unwrap();
    s.set_audit(true);
    Some(ResidentSession::new(s, Path::new(&worker)).unwrap())
}
fn check(eg: &EGraph, text: &str) -> bool {
    eg.clone().parse_and_run_program(None, text).is_ok()
}
#[test]
fn device_maintains_cells_and_late_union_boundary() {
    let Some(mut s) = session() else { return };
    s.input("(Single (Leaf 100)) (Add-Gets (Leaf 200) (Leaf 999) 7)")
        .unwrap();
    s.round().unwrap();
    assert!(!check(s.engine(), "(check (Get (Leaf 200) 7))"));
    s.input("(union (Leaf 999) (Single (Leaf 100)))").unwrap();
    s.round().unwrap();
    assert!(check(
        s.engine(),
        "(check (= (Get (Leaf 200) 7) (Leaf 100)))"
    ));
    assert!(s.stats.union_deltas > 0);
    assert_eq!(s.stats.cpu_cell_point_reads, 0);
    assert_eq!(s.stats.cpu_cell_validations, 0);
    assert_eq!(s.stats.diagnostic_trace_events, 0);
}
#[test]
fn delete_and_subsume_revoke_device_proof() {
    let Some(mut s) = session() else { return };
    s.input("(let root (Single (Leaf 100)))").unwrap();
    s.round().unwrap();
    s.input("(subsume (Single (Leaf 100))) (Add-Gets (Leaf 201) root 8)")
        .unwrap();
    s.round().unwrap();
    assert!(!check(s.engine(), "(check (Get (Leaf 201) 8))"));
    let Some(mut s) = session() else { return };
    s.input("(let root (Single (Leaf 100)))").unwrap();
    s.round().unwrap();
    s.input("(delete (Get root 0)) (Add-Gets (Leaf 201) root 8)")
        .unwrap();
    s.round().unwrap();
    assert!(!check(s.engine(), "(check (Get (Leaf 201) 8))"));
    // A temporarily incomplete Cell is uncovered, never a false negative fact.
    assert_eq!(s.stats.audit_uncovered_per_round.last(), Some(&1));
}
#[test]
fn late_fact_and_reinsert_are_not_missed() {
    let Some(mut s) = session() else { return };
    s.input("(Single (Leaf 100))").unwrap();
    s.round().unwrap();
    s.input("(Add-Gets (Leaf 201) (Single (Leaf 100)) 13)")
        .unwrap();
    s.round().unwrap();
    assert!(check(
        s.engine(),
        "(check (= (Get (Leaf 201) 13) (Leaf 100)))"
    ));
    s.input("(delete (Add-Gets (Leaf 201) (Single (Leaf 100)) 13))")
        .unwrap();
    s.round().unwrap();
    let before = s.stats.dispatched_bindings;
    s.input("(Add-Gets (Leaf 201) (Single (Leaf 100)) 13)")
        .unwrap();
    s.round().unwrap();
    assert_eq!(s.stats.dispatched_bindings, before + 1);
}
#[test]
fn idle_round_does_not_revalidate_or_resubmit_device_work() {
    let Some(mut s) = session() else { return };
    s.input("(Single (Leaf 100))").unwrap();
    s.round().unwrap();
    let d = s.stats.gpu.clone();
    let uploads = s.stats.uploaded_bytes;
    assert!(!s.round().unwrap());
    assert_eq!(s.stats.uploaded_bytes, uploads);
    assert_eq!(s.stats.gpu, d);
    assert_eq!(s.stats.quiet_rounds, 1);
}
#[test]
fn output_and_watch_growth_retry_without_losing_bindings() {
    let Some(mut s) = session() else { return };
    let input = (0..300)
        .map(|i| format!("(Add-Gets (Leaf {}) (Single (Leaf {i})) {i})", 1000 + i))
        .collect::<String>();
    s.input(&input).unwrap();
    s.round().unwrap();
    assert_eq!(s.stats.dispatched_bindings, 300);
    assert_eq!(s.stats.audit_uncovered_per_round.last(), Some(&0));
    assert!(check(
        s.engine(),
        "(check (= (Get (Leaf 1299) 299) (Leaf 299)))"
    ));
    assert!(s.stats.gpu["buffer_resizes"].as_u64().unwrap() > 1);
}

#[test]
fn union_of_already_admitted_cells_deduplicates_projections() {
    let Some(mut s) = session() else { return };
    s.input("(Single (Leaf 100)) (Single (Leaf 101))").unwrap();
    s.round().unwrap();
    assert_eq!(s.stats.gpu["active_instances"], 2);
    s.input("(union (Leaf 100) (Leaf 101)) (Add-Gets (Leaf 200) (Single (Leaf 100)) 4)")
        .unwrap();
    s.round().unwrap();
    assert_eq!(s.stats.dispatched_bindings, 1);
    assert_eq!(s.stats.audit_uncovered_per_round.last(), Some(&0));
    assert!(check(
        s.engine(),
        "(check (= (Get (Leaf 200) 4) (Leaf 100)))"
    ));
    assert!(s.stats.gpu["watch_hits"].as_u64().unwrap() > 0);
}
