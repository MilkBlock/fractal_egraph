use egg_layout::reuse_graph::adaptive;
use serde_json::Value;
use std::path::PathBuf;
fn dir(s: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "evolution-{s}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
fn compare(env: &str, training: &str, work: &str) -> Value {
    let bake = dir("bake");
    adaptive::run_with_min_reuse(
        &format!("{env} {training} (run 2)"),
        None,
        &bake,
        "adaptive",
        1,
    )
    .unwrap();
    let library = bake.join("bake_lib_round1.json");
    let source = format!("{env} {work}");
    let control =
        adaptive::run_with_min_reuse(&source, Some(&library), &dir("control"), "frozen", 1)
            .unwrap();
    let observed_dir = dir("observed");
    let observed =
        adaptive::run_with_evolution(&source, Some(&library), &observed_dir, "frozen", 1).unwrap();
    for k in ["enodes", "eclasses"] {
        assert_eq!(control["final"][k], observed["final"][k]);
    }
    assert_eq!(control["matches"], observed["matches"]);
    serde_json::from_slice(&std::fs::read(observed_dir.join("cell-evolution.json")).unwrap())
        .unwrap()
}
#[test]
fn repeated_boundary_extensions_share_patterns_without_concrete_ids() {
    let env = "(datatype Math (V i64) (A Math) (B Math) (Flag Math) (Pair Math Math) (Done Math)) (rewrite (A x) (B x)) (rewrite (Pair (B x) (Flag x)) (Done x))";
    let mut work = String::new();
    for i in 0..12 {
        work += &format!("(A (V {i}))");
    }
    work += "(run 2)";
    for i in 0..12 {
        work += &format!("(Pair (B (V {i})) (Flag (V {i})))");
    }
    work += "(run 3)";
    for i in 0..12 {
        work += &format!("(check (= (Pair (B (V {i})) (Flag (V {i}))) (Done (V {i}))))");
    }
    let r = compare(env, "(A (V 99)) (Pair (B (V 99)) (Flag (V 99)))", &work);
    assert!(
        r["patterns_repeated_across_instances"].as_u64().unwrap() > 0,
        "{r:#}"
    );
    assert!(r["boundary_linked_births"].as_u64().unwrap() >= 12, "{r:#}");
    assert!(
        r["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["distinct_source_cells"].as_u64().unwrap() >= 12)
    );
}
#[test]
fn input_refinement_and_two_parent_growth_are_visible() {
    let env = "(datatype Math (V i64) (A Math) (B Math) (C Math) (Done Math)) (rewrite (A x) (B x)) (rewrite (B (C x)) (Done x))";
    let r = compare(
        env,
        "(A (V 99)) (B (C (V 98)))",
        "(A (V 0)) (run 2) (union (V 0) (C (V 1))) (run 3) (check (= (A (V 0)) (Done (V 1))))",
    );
    assert!(
        r["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["transition"]["input_refinement"] == true),
        "{r:#}"
    );
    let env = "(datatype Math (V i64) (A Math) (B Math) (Pair Math Math) (Done Math Math)) (rewrite (A x) (B x)) (rewrite (Pair (B x) (B y)) (Done x y))";
    let r = compare(
        env,
        "(A (V 99)) (Pair (B (V 98)) (B (V 97)))",
        "(A (V 0)) (A (V 1)) (run 2) (Pair (B (V 0)) (B (V 1))) (run 3) (check (= (Pair (B (V 0)) (B (V 1))) (Done (V 0) (V 1))))",
    );
    assert!(r["multi_support_events"].as_u64().unwrap() > 0, "{r:#}");
    assert!(
        r["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["example"]["support_cells"].as_array().unwrap().len() >= 2)
    );
}
#[test]
fn unrelated_same_round_cells_are_not_linked() {
    let env = "(datatype Math (V i64) (A Math) (B Math) (C Math) (D Math)) (rewrite (A x) (B x)) (rewrite (C x) (D x))";
    let r = compare(
        env,
        "(A (V 99)) (C (V 98))",
        "(A (V 0)) (run 2) (C (V 0)) (run 3)",
    );
    assert_eq!(r["boundary_linked_births"], 0, "{r:#}");
    assert_eq!(r["transition_patterns"], 0);
    assert!(r["loose_interface_contacts_not_expanded"].as_u64().unwrap() > 0);
}

#[test]
fn union_coalescing_does_not_invent_new_cell_births() {
    let env = "(datatype Math (V i64) (A Math) (B Math)) (rewrite (A x) (B x))";
    let r = compare(
        env,
        "(A (V 99))",
        "(A (V 0)) (A (V 1)) (run 2) (union (V 0) (V 1)) (run 3)",
    );
    assert_eq!(r["observed_instance_births"], 2, "{r:#}");
    assert_eq!(r["canonical_instances"], 1, "{r:#}");
    assert_eq!(r["coalesced_cells"], 1, "{r:#}");
}

#[test]
fn an_input_aliased_to_a_body_output_keeps_its_lower_port_role() {
    let env = "(datatype Math (V i64) (A Math) (C Math) (Done Math)) (rewrite (A x) x) (rewrite (C x) (Done x))";
    let r = compare(
        env,
        "(A (V 99)) (C (V 98))",
        "(A (V 0)) (run 2) (union (V 0) (C (V 1))) (run 3) (check (= (A (V 0)) (Done (V 1))))",
    );
    assert!(
        r["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["transition"]["source_support"] == "input_refinement"),
        "{r:#}"
    );
}
