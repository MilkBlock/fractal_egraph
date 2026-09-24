use serde_json::Value;
use std::{fs, path::Path, process::Command};
fn json(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn analyze(source: &Path, out: &Path, library: Option<&Path>, planned: bool) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_egg_layout"));
    cmd.args([
        "analyze",
        "--source",
        source.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
        "--save-history",
        "--ripen-observe-ms",
        "60000",
        "--ripen-observe-facts",
        "80",
    ]);
    if let Some(library) = library {
        cmd.args(["--reuse-library", library.to_str().unwrap()]);
    }
    if planned {
        cmd.arg("--reuse-plan");
    }
    let result = cmd.env("EGG_LAYOUT_GRAPHS", "0").output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
fn planned_singlepass_retains_source_layers_and_native_reentry() {
    let root = std::env::temp_dir().join(format!("singlepass-reuse-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let seed = root.join("seed");
    analyze(
        &repo.join("experiments/reuse_feedback/seed.egg"),
        &seed,
        None,
        false,
    );
    let library = root.join("library.json");
    egg_layout::reuse_graph::library::bake(&[seed.to_string_lossy().into_owned()], &library)
        .unwrap();
    let nested = root.join("nested.egg");
    fs::write(
        &nested,
        fs::read_to_string(repo.join("experiments/reuse_feedback/graft.egg"))
            .unwrap()
            .replace("(Flag x)", "(Flag (V n))"),
    )
    .unwrap();
    for (name, file) in [
        ("nested", nested.to_str().unwrap()),
        ("graft", "experiments/reuse_feedback/graft.egg"),
        ("dabc", "experiments/reentry_forms/dabc.egg"),
        ("branch", "experiments/reentry_forms/branching.egg"),
    ] {
        let normal = root.join(format!("{name}-native"));
        let planned = root.join(format!("{name}-planned"));
        analyze(&repo.join(file), &normal, Some(&library), false);
        analyze(&repo.join(file), &planned, Some(&library), true);
        let a = json(normal.join("analysis.json"));
        let b = json(planned.join("analysis.json"));
        assert_eq!(a["summary"], b["summary"]);
        let la = json(normal.join("layers.json"));
        let lb = json(planned.join("layers.json"));
        for key in ["combs", "coarse_layers", "smooth_layers"] {
            assert_eq!(la[key], lb[key], "{name}: {key}");
        }
        assert_eq!(
            a["saturated_rule_composition"]["counts"],
            b["saturated_rule_composition"]["counts"]
        );
        assert_eq!(
            a["saturated_rule_composition"]["reuse_learning"]["current"],
            b["saturated_rule_composition"]["reuse_learning"]["current"]
        );
        assert!(
            !fs::read_to_string(planned.join("history.json"))
                .unwrap()
                .contains("reuse_views"),
            "summary internals must not leak into source history"
        );
        if name == "graft" {
            assert_eq!(b["reuse_plan"]["views"], 1);
            assert_eq!(b["reuse_plan"]["grafts"]["reuse_count"], 2);
            let loaded = egg_layout::reuse_graph::library::Library::load(
                &planned.join("reuse-library.json"),
            )
            .unwrap();
            assert!(!loaded.observed_grafts().is_empty());
        } else if name == "nested" {
            assert_eq!(b["reuse_plan"]["views"], 0);
            assert!(
                b["reuse_plan"]["fallback_rules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["reason"].as_str().unwrap().contains("source witness"))
            );
        } else {
            assert_eq!(
                b["saturated_rule_composition"]["counts"]["ObservedUnfinished"],
                1
            );
        }
    }
    // CLI feedback is now a single source execution. A fresh concrete binding
    // must reuse the descriptor and prediction rather than rebaking an instance.
    let mut previous = library.clone();
    let mut first_counts = Value::Null;
    for (round, values) in [(0, (1, 2)), (1, (11, 12)), (2, (11, 12))] {
        let source = root.join(format!("cycle-{round}.egg"));
        let text = fs::read_to_string(repo.join("experiments/reuse_feedback/graft.egg"))
            .unwrap()
            .replace("(V 1)", &format!("(V {})", values.0))
            .replace("(V 2)", &format!("(V {})", values.1));
        fs::write(&source, text).unwrap();
        let out = root.join(format!("cycle-{round}"));
        let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .args([
                "reuse-cycle",
                source.to_str().unwrap(),
                previous.to_str().unwrap(),
                out.to_str().unwrap(),
            ])
            .env("EGG_LAYOUT_GRAPHS", "0")
            .env("EGG_LAYOUT_RIPEN_OBSERVE_MS", "60000")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report = json(out.join("reuse-cycle.json"));
        assert_eq!(report["source_executions"], 1);
        assert_eq!(report["execution"]["grafts"]["reuse_count"], 2);
        assert_eq!(report["execution"]["grafts"]["incomplete_witnesses"], 0);
        assert_eq!(
            report["execution"]["grafts"]["unexpected_break_count"],
            usize::from(round == 0)
        );
        if round == 0 {
            assert!(report["learning"]["new_enclosure_count"].as_u64().unwrap() > 0);
            first_counts = report["learning"]["current"].clone();
        } else {
            assert_eq!(report["learning"]["new_enclosure_count"], 0);
            assert_eq!(report["learning"]["current"], first_counts);
        }
        previous = out.join("reuse-library.json");
        assert_eq!(report["library"], previous.to_str().unwrap());
        assert!(!out.join("adaptive-library.json").exists());
    }

    // Subsumption is outside the positive planner subset: retain native source
    // execution and ripen, with an explicit fallback instead of a stale view.
    let source = root.join("subsume.egg");
    fs::write(
        &source,
        fs::read_to_string(repo.join("experiments/reuse_feedback/graft.egg")).unwrap()
            + "\n(subsume (A (V 1)))\n(run 1)\n",
    )
    .unwrap();
    let native = root.join("subsume-native");
    let planned = root.join("subsume-planned");
    analyze(&source, &native, Some(&library), false);
    analyze(&source, &planned, Some(&library), true);
    let a = json(native.join("analysis.json"));
    let b = json(planned.join("analysis.json"));
    assert_eq!(b["reuse_plan"]["status"], "native_fallback");
    assert_eq!(a["summary"], b["summary"]);
    assert_eq!(
        json(native.join("history.json")),
        json(planned.join("history.json"))
    );
    fs::remove_dir_all(root).unwrap();
}
