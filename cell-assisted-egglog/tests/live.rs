use cell_assisted_egglog::{Part, Session, environment, live::LiveSession};
use egglog::EGraph;
use serde_json::Value as Json;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> Fixture {
    static N: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "live-matrix-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(root.join("catalog/states")).unwrap();
    let f: Json = serde_json::from_str(include_str!("data/live_ports.json")).unwrap();
    for (file, v) in [
        ("history.json", &f["history"]),
        ("catalog/catalog.json", &f["catalog"]),
    ] {
        std::fs::write(root.join(file), serde_json::to_vec(v).unwrap()).unwrap();
    }
    for (id, state) in f["states"].as_object().unwrap() {
        std::fs::write(
            root.join(format!(
                "catalog/states/state-{:04}.json",
                id.parse::<usize>().unwrap()
            )),
            serde_json::to_vec(state).unwrap(),
        )
        .unwrap();
    }
    Fixture(root)
}
fn session(root: &Path, worker: &Path) -> LiveSession {
    let mut s = Session::new(
        root,
        "",
        "overlap",
        [
            "left-to-pair",
            "right-to-pair",
            "pair-to-left",
            "pair-to-right",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        vec![
            Part {
                atoms: vec![0],
                cell: true,
            },
            Part {
                atoms: vec![1],
                cell: true,
            },
        ],
    )
    .unwrap();
    s.set_audit(true);
    LiveSession::new(s, &environment(root).unwrap().0, worker).unwrap()
}
fn exists(eg: &EGraph, text: &str) -> bool {
    eg.clone().parse_and_run_program(None, text).is_ok()
}
fn rounds(s: &mut LiveSession) {
    for _ in 0..5 {
        s.round().unwrap();
    }
}
fn pairs(eg: &EGraph, name: &str) -> BTreeSet<Vec<i64>> {
    let mut atoms = std::collections::BTreeMap::new();
    eg.function_for_each("Atom", |r| {
        let v = eg.get_canonical_value(*r.vals.last().unwrap(), eg.get_sort_by_name("E").unwrap());
        atoms.insert(v, eg.value_to_base::<i64>(r.vals[0]));
    })
    .unwrap();
    let mut out = BTreeSet::new();
    eg.function_for_each(name, |r| {
        if !r.subsumed {
            out.insert(
                r.vals[..r.vals.len() - 1]
                    .iter()
                    .map(|v| atoms[&eg.get_canonical_value(*v, eg.get_sort_by_name("E").unwrap())])
                    .collect(),
            );
        }
    })
    .unwrap();
    out
}
#[test]
fn real_native_loop_bakes_unknown_aliases_resumes_and_reuses_with_fresh_values() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let f = fixture();
    let mut s = session(&f.0, Path::new(&worker));
    let input = "(L (Atom 101) (Atom 102)) (R (Atom 102) (Atom 101))";
    s.input(input).unwrap();
    rounds(&mut s);
    assert!(exists(
        s.engine(),
        "(check (Out (Atom 101) (Atom 102) (Atom 101)))"
    ));
    assert!(s.stats.bake_completed > 0);
    assert!(s.stats.learned_instances > 0);
    assert!(s.stats.learned_frames > 0);
    assert_eq!(s.stats.dispatched_bindings, 2);
    let first = s.stats.bake_completed;
    let counts = s.stats.device["execution_counts"].clone();
    s.round().unwrap();
    assert_eq!(counts, s.stats.device["execution_counts"]);
    let second = "(L (Atom 201) (Atom 202)) (R (Atom 202) (Atom 201))";
    s.input(second).unwrap();
    rounds(&mut s);
    assert_eq!(s.stats.bake_completed, first);
    assert!(s.stats.bake_cache_hits > 0);
    assert_eq!(s.stats.dispatched_bindings, 4);
    assert_eq!(
        s.stats.audit_uncovered.last(),
        Some(&0),
        "{:?}",
        s.stats.audit_uncovered
    );
    let mut baseline = EGraph::default();
    baseline
        .parse_and_run_program(None, &environment(&f.0).unwrap().0)
        .unwrap();
    baseline
        .parse_and_run_program(
            None,
            &format!("{input} {second} (run-schedule (saturate (run)))"),
        )
        .unwrap();
    exact_graph(s.engine(), &baseline);
    for table in ["L", "R", "Pair", "Out"] {
        assert_eq!(pairs(s.engine(), table), pairs(&baseline, table));
    }
    assert!(
        s.stats.device["execution_counts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c[1].as_u64().unwrap() <= 1)
    );
}
#[test]
fn unfinished_bake_preserves_the_match_and_resumes_without_new_matrix_matching() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let f = fixture();
    let mut s = session(&f.0, Path::new(&worker));
    s.facts = 1;
    s.input("(L (Atom 1) (Atom 2)) (R (Atom 2) (Atom 1))")
        .unwrap();
    rounds(&mut s);
    assert_eq!(s.stats.dispatched_bindings, 0);
    assert!(s.stats.bake_started > 0);
    assert_eq!(s.stats.bake_completed, 0);
    assert!(s.pending_matches() > 0);
    let counts = s.stats.device["execution_counts"].clone();
    let started = s.stats.bake_started;
    s.facts = 200_000;
    s.round().unwrap();
    assert_eq!(s.stats.bake_started, started);
    assert_eq!(s.stats.device["execution_counts"], counts);
    assert_eq!(s.stats.device["host_count_reads"], 0);
    assert_eq!(s.pending_matches(), 0);
    assert_eq!(s.stats.dispatched_bindings, 2);
    assert!(exists(
        s.engine(),
        "(check (Out (Atom 1) (Atom 2) (Atom 1)))"
    ));
}
#[test]
fn native_subsume_cancels_a_pending_completion_before_bake_resume() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let f = fixture();
    let mut s = session(&f.0, Path::new(&worker));
    s.facts = 1;
    s.input("(L (Atom 1) (Atom 2)) (R (Atom 2) (Atom 1))")
        .unwrap();
    rounds(&mut s);
    s.input("(subsume (Pair (Atom 1) (Atom 2)))").unwrap();
    s.facts = 200_000;
    s.round().unwrap();
    assert!(s.stats.cancelled_matches > 0);
    assert_eq!(s.stats.dispatched_bindings, 0);
    assert!(!exists(
        s.engine(),
        "(check (Out (Atom 1) (Atom 2) (Atom 1)))"
    ));
}
#[test]
fn late_union_and_late_fact_drive_new_cell_frames() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let f = fixture();
    let mut s = session(&f.0, Path::new(&worker));
    s.input("(L (Atom 1) (Atom 2)) (R (Atom 3) (Atom 4))")
        .unwrap();
    rounds(&mut s);
    assert_eq!(s.stats.dispatched_bindings, 0);
    s.input("(union (Atom 2) (Atom 3))").unwrap();
    rounds(&mut s);
    assert!(exists(
        s.engine(),
        "(check (Out (Atom 1) (Atom 2) (Atom 4)))"
    ));
    assert!(s.stats.union_events > 0);
    assert!(s.stats.retired_frames > 0);
    s.input("(R (Atom 4) (Atom 5))").unwrap();
    rounds(&mut s);
    assert!(exists(
        s.engine(),
        "(check (Out (Atom 2) (Atom 4) (Atom 5)))"
    ));
    assert_eq!(
        s.stats.audit_uncovered.last(),
        Some(&0),
        "{:?}",
        s.stats.audit_uncovered
    );
}

