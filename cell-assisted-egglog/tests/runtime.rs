use cell_assisted_egglog::{
    Part, Session,
    matrix::{Cpu, Matcher, Metal},
};
use egglog::EGraph;
use std::path::Path;
fn session() -> Session {
    Session::new(
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
    .unwrap()
}
fn check(eg: &EGraph, code: &str) -> bool {
    eg.clone().parse_and_run_program(None, code).is_ok()
}
#[test]
fn fresh_bindings_do_not_replay_training_input() {
    let mut s = session();
    s.input("(Single (Leaf 100)) (Add-Gets (Leaf 200) (Single (Leaf 100)) 9) (Add-Gets (Leaf 300) (Leaf 400) 8)").unwrap();
    assert!(s.round(&mut Cpu).unwrap());
    assert!(!s.round(&mut Cpu).unwrap());
    assert!(check(
        s.engine(),
        "(check (= (Get (Leaf 200) 9) (Leaf 100)))"
    ));
    assert!(!check(s.engine(), "(check (Get (Leaf 300) 8))"));
    assert_eq!(s.stats.assisted_full_lhs_queries, 0);
    assert!(s.stats.valid_instances > 0);
}
#[test]
fn late_union_enables_boundary_match_without_new_source_match() {
    let mut s = session();
    s.input("(Single (Leaf 100)) (Add-Gets (Leaf 200) (Leaf 999) 7)")
        .unwrap();
    s.round(&mut Cpu).unwrap();
    assert!(!check(s.engine(), "(check (Get (Leaf 200) 7))"));
    let before = s.stats.native_matches;
    s.input("(union (Leaf 999) (Single (Leaf 100)))").unwrap();
    s.round(&mut Cpu).unwrap();
    assert!(check(
        s.engine(),
        "(check (= (Get (Leaf 200) 7) (Leaf 100)))"
    ));
    // Whether rebuild reschedules the helper is engine-dependent; no discovery
    // from a complete route-get query is permitted either way.
    assert!(s.stats.native_matches >= before);
    assert_eq!(s.stats.assisted_full_lhs_queries, 0);
}
#[test]
fn late_external_fact_is_observed_without_another_bootstrap_scan() {
    let mut s = session();
    s.input("(Single (Leaf 100))").unwrap();
    s.round(&mut Cpu).unwrap();
    let scans = s.stats.external_bootstrap_rows;
    s.input("(Add-Gets (Leaf 201) (Single (Leaf 100)) 13)")
        .unwrap();
    s.round(&mut Cpu).unwrap();
    assert!(check(
        s.engine(),
        "(check (= (Get (Leaf 201) 13) (Leaf 100)))"
    ));
    assert_eq!(s.stats.external_bootstrap_rows, scans);
}
#[test]
fn subsumed_cell_does_not_keep_authorizing_new_matches() {
    let mut s = session();
    s.input("(let s (Single (Leaf 100)))").unwrap();
    s.round(&mut Cpu).unwrap();
    s.input("(subsume (Single (Leaf 100))) (Add-Gets (Leaf 202) s 14)")
        .unwrap();
    s.round(&mut Cpu).unwrap();
    assert!(!check(s.engine(), "(check (Get (Leaf 202) 14))"));
    assert!(s.stats.pending_instances > 0);
}
#[test]
fn overlapping_instances_do_not_duplicate_the_substitution() {
    let mut s = session();
    s.input("(Single (Leaf 100)) (Single (Leaf 101)) (union (Leaf 100) (Leaf 101)) (Add-Gets (Leaf 200) (Single (Leaf 100)) 4)").unwrap();
    s.round(&mut Cpu).unwrap();
    assert_eq!(s.stats.unique_dispatched_bindings, 1);
}
#[test]
fn invalid_partition_and_overlapping_execution_lanes_fail() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    assert!(
        Session::new(
            &dir,
            "",
            "route-get",
            vec!["single-get".into()],
            vec![Part {
                atoms: vec![0],
                cell: true
            }]
        )
        .is_err()
    );
    assert!(Session::new(&dir, "", "route-get", vec!["route-get".into()], vec![]).is_err());
}
#[test]
fn unsupported_primitive_query_does_not_silently_become_a_fact() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(relation P (i64)) (relation Q (i64)) (rule ((P x) (> x 0)) ((Q x)) :name \"guarded\")",
    )
    .unwrap();
    assert!(eg.prepare_cell_action("", "guarded").is_err());
}
#[test]
fn no_wrong_gpu_result_can_reach_the_action_sink() {
    struct Wrong;
    impl Matcher for Wrong {
        fn match_rows(
            &mut self,
            a: &[Vec<u32>],
            b: &[Vec<u32>],
        ) -> cell_assisted_egglog::Result<Vec<(usize, usize)>> {
            Ok((0..a.len())
                .flat_map(|i| (0..b.len()).map(move |j| (i, j)))
                .collect())
        }
    }
    let mut s = session();
    s.input("(Single (Leaf 100)) (Add-Gets (Leaf 200) (Leaf 999) 7)")
        .unwrap();
    assert!(s.round(&mut Wrong).is_err());
    assert!(!check(s.engine(), "(check (Get (Leaf 200) 7))"));
}
#[test]
fn metal_exact_matrix_agrees_across_tiles_and_aliases() {
    let Some(path) = std::env::var_os("CELL_METAL_WORKER") else {
        return;
    };
    let mut gpu = Metal::new(Path::new(&path)).unwrap();
    let a: Vec<_> = (0..270u32)
        .map(|i| vec![i % 101, u32::MAX - i % 11])
        .collect();
    let b: Vec<_> = (0..261u32)
        .map(|i| vec![i % 101, u32::MAX - i % 11])
        .collect();
    assert_eq!(
        Cpu.match_rows(&a, &b).unwrap(),
        gpu.match_rows(&a, &b).unwrap()
    );
    assert_eq!(gpu.tiles, 4);
    let mut s = session();
    s.input("(Single (Leaf 100)) (Add-Gets (Leaf 200) (Leaf 999) 7)")
        .unwrap();
    s.round(&mut gpu).unwrap();
    s.input("(union (Leaf 999) (Single (Leaf 100)))").unwrap();
    s.round(&mut gpu).unwrap();
    assert!(check(
        s.engine(),
        "(check (= (Get (Leaf 200) 7) (Leaf 100)))"
    ));
}
