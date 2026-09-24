use egglog::EGraph;
fn allocation(eg: &EGraph, name: &str) -> Option<(usize, usize, usize)> {
    eg.shared_row_storage()
        .into_iter()
        .find(|(n, _, _, _)| n.as_deref() == Some(name))
        .map(|(_, id, n, b)| (id, n, b))
}
#[test]
fn native_snapshot_borrows_rows_then_detaches_on_insert_update_and_subsume() {
    let mut base = EGraph::default();
    base.parse_and_run_program(None,"(datatype E (A) (B)) (relation P (E)) (function score (E) i64 :merge (max old new)) (P (A)) (set (score (A)) 1)").unwrap();
    let mut branch = base.fork_shared();
    assert_eq!(
        allocation(&base, "P").unwrap().0,
        allocation(&branch, "P").unwrap().0
    );
    assert_eq!(allocation(&base, "P").unwrap().1, 2);
    branch.parse_and_run_program(None, "(P (A))").unwrap();
    assert_eq!(
        allocation(&base, "P").unwrap().0,
        allocation(&branch, "P").unwrap().0,
        "duplicate hashcons must not detach"
    );
    branch
        .parse_and_run_program(None, "(P (B)) (set (score (A)) 2) (subsume (P (A)))")
        .unwrap();
    base.parse_and_run_program(
        None,
        "(check (P (A))) (check (= (score (A)) 1)) (fail (check (P (B))))",
    )
    .unwrap();
    branch
        .parse_and_run_program(None, "(check (P (B))) (check (= (score (A)) 2))")
        .unwrap();
    let mut hidden = 0;
    branch
        .function_for_each("P", |r| hidden += usize::from(r.subsumed))
        .unwrap();
    assert_eq!(hidden, 1);
    let mut base_hidden = 0;
    base.function_for_each("P", |r| base_hidden += usize::from(r.subsumed))
        .unwrap();
    assert_eq!(base_hidden, 0);
    assert!(allocation(&branch, "P").is_none());
}
#[test]
fn union_rebuild_and_rollback_preserve_shared_parent() {
    let mut base = EGraph::default();
    base.parse_and_run_program(
        None,
        "(datatype E (A) (B)) (relation P (E)) (relation Q (E)) (P (A)) (Q (B))",
    )
    .unwrap();
    let mut branch = base.fork_shared();
    branch
        .parse_and_run_program(
            None,
            "(push) (union (A) (B)) (check (P (B))) (check (Q (A))) (pop) (fail (check (P (B))))",
        )
        .unwrap();
    base.parse_and_run_program(None, "(fail (check (P (B)))) (fail (check (Q (A))))")
        .unwrap();
    branch
        .parse_and_run_program(None, "(union (A) (B)) (check (P (B)))")
        .unwrap();
    base.parse_and_run_program(None, "(fail (check (P (B))))")
        .unwrap();
}
#[test]
fn shared_branches_have_independent_incremental_notifications() {
    let mut base = EGraph::default();
    base.parse_and_run_program(None,"(datatype E (A) (B) (C)) (relation P (E)) (relation Q (E)) (rule ((P x)) ((Q x))) (P (A)) (run 1)").unwrap();
    let mut a = base.fork_shared();
    let mut b = base.fork_shared();
    a.parse_and_run_program(None, "(P (B)) (run 1)").unwrap();
    b.parse_and_run_program(None, "(P (C)) (run 1)").unwrap();
    a.parse_and_run_program(None, "(check (Q (B))) (fail (check (Q (C))))")
        .unwrap();
    b.parse_and_run_program(None, "(check (Q (C))) (fail (check (Q (B))))")
        .unwrap();
    base.parse_and_run_program(None, "(fail (check (Q (B)))) (fail (check (Q (C))))")
        .unwrap();
}
