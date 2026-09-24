use egg_layout::reuse_graph::planner::{Engine, Query};
fn query() -> Query {
    Query {
        values: vec![("E".into(), None), ("E".into(), None)],
        atoms: vec![
            ("P".into(), vec![0]),
            ("Link".into(), vec![0, 1]),
            ("Q".into(), vec![1]),
        ],
        projection: vec![0, 1],
    }
}
#[test]
fn native_fragment_join_preserves_union_subsume_and_all_bindings() {
    let mut optimized = Engine::default();
    let mut reference = Engine::default();
    let setup = "(datatype E (A) (B) (C)) (relation P (E)) (relation Q (E)) (relation Link (E E)) (P (A)) (P (C)) (Link (A) (B)) (Link (C) (B)) (Q (C))";
    optimized.execute(setup).unwrap();
    reference.execute(setup).unwrap();
    let q = query();
    let mut prefix = q.clone();
    prefix.atoms.pop();
    for update in [
        "",
        "(union (B) (C))",
        "(subsume (P (A)))",
        "(Link (C) (A)) (Q (A))",
    ] {
        if !update.is_empty() {
            optimized.execute(update).unwrap();
            reference.execute(update).unwrap();
        }
        optimized.query(&prefix).unwrap();
        assert_eq!(
            optimized.query(&q).unwrap(),
            reference.reference(&q).unwrap()
        );
    }
    assert_eq!(optimized.stats.fragment_joins, 4);
    assert!(optimized.stats.result_hits >= 4);
}
#[test]
fn rollback_and_updates_invalidate_results() {
    let mut e = Engine::default();
    e.execute("(datatype E (A)) (function score (E) i64 :merge (max old new)) (set (score (A)) 1)")
        .unwrap();
    let q = Query {
        values: vec![("E".into(), None), ("i64".into(), Some("1".into()))],
        atoms: vec![("score".into(), vec![0, 1])],
        projection: vec![0],
    };
    assert_eq!(e.query(&q).unwrap().len(), 1);
    e.execute("(push) (set (score (A)) 2)").unwrap();
    assert!(e.query(&q).unwrap().is_empty());
    e.execute("(pop)").unwrap();
    assert_eq!(e.query(&q).unwrap().len(), 1);
}
#[test]
fn alpha_renaming_reuses_plan_but_alias_constraints_do_not() {
    let mut e = Engine::default();
    e.execute("(datatype E (A) (B)) (relation Pair (E E)) (Pair (A) (B))")
        .unwrap();
    let q = Query {
        values: vec![("E".into(), None), ("E".into(), None)],
        atoms: vec![("Pair".into(), vec![0, 1])],
        projection: vec![0, 1],
    };
    let first = e.query(&q).unwrap();
    let mut renamed = q.clone();
    renamed.atoms[0].1 = vec![1, 0];
    renamed.projection = vec![1, 0];
    assert_eq!(e.query(&renamed).unwrap(), first);
    assert_eq!(e.stats.compilations, 1);
    let mut alias = q;
    alias.atoms[0].1 = vec![0, 0];
    alias.projection = vec![0];
    assert!(e.query(&alias).unwrap().is_empty());
}
