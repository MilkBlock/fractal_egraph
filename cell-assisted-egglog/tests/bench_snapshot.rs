#[path = "../src/public_snapshot.rs"]
mod public_snapshot;

#[test]
fn optional_snapshot_preserves_primitive_values_instead_of_intern_ids() {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None,"(datatype E (Leaf i64)) (function Cost (E) i64 :merge (min old new)) (set (Cost (Leaf 7)) 9007199254740993)").unwrap();
    assert!(public_snapshot::snapshot(&eg, false).unwrap().is_null());
    let graph = public_snapshot::snapshot(&eg, true).unwrap();
    let row = graph
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["op"] == "Cost")
        .unwrap();
    assert_eq!(row["sort"], "i64");
    assert_eq!(row["result"], "9007199254740993");
    assert_eq!(row["args"][0][0], "E");
}

#[test]
fn real_table_query_benchmark_preserves_aliases_literals_visibility_and_guards() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype E (Leaf i64))
      (relation P (E E i64))
      (relation Q (E))
      (rule ((P a a n) (Q a) (< n 5)) ((union a (Leaf 9))) :name "query")
      (rule ((P (Leaf 1) a -1) (Q a)) ((union a (Leaf 9))) :name "literal")
      (P (Leaf 1) (Leaf 2) -1) (P (Leaf 1) (Leaf 1) 8)
      (P (Leaf 3) (Leaf 3) 2) (Q (Leaf 1)) (Q (Leaf 3))
      (union (Leaf 1) (Leaf 2))
      (subsume (P (Leaf 3) (Leaf 3) 2))
    "#,
    )
    .unwrap();
    for rule in ["query", "literal"] {
        let r = cell_assisted_egglog::table_query::benchmark(
            &eg,
            "",
            rule,
            std::path::Path::new(&worker),
        )
        .unwrap();
        assert_eq!(r["matched_bindings"], 1);
        assert_eq!(r["exact_native_binding_set"], true);
        assert_eq!(r["cell_templates"], 0);
        assert!(
            r["gpu_cold_query_seconds"].as_f64().unwrap()
                >= r["gpu_resident_compute_and_return_seconds"]
                    .as_f64()
                    .unwrap()
        );
    }
    assert_eq!(
        eg.get_size("Leaf"),
        3,
        "benchmark must not execute original RHS"
    );
}
