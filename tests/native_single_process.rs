use std::{path::PathBuf, process::Command};
fn folder(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("egg-native-{tag}-{}", std::process::id()))
}
#[test]
fn native_capture_works_without_python_or_helper_executables() {
    let out = folder("process");
    let _ = std::fs::remove_dir_all(&out);
    let result = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("PATH", "/nonexistent")
        .env("PYTHON", "/nonexistent/python")
        .args([
            "analyze",
            "--recapture-tier0",
            "--source",
            "tests/fixtures/cli_math_run11.egg",
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
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("analysis.json")).unwrap()).unwrap();
    assert_eq!(report["summary"]["executed_rounds"], 11);
    assert_eq!(
        report["summary"]["tier1_backend"],
        "rust-coarse-smooth-layers"
    );
    assert_eq!(
        report["layers"]["occurrences"].as_array().unwrap().len(),
        report["summary"]["imported_events"].as_u64().unwrap() as usize
    );
    assert!(out.join("layers.json").exists());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("[perf-tier1]"));
    assert_eq!(report["summary"]["subprocesses"], 0);
    assert_eq!(report["summary"]["events"], 2);
    assert_eq!(report["view"]["stats"]["total_comb_templates"], 3);
    for forbidden in [
        "profile.json",
        "history.json",
        "manifest.json",
        "imported.egg",
        "native.json",
        "interfaces.egg",
    ] {
        assert!(!out.join(forbidden).exists());
    }
    let reuse = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("PATH", "/nonexistent")
        .args(["analyze", "--reuse-tier0", "--output"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(reuse.status.success());
    std::fs::remove_dir_all(out).unwrap();
}
#[test]
fn native_override_rejects_ambiguous_schedules() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = folder("invalid");
    let _ = std::fs::remove_dir_all(&out);
    let source = folder("input.egg");
    std::fs::write(&source, "(datatype Math (Var String)) (run 1) (run 2)").unwrap();
    assert!(egg_layout::native_analyze::run(&root, &source, Some(3), &out).is_err());
    let status: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("run.json")).unwrap()).unwrap();
    assert_eq!(status["status"], "failed");
    std::fs::remove_file(source).unwrap();
    std::fs::remove_dir_all(out).unwrap();
}
#[test]
#[ignore = "release end-to-end Math regression"]
fn math_six_rounds_keep_the_witnessed_higher_rules() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = folder("math");
    let _ = std::fs::remove_dir_all(&out);
    let report = egg_layout::native_analyze::run(
        &root,
        &root.join("egglog/tests/math-microbenchmark.egg"),
        Some(6),
        &out,
    )
    .unwrap();
    assert_eq!(report["summary"]["imported_events"], 1529);
    assert_eq!(report["summary"]["higher_rules"], 13);
    assert_eq!(report["view"]["stats"]["maximal_chains"], 4);
    assert_eq!(report["view"]["stats"]["applications"], 12);
    assert_eq!(report["view"]["stats"]["total_comb_templates"], 1389);
    for n in report["view"]["nodes"].as_object().unwrap().values() {
        assert!(n["reason"].is_null(), "{}", n["reason"]);
        assert!(n["combined"].as_str().unwrap().contains("(rule"));
    }
    std::fs::remove_dir_all(out).unwrap();
}

#[test]
#[ignore = "independent release source replay for selected Math combinations"]
fn selected_effects_follow_from_original_source_rules() {
    use egglog::{
        EGraph,
        ast::{Action, Command, Expr, Fact, Literal, Span},
    };
    use std::collections::{BTreeMap, BTreeSet};
    fn vars(e: &Expr, out: &mut BTreeSet<String>) {
        match e {
            Expr::Var(_, n) => {
                out.insert(n.clone());
            }
            Expr::Call(_, _, args) => {
                for a in args {
                    vars(a, out)
                }
            }
            _ => {}
        }
    }
    fn expand(e: &Expr, env: &BTreeMap<String, Expr>) -> Expr {
        match e {
            Expr::Var(_, n) => env.get(n).cloned().unwrap_or_else(|| e.clone()),
            Expr::Call(s, o, a) => Expr::Call(
                s.clone(),
                o.clone(),
                a.iter().map(|a| expand(a, env)).collect(),
            ),
            _ => e.clone(),
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = folder("replay");
    let _ = std::fs::remove_dir_all(&out);
    let report = egg_layout::native_analyze::run(
        &root,
        &root.join("egglog/tests/math-microbenchmark.egg"),
        Some(6),
        &out,
    )
    .unwrap();
    let mut parser = EGraph::default();
    let mut setup = vec![];
    let mut n = 0;
    for c in parser
        .parse_program(
            None,
            &std::fs::read_to_string(root.join("egglog/tests/math-microbenchmark.egg")).unwrap(),
        )
        .unwrap()
    {
        match egg_layout::visual_rule::normalize(c, n) {
            Command::Datatype {
                span,
                name,
                variants,
            } => setup.push(Command::Datatype {
                span,
                name,
                variants,
            }),
            Command::Rule { mut rule } => {
                if rule.name.is_empty() {
                    rule.name = format!("R{n}");
                }
                n += 1;
                rule.ruleset = format!("src-{}", rule.name);
                setup.extend(
                    parser
                        .parse_program(None, &format!("(ruleset {})", rule.ruleset))
                        .unwrap(),
                );
                setup.push(Command::Rule { rule });
            }
            _ => {}
        }
    }
    for node in report["view"]["nodes"].as_object().unwrap().values() {
        let code = node["combined"].as_str().unwrap();
        let Command::Rule { rule } = parser.parse_program(None, code).unwrap().remove(0) else {
            panic!()
        };
        let mut eg = EGraph::default();
        eg.run_program(setup.clone()).unwrap();
        eg.parse_and_run_program(None, code).unwrap();
        let mut names = BTreeSet::new();
        for f in &rule.body {
            if let Fact::Eq(_, a, b) = f {
                vars(a, &mut names);
                vars(b, &mut names);
            }
        }
        let mut seed = vec![];
        let mut env = BTreeMap::new();
        for name in names {
            env.insert(name.clone(), Expr::Var(Span::Panic, name.clone()));
            seed.push(Command::Action(Action::Let(
                Span::Panic,
                name.clone(),
                Expr::Call(
                    Span::Panic,
                    "Var".into(),
                    vec![Expr::Lit(Span::Panic, Literal::String(name))],
                ),
            )));
        }
        for f in &rule.body {
            if let Fact::Eq(s, a, b) = f {
                seed.push(Command::Action(Action::Union(
                    s.clone(),
                    a.clone(),
                    b.clone(),
                )));
            }
        }
        eg.run_program(seed).unwrap();
        for step in node["source_steps"].as_array().unwrap() {
            eg.parse_and_run_program(None, &format!("(run src-{} 1)", step.as_str().unwrap()))
                .unwrap();
        }
        for action in &rule.head.0 {
            match action {
                Action::Let(s, name, e) => {
                    let e = expand(e, &env);
                    eg.run_program(vec![Command::Check(
                        s.clone(),
                        vec![Fact::Eq(
                            s.clone(),
                            Expr::Var(s.clone(), "present".into()),
                            e.clone(),
                        )],
                    )])
                    .unwrap();
                    env.insert(name.clone(), e);
                }
                Action::Union(s, a, b) => {
                    eg.run_program(vec![Command::Check(
                        s.clone(),
                        vec![Fact::Eq(s.clone(), expand(a, &env), expand(b, &env))],
                    )])
                    .unwrap();
                }
                _ => panic!(),
            }
        }
    }
    std::fs::remove_dir_all(out).unwrap();
}
