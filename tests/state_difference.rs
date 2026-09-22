use egg_layout::{
    saturated_rule_composition::{Row, SaturatedRuleComposition as State, Vertex},
    state_difference::explain,
};
use serde_json::json;
use std::collections::BTreeMap;
fn state() -> State {
    State {
        version: 2,
        local_ids: vec![],
        scope: json!({"rules":"same","ports":"fixed"}),
        values: vec![
            Vertex {
                sort: "M".into(),
                literal: None
            };
            3
        ],
        rows: vec![Row {
            op: "Add".into(),
            args: vec![0, 1],
            result: 2,
        }],
        ports: BTreeMap::from([("x".into(), 0), ("y".into(), 1), ("root".into(), 2)]),
        subsumed_rows: vec![],
    }
}
#[test]
fn swapped_arguments_produce_two_directional_obligations() {
    let a = state();
    let mut b = a.clone();
    b.rows[0].args.reverse();
    let r = explain(&a, &b, &[]).unwrap();
    assert_eq!(r["status"], "different_contents");
    assert_eq!(r["left_only"].as_array().unwrap().len(), 1);
    assert_eq!(r["right_only"].as_array().unwrap().len(), 1);
    assert_eq!(
        r["closure_equivalence"],
        "not_certified_by_difference_analysis"
    );
}
#[test]
fn fixed_ports_allow_renaming_but_not_alias_changes() {
    let a = state();
    let mut b = a.clone();
    for v in b.ports.values_mut() {
        *v = 2 - *v;
    }
    b.rows[0].args = vec![2, 1];
    b.rows[0].result = 0;
    assert_eq!(explain(&a, &b, &[]).unwrap()["status"], "same_contents");
    b.ports.insert("y".into(), 2);
    assert_eq!(
        explain(&a, &b, &[]).unwrap()["status"],
        "interface_conflict"
    );
}
#[test]
fn unknown_binding_is_not_fabricated_and_scope_and_visibility_survive() {
    let a = state();
    let mut b = a.clone();
    b.subsumed_rows.push(0);
    let r = explain(&a, &b, &[]).unwrap();
    assert_eq!(r["visibility_changes"].as_array().unwrap().len(), 1);
    assert_eq!(r["status"], "different_contents");
    b.scope = json!("other rules");
    assert_eq!(
        explain(&a, &b, &[]).unwrap()["status"],
        "incompatible_scope"
    );
    let mut a = state();
    a.ports.clear();
    let b = a.clone();
    assert_eq!(explain(&a, &b, &[]).unwrap()["status"], "partial_alignment");
    assert_eq!(
        explain(&a, &b, &[(0, 0), (1, 1)]).unwrap()["status"],
        "same_contents"
    );
    assert!(explain(&a, &b, &[(9, 0)]).is_err());
}
#[test]
fn constructor_keys_propagate_without_graph_search() {
    let mut a = state();
    a.ports.clear();
    a.rows = vec![
        Row {
            op: "A".into(),
            args: vec![],
            result: 0,
        },
        Row {
            op: "F".into(),
            args: vec![0],
            result: 1,
        },
        Row {
            op: "G".into(),
            args: vec![1],
            result: 2,
        },
    ];
    let mut b = a.clone();
    for r in &mut b.rows {
        for v in &mut r.args {
            *v = 2 - *v;
        }
        r.result = 2 - r.result;
    }
    b.rows.reverse();
    let r = explain(&a, &b, &[]).unwrap();
    assert_eq!(r["status"], "same_contents");
    assert_eq!(r["left_to_right"], json!([2, 1, 0]));
    assert!(r["rows_examined"].as_u64().unwrap() < 20);
}
