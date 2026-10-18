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