fn exact_graph(left: &EGraph, right: &EGraph) {
    let names = ["Atom", "L", "R", "Pair", "Out"];
    let mut mapping = std::collections::BTreeMap::new();
    let mut inverse = std::collections::BTreeMap::new();
    let mut pending = vec![];
    for name in names {
        assert_eq!(left.get_size(name), right.get_size(name));
        left.function_for_each(name, |r| {
            if !r.subsumed {
                pending.push((name.to_string(), r.vals.to_vec()));
            }
        })
        .unwrap();
    }
    let sort = left.get_sort_by_name("E").unwrap();
    while !pending.is_empty() {
        let before = pending.len();
        let mut later = vec![];
        for (op, row) in pending {
            let args = if op == "Atom" {
                // base i64 literal, not an equality-sort ID
                let n = left.value_to_base::<i64>(row[0]);
                let mut r = right.clone();
                let e = egglog::ast::Parser::default()
                    .get_expr_from_string(None, &n.to_string())
                    .unwrap();
                Some(vec![r.eval_expr(&e).unwrap().1])
            } else {
                row[..row.len() - 1]
                    .iter()
                    .map(|v| mapping.get(&left.get_canonical_value(*v, sort)).copied())
                    .collect::<Option<Vec<_>>>()
            };
            let Some(args) = args else {
                later.push((op, row));
                continue;
            };
            let r = right
                .cell_visible_row(&op, &args)
                .expect("missing native constructor row");
            let src = left.get_canonical_value(*row.last().unwrap(), sort);
            let dst = *r.last().unwrap();
            if let Some(old) = mapping.insert(src, dst) {
                assert_eq!(old, dst)
            }
            if let Some(old) = inverse.insert(dst, src) {
                assert_eq!(old, src)
            }
        }
        assert!(later.len() < before, "ungrounded comparison");
        pending = later;
    }
}
