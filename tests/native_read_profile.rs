#![cfg(feature = "native-read-profile")]
use egglog::EGraph;

fn scans(eg: &EGraph, table: &str) -> usize {
    eg.join_read_profile()
        .into_iter()
        .find(|(name, _)| name.as_deref() == Some(table))
        .unwrap()
        .1
        .unwrap()
        .scan_rows
}
#[test]
fn profiles_count_native_query_work_and_are_branch_local() {
    let mut eg = EGraph::default();
    eg.reset_join_read_profile(true); // Tables declared later must participate.
    eg.parse_and_run_program(None, "(datatype E (V i64)) (V 1) (V 2) (V 3)")
        .unwrap();
    assert_eq!(scans(&eg, "V"), 0, "initial actions are not a Join scan");
    let q = eg
        .compile_fact_query(&[("i64".into(), None)], &[("V".into(), vec![0])], &[0])
        .unwrap();
    let mut expected = eg.run_fact_query(&q).unwrap();
    expected.sort();
    assert_eq!(expected.len(), 3);
    assert!(scans(&eg, "V") >= 3);
    let before = scans(&eg, "V");
    eg.function_for_each("V", |_| {}).unwrap();
    assert_eq!(
        scans(&eg, "V"),
        before,
        "diagnostic export must not count as Join work"
    );
    let mut branch = eg.fork_shared();
    assert_eq!(scans(&branch, "V"), 0, "clones start independent counters");
    let bq = branch
        .compile_fact_query(&[("i64".into(), None)], &[("V".into(), vec![0])], &[0])
        .unwrap();
    assert_eq!(branch.run_fact_query(&bq).unwrap().len(), 3);
    assert!(scans(&branch, "V") > 0);
    assert_eq!(scans(&eg, "V"), before);
    branch.free_fact_query(bq);
    eg.reset_join_read_profile(true);
    assert_eq!(scans(&eg, "V"), 0);
    let mut after = eg.run_fact_query(&q).unwrap();
    after.sort();
    assert_eq!(expected, after);
    eg.reset_join_read_profile(false);
    assert!(eg.join_read_profile().iter().all(|(_, p)| p.is_none()));
    let mut after = eg.run_fact_query(&q).unwrap();
    after.sort();
    assert_eq!(expected, after);
    eg.free_fact_query(q);
}
