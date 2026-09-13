use egglog::EGraph;
fn graph() -> EGraph {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
        (datatype E (N i64) (Pair E E))
        (function Slot (i64) E :no-merge)
        (relation Seen (E))
    "#,
    )
    .unwrap();
    eg
}
#[test]
fn ground_loader_matches_actions_and_preserves_set_visibility() {
    let source = r#"
        (set (Slot 0) (N 1))
        (Seen (Pair (Slot 0) (N 1)))
        (Seen (Pair (N 1) (Slot 0)))
        (set (Slot 0) (N 1))
    "#;
    let mut fast = graph();
    let mut normal = graph();
    let commands = fast.parse_program(None, source).unwrap();
    fast.run_ground_import(&commands).unwrap();
    normal.run_program(commands).unwrap();
    for eg in [&mut fast, &mut normal] {
        eg.parse_and_run_program(
            None,
            "(check (Seen (Pair (N 1) (N 1)))) (check (= (Slot 0) (N 1)))",
        )
        .unwrap();
    }
    for name in ["N", "Pair", "Slot", "Seen"] {
        assert_eq!(fast.get_size(name), normal.get_size(name));
    }
}
#[test]
fn validates_whole_batch_before_mutation_and_rejects_missing_keys() {
    let mut eg = graph();
    let commands = eg
        .parse_program(None, r#"(N 777) (Seen (N "wrong"))"#)
        .unwrap();
    assert!(eg.run_ground_import(&commands).is_err());
    assert_eq!(eg.get_size("N"), 0);
    for source in [
        "(Seen (Slot 99))",
        "(N)",
        "(N (+ 1 2))",
        "(union (N 1) (N 2))",
        "(set (N 1) (N 2))",
    ] {
        let commands = eg.parse_program(None, source).unwrap();
        assert!(eg.run_ground_import(&commands).is_err(), "{source}");
    }
    let commands = eg
        .parse_program(None, "(set (Slot 0) (N 1)) (set (Slot 0) (N 2))")
        .unwrap();
    assert!(eg.run_ground_import(&commands).is_err());
    eg.parse_and_run_program(None, "(check (= (Slot 0) (N 1)))")
        .unwrap();
}
