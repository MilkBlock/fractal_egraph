use egg_layout::visual_rule::surface_program;
use egglog::EGraph;
#[test]
fn surface_names_preserve_wildcard_independence_and_avoid_globals() {
    let source = r#"
(datatype E (Num i64) (Pair E E))
(relation Matched (E))
(let __egg_internal_0 (Num 99))
(rule ((= p (Pair (Num _) (Num _)))) ((Matched p)))
(Pair (Num 1) (Num 2))
(run 1)
(check (Matched (Pair (Num 1) (Num 2))))
(check (= __egg_internal_0 (Num 99)))
"#;
    let mut eg = EGraph::default();
    let commands = surface_program(eg.parse_program(None, source).unwrap());
    let text = commands
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains("@_"));
    assert!(text.contains("__egg_internal_1"));
    assert!(text.contains("__egg_internal_2"));
    eg.run_program(commands).unwrap();
    let mut fresh = EGraph::default();
    fresh.parse_and_run_program(None, &text).unwrap();
}
#[test]
fn repeated_internal_binding_stays_equal_and_strings_are_untouched() {
    use egglog::ast::{Action, Command, Expr};
    let mut eg = EGraph::default();
    let mut cmds = eg
        .parse_program(
            None,
            r#"(datatype E (S String)) (let x (S "@_")) (check (= x x))"#,
        )
        .unwrap();
    // Model an internally generated name that is used more than once.
    cmds = cmds
        .into_iter()
        .map(|c| {
            c.map_symbols(&mut |h| h, &mut |v: String| {
                if v == "x" { "@same".into() } else { v }
            })
        })
        .collect();
    let cmds = surface_program(cmds);
    let Command::Action(Action::Let(_, name, Expr::Call(_, _, args))) = &cmds[1] else {
        panic!()
    };
    assert_eq!(name, "__egg_internal_0");
    assert_eq!(args[0].to_string(), "\"@_\"");
    let text = cmds
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    EGraph::default()
        .parse_and_run_program(None, &text)
        .unwrap();
}

#[test]
fn wildcard_rule_survives_capture_ripen_and_history_replay() {
    use serde_json::Value;
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!("wildcard-pipeline-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let source = root.join("input.egg");
    let mut text="(datatype E (V String) (A E) (B E) (C E) (N i64))\n(relation Seen (E))\n(rewrite (A x) (B x))\n(rewrite (B x) (C x))\n(rule ((= n (N _))) ((Seen n)))\n".to_string();
    for i in 0..8 {
        text += &format!("(A (V \"v{i}\"))\n");
    }
    text += "(run 4)";
    fs::write(&source, text).unwrap();
    let out = root.join("run");
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--recapture-tier0", "--save-history", "--source"])
        .arg(&source)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let q: Value =
        serde_json::from_slice(&fs::read(out.join("closed/queue.json")).unwrap()).unwrap();
    assert!(q["counts"]["Closed"].as_u64().unwrap_or(0) > 0, "{q}");
    for cell in fs::read_dir(out.join("closed/cells")).unwrap() {
        assert!(
            !fs::read_to_string(cell.unwrap().path().join("entry.egg"))
                .unwrap()
                .contains("@_")
        );
    }
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--replay-history"])
        .arg(out.join("history.json"))
        .arg("--output")
        .arg(root.join("replay"))
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    fs::remove_dir_all(root).unwrap();
}
