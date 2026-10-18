//! Native checker tests run without a GPU. These deliberately corrupt outputs,
//! including a losing proposal whose corruption is invisible after min merge.
use egglog::{
    EGraph,
    cell_assist::{CellI64Group, CellI64Proposal},
};

const SOURCE: &str = r#"
(datatype Expr (Leaf i64))
(function Best (Expr) i64 :merge (min old new))
(relation Offer (Expr i64))
(rule ((Offer dst cost) (= old (Best dst)) (< cost old))
      ((set (Best dst) (- cost 1))) :name "improve")
(set (Best (Leaf 0)) 100)
(Offer (Leaf 0) 3)
(Offer (Leaf 0) 5)
(Offer (Leaf 0) 200)
"#;

#[test]
fn native_checker_rejects_wrong_binding_rhs_merge_key_and_snapshot_without_writes() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, SOURCE).unwrap();
    let sink = eg.prepare_cell_action("", "improve").unwrap();
    let checker = eg.prepare_cell_i64_check("", &sink).unwrap();
    let rows = eg.audit_cell_matches(&sink).unwrap();
    assert_eq!(rows.len(), 2);
    let slots = sink.slots();
    let dst = slots.iter().position(|(n, _)| *n == "dst").unwrap();
    let cost = slots.iter().position(|(n, _)| *n == "cost").unwrap();
    let make_group = || CellI64Group {
        key: vec![rows[0][dst]],
        initial: Some(100),
        result: Some(2),
        proposals: rows
            .iter()
            .map(|r| CellI64Proposal {
                binding: r.clone(),
                value: eg.value_to_base::<i64>(r[cost]) - 1,
            })
            .collect(),
    };
    let group = make_group();
    let key = group.key.clone();
    let mut groups = vec![group];
    let r = eg
        .check_cell_i64_batch(&sink, &checker, &rows, &groups)
        .unwrap();
    assert_eq!(r.proposals, 2);
    assert_eq!(r.groups, 1);
    assert_eq!(
        eg.value_to_base::<i64>(eg.lookup_function_row("Best", &key).unwrap()[1]),
        100
    );
    let losing = groups[0]
        .proposals
        .iter()
        .position(|p| p.value == 4)
        .unwrap();
    groups[0].proposals[losing].value = 99;
    assert!(
        eg.check_cell_i64_batch(&sink, &checker, &rows, &groups)
            .unwrap_err()
            .to_string()
            .contains("RHS/key mismatch")
    );
    groups[0].proposals[losing].value = 4;
    groups[0].result = Some(1);
    assert!(
        eg.check_cell_i64_batch(&sink, &checker, &rows, &groups)
            .unwrap_err()
            .to_string()
            .contains("merge mismatch")
    );
    groups[0].result = Some(2);
    groups[0].initial = Some(99);
    assert!(
        eg.check_cell_i64_batch(&sink, &checker, &rows, &groups)
            .unwrap_err()
            .to_string()
            .contains("stale native snapshot")
    );
    groups[0].initial = Some(100);
    eg.parse_and_run_program(None, "(set (Best (Leaf 1)) 100)")
        .unwrap();
    let other = egglog::ast::Parser::default()
        .get_expr_from_string(None, "(Leaf 1)")
        .unwrap();
    groups[0].key = vec![eg.eval_expr(&other).unwrap().1];
    assert!(
        eg.check_cell_i64_batch(&sink, &checker, &rows, &groups)
            .unwrap_err()
            .to_string()
            .contains("RHS/key mismatch")
    );
    groups[0].key = key.clone();
    assert!(
        eg.check_cell_i64_batch(&sink, &checker, &rows[..1], &groups)
            .unwrap_err()
            .to_string()
            .contains("missing")
    );
    let mut wrong = rows.clone();
    wrong[0][cost] = eg.base_to_value(200i64);
    assert!(
        eg.check_cell_i64_batch(&sink, &checker, &wrong, &groups)
            .unwrap_err()
            .to_string()
            .contains("unexpected")
    );
    assert_eq!(
        eg.value_to_base::<i64>(eg.lookup_function_row("Best", &key).unwrap()[1]),
        100
    );
    // Reuse the same compiled oracle after updates. Old guard bindings expire.
    eg.parse_and_run_program(None, "(set (Best (Leaf 0)) 4)")
        .unwrap();
    assert!(
        eg.check_cell_i64_batch(&sink, &checker, &rows, &[])
            .is_err()
    );
    let fresh = eg.audit_cell_matches(&sink).unwrap();
    assert_eq!(fresh.len(), 1);
    eg.check_cell_i64_batch(&sink, &checker, &fresh, &[])
        .unwrap();
    eg.free_cell_i64_check(checker);
}
