use serde_json::Value;
use std::{path::PathBuf, process::Command};
fn run(source: &str) -> (PathBuf, Value, Value) {
    let dir = std::env::temp_dir().join(format!(
        "observed-states-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("source.egg");
    std::fs::write(&file, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--source"])
        .arg(file)
        .arg("--output")
        .arg(dir.join("run"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env_remove("EGG_LAYOUT_COMPOSITION_DISCOVERY")
        .env_remove("EGG_LAYOUT_LAYER_VIEW")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let read =
        |name| serde_json::from_slice(&std::fs::read(dir.join("run").join(name)).unwrap()).unwrap();
    let report = read("analysis.json");
    let transitions = read("observed-transitions.json");
    (dir, report, transitions)
}
#[test]
fn flat_multi_parent_graft_is_registered_without_layers_or_query_cut() {
    let (dir, report, transitions) = run(r#"
(datatype Math (V i64) (A Math) (B Math) (Pair Math Math) (Done Math Math))
(rewrite (A x) (B x))
(rewrite (Pair (B x) (B y)) (Done x y))
(rewrite (B x) (B x))
(Pair (A (V 0)) (A (V 1)))
(run 3)
(check (= (Pair (A (V 0)) (A (V 1))) (Done (V 0) (V 1))))
"#);
    let cs = &report["saturated_rule_composition"]["cs"];
    assert_eq!(cs["discovery"], "committed_apply_dag");
    assert!(
        cs["confirmations_without_new_effect"].as_u64().unwrap() > 0,
        "{report:#}"
    );
    let events = transitions["transitions"].as_array().unwrap();
    assert!(
        events
            .iter()
            .any(|t| t["rule"] == 1 && t["producer_states"].as_array().unwrap().len() >= 2),
        "{transitions:#}"
    );
    assert!(
        events.iter().all(|t| t["rule"] != 2),
        "noop must not claim a mutation"
    );
    assert!(
        cs["units"]
            .as_array()
            .unwrap()
            .iter()
            .all(|u| u["coarse_layer"].is_null())
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn same_batch_peers_are_not_serialized_and_next_batch_uses_root_frontier() {
    let (dir, _, transitions) = run(r#"
(datatype Math (V i64) (A Math) (B Math) (C Math) (D Math))
(rewrite (A x) (B x))
(rewrite (A x) (C x))
(rewrite (B x) (D x))
(rule ((= a (V 1)) (= b (V 2))) ((union a b)) :name "explicit-union")
(A (V 0))
(V 1) (V 2)
(run 3)
"#);
    let events = transitions["transitions"].as_array().unwrap();
    for rule in [0, 1] {
        let first = events.iter().find(|t| t["rule"] == rule).unwrap();
        assert!(
            first["parents"].as_array().unwrap().is_empty(),
            "{transitions:#}"
        );
    }
    assert!(
        events
            .iter()
            .any(|t| t["rule"] == 2 && t["context_states"].as_array().unwrap().len() >= 2),
        "{transitions:#}"
    );
    assert!(
        events
            .iter()
            .any(|t| !t["union_effects"].as_array().unwrap().is_empty())
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn accelerated_runtime_discovers_graft_even_when_no_fast_cut_exists() {
    use egg_layout::reuse_graph::adaptive;
    let env = r#"(datatype Math (V i64) (A Math) (B Math) (Pair Math Math) (Done Math Math))
(rewrite (A x) (B x))
(rewrite (Pair (B x) (B y)) (Done x y))"#;
    let dir = std::env::temp_dir().join(format!(
        "observed-runtime-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let training = format!("{env} (A (V 90)) (Pair (B (V 91)) (B (V 92))) (run 2)");
    adaptive::run_with_min_reuse(&training, None, &dir.join("bake"), "adaptive", 1).unwrap();
    let source = format!(
        "{env} (let a (A (V 0))) (let b (A (V 1))) (run 1) (Pair a b) (run 2) (check (= (Pair a b) (Done (V 0) (V 1))))"
    );
    let result = adaptive::run_cell_machine(
        &source,
        &dir.join("bake/bake_lib.json"),
        &dir.join("run"),
        &[],
        1,
    )
    .unwrap();
    assert_eq!(result["cell_transitions_enabled"], false);
    let runtime = &result["dynamic_instances"];
    assert_eq!(runtime["compiled_link_count"], 0);
    assert!(
        runtime["observed_grafts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["rule"] == 1
                && g["parents"].as_array().unwrap().len() == 2
                && runtime["graft_binding_patterns"]
                    [g["relative_binding_pattern"].as_u64().unwrap() as usize]["parent_slots"]
                    .as_array()
                    .unwrap()
                    .len()
                    == 2),
        "{runtime:#}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn offline_history_keeps_commit_boundaries_and_external_union_context() {
    let dir = std::env::temp_dir().join(format!(
        "observed-replay-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("source.egg");
    std::fs::write(
        &file,
        r#"
(datatype Math (V i64) (A Math) (B Math) (Pair Math Math) (Done Math Math))
(rewrite (A x) (B x))
(rewrite (Pair (B x) (B y)) (Done x y))
(let a (A (V 0))) (let b (A (V 1)))
(run 1)
(union (V 0) (V 1))
(Pair a b)
(run 2)
"#,
    )
    .unwrap();
    let invoke = |args: Vec<String>| {
        let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .args(args)
            .env("EGG_LAYOUT_GRAPHS", "0")
            .env_remove("EGG_LAYOUT_COMPOSITION_DISCOVERY")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    invoke(vec![
        "analyze".into(),
        "--source".into(),
        file.display().to_string(),
        "--output".into(),
        dir.join("online").display().to_string(),
        "--save-history".into(),
    ]);
    invoke(vec![
        "analyze".into(),
        "--replay-history".into(),
        dir.join("online/history.json").display().to_string(),
        "--output".into(),
        dir.join("offline").display().to_string(),
    ]);
    let read = |name: &str| -> Value {
        serde_json::from_slice(
            &std::fs::read(dir.join(name).join("observed-transitions.json")).unwrap(),
        )
        .unwrap()
    };
    let mut a = read("online");
    let mut b = read("offline");
    for value in [&mut a, &mut b] {
        for t in value["transitions"].as_array_mut().unwrap() {
            t.as_object_mut().unwrap().remove("eclass");
        }
    }
    assert_eq!(a, b);
    std::fs::remove_dir_all(dir).unwrap();
}
