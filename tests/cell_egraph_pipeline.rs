//! Verify the production analyze path registers and normalizes actual CS jobs.
use serde_json::Value;
#[test]
fn analyze_registers_cells_without_a_separate_demo_command() {
    let dir = std::env::temp_dir().join(format!(
        "cell-pipeline-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.egg");
    std::fs::write(
        &source,
        r#"
(datatype Math (V i64) (A Math) (B Math) (C Math) (Pair Math Math))
(rewrite (A x) (B x) :name "ab")
(rewrite (B x) (C x) :name "bc")
(rule ((= bx (B x)) (= cy (C y))) ((Pair bx cy)) :name "cross")
(A (V 0))
(A (V 1))
(Pair (A (V 0)) (A (V 1)))
(run 3)
"#,
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--source"])
        .arg(&source)
        .arg("--output")
        .arg(dir.join("run"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value =
        serde_json::from_slice(&std::fs::read(dir.join("run/cell-egraph.json")).unwrap()).unwrap();
    assert!(
        report["registered_compositions"].as_u64().unwrap() > 0,
        "{report}"
    );
    assert!(report["certified_unions"].as_u64().unwrap() > 0, "{report}");
    assert!(
        report["eclasses"].as_u64().unwrap() < report["registered_compositions"].as_u64().unwrap()
    );
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}
