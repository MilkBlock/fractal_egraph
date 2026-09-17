use egg_layout::native_analyze::{self, debug};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

#[test]
fn parser_maps_unicode_multiline_rules_without_annotations() {
    let source = "; 中文注释 (rewrite fake)\n(datatype Math (Const i64) (Add Math Math))\n(rewrite\n (Add x (Const 0))\n x :name \"zero\")\n(rule ((= a (Const 1)))\n ((Const 2)) :name \"two\")\n";
    let data = debug::patterns(source).unwrap();
    let rows = data["patterns"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            rows[0]["source_line"].as_u64(),
            rows[0]["end_line"].as_u64()
        ),
        (Some(3), Some(5))
    );
    assert_eq!(
        (
            rows[1]["source_line"].as_u64(),
            rows[1]["end_line"].as_u64()
        ),
        (Some(6), Some(7))
    );
    assert!(rows[0]["typst"].as_str().unwrap().contains("==>"));
    assert!(rows[0]["dot"].as_str().unwrap().contains("union"));
    assert!(debug::patterns("(rewrite").is_err());
}

#[test]
fn birewrite_previews_its_forward_rewrite_at_its_own_line() {
    let source = "(datatype Math (Const i64) (Add Math Math))\n(birewrite (Add x (Const 0)) x)\n(rewrite (Add x (Const 1)) x)\n";
    let data = debug::patterns(source).unwrap();
    let rows = data["patterns"].as_array().unwrap();
    // The birewrite has no transpiled `add_rule` scope, but it still gets a row and
    // must not shift the rewrite that follows it.
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["source_line"].as_u64(), Some(2));
    assert_eq!(rows[1]["source_line"].as_u64(), Some(3));
    assert!(rows[0]["source"].as_str().unwrap().contains("__viz_root"));
    assert!(rows[0]["typst"].as_str().unwrap().contains("==>"));
}

#[test]
fn stream_uses_committed_evidence_and_matches_offline_fractal_paths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("experiments/bake/increment-3.egg");
    let mut rows = vec![];
    debug::stream(root, &source, &mut |row| {
        rows.push(row);
        Ok(())
    })
    .unwrap();
    let filter = |kind| rows.iter().filter(move |r| r["kind"] == kind);
    assert_eq!(filter("application").count(), 6);
    assert_eq!(filter("compose").count(), 5);
    assert_eq!(filter("fractal").count(), 4);
    assert_eq!(rows.last().unwrap()["kind"], "complete");
    let mut ids = BTreeSet::new();
    for row in rows.iter().filter(|r| r.get("id").is_some()) {
        assert!(ids.insert(row["id"].as_str().unwrap()));
        assert!(!row["typst"].as_str().unwrap().is_empty());
        assert!(!row["dot"].as_str().unwrap().is_empty());
        assert_eq!(row["source_line"], 3);
    }
    let applications: Vec<_> = filter("application").collect();
    assert_ne!(applications[0]["typst"], applications[1]["typst"]);
    for row in filter("compose") {
        assert!(row["steps"].as_array().unwrap().len() >= 2);
        assert!(row["dot"].as_str().unwrap().contains("parent 0"));
    }
    let boundaries: Vec<_> = filter("boundary").collect();
    assert_eq!(boundaries[0]["applications"], 1);
    assert_eq!(boundaries[1]["applications"], 2);
    assert_eq!(boundaries.last().unwrap()["applications"], 6);
    assert!(
        filter("fractal").next().unwrap()["boundary"]
            .as_u64()
            .unwrap()
            < 8
    );
    let out = std::env::temp_dir().join(format!("native-debug-compare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let report = native_analyze::run(root, &source, None, &out).unwrap();
    for lane in report["view"]["lanes"].as_array().unwrap() {
        assert!(
            filter("fractal").any(|r| r["evidence"]["events"] == lane["events"]
                && r["evidence"]["trigger"] == lane["trigger"])
        );
    }
    assert_eq!(report["summary"]["imported_events"], 6);
    std::fs::remove_dir_all(out).unwrap();
}

#[test]
fn no_effect_matches_do_not_become_application_logs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::env::temp_dir().join(format!("native-debug-noop-{}.egg", std::process::id()));
    std::fs::write(
        &source,
        "(datatype Math (A i64)) (A 1) (rule ((= x (A n))) ((A n)) :name \"noop\") (run 3)",
    )
    .unwrap();
    let mut rows: Vec<Value> = vec![];
    debug::stream(root, &source, &mut |r| {
        rows.push(r);
        Ok(())
    })
    .unwrap();
    assert!(
        !rows.iter().any(|r| r["kind"] == "application"
            || r["kind"] == "compose"
            || r["kind"] == "fractal")
    );
    std::fs::remove_file(source).unwrap();
}
