use egg_layout::closure_contract::{Change, Registry, Resource};
use std::collections::BTreeSet;
#[test]
fn notifications_only_visit_related_subscribers_and_follow_union_aliases() {
    let mut r = Registry::default();
    for i in 0..100 {
        r.watch(i, [Resource::Token(i), Resource::Table(format!("T{i}"))]);
    }
    assert_eq!(
        r.apply(&Change {
            unions: vec![(0, 1)],
            ..Default::default()
        }),
        BTreeSet::from([0, 1])
    );
    assert!(!r.valid(0));
    assert!(!r.valid(1));
    assert!(r.valid(2));
    r.watch(100, [Resource::Token(1)]);
    assert_eq!(
        r.apply(&Change {
            tokens: vec![0],
            ..Default::default()
        }),
        BTreeSet::from([0, 1, 100])
    );
    assert!(!r.valid(100));
    assert!(r.valid(99));
    assert_eq!(
        r.apply(&Change {
            tables: vec!["T2".into()],
            ..Default::default()
        }),
        BTreeSet::from([2])
    );
    assert!(!r.valid(2));
    assert!(r.valid(3));
    assert_eq!(
        r.apply(&Change {
            reset: true,
            ..Default::default()
        })
        .len(),
        101
    );
}
#[test]
fn subsume_is_preserved_by_native_export() {
    use serde_json::Value;
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!("visibility-contract-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let src = root.join("source.egg");
    fs::write(
        &src,
        "(datatype E (A))\n(rule ((= a (A))) ((subsume (A))))\n(A)",
    )
    .unwrap();
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .arg("ripen")
        .arg(src)
        .arg(root.join("run"))
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let s: Value =
        serde_json::from_slice(&fs::read(root.join("run/closed-state.json")).unwrap()).unwrap();
    assert_eq!(s["rows"].as_array().unwrap().len(), 1);
    assert_eq!(s["subsumed_rows"], serde_json::json!([0]));
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn native_updates_unions_and_top_level_subsume_are_in_history() {
    use serde_json::Value;
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!("events-contract-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let src = root.join("source.egg");
    fs::write(&src,"(datatype E (A) (B))\n(function flag (E) i64 :merge (max old new))\n(A)\n(B)\n(rule ((= a (A)) (= b (B))) ((union a b) (set (flag a) 1)))\n(run 1)\n(set (flag (B)) 2)\n(subsume (A))\n(run 1)").unwrap();
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--recapture-tier0", "--save-history", "--source"])
        .arg(src)
        .arg("--output")
        .arg(root.join("run"))
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let h: Value =
        serde_json::from_slice(&fs::read(root.join("run/history.json")).unwrap()).unwrap();
    let es = h["changes"].as_array().unwrap();
    assert!(
        es.iter()
            .any(|e| !e["unions"].as_array().unwrap().is_empty())
    );
    for name in ["A", "flag"] {
        assert!(
            es.iter()
                .any(|e| e["tables"].as_array().unwrap().iter().any(|t| t == name))
        );
    }
    fs::remove_dir_all(root).unwrap();
}
