use egg_layout::{
    ripen_convergence::{Fingerprint, Index},
    saturated_rule_composition::{Row, SaturatedRuleComposition as State, Vertex},
};
use serde_json::json;
use std::collections::BTreeMap;
fn cycle(split: bool) -> State {
    State {
        version: 2,
        local_ids: vec![],
        scope: json!({"rules":"same"}),
        values: vec![
            Vertex {
                sort: "T".into(),
                literal: None
            };
            6
        ],
        rows: (0..6)
            .map(|i| Row {
                op: "Link".into(),
                args: vec![i],
                result: if split {
                    (i / 3) * 3 + (i + 1) % 3
                } else {
                    (i + 1) % 6
                },
            })
            .collect(),
        subsumed_rows: vec![],
        ports: BTreeMap::new(),
    }
}
#[test]
fn collisions_do_not_share_and_ids_do_not_matter() {
    let a = cycle(false);
    let b = cycle(true);
    let mut idx = Index::new(10, 10000);
    idx.observe("a", 0, a.clone(), "contract", &mut Fingerprint::default());
    assert_eq!(
        idx.observe("b", 0, b, "contract", &mut Fingerprint::default())["status"],
        "collision_or_unknown"
    );
    let mut renamed = a.clone();
    for r in &mut renamed.rows {
        r.args[0] = 5 - r.args[0];
        r.result = 5 - r.result;
    }
    renamed.rows.reverse();
    assert_eq!(
        idx.observe("c", 0, renamed, "contract", &mut Fingerprint::default())["status"],
        "verified"
    );
    assert_eq!(
        idx.observe(
            "d",
            0,
            a.clone(),
            "other obligations",
            &mut Fingerprint::default()
        )["status"],
        "miss"
    );
    let mut hidden = a;
    hidden.subsumed_rows = vec![0];
    assert_ne!(
        idx.observe("e", 0, hidden, "contract", &mut Fingerprint::default())["status"],
        "verified"
    );
}
#[test]
fn native_intermediate_states_converge_before_saturation() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join(format!(
        "out/convergence-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sources = ["a.egg", "b.egg"].map(|p| root.join("experiments/ripen_convergence").join(p));
    let report = egg_layout::native_analyze::ripen::probe(&sources, &out, 8).unwrap();
    assert!(report["index"]["stats"]["verified_hits"].as_u64().unwrap() > 0);
    assert_eq!(report["opportunities"][0]["first_hit"]["round"], 0);
    assert!(
        report["opportunities"][0]["remaining_observed_rounds"]
            .as_u64()
            .unwrap()
            >= 2
    );
    for c in report["cells"].as_array().unwrap() {
        assert_eq!(c["checks"], "passed");
    }
    assert_eq!(report["cells"][1]["native_rounds"], 0);
    assert_eq!(
        report["cells"][1]["convergence"]["actual_rounds_skipped"],
        3
    );
    assert_eq!(report["cells"][1]["imported_applies"], 0);
    assert!(report["cells"][1]["tier1"]["shared_continuation"].is_object());
    let link = &report["cells"][1]["tier1"]["shared_continuation"];
    assert!(link.get("donor_tier1").is_none());
    let library: serde_json::Value =
        serde_json::from_slice(&std::fs::read(link["library"].as_str().unwrap()).unwrap()).unwrap();
    let evidence = &library["evidence"][link["evidence_id"].as_u64().unwrap() as usize];
    assert!(evidence["tier1"].is_object());
    assert!(
        library["nodes"][link["packet_suffix"].as_u64().unwrap() as usize]["len"]
            .as_u64()
            .unwrap()
            > 0
    );

    let baseline =
        egg_layout::native_analyze::ripen::probe_mode(&sources, &out.join("baseline"), 8, false)
            .unwrap();
    for i in 0..2 {
        assert_eq!(report["cells"][i]["tables"], baseline["cells"][i]["tables"]);
    }
    let mut replay = egglog::EGraph::default();
    replay
        .parse_and_run_program(
            None,
            &std::fs::read_to_string(out.join("cell-1/ripened.egg")).unwrap(),
        )
        .unwrap();
    let shared = egg_layout::saturated_rule_composition::read(
        &out.join("cell-1/saturated-rule-composition.json"),
    )
    .unwrap();
    let plain = egg_layout::saturated_rule_composition::read(
        &out.join("baseline/cell-1/saturated-rule-composition.json"),
    )
    .unwrap();
    assert!(matches!(
        egg_layout::saturated_rule_composition::compare(&shared, &plain, 10000).unwrap(),
        egg_layout::saturated_rule_composition::Comparison::Equivalent { .. }
    ));
}

#[test]
fn union_packet_deduplicates_and_preserves_ports() {
    use egg_layout::ripen_convergence::Packets;
    let mut s = cycle(false);
    s.ports.insert("left".into(), 0);
    s.ports.insert("right".into(), 1);
    let mut p = Packets::new(&s);
    p.union(0, 1);
    p.union(2, 3);
    p.union(1, 3);
    let snapshot = p.snapshot();
    let mut index = Index::new(16, 10000);
    index.observe_packets("first", 0, &p, "contract");
    assert_eq!(
        index.observe_packets("second", 0, &Packets::new(&snapshot), "contract")["status"],
        "verified"
    );
    let mut collision_index = Index::new(8, 10000);
    collision_index.observe_packets("cycle", 0, &Packets::new(&cycle(false)), "same");
    assert_eq!(
        collision_index.observe_packets("triangles", 0, &Packets::new(&cycle(true)), "same")["status"],
        "collision_or_unknown"
    );
    let before = p.changes;
    p.union(0, 3);
    p.ensure(Row {
        op: "Link".into(),
        args: vec![0],
        result: 1,
    });
    assert_eq!(before, p.changes);
    assert_eq!(snapshot.ports["left"], snapshot.ports["right"]);
}

#[test]
fn native_packets_match_export_and_subsume_falls_back() {
    use std::process::Command;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join(format!(
        "out/packet-audit-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_RIPEN_PACKET_AUDIT", "1")
        .args([
            "ripen-probe",
            out.to_str().unwrap(),
            "12",
            "experiments/ripen_convergence/math-right.egg",
            "experiments/ripen_convergence/math-left.egg",
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let d: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("convergence.json")).unwrap()).unwrap();
    for c in d["cells"].as_array().unwrap() {
        assert_eq!(c["convergence"]["snapshot_exports"], 1);
        assert_eq!(c["convergence"]["fallback_reasons"], json!([]));
    }
    let congruence = out.join("congruence.egg");
    std::fs::write(
        &congruence,
        r#"
(datatype T (V String) (F T) (Pair T T))
(rule ((= a (V "a")) (= b (V "b"))) ((union a b)))
(let root (Pair (F (V "a")) (F (V "b"))))
(check (= (F (V "a")) (F (V "b"))))
"#,
    )
    .unwrap();
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_RIPEN_PACKET_AUDIT", "1")
        .args([
            "ripen-probe",
            out.join("congruence").to_str().unwrap(),
            "8",
            congruence.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let source = out.join("visibility.egg");
    std::fs::write(&source,"(datatype T (A) (B))\n(rewrite (A) (B))\n(rule ((= x (A))) ((subsume (A))))\n(let root (A))\n").unwrap();
    let d = egg_layout::native_analyze::ripen::probe(&[source], &out.join("fallback"), 8).unwrap();
    assert!(
        !d["cells"][0]["convergence"]["fallback_reasons"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reused_engine_runs_current_checks() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join(format!(
        "out/reuse-negative-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&out).unwrap();
    for (name, bad) in [("a", false), ("b", true)] {
        let mut s =
            std::fs::read_to_string(root.join(format!("experiments/ripen_convergence/{name}.egg")))
                .unwrap()
                .replace(
                    "(datatype T (A) (B) (C) (D))",
                    "(datatype T (A) (B) (C) (D) (Z))",
                );
        if bad {
            s.push_str("(check (= root (Z)))\n");
        }
        std::fs::write(out.join(format!("{name}.egg")), s).unwrap();
    }
    assert!(
        egg_layout::native_analyze::ripen::probe(
            &[out.join("a.egg"), out.join("b.egg")],
            &out.join("run"),
            8
        )
        .is_err()
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("run/cell-1/ripen.json")).unwrap()).unwrap();
    assert_eq!(report["state"], "Failed");
}

#[test]
fn continuation_respects_remaining_budget() {
    use egg_layout::ripen_convergence::Packets;
    let s = cycle(false);
    let p = Packets::new(&s);
    let mut index = Index::new(8, 10000);
    index.observe_packets("donor", 1, &p, "contract");
    index.finish(
        "donor",
        &egglog::EGraph::default(),
        s,
        json!({"ripen":{"state":"Saturated","round":5}}),
    );
    let hit = index.observe_packets("current", 0, &p, "contract");
    assert!(index.continuation(&hit, 3).is_none());
    assert_eq!(index.continuation(&hit, 4).unwrap().remaining, 4);
}
