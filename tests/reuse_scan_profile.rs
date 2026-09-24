#![cfg(feature = "native-read-profile")]
use egg_layout::reuse_graph::{library, planner::rule_runner};
use std::{fs, path::Path, process::Command};

fn program(consumers: usize) -> String {
    let mut code =
        "(datatype E (V i64) (A E) (B E))\n(rewrite (A x) (B x) :name \"a-to-b\")\n".to_string();
    for i in 0..consumers {
        code += &format!(
            "(relation Flag{i} (E)) (relation Done{i} (E))\n(rule ((= root (A x)) (= root (B x)) (Flag{i} x)) ((Done{i} root)) :name \"consumer{i}\")\n"
        );
    }
    for x in 0..64 {
        code += &format!("(A (V {x}))\n");
        for i in 0..consumers {
            code += &format!("(Flag{i} (V {x}))\n");
        }
    }
    code + "(run 3)"
}
#[test]
fn repeated_consumers_reduce_actual_scans_including_view_refresh() {
    let root = std::env::temp_dir().join(format!("reuse-scans-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let seed = root.join("seed");
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "analyze",
            "--source",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("experiments/reuse_feedback/seed.egg")
                .to_str()
                .unwrap(),
            "--output",
            seed.to_str().unwrap(),
            "--ripen-observe-ms",
            "60000",
        ])
        .env("EGG_LAYOUT_GRAPHS", "0")
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let file = root.join("library.json");
    library::bake(&[seed.to_string_lossy().into_owned()], &file).unwrap();
    let library = library::Library::load(&file).unwrap();
    for consumers in [1, 16] {
        let (report, _) =
            rule_runner::run_profiled(&program(consumers), &library, false, true).unwrap();
        assert_eq!(report["reference_equivalent"], true);
        assert_eq!(report["views"], 1, "all consumers must share one plan");
        let p = &report["read_profile"];
        let actual = p["optimized"]["total"]["scan_rows"].as_u64().unwrap();
        let reference = p["reference"]["total"]["scan_rows"].as_u64().unwrap();
        let refresh = p["view_refresh"]["scan_rows"].as_u64().unwrap();
        assert!(refresh > 0);
        assert_eq!(
            actual,
            refresh + p["source_and_other_queries"]["scan_rows"].as_u64().unwrap()
        );
        if consumers == 16 {
            assert!(actual < reference, "{report}");
        } else {
            assert!(
                actual > reference,
                "single-consumer overhead must remain visible: {report}"
            );
        }
        assert!(report["feature_filter"]["rejected"].as_u64().unwrap() > 0);
    }
    fs::remove_dir_all(root).unwrap();
}
