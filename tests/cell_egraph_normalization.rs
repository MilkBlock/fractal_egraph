//! Native ripen certificates feed a separate native Cell egraph.
//! This is a finite, fixed-interface normalization experiment, not runtime packing.
use egg_layout::{
    native_analyze::ripen,
    saturated_rule_composition::{self as state, Comparison},
};
use egglog::EGraph;
use std::path::PathBuf;

fn exec(eg: &mut EGraph, text: &str) {
    let cmds = eg.parse_program(None, text).unwrap();
    eg.run_program(cmds).unwrap();
}
fn equivalent(eg: &mut EGraph, a: &str, b: &str) -> bool {
    let cmds = eg
        .parse_program(None, &format!("(check (= {a} {b}))"))
        .unwrap();
    eg.run_program(cmds).is_ok()
}
fn temp() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "cell-normalization-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn ripen_equivalence_rebuilds_existing_successors_without_ripening_them() {
    let dir = temp();
    let environment = r#"
(datatype Math (Input i64) (A Math) (B Math))
(rewrite (A x) (B x) :name "a-to-b")
(let input (Input 0))
(let root (A input))
"#;
    // R1 injects Input; R2 injects A. R3 injects Input, A and B together.
    // The paths differ before normalization; neither certificate is fabricated.
    for (name, actions) in [("left", ""), ("right", "(B input)")] {
        let source = dir.join(format!("{name}.egg"));
        std::fs::write(&source, format!("{environment}\n{actions}")).unwrap();
        ripen::run(&source, &dir.join(name)).unwrap();
    }
    let initial_left = state::read(&dir.join("left/initial-state.json")).unwrap();
    let initial_right = state::read(&dir.join("right/initial-state.json")).unwrap();
    assert!(!matches!(
        state::compare(&initial_left, &initial_right).unwrap(),
        Comparison::Equivalent { .. }
    ));
    let left = state::read(&dir.join("left/saturated-rule-composition.json")).unwrap();
    let right = state::read(&dir.join("right/saturated-rule-composition.json")).unwrap();
    assert!(matches!(
        state::compare(&left, &right).unwrap(),
        Comparison::Equivalent { .. }
    ));
    // Different fixed port mapping is not an interchangeable interface.
    let mut wrong_interface = right.clone();
    let input = wrong_interface.ports["input"];
    let root = wrong_interface.ports["root"];
    wrong_interface.ports.insert("input".into(), root);
    wrong_interface.ports.insert("root".into(), input);
    assert!(!matches!(
        state::compare(&left, &wrong_interface).unwrap(),
        Comparison::Equivalent { .. }
    ));
    let mut wrong_environment = right.clone();
    wrong_environment.scope["test_environment"] = serde_json::json!("different");
    assert!(matches!(
        state::compare(&left, &wrong_environment).unwrap(),
        Comparison::IncompatibleScope
    ));

    let mut eg = EGraph::default();
    exec(&mut eg, include_str!("../experiments/cell_egraph/ir.egg"));
    exec(
        &mut eg,
        r#"
(let beta (Bind (Port 0 (PNil)) (PNil)))
(let a (CoarseApply (Empty) "R1" beta))
(let b (CoarseApply a "R2" beta))
(let c (CoarseApply (Empty) "R3" beta))
(let next-b (CoarseApply b "R4" beta))
(let next-c (CoarseApply c "R4" beta))
(let grand-b (CoarseApply next-b "R5" beta))
(let grand-c (CoarseApply next-c "R5" beta))
(let other-port (CoarseApply c "R4" (Bind (Port 1 (PNil)) (PNil))))
(let other-obligation (CoarseApply c "R4" (Bind (Port 0 (PNil)) (Port 7 (PNil)))))
(let other-rule (CoarseApply c "R6" beta))
"#,
    );
    assert!(!equivalent(&mut eg, "b", "c"));
    assert!(!equivalent(&mut eg, "next-b", "next-c"));
    // Sole semantic union: admitted only by the exact native ripen comparison.
    if matches!(
        state::compare(&left, &right).unwrap(),
        Comparison::Equivalent { .. }
    ) {
        exec(&mut eg, "(union b c)");
    } else {
        panic!("no closure certificate");
    }
    assert!(equivalent(&mut eg, "b", "c"));
    assert!(equivalent(&mut eg, "next-b", "next-c"));
    assert!(equivalent(&mut eg, "grand-b", "grand-c"));
    for distinct in ["other-port", "other-obligation", "other-rule"] {
        assert!(
            !equivalent(&mut eg, "next-b", distinct),
            "incorrect merge: {distinct}"
        );
    }
    println!(
        "native ripens=2; certified parent unions=1; congruent successor pairs=2; successor ripens=0; negative controls=5"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
