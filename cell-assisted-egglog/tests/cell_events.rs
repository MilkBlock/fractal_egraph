use egglog::{EGraph, Value};
use std::collections::BTreeMap;
type Mirror = BTreeMap<(usize, Vec<Value>), (Vec<Value>, bool)>;
fn drain(eg: &EGraph, feed: &mut egglog::cell_events::CellCommitFeed, mirror: &mut Mirror) {
    let batch = eg.drain_cell_commits(feed).unwrap();
    for t in batch.clears {
        mirror.retain(|(table, _), _| *table != t);
    }
    for r in batch.rows {
        let key = (r.table, r.key);
        if let Some(row) = r.row {
            mirror.insert(key, (row, r.visible));
        } else {
            mirror.remove(&key);
        }
    }
    let mut actual = Mirror::new();
    for (i, t) in feed.tables.iter().enumerate() {
        eg.function_for_each(&t.name, |r| {
            actual.insert(
                (i, r.vals[..t.keys].to_vec()),
                (r.vals.to_vec(), !r.subsumed),
            );
        })
        .unwrap();
    }
    assert_eq!(*mirror, actual);
}
#[test]
fn commits_cover_insert_update_rebuild_delete_subsume_and_noops() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (V i64) (A E)) (relation P (E)) (function Score (E) i64 :merge (max old new))",
    )
    .unwrap();
    let (mut feed, initial) = eg
        .subscribe_cell_commits(&["V".into(), "A".into(), "P".into(), "Score".into()])
        .unwrap();
    assert!(initial.rows.is_empty());
    let mut m = Mirror::new();
    for text in [
        "(A (V 1)) (A (V 2)) (P (V 1)) (P (V 2)) (set (Score (V 1)) 4)",
        "(set (Score (V 1)) 9)",
        "(union (V 1) (V 2))",
        "(delete (P (V 1)))",
        "(subsume (A (V 1)))",
        "(P (V 1))",
    ] {
        eg.parse_and_run_program(None, text).unwrap();
        drain(&eg, &mut feed, &mut m);
    }
    eg.parse_and_run_program(None, "(P (V 1))").unwrap();
    assert!(eg.drain_cell_commits(&mut feed).unwrap().rows.is_empty());
}
#[test]
fn native_clone_does_not_share_event_queue() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, "(datatype E (V i64))")
        .unwrap();
    let (mut feed, _) = eg.subscribe_cell_commits(&["V".into()]).unwrap();
    eg.parse_and_run_program(None, "(V 1)").unwrap();
    let mut copy = eg.clone();
    copy.parse_and_run_program(None, "(V 2)").unwrap();
    let a = eg.drain_cell_commits(&mut feed).unwrap();
    assert_eq!(a.rows.len(), 1);
    let (mut other, initial) = copy.subscribe_cell_commits(&["V".into()]).unwrap();
    assert_eq!(initial.rows.len(), 2);
    copy.parse_and_run_program(None, "(V 3)").unwrap();
    assert_eq!(copy.drain_cell_commits(&mut other).unwrap().rows.len(), 1);
    assert!(eg.drain_cell_commits(&mut feed).unwrap().rows.is_empty());
}
#[test]
fn bulk_commit_keeps_native_parallel_writer_path() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (V i64) (A E)) (rule ((= v (V x))) ((A v)))",
    )
    .unwrap();
    let source = (0..5000).map(|i| format!("(V {i})")).collect::<String>();
    let cmds = eg.parse_program(None, &source).unwrap();
    eg.run_ground_import(&cmds).unwrap();
    let (mut feed, _) = eg.subscribe_cell_commits(&["A".into()]).unwrap();
    eg.step_rules("").unwrap();
    let mut m = Mirror::new();
    drain(&eg, &mut feed, &mut m);
    assert_eq!(m.len(), 5000);
}
#[test]
fn producer_tap_keeps_only_requested_named_fields() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (V i64) (A E)) (rule ((= v (V x))) ((A v)) :name \"a\") (V 7)",
    )
    .unwrap();
    let tap = eg
        .prepare_cell_producers("", &[("a".into(), vec!["x".into()])])
        .unwrap();
    eg.step_cell_producers(&tap, "").unwrap();
    let e = tap.drain();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].values.len(), 1);
    assert_eq!(eg.value_to_base::<i64>(e[0].values[0]), 7);
    assert!(tap.drain().is_empty());
}

#[test]
fn producer_tap_preserves_naive_and_global_modes() {
    for (global, option, expected) in [(true, "", 0), (true, ":naive", 1), (false, "", 1)] {
        let mut eg = EGraph::default();
        eg.seminaive = global;
        eg.parse_and_run_program(
            None,
            &format!(
                "(datatype E (V i64) (A E)) (rule ((= v (V x))) ((A v)) :name \"a\" {option}) (V 7)"
            ),
        )
        .unwrap();
        let tap = eg
            .prepare_cell_producers("", &[("a".into(), vec!["x".into()])])
            .unwrap();
        eg.step_cell_producers(&tap, "").unwrap();
        assert_eq!(tap.drain().len(), 1);
        eg.step_cell_producers(&tap, "").unwrap();
        assert_eq!(tap.drain().len(), expected);
    }
}
