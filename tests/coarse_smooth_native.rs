use std::path::PathBuf;
#[test]
fn native_join_builds_a_witnessed_layer_without_tier1_rules() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::temp_dir().join(format!("native-coarse-smooth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let r = egg_layout::native_analyze::run(
        &root,
        &root.join("tests/fixtures/coarse_smooth_join.egg"),
        None,
        &out,
    )
    .unwrap();
    assert_eq!(r["summary"]["tier1_backend"], "rust-coarse-smooth-layers");
    let layers = &r["layers"];
    assert_eq!(layers["occurrences"].as_array().unwrap().len(), 3);
    let joined: Vec<_> = layers["coarse_layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["members"].as_array().unwrap().len() == 2)
        .collect();
    assert_eq!(joined.len(), 1);
    assert!(!joined[0]["witness"].is_null());
    let witness = joined[0]["witness"].as_u64().unwrap() as usize;
    assert!(!layers["occurrences"][witness]["smooth_layer"].is_null());
    assert_eq!(
        layers["occurrences"][witness]["parents"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    std::fs::remove_dir_all(&out).unwrap();
}
