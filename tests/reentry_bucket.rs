use egg_layout::{
    reentry_bucket::Buckets,
    saturated_rule_composition::{Row, SaturatedRuleComposition as State, Vertex},
};
use serde_json::json;
use std::collections::BTreeMap;
fn condition(ops: &[&str]) -> State {
    State {
        version: 2,
        local_ids: vec![],
        scope: json!({"rules":"test"}),
        values: vec![
            Vertex {
                sort: "E".into(),
                literal: None
            };
            ops.len() + 1
        ],
        rows: ops
            .iter()
            .enumerate()
            .map(|(i, op)| Row {
                op: (*op).into(),
                args: vec![0],
                result: i + 1,
            })
            .collect(),
        ports: BTreeMap::new(),
        subsumed_rows: vec![],
    }
}
fn observe(b: &mut Buckets, id: usize, s: &State) {
    b.observe(
        id,
        id,
        1,
        s,
        "RipenInput",
        &BTreeMap::from([("F".into(), 3)]),
        false,
        &json!({"new_effects":true,"new_bindings":[["fresh"]],"scope":"running prefix"}),
    )
    .unwrap();
}
#[test]
fn keeps_multiple_minima_and_retains_larger_observed_conditions() {
    let mut b = Buckets::default();
    observe(&mut b, 0, &condition(&["A", "Extra"]));
    observe(&mut b, 1, &condition(&["A"]));
    observe(&mut b, 2, &condition(&["B"]));
    let r = b.report();
    assert_eq!(r["groups"].as_array().unwrap().len(), 1);
    assert_eq!(r["groups"][0]["minimal_starting_conditions"], json!([1, 2]));
    assert_eq!(r["groups"][0]["cover"][0]["stronger"], 0);
    assert_eq!(r["groups"][0]["cover"][0]["weaker"], 1);
    assert_eq!(r["conditions"].as_array().unwrap().len(), 3);
}
#[test]
fn erases_literal_payload_but_keeps_originals_and_sorts() {
    let mut b = Buckets::default();
    let mut s = condition(&["Length"]);
    s.values[0] = Vertex {
        sort: "i64".into(),
        literal: Some("10".into()),
    };
    observe(&mut b, 0, &s);
    s.values[0].literal = Some("9".into());
    observe(&mut b, 1, &s);
    let r = b.report();
    assert_eq!(r["conditions"].as_array().unwrap().len(), 1);
    assert_eq!(r["original_conditions"]["0"]["values"][0]["literal"], "10");
    assert_eq!(r["original_conditions"]["1"]["values"][0]["literal"], "9");
    s.values[0].sort = "String".into();
    s.values[0].literal = Some("\"9\"".into());
    observe(&mut b, 2, &s);
    assert_eq!(
        b.report()["groups"][0]["minimal_starting_conditions"],
        json!([0, 1])
    );
}
#[test]
fn aliases_are_constraints_not_inequalities() {
    let mut b = Buckets::default();
    let mut general = condition(&["Pair"]);
    general.values.push(general.values[0].clone());
    general.rows[0].args = vec![0, 2];
    observe(&mut b, 0, &general);
    let mut same = general.clone();
    same.rows[0].args = vec![0, 0];
    observe(&mut b, 1, &same);
    let r = b.report();
    assert_eq!(r["conditions"].as_array().unwrap().len(), 2);
    assert_eq!(r["groups"][0]["minimal_starting_conditions"], json!([0]));
    assert_eq!(r["groups"][0]["cover"][0]["stronger"], 1);
}
#[test]
fn ignores_trivial_observation_and_separates_effect_components() {
    let mut b = Buckets::default();
    let s = condition(&["A"]);
    b.observe(
        0,
        0,
        1,
        &s,
        "",
        &BTreeMap::new(),
        false,
        &json!({"new_effects":true,"new_bindings":[]}),
    )
    .unwrap();
    assert!(b.report()["groups"].as_array().unwrap().is_empty());
    observe(&mut b, 0, &s);
    observe(&mut b, 0, &s);
    assert_eq!(
        b.report()["conditions"][0]["observations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    b.observe(
        1,
        1,
        1,
        &s,
        "",
        &BTreeMap::from([("G".into(), 1)]),
        false,
        &json!({"new_effects":true,"new_bindings":[["fresh"]]}),
    )
    .unwrap();
    assert_eq!(b.report()["groups"].as_array().unwrap().len(), 2);
}
