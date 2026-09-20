use egg_layout::closed_state::*;
use serde_json::json;
use std::collections::BTreeMap;
fn state() -> ClosedState {
    ClosedState {
        version: 1,
        local_ids: vec![],
        scope: json!({"rules":"fixed"}),
        values: vec![
            Vertex {
                sort: "E".into(),
                literal: None
            };
            3
        ],
        rows: vec![
            Row {
                op: "Pair".into(),
                args: vec![0, 1],
                result: 2,
            },
            Row {
                op: "Cycle".into(),
                args: vec![2],
                result: 0,
            },
        ],
        ports: BTreeMap::from([("out".into(), 2)]),
    }
}
#[test]
fn exact_check_preserves_sharing_cycles_and_ports() {
    let a = state();
    let mut b = a.clone();
    let p = [2, 0, 1];
    for r in &mut b.rows {
        r.args = r.args.iter().map(|i| p[*i]).collect();
        r.result = p[r.result];
    }
    b.rows.reverse();
    b.ports.insert("out".into(), p[2]);
    let Comparison::Equivalent { value_map, .. } = compare(&a, &b, 100).unwrap() else {
        panic!("renamed cyclic graph")
    };
    assert_eq!(value_map, p);
    b.ports.insert("out".into(), 0);
    assert!(!matches!(
        compare(&a, &b, 100).unwrap(),
        Comparison::Equivalent { .. }
    ));
    let mut b = a.clone();
    b.rows[0].args[1] = 0;
    assert!(!matches!(
        compare(&a, &b, 100).unwrap(),
        Comparison::Equivalent { .. }
    ));
    assert!(matches!(
        compare(&a, &a, 0).unwrap(),
        Comparison::UnknownBudget { .. }
    ));
    let mut b = a.clone();
    b.scope = json!({"rules":"other"});
    assert!(matches!(
        compare(&a, &b, 100).unwrap(),
        Comparison::IncompatibleScope
    ));
}
#[test]
fn equal_counts_do_not_hide_different_literals_or_facts() {
    let mut a = state();
    a.values[1] = Vertex {
        sort: "i64".into(),
        literal: Some("1".into()),
    };
    let mut b = a.clone();
    b.values[1].literal = Some("2".into());
    assert!(matches!(
        compare(&a, &b, 100).unwrap(),
        Comparison::Different { .. }
    ));
    let mut b = a.clone();
    b.rows[0].op = "Other".into();
    assert!(matches!(
        compare(&a, &b, 100).unwrap(),
        Comparison::Different { .. }
    ));
}
#[test]
fn different_native_triggers_share_one_state() {
    use std::{fs, process::Command};
    let base = std::env::temp_dir().join(format!("closed-state-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    for (name, source) in [
        ("a", "experiments/closed-state/from-a.egg"),
        ("b", "experiments/closed-state/from-b.egg"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .args(["ripen", source, base.join(name).to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let a = read(&base.join("a/closed-state.json")).unwrap();
    let b = read(&base.join("b/closed-state.json")).unwrap();
    assert_eq!(a.rows.len(), 2);
    assert_eq!(a.values.len(), 1);
    assert!(matches!(
        compare(&a, &b, 100).unwrap(),
        Comparison::Equivalent { .. }
    ));
    let r = catalog(
        &[base.join("a"), base.join("b")],
        &base.join("catalog"),
        100,
    )
    .unwrap();
    assert_eq!(r["closed_states"], 1);
    assert_eq!(r["triggers"].as_array().unwrap().len(), 2);
    assert_ne!(r["triggers"][0]["entry"], r["triggers"][1]["entry"]);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn regular_graphs_need_exact_confirmation_not_just_refinement() {
    let mut a = state();
    a.ports.clear();
    a.values = vec![
        Vertex {
            sort: "E".into(),
            literal: None
        };
        6
    ];
    a.rows = (0..6)
        .map(|i| Row {
            op: "Next".into(),
            args: vec![i],
            result: (i + 1) % 6,
        })
        .collect();
    let mut b = a.clone();
    b.rows = (0..6)
        .map(|i| Row {
            op: "Next".into(),
            args: vec![i],
            result: (i / 3) * 3 + (i + 1) % 3,
        })
        .collect();
    assert!(matches!(
        compare(&a, &b, 1).unwrap(),
        Comparison::UnknownBudget { .. }
    ));
    assert!(matches!(
        compare(&a, &b, 10000).unwrap(),
        Comparison::Different { .. }
    ));
}

#[test]
fn native_export_keeps_literals_and_global_ports() {
    use std::{fs, process::Command};
    let base = std::env::temp_dir().join(format!("closed-fields-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let run = |name: &str, source: &str| {
        let file = base.join(format!("{name}.egg"));
        fs::write(&file, source).unwrap();
        let out = base.join(name);
        let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .args(["ripen", file.to_str().unwrap(), out.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
        out
    };
    let a = run("one", "(datatype E (N i64) (S String)) (N 1) (S \"x\")");
    let b = run("two", "(datatype E (N i64) (S String)) (N 2) (S \"x\")");
    let aa = read(&a.join("closed-state.json")).unwrap();
    let bb = read(&b.join("closed-state.json")).unwrap();
    assert!(
        aa.values
            .iter()
            .any(|v| v.sort == "String" && v.literal.is_some())
    );
    assert!(matches!(
        compare(&aa, &bb, 100).unwrap(),
        Comparison::Different { .. }
    ));
    let a = run("root-a", "(datatype E (A) (B)) (let root (A)) (B)");
    let b = run("root-b", "(datatype E (A) (B)) (A) (let root (B))");
    assert!(matches!(
        compare(
            &read(&a.join("closed-state.json")).unwrap(),
            &read(&b.join("closed-state.json")).unwrap(),
            100
        )
        .unwrap(),
        Comparison::Different { .. }
    ));
    let out = run(
        "primitive",
        "(datatype E (N i64)) (N 0) (rule ((= e (N 0))) ((N (+ 0 1))))",
    );
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("ripen.json")).unwrap()).unwrap();
    assert_eq!(report["ripen"]["state"], "Closed");
    assert_eq!(report["closed_state"]["status"], "unavailable");
    assert!(!out.join("closed-state.json").exists());
    assert!(catalog(&[out], &base.join("invalid-catalog"), 100).is_err());
    fs::remove_dir_all(base).unwrap();
}
