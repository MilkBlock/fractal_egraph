use egg_layout::{
    ripen_convergence::{Fingerprint, Index},
    saturated_rule_composition::{Row, SaturatedRuleComposition as State, Vertex},
};
use serde_json::json;
use std::collections::BTreeMap;
fn cycle(split: bool) -> State {
    State {
        version: 2,
        local_ids: vec![],
        scope: json!({"rules":"same"}),
        values: vec![
            Vertex {
                sort: "T".into(),
                literal: None
            };
            6
        ],
        rows: (0..6)
            .map(|i| Row {
                op: "Link".into(),
                args: vec![i],
                result: if split {
                    (i / 3) * 3 + (i + 1) % 3
                } else {
                    (i + 1) % 6
                },
            })
            .collect(),
        subsumed_rows: vec![],
        ports: BTreeMap::new(),
    }
}
#[test]
fn collisions_do_not_share_and_ids_do_not_matter() {
    let a = cycle(false);
    let b = cycle(true);
    let mut idx = Index::new(10, 10000);
    idx.observe("a", 0, a.clone(), "contract", &mut Fingerprint::default());
    assert_eq!(
        idx.observe("b", 0, b, "contract", &mut Fingerprint::default())["status"],
        "collision_or_unknown"
    );
    let mut renamed = a.clone();
    for r in &mut renamed.rows {
        r.args[0] = 5 - r.args[0];
        r.result = 5 - r.result;
    }
    renamed.rows.reverse();
    assert_eq!(
        idx.observe("c", 0, renamed, "contract", &mut Fingerprint::default())["status"],
        "verified"
    );
    assert_eq!(
        idx.observe(
            "d",
            0,
            a.clone(),
            "other obligations",
            &mut Fingerprint::default()
        )["status"],
        "miss"
    );
    let mut hidden = a;
    hidden.subsumed_rows = vec![0];
    assert_ne!(
        idx.observe("e", 0, hidden, "contract", &mut Fingerprint::default())["status"],
        "verified"
    );
}
#[test]
fn native_intermediate_states_converge_before_saturation() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join(format!(
        "out/convergence-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sources = ["a.egg", "b.egg"].map(|p| root.join("experiments/ripen_convergence").join(p));
    let report = egg_layout::native_analyze::ripen::probe(&sources, &out, 8).unwrap();
    assert!(report["index"]["stats"]["verified_hits"].as_u64().unwrap() > 0);
    assert_eq!(report["opportunities"][0]["first_hit"]["round"], 0);
    assert!(
        report["opportunities"][0]["remaining_observed_rounds"]
            .as_u64()
            .unwrap()
            >= 2
    );
    for c in report["cells"].as_array().unwrap() {
        assert_eq!(c["checks"], "passed");
        assert_eq!(c["convergence"]["actual_rounds_skipped"], 0);
    }
}
