use eggfuse_bake::closure_contract::{Change, Registry, Resource};
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
    use std::fs;
    let root = std::env::temp_dir().join(format!("visibility-contract-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let src = root.join("source.egg");
    fs::write(
        &src,
        "(datatype E (A))\n(rule ((= a (A))) ((subsume (A))))\n(A)",
    )
    .unwrap();
    eggfuse_bake::native_analyze::ripen::run(&src, &root.join("run")).unwrap();
    let s: Value = serde_json::from_slice(
        &fs::read(root.join("run/saturated-rule-composition.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(s["rows"].as_array().unwrap().len(), 1);
    assert_eq!(s["subsumed_rows"], serde_json::json!([0]));
    fs::remove_dir_all(root).unwrap();
}
