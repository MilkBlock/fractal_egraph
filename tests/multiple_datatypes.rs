use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn native_ripen_exports_typed_cross_sort_rows() {
    let dir = std::env::temp_dir().join(format!("multi-ripen-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.egg");
    fs::write(
        &source,
        r#"
(sort Expr) (sort Type)
(constructor Num (i64) Expr)
(constructor Annotated (Expr Type) Expr)
(constructor Ty (String) Type)
(constructor Other (String) Type)
(relation Seen (Expr Type))
(function Weight (Expr) f64 :merge (min old new))
(rewrite (Ty s) (Other s))
(rule ((= e (Annotated x t))) ((Seen e t)))
(let root (Annotated (Num 1) (Ty "int")))
(set (Weight root) 1.5)
(check (= (Ty "int") (Other "int")))
"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .arg("ripen")
        .arg(&source)
        .arg(dir.join("run"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let state: Value =
        serde_json::from_slice(&fs::read(dir.join("run/saturated-rule-composition.json")).unwrap())
            .unwrap();
    assert_eq!(
        state["scope"]["datatypes"],
        serde_json::json!(["Expr", "Type"])
    );
    let vertices = state["values"].as_array().unwrap();
    for row in state["rows"].as_array().unwrap() {
        let result = &vertices[row["result"].as_u64().unwrap() as usize];
        match row["op"].as_str().unwrap() {
            "Num" | "Annotated" => assert_eq!(result["sort"], "Expr"),
            "Ty" | "Other" => assert_eq!(result["sort"], "Type"),
            "Weight" => {
                assert_eq!(result["sort"], "f64");
                assert_eq!(result["literal"], "1.5");
            }
            "Seen" => assert_eq!(result["sort"], "Unit"),
            op => panic!("unexpected row {op}"),
        }
    }
    assert!(
        state["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["op"] == "Seen")
    );
    fs::remove_dir_all(dir).unwrap();
}
