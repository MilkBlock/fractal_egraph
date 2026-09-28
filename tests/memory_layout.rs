use egglog::{EGraph, VirtualFactRow};
fn graph() -> EGraph {
    let mut eg = EGraph::default();
    let mut text = "(datatype E (V i64) (R E))".to_owned();
    for i in 0..4096 {
        text += &format!("(R (V {i}))");
    }
    eg.parse_and_run_program(None, &text).unwrap();
    eg
}
fn query(eg: &mut EGraph, atoms: usize) -> egglog::CompiledFactQuery {
    eg.compile_fact_query(
        &[("E".into(), None), ("E".into(), None)],
        &vec![("R".into(), vec![0, 1]); atoms],
        &[0, 1],
    )
    .unwrap()
}
#[test]
fn repeated_atoms_share_candidate_storage_and_refresh_after_updates() {
    let mut eg = graph();
    let one = query(&mut eg, 1);
    let many = query(&mut eg, 32);
    let single = eg.fact_query_unique_subset_bytes(&one).unwrap();
    let repeated = eg.fact_query_unique_subset_bytes(&many).unwrap();
    assert!(
        repeated <= single * 2,
        "single={single}, repeated={repeated}"
    );
    assert_eq!(eg.run_fact_query(&many).unwrap().len(), 4096);
    for update in ["(R (V 4096))", "(subsume (R (V 0)))", "(union (V 1) (V 2))"] {
        eg.parse_and_run_program(None, update).unwrap();
        let mut a = eg.run_fact_query(&one).unwrap();
        let mut b = eg.run_fact_query(&many).unwrap();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
    eg.free_fact_query(one);
    eg.free_fact_query(many);
}
#[test]
fn sparse_packing_only_describes_virtual_rows() {
    let mut eg = graph();
    let q = query(&mut eg, 1);
    let bindings = eg.run_fact_query(&q).unwrap();
    let template = eg
        .virtual_fact_template(vec![VirtualFactRow {
            function: "R".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    eg.virtualize_existing_facts(&template, &bindings[..1])
        .unwrap();
    let stats = eg
        .virtual_fact_stats()
        .into_iter()
        .find(|(n, _)| n == "R")
        .unwrap()
        .1;
    assert_eq!(stats.virtual_rows, 1);
    assert_eq!(stats.direct_base_rows, 4095);
    assert!(stats.descriptor_bytes <= 16, "{}", stats.descriptor_bytes);
    assert!(
        stats.local_header_bytes <= 4096,
        "{}",
        stats.local_header_bytes
    );
    let mut after = eg.run_fact_query(&q).unwrap();
    let mut before = bindings.clone();
    after.sort();
    before.sort();
    assert_eq!(before, after);
    eg.parse_and_run_program(None, "(union (V 0) (V 4095)) (R (V 5000))")
        .unwrap();
    assert_eq!(eg.run_fact_query(&q).unwrap().len(), 4096);
    eg.free_fact_query(q);
}

#[test]
fn shared_roots_do_not_mix_alias_filters() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        "(datatype E (V i64) (Pair E E)) (Pair (V 0) (V 0)) (Pair (V 0) (V 1)) (Pair (V 1) (V 1))",
    )
    .unwrap();
    let q = eg
        .compile_fact_query(
            &vec![("E".into(), None); 4],
            &[
                ("Pair".into(), vec![0, 0, 2]),
                ("Pair".into(), vec![0, 1, 3]),
            ],
            &[0, 1],
        )
        .unwrap();
    assert_eq!(eg.run_fact_query(&q).unwrap().len(), 3);
    eg.free_fact_query(q);
}

#[test]
fn bitmap_rank_is_exact_across_words_pages_and_repacking() {
    let mut eg = graph();
    let q = query(&mut eg, 1);
    let mut expected = eg.run_fact_query(&q).unwrap();
    expected.sort();
    let template = eg
        .virtual_fact_template(vec![VirtualFactRow {
            function: "R".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    for parity in [0, 1] {
        let chosen = expected
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == parity)
            .map(|(_, b)| b.clone())
            .collect::<Vec<_>>();
        eg.virtualize_existing_facts(&template, &chosen).unwrap();
        let mut actual = eg.run_fact_query(&q).unwrap();
        actual.sort();
        assert_eq!(actual, expected);
    }
    assert_eq!(
        eg.virtual_fact_stats()
            .into_iter()
            .find(|(n, _)| n == "R")
            .unwrap()
            .1
            .virtual_rows,
        4096
    );
    eg.free_fact_query(q);
}
