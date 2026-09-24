//! The original A->B, B+Extra->C->D acceptance case, with two y bindings.
use serde_json::Value;
use std::{fs, path::Path, process::Command};
fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn run(args: &[&str]) -> Value {
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(args)
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env("EGG_LAYOUT_RIPEN_OBSERVE_MS", "60000")
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    serde_json::from_slice(&p.stdout).unwrap()
}
// Native Join order may permute independent applications. Compare the complete
// occurrence/layer DAG up to a bijection, preserving sharing and parent edges;
// comparing raw member IDs (or only counts/tree hashes) would be incorrect.
fn layer_shape(layers: &Value) -> egg_layout::saturated_rule_composition::SaturatedRuleComposition {
    use egg_layout::saturated_rule_composition::{Row, SaturatedRuleComposition, Vertex};
    let occurrences = layers["occurrences"].as_array().unwrap();
    let coarse = layers["coarse_layers"].as_array().unwrap();
    let smooth = layers["smooth_layers"].as_array().unwrap();
    let mut graph = SaturatedRuleComposition {
        version: 2,
        local_ids: vec![],
        scope: serde_json::json!("layer-shape-test"),
        values: vec![],
        rows: vec![],
        subsumed_rows: vec![],
        ports: Default::default(),
    };
    for o in occurrences {
        graph.values.push(Vertex {
            sort: format!("occurrence:{}:{}", o["comb"], o["boundary_restart"]),
            literal: None,
        });
    }
    let cbase = graph.values.len();
    for _ in coarse {
        graph.values.push(Vertex {
            sort: "coarse".into(),
            literal: None,
        });
    }
    let sbase = graph.values.len();
    for _ in smooth {
        graph.values.push(Vertex {
            sort: "smooth".into(),
            literal: None,
        });
    }
    let mut edge = |op: String, a: usize, b: usize| {
        graph.rows.push(Row {
            op,
            args: vec![a],
            result: b,
        })
    };
    for (i, o) in occurrences.iter().enumerate() {
        edge(
            "in-coarse".into(),
            i,
            cbase + o["coarse_layer"].as_u64().unwrap() as usize,
        );
        if let Some(s) = o["smooth_layer"].as_u64() {
            edge("in-smooth".into(), i, sbase + s as usize);
        }
        for (port, p) in o["parents"].as_array().unwrap().iter().enumerate() {
            edge(format!("parent-{port}"), i, p.as_u64().unwrap() as usize);
        }
    }
    for (i, c) in coarse.iter().enumerate() {
        for m in c["members"].as_array().unwrap() {
            edge(
                "coarse-member".into(),
                cbase + i,
                m.as_u64().unwrap() as usize,
            );
        }
        for child in c["children"].as_array().unwrap() {
            edge(
                "child".into(),
                cbase + i,
                cbase + child.as_u64().unwrap() as usize,
            );
        }
        if let Some(w) = c["witness"].as_u64() {
            edge("witness".into(), cbase + i, w as usize);
        }
    }
    for (i, s) in smooth.iter().enumerate() {
        edge(
            "smooth-support".into(),
            sbase + i,
            cbase + s["coarse_layer"].as_u64().unwrap() as usize,
        );
        for m in s["members"].as_array().unwrap() {
            edge(
                "smooth-member".into(),
                sbase + i,
                m.as_u64().unwrap() as usize,
            );
        }
    }
    graph
}
#[test]
fn learns_then_reuses_a_fragment_of_the_new_body_with_fresh_bindings() {
    let root = std::env::temp_dir().join(format!("minimal-cycle-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = repo.join("experiments/reuse_feedback/minimal");
    let seed = root.join("seed");
    run(&[
        "analyze",
        "--source",
        fixtures.join("seed.egg").to_str().unwrap(),
        "--output",
        seed.to_str().unwrap(),
    ]);
    let mut library = root.join("library.json");
    run(&[
        "reuse-bake",
        library.to_str().unwrap(),
        seed.to_str().unwrap(),
    ]);
    let mut counts = Value::Null;
    for step in 0..3 {
        let source = root.join(format!("graft-{step}.egg"));
        let mut code = fs::read_to_string(fixtures.join("graft.egg")).unwrap();
        if step > 0 {
            for n in 1..=3 {
                code = code.replace(&format!("(V {n})"), &format!("(V {})", n + 10));
            }
        }
        fs::write(&source, code).unwrap();
        let out = root.join(format!("cycle-{step}"));
        let cycle = run(&[
            "reuse-cycle",
            source.to_str().unwrap(),
            library.to_str().unwrap(),
            out.to_str().unwrap(),
        ]);
        assert_eq!(cycle["source_executions"], 1);
        let planned = read(out.join("analysis.json"));
        let native = root.join(format!("native-{step}"));
        run(&[
            "analyze",
            "--source",
            source.to_str().unwrap(),
            "--output",
            native.to_str().unwrap(),
            "--reuse-library",
            library.to_str().unwrap(),
        ]);
        assert_eq!(
            planned["summary"],
            read(native.join("analysis.json"))["summary"]
        );
        let a = read(out.join("layers.json"));
        let b = read(native.join("layers.json"));
        assert_eq!(a["combs"], b["combs"]);
        assert!(
            matches!(
                egg_layout::saturated_rule_composition::compare(&layer_shape(&a), &layer_shape(&b))
                    .unwrap(),
                egg_layout::saturated_rule_composition::Comparison::Equivalent { .. }
            ),
            "step={step}: layer DAGs differ"
        );
        let exact = run(&[
            "reuse-run",
            source.to_str().unwrap(),
            library.to_str().unwrap(),
        ]);
        assert_eq!(exact["reference_equivalent"], true);
        if step == 0 {
            assert!(cycle["learning"]["new_enclosure_count"].as_u64().unwrap() > 0);
            counts = cycle["learning"]["current"].clone();
        } else {
            assert_eq!(cycle["execution"]["views"], 1);
            assert_eq!(cycle["execution"]["fragment_validation_cache_entries"], 1);
            assert_eq!(cycle["execution"]["grafts"]["reuse_count"], 2);
            assert_eq!(cycle["execution"]["grafts"]["incomplete_witnesses"], 0);
            assert_eq!(cycle["learning"]["new_enclosure_count"], 0);
            assert_eq!(cycle["learning"]["current"], counts);
            assert!(
                cycle["execution"]["plans"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["reordered_for_bound_keys"] == true)
            );
        }
        library = out.join("reuse-library.json");
    }
    fs::remove_dir_all(root).unwrap();
}
