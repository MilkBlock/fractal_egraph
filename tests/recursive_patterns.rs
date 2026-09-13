use std::{path::PathBuf, process::Command};
fn run(source: &str, tag: &str) -> (PathBuf, serde_json::Value) {
    let out = std::env::temp_dir().join(format!("recursive-patterns-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("EGG_LAYOUT_DISCOVER_RECURSION", "1")
        .env(
            "EGG_LAYOUT_BINDING_ALGEBRA",
            if source.ends_with("math_binding.egg") {
                "math"
            } else {
                "integer-safe"
            },
        )
        .args([
            "analyze",
            "--recapture-tier0",
            "--source",
            source,
            "--save-history",
            "--output",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report =
        serde_json::from_slice(&std::fs::read(out.join("recursive_patterns.json")).unwrap())
            .unwrap();
    (out, report)
}
#[test]
fn binary_and_ternary_are_the_same_parameterized_constructor() {
    for (source, tag, m, depth, units, applies) in [
        (
            "experiments/recursive_patterns/binary.egg",
            "two",
            2,
            4,
            15,
            45,
        ),
        (
            "experiments/recursive_patterns/ternary.egg",
            "three",
            3,
            3,
            13,
            52,
        ),
    ] {
        let (out, r) = run(source, tag);
        assert_eq!(r["pattern_count"], 1);
        let p = &r["patterns"][0];
        assert_eq!(p["observed_arity"], m);
        assert!(p["additional_returning_witnesses"].as_u64().unwrap() > 0);
        let instances = p["instances"].as_array().unwrap();
        assert_eq!(instances.len(), m as usize);
        for i in instances {
            assert_eq!(i["extent_kind"], "Depth");
            assert_eq!(i["depth"], depth);
            assert_eq!(i["observed_units"], units);
            assert_eq!(i["observed_apply_events"], applies);
            assert_eq!(
                i["binding_reduction"]["recursive_ports"]
                    .as_array()
                    .unwrap()
                    .len(),
                m as usize
            );
            for p in i["binding_reduction"]["recursive_ports"]
                .as_array()
                .unwrap()
            {
                assert_eq!(p["status"], "linked", "{p}");
            }
            assert!(i["binding_reduction"]["effect_count"].as_u64().unwrap() > 0);
            assert!(
                i["binding_reduction"]["requirement_count"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
            assert_eq!(
                i["binding_reduction"]["status"], "substituted",
                "{}",
                i["binding_reduction"]
            );
            assert_eq!(
                i["binding_reduction"]["one_layer_events"]
                    .as_array()
                    .unwrap()
                    .len(),
                1 + m as usize
            );
            assert!(
                !i["binding_reduction"]["output_addresses"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert!(
                i["fractal_comb"]
                    .as_str()
                    .unwrap()
                    .starts_with("(FractalComb (Depth")
            );
        }
        // Repeat downstream analysis without running tier0.
        let replay = out.with_extension("replay");
        let _ = std::fs::remove_dir_all(&replay);
        let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("EGG_LAYOUT_DISCOVER_RECURSION", "1")
            .args(["analyze", "--replay-history"])
            .arg(out.join("history.json"))
            .arg("--output")
            .arg(&replay)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(replay.join("recursive_patterns.json")).unwrap())
                .unwrap();
        assert!(
            actual == r,
            "canonical report differs after history replay; inspect retained temporary reports"
        );
        std::fs::remove_dir_all(out).unwrap();
        std::fs::remove_dir_all(replay).unwrap();
    }
}
#[test]
fn duplicate_sites_do_not_mint_two_branches() {
    let (out, r) = run("experiments/recursive_patterns/deduplicated.egg", "dedup");
    assert!(r["pattern_count"].as_u64().unwrap() > 0);
    for p in r["patterns"].as_array().unwrap() {
        assert_eq!(p["observed_arity"], 1);
    }
    std::fs::remove_dir_all(out).unwrap();
}
#[test]
fn partial_local_applies_keep_effects_and_do_not_fake_b_events() {
    let (out, r) = run("experiments/recursive_patterns/partial.egg", "partial");
    assert_eq!(r["pattern_count"], 1);
    let p = &r["patterns"][0];
    assert_eq!(p["observed_arity"], 2);
    for i in p["instances"].as_array().unwrap() {
        assert_eq!(i["extent_kind"], "SparseExtent");
        assert!(i["depth"].is_null());
        assert_eq!(i["observed_apply_events"], 13);
        let mut seen = std::collections::BTreeSet::new();
        let mut pending = 0;
        for n in i["nodes"].as_array().unwrap() {
            seen.insert(n["entry_event"].as_u64().unwrap());
            for port in n["ports"].as_array().unwrap() {
                if port["state"] == "OpenPort" {
                    pending += 1;
                    assert!(port["observed_child"].is_null());
                } else {
                    seen.insert(port["observed_child"].as_u64().unwrap());
                }
            }
        }
        assert_eq!(pending, 8);
        assert_eq!(seen.len(), 13);
        assert_eq!(i["event_refs"].as_array().unwrap().len(), 13);
    }
    std::fs::remove_dir_all(out).unwrap();
}

#[test]
fn actual_catalog_has_certified_observed_dominance_without_double_counting() {
    let (out, r) = run("experiments/recursive_patterns/dominance.egg", "dominance");
    let edges = r["observed_dominance"].as_array().unwrap();
    assert!(!edges.is_empty());
    let ranks: std::collections::BTreeMap<_, _> = r["patterns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_u64().unwrap(),
                p["ranking"]["rank"].as_u64().unwrap(),
            )
        })
        .collect();
    for edge in edges {
        assert!(
            ranks[&edge["dominant"].as_u64().unwrap()]
                < ranks[&edge["dominated"].as_u64().unwrap()]
        );
        assert_eq!(edge["semantic_dominance"], "unproved");
    }
    let marginal: u64 = r["patterns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["ranking"]["new_apply_events"].as_u64().unwrap())
        .sum();
    assert_eq!(
        marginal,
        r["event_witnesses"].as_object().unwrap().len() as u64
    );
    assert!(
        r["sharing"]["repeated_witness_records_removed"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        r["sharing"]["shared_catalog_json_bytes"].as_u64().unwrap()
            < r["sharing"]["raw_catalog_json_bytes"].as_u64().unwrap()
    );
    std::fs::remove_dir_all(out).unwrap();
}

#[test]
fn real_arithmetic_and_recursive_interfaces_reach_reduce() {
    for (source, tag) in [
        (
            "experiments/recursive_patterns/integer_binding.egg",
            "integer-binding",
        ),
        (
            "experiments/recursive_patterns/math_binding.egg",
            "math-binding",
        ),
    ] {
        let (out, r) = run(source, tag);
        assert!(r["native_pattern_count"].as_u64().unwrap() > 0, "{r}");
        let mut checked = 0;
        for p in r["patterns"].as_array().unwrap() {
            if p["kind"] != "recursive_dag" {
                continue;
            };
            for i in p["instances"].as_array().unwrap() {
                let b = &i["binding_reduction"];
                assert_eq!(b["status"], "substituted", "{b}");
                assert!(b["math_calls"].as_u64().unwrap() > 0, "{b}");
                assert!(b["recursive_output_count"].as_u64().unwrap() > 0, "{b}");
                let endpoints = b["endpoint_outputs"].as_array().unwrap();
                if tag == "integer-binding" {
                    assert!(b["definedness_obligations"].as_u64().unwrap() > 0);
                    assert!(
                        endpoints.iter().any(|e| e == "(EIMul (EParam 0) (EInt 2))"),
                        "{endpoints:?}"
                    );
                    assert!(
                        endpoints
                            .iter()
                            .any(|e| e == "(EIAdd (EIMul (EParam 0) (EInt 2)) (EInt 1))"),
                        "{endpoints:?}"
                    );
                    assert!(
                        !endpoints
                            .iter()
                            .any(|e| e == "(EIAdd (EIMul (EParam 0) (EInt 2)) (EInt 0))")
                    );
                } else {
                    assert!(
                        endpoints
                            .iter()
                            .all(|e| !e.as_str().unwrap().contains("(EAdd ")),
                        "{endpoints:?}"
                    );
                }
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "no source arithmetic simplification was checked"
        );
        std::fs::remove_dir_all(out).unwrap();
    }
}
