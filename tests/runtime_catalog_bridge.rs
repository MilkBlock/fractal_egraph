//! Real analysis catalog -> exact slot certificates -> native runtime adapters.
use egg_layout::reuse_graph::adaptive;
use serde_json::{Value, json};
use std::{path::Path, process::Command};
const ENV: &str = r#"
(datatype Math (V i64) (A Math) (B Math) (C Math) (Flag Math) (Pair Math Math) (Done Math Math))
(rewrite (A x) (B (C x)))
(rewrite (B (C x)) (A x))
(rewrite (Pair (B (C x)) (Flag y)) (Done x y))
"#;
fn command(args: &[&str], catalog: Option<&Path>, plans: Option<&Path>) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_egg_layout"));
    c.args(args)
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env_remove("EGG_LAYOUT_CELL_CATALOG");
    if let Some(p) = catalog {
        c.env("EGG_LAYOUT_CELL_CATALOG", p);
    }
    if let Some(p) = plans {
        c.env("EGG_LAYOUT_CELL_TRANSITIONS", p);
    }
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn analysis_equivalence_transfers_a_transition_to_another_bake_recipe() {
    let dir = std::env::temp_dir().join(format!(
        "catalog-bridge-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let training =
        format!("{ENV} (A (V 90)) (B (C (V 91))) (Pair (B (C (V 92))) (Flag (V 93))) (run 2)");
    let source = dir.join("train.egg");
    std::fs::write(&source, &training).unwrap();
    adaptive::run_with_min_reuse(&training, None, &dir.join("bake"), "adaptive", 1).unwrap();
    command(
        &[
            "analyze",
            "--source",
            source.to_str().unwrap(),
            "--output",
            dir.join("analysis").to_str().unwrap(),
        ],
        None,
        None,
    );
    let mut bake: adaptive::Bake =
        serde_json::from_slice(&std::fs::read(dir.join("bake/bake_lib.json")).unwrap()).unwrap();
    let cell = bake.macros.get_mut(&1).unwrap();
    let size = cell.slots.len();
    cell.slots.reverse();
    for (_, columns) in &mut cell.template {
        for c in columns {
            *c = size - 1 - *c;
        }
    }
    let permuted = dir.join("permuted.json");
    std::fs::write(&permuted, serde_json::to_vec(&bake).unwrap()).unwrap();
    let plans = dir.join("plans.json");
    std::fs::write(
        &plans,
        json!([{"source_rule":0,"consumer_rule":2,"cut_path":[0],"anchor_path":[0]}]).to_string(),
    )
    .unwrap();
    let runtime = dir.join("runtime.egg");
    std::fs::write(&runtime,format!("{ENV} (let root (B (C (V 0)))) (A (V 2)) (run 1) (Pair root (Flag (V 1))) (run 3) (check (= (Pair root (Flag (V 1))) (Done (V 0) (V 1))))")).unwrap();
    command(
        &[
            "reuse-adaptive",
            "frozen",
            runtime.to_str().unwrap(),
            dir.join("run").to_str().unwrap(),
            permuted.to_str().unwrap(),
        ],
        Some(&dir.join("analysis/catalog")),
        Some(&plans),
    );
    let read = |file: &str| -> Value {
        serde_json::from_slice(&std::fs::read(dir.join("run").join(file)).unwrap()).unwrap()
    };
    let catalog = read("cell-equivalences.json");
    let entries = catalog["entries"].as_array().unwrap();
    let a = entries.iter().find(|x| x["rule"] == 0).unwrap();
    let b = entries.iter().find(|x| x["rule"] == 1).unwrap();
    assert!(!a["analysis_state"].is_null(), "{catalog:#}");
    assert_eq!(a["analysis_state"], b["analysis_state"]);
    assert_eq!(a["eclass"], b["eclass"]);
    assert_ne!(a["slot_map"], b["slot_map"]);
    let transitions = read("cell-transitions.json");
    assert!(
        transitions
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["source_cell"] == 1 && p["equivalent_source"]["from"] == 0),
        "{transitions:#}"
    );
    let runtime_report = read("report.json");
    assert_eq!(
        runtime_report["dynamic_instances"]["instances"], 3,
        "{runtime_report:#}"
    );
    assert_eq!(
        runtime_report["dynamic_instances"]["state_revisions"], 1,
        "equivalent plan adapters must share one transition"
    );
    println!(
        "{}",
        json!({"catalog":catalog,"runtime":read("report.json")["dynamic_instances"]})
    );
    let union_source = dir.join("union.egg");
    let original = std::fs::read_to_string(&runtime).unwrap();
    std::fs::write(
        &union_source,
        original.replacen("(run 1)", "(run 1) (union (V 0) (V 2))", 1),
    )
    .unwrap();
    command(
        &[
            "reuse-adaptive",
            "frozen",
            union_source.to_str().unwrap(),
            dir.join("union-run").to_str().unwrap(),
            permuted.to_str().unwrap(),
        ],
        Some(&dir.join("analysis/catalog")),
        Some(&plans),
    );
    let union_report: Value =
        serde_json::from_slice(&std::fs::read(dir.join("union-run/report.json")).unwrap()).unwrap();
    assert_eq!(
        union_report["dynamic_instances"]["instances"], 2,
        "{union_report:#}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
