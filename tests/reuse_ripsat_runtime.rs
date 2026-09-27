use egg_layout::reuse_graph::ripsat_runtime::RipSatRuntime;
#[test]
fn graft_uses_delta_and_repeated_transition_reuses_saturation() {
    let mut r=RipSatRuntime::new("(datatype E (V i64) (A E) (B E)) (relation External (E)) (relation Out (E)) (rule ((= a (A x))) ((B a))) (rule ((= a (A x)) (= b (B a)) (External x)) ((Out b))) (A (V 1))").unwrap();
    let empty = r.graft(0, "").unwrap();
    assert_eq!(empty["native_matches"], 0);
    let first = r.graft(0, "(External (V 1))").unwrap();
    assert_eq!(first["native_matches"], 1);
    r.check(
        first["state"].as_u64().unwrap() as usize,
        "(check (Out (B (A (V 1)))))",
    )
    .unwrap();
    r.check(0, "(check (= (B (A (V 1))) (B (A (V 1)))))")
        .unwrap();
    let again = r.graft(0, "(External (V 1))").unwrap();
    assert_eq!(again["snapshot_reused"], true);
    assert_eq!(again["native_matches"], 0);
    assert!(r.graft(0, "(relation Other (E))").is_err());
}

#[test]
fn union_graft_enables_boundary_join_without_changing_parent() {
    let mut r=RipSatRuntime::new("(datatype E (V i64)) (relation L (E)) (relation R (E)) (relation Out (E)) (rule ((L x) (R x)) ((Out x))) (L (V 1)) (R (V 2))").unwrap();
    let result = r.graft(0, "(union (V 1) (V 2))").unwrap();
    assert_eq!(result["native_matches"], 1);
    r.check(
        result["state"].as_u64().unwrap() as usize,
        "(check (Out (V 1)))",
    )
    .unwrap();
    assert!(r.check(0, "(check (Out (V 1)))").is_err());
}
