use egg_layout::native_analyze::debug;
use serde_json::Value;

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
