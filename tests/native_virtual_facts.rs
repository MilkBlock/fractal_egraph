use egglog::{EGraph, TraceSession, Value, VirtualFactRow, WriteOutcome};
fn value(eg: &mut EGraph, term: &str) -> Value {
    let e = eg
        .parse_program(None, &format!("(let temporary_unused {term})"))
        .unwrap();
    let egglog::ast::Command::Action(egglog::ast::Action::Let(_, _, expr)) = &e[0] else {
        unreachable!()
    };
    eg.eval_expr(expr).unwrap().1
}
fn source() -> &'static str {
    "(datatype E (V i64) (A E) (B E)) (relation Left (E)) (relation Right (E)) (relation Joined (E)) (rule ((Left x) (Right x)) ((Joined x))) (B (A (V 1))) (B (A (V 2))) (Left (A (V 1))) (Right (A (V 2)))"
}
fn pack(eg: &mut EGraph) -> egglog::VirtualizeReport {
    let template = eg
        .virtual_fact_template(vec![
            VirtualFactRow {
                function: "A".into(),
                columns: vec![0, 1],
            },
            VirtualFactRow {
                function: "B".into(),
                columns: vec![1, 2],
            },
        ])
        .unwrap();
    let bindings: Vec<_> = (1..=2)
        .map(|i| {
            vec![
                value(eg, &format!("(V {i})")),
                value(eg, &format!("(A (V {i}))")),
                value(eg, &format!("(B (A (V {i})))")),
            ]
        })
        .collect();
    eg.virtualize_existing_facts(
        &template,
        &[
            bindings[0].clone(),
            bindings[1].clone(),
            bindings[0].clone(),
        ],
    )
    .unwrap()
}
#[test]
fn overlaps_keep_one_hashcons_identity_and_reads_do_not_materialize() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, source()).unwrap();
    let before = value(&mut eg, "(A (V 1))");
    let report = pack(&mut eg);
    assert_eq!(report.requested_rows, 6);
    assert_eq!(report.distinct_native_rows, 4);
    assert_eq!(value(&mut eg, "(A (V 1))"), before);
    assert_eq!(eg.get_size("A"), 2);
    let q = eg
        .compile_fact_query(
            &[("E".into(), None), ("E".into(), None)],
            &[("A".into(), vec![0, 1])],
            &[0, 1],
        )
        .unwrap();
    assert_eq!(eg.run_fact_query(&q).unwrap().len(), 2);
    eg.free_fact_query(q);
    for (_, s) in eg.virtual_fact_stats() {
        assert_eq!(s.virtual_rows, 2);
        assert_eq!(s.overlay_rows, 0);
    }
    eg.parse_and_run_program(None, "(B (A (V 3)))").unwrap();
    assert_eq!(eg.get_size("A"), 3);
    for (_, s) in eg.virtual_fact_stats() {
        assert_eq!(s.virtual_rows, 2);
        assert_eq!(s.overlay_rows, 1);
    }
}
#[test]
fn displaced_boundary_rebuilds_colliding_virtual_keys_and_enables_join() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, source()).unwrap();
    let mut reference = eg.clone();
    pack(&mut eg);
    let updates = "(fail (check (Joined (A (V 1))))) (union (V 1) (V 2)) (run 1) (check (= (A (V 1)) (A (V 2)))) (check (= (B (A (V 1))) (B (A (V 2))))) (check (Joined (A (V 1))))";
    eg.parse_and_run_program(None, updates).unwrap();
    reference.parse_and_run_program(None, updates).unwrap();
    for table in ["V", "A", "B", "Left", "Right", "Joined"] {
        assert_eq!(eg.get_size(table), reference.get_size(table), "{table}");
    }
}
#[test]
fn virtual_function_merge_subsume_delete_and_snapshot_are_native() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,"(datatype E (A) (B)) (function score (E) i64 :merge (max old new)) (set (score (A)) 1) (set (score (B)) 2)").unwrap();
    let template = eg
        .virtual_fact_template(vec![VirtualFactRow {
            function: "score".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let a = value(&mut eg, "(A)");
    let b = value(&mut eg, "(B)");
    let one = value(&mut eg, "1");
    let two = value(&mut eg, "2");
    eg.virtualize_existing_facts(&template, &[vec![a, one], vec![b, two]])
        .unwrap();
    let mut branch = eg.fork_shared();
    branch
        .parse_and_run_program(None, "(set (score (A)) 9) (check (= (score (A)) 9))")
        .unwrap();
    eg.parse_and_run_program(None, "(check (= (score (A)) 1))")
        .unwrap();
    branch.parse_and_run_program(None,"(union (A) (B)) (check (= (score (B)) 9)) (delete (score (A))) (fail (check (= (score (A)) 9)))").unwrap();
    eg.parse_and_run_program(None, "(check (= (score (A)) 1)) (check (= (score (B)) 2))")
        .unwrap();
    let mut e = EGraph::default();
    e.parse_and_run_program(None, source()).unwrap();
    pack(&mut e);
    e.parse_and_run_program(None, "(subsume (A (V 1)))")
        .unwrap();
    let q = e
        .compile_fact_query(
            &[("E".into(), None), ("E".into(), None)],
            &[("A".into(), vec![0, 1])],
            &[1],
        )
        .unwrap();
    assert_eq!(e.run_fact_query(&q).unwrap().len(), 1);
    e.free_fact_query(q);
}
#[test]
fn invalid_binding_cannot_create_or_partly_virtualize_facts() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, source()).unwrap();
    let t = eg
        .virtual_fact_template(vec![VirtualFactRow {
            function: "A".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let x = value(&mut eg, "(V 1)");
    let y = value(&mut eg, "(V 2)");
    let a = value(&mut eg, "(A (V 1))");
    assert!(
        eg.virtualize_existing_facts(&t, &[vec![x, a], vec![y, a]])
            .is_err()
    );
    assert!(eg.virtual_fact_stats().is_empty());
    assert_eq!(eg.get_size("A"), 2);
    assert!(
        eg.virtual_fact_template(vec![VirtualFactRow {
            function: "V".into(),
            columns: vec![0, 0]
        }])
        .is_err()
    );
}
#[test]
fn virtual_reads_retain_committed_producer_certificates() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,"(datatype E (V i64) (A E) (B E)) (relation Seed (E)) (ruleset make) (ruleset use) (rule ((Seed x)) ((A x)) :ruleset make :name \"make\") (rule ((= a (A x))) ((B a)) :ruleset use :name \"use\") (Seed (V 1))").unwrap();
    let trace = TraceSession::with_dependencies();
    let cmd = eg.parse_program(None, "(run make 1)").unwrap();
    eg.run_program_with_trace(cmd, &trace).unwrap();
    let produced = trace.drain_completed();
    let x = value(&mut eg, "(V 1)");
    let a = value(&mut eg, "(A (V 1))");
    let template = eg
        .virtual_fact_template(vec![VirtualFactRow {
            function: "A".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    eg.virtualize_existing_facts(&template, &[vec![x, a]])
        .unwrap();
    let cmd = eg.parse_program(None, "(run use 1)").unwrap();
    eg.run_program_with_trace(cmd, &trace).unwrap();
    let used = trace.drain_completed();
    let read = used
        .reads
        .iter()
        .find(|r| r.table_name.as_deref() == Some("A"))
        .unwrap();
    assert!(read.producer_write_event_id.is_some_and(|id| {
        produced
            .writes
            .iter()
            .any(|w| w.event_id == id && w.outcome == WriteOutcome::Inserted)
    }));
    assert!(
        used.matches
            .iter()
            .any(|m| m.rule.as_ref() == "use" && m.physical_witness_complete)
    );
}

#[test]
fn cached_joins_and_long_update_sequences_agree_with_explicit_storage() {
    use std::collections::BTreeSet;
    let mut e = EGraph::default();
    let mut source="(datatype E (V i64)) (function Score (E) i64 :merge (max old new)) (relation Above (E)) (rule ((= s (Score x)) (> s 50)) ((Above x)))".to_string();
    for i in 0..40 {
        source.push_str(&format!("(set (Score (V {i})) {i})"));
    }
    e.parse_and_run_program(None, &source).unwrap();
    let mut reference = e.clone();
    let q = e
        .compile_fact_query(
            &[("E".into(), None), ("i64".into(), None)],
            &[("Score".into(), vec![0, 1])],
            &[0, 1],
        )
        .unwrap();
    assert_eq!(e.run_fact_query(&q).unwrap().len(), 40);
    let t = e
        .virtual_fact_template(vec![VirtualFactRow {
            function: "Score".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let bindings = (0..40)
        .map(|i| {
            vec![
                value(&mut e, &format!("(V {i})")),
                value(&mut e, &i.to_string()),
            ]
        })
        .collect::<Vec<_>>();
    e.virtualize_existing_facts(&t, &bindings).unwrap();
    assert_eq!(e.run_fact_query(&q).unwrap().len(), 40);
    for i in 0..96 {
        let k = i % 40;
        let update = match i % 4 {
            0 => format!("(set (Score (V {k})) {}) (run 1)", 100 + i),
            1 => format!("(delete (Score (V {k})))"),
            2 => format!("(set (Score (V {k})) 1)"),
            _ => format!("(union (V {k}) (V {})) (run 1)", (k + 1) % 40),
        };
        e.parse_and_run_program(None, &update).unwrap();
        reference.parse_and_run_program(None, &update).unwrap();
        let mut expected = BTreeSet::new();
        reference
            .function_for_each("Score", |r| {
                expected.insert(r.vals.to_vec());
            })
            .unwrap();
        assert_eq!(
            e.run_fact_query(&q)
                .unwrap()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            expected,
            "step {i}: {update}"
        );
        let mut a = BTreeSet::new();
        let mut b = BTreeSet::new();
        e.function_for_each("Above", |r| {
            a.insert(r.vals[0]);
        })
        .unwrap();
        reference
            .function_for_each("Above", |r| {
                b.insert(r.vals[0]);
            })
            .unwrap();
        assert_eq!(a, b, "delta step {i}");
    }
    e.free_fact_query(q);
}
#[test]
fn nullary_keys_and_zero_new_rows_keep_native_identity() {
    let mut e = EGraph::default();
    e.parse_and_run_program(None, "(datatype E (A) (B)) (A) (B)")
        .unwrap();
    let a = value(&mut e, "(A)");
    let t = e
        .virtual_fact_template(vec![VirtualFactRow {
            function: "A".into(),
            columns: vec![0],
        }])
        .unwrap();
    e.virtualize_existing_facts(&t, &[vec![a]]).unwrap();
    e.parse_and_run_program(None, "(union (A) (B)) (check (= (A) (B)))")
        .unwrap();
    assert_eq!(e.get_size("A"), 1);
}

#[test]
fn displaced_notifications_leave_unrelated_instances_virtual() {
    let mut e = EGraph::default();
    let mut setup = "(datatype E (V i64) (A E))".to_string();
    for i in 0..64 {
        setup.push_str(&format!("(A (V {i}))"));
    }
    e.parse_and_run_program(None, &setup).unwrap();
    let t = e
        .virtual_fact_template(vec![VirtualFactRow {
            function: "A".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let bindings = (0..64)
        .map(|i| {
            vec![
                value(&mut e, &format!("(V {i})")),
                value(&mut e, &format!("(A (V {i}))")),
            ]
        })
        .collect::<Vec<_>>();
    e.virtualize_existing_facts(&t, &bindings).unwrap();
    e.parse_and_run_program(None, "(union (V 0) (V 1)) (check (= (A (V 0)) (A (V 1))))")
        .unwrap();
    let (_, s) = e
        .virtual_fact_stats()
        .into_iter()
        .find(|(n, _)| n == "A")
        .unwrap();
    assert!(s.virtual_rows >= 62);
    assert!(
        s.rebuild_rows_examined > 0 && s.rebuild_rows_examined < 8,
        "examined {}",
        s.rebuild_rows_examined
    );
    assert!(s.rebuild_rows_changed > 0);
}

#[test]
fn baked_library_matches_real_instances_before_packing_native_tables() {
    use std::{fs, path::Path, process::Command};
    let root = std::env::temp_dir().join(format!("native-virtual-bake-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments/reuse_feedback/seed.egg");
    let out = root.join("run");
    let run = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args([
            "analyze",
            "--source",
            source.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--ripen-observe-ms",
            "60000",
        ])
        .env("EGG_LAYOUT_GRAPHS", "0")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let path = root.join("library.json");
    egg_layout::reuse_graph::library::bake(&[out.to_string_lossy().into_owned()], &path).unwrap();
    let library = egg_layout::reuse_graph::library::Library::load(&path).unwrap();
    let mut e = EGraph::default();
    e.parse_and_run_program(
        None,
        "(datatype E (V i64) (A E) (B E)) (rewrite (A x) (B x)) (A (V 10)) (A (V 20)) (run 2)",
    )
    .unwrap();
    let report = egg_layout::reuse_graph::native_storage::pack_library(&mut e, &library).unwrap();
    assert!(
        report["admissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["matched_instances"] == 2),
        "{report}"
    );
    assert_eq!(
        e.virtual_fact_stats()
            .iter()
            .map(|(_, s)| s.virtual_rows)
            .sum::<usize>(),
        4
    );
    e.parse_and_run_program(
        None,
        "(union (V 10) (V 20)) (run 1) (check (= (A (V 10)) (B (V 20))))",
    )
    .unwrap();
    assert_eq!(e.get_size("A"), 1);
    assert_eq!(e.get_size("B"), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn container_keys_and_relation_outputs_rebuild_without_expanding_other_rows() {
    let mut e = EGraph::default();
    e.parse_and_run_program(None,"(datatype E (V i64)) (sort VE (Vec E)) (datatype W (Wrap VE)) (relation P (E)) (Wrap (vec-of (V 1))) (Wrap (vec-of (V 2))) (P (V 1)) (P (V 2))").unwrap();
    let mut reference = e.clone();
    let wrap = e
        .virtual_fact_template(vec![VirtualFactRow {
            function: "Wrap".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let wb = (1..=2)
        .map(|i| {
            vec![
                value(&mut e, &format!("(vec-of (V {i}))")),
                value(&mut e, &format!("(Wrap (vec-of (V {i})))")),
            ]
        })
        .collect::<Vec<_>>();
    e.virtualize_existing_facts(&wrap, &wb).unwrap();
    let p = e
        .virtual_fact_template(vec![VirtualFactRow {
            function: "P".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let pb = (1..=2)
        .map(|i| {
            vec![
                value(&mut e, &format!("(V {i})")),
                value(&mut e, &format!("(P (V {i}))")),
            ]
        })
        .collect::<Vec<_>>();
    e.virtualize_existing_facts(&p, &pb).unwrap();
    let update = "(union (V 1) (V 2)) (check (= (Wrap (vec-of (V 1))) (Wrap (vec-of (V 2))))) (check (P (V 1)))";
    e.parse_and_run_program(None, update).unwrap();
    reference.parse_and_run_program(None, update).unwrap();
    assert_eq!(e.get_size("Wrap"), reference.get_size("Wrap"));
    assert_eq!(e.get_size("P"), reference.get_size("P"));
}

#[test]
fn repeated_registration_does_not_accumulate_bindings_and_materialization_is_not_a_delta() {
    let mut e = EGraph::default();
    e.parse_and_run_program(None,"(datatype E (V i64) (A E)) (function Count (E) i64 :merge (+ old new)) (rule ((= a (A x))) ((set (Count x) 1))) (A (V 1)) (A (V 2)) (run 1)").unwrap();
    let t = e
        .virtual_fact_template(vec![VirtualFactRow {
            function: "A".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let bindings = (1..=2)
        .map(|i| {
            vec![
                value(&mut e, &format!("(V {i})")),
                value(&mut e, &format!("(A (V {i}))")),
            ]
        })
        .collect::<Vec<_>>();
    e.virtualize_existing_facts(&t, &bindings).unwrap();
    let before = e.virtual_fact_stats()[0].1.instances.clone();
    e.virtualize_existing_facts(&t, &bindings).unwrap();
    assert_eq!(e.virtual_fact_stats()[0].1.instances, before);
    let q = e
        .compile_fact_query(
            &[("E".into(), None), ("E".into(), None)],
            &[("A".into(), vec![0, 1])],
            &[0, 1],
        )
        .unwrap();
    assert_eq!(e.run_fact_query(&q).unwrap().len(), 2);
    e.parse_and_run_program(None, "(subsume (A (V 2)))")
        .unwrap();
    assert_eq!(e.materialize_virtual_facts(), 1);
    assert!(e.virtual_fact_stats().is_empty());
    assert_eq!(e.run_fact_query(&q).unwrap().len(), 1);
    e.free_fact_query(q);
    e.parse_and_run_program(
        None,
        "(run 1) (check (= (Count (V 1)) 1)) (check (= (Count (V 2)) 1))",
    )
    .unwrap();
}

#[test]
fn external_overlay_joins_virtual_body_and_writes_new_native_facts() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, source()).unwrap();
    let mut reference = eg.clone();
    pack(&mut eg);
    // Two virtual atoms plus an external relation inserted only AFTER packing.
    let program = "(relation External (E)) (relation Out (E)) (rule ((= a (A x)) (= b (B a)) (External x)) ((Out b))) (External (V 1)) (run 1) (check (Out (B (A (V 1))))) (fail (check (Out (B (A (V 2))))))";
    eg.parse_and_run_program(None, program).unwrap();
    reference.parse_and_run_program(None, program).unwrap();
    assert_eq!(eg.get_size("Out"), reference.get_size("Out"));
    assert_eq!(eg.get_size("Out"), 1);
    for (_, s) in eg.virtual_fact_stats() { assert_eq!(s.virtual_rows, 2); }
}

#[test]
fn cell_wave_validates_residuals_atomically_and_reuses_existing_owners() {
    let mut eg=EGraph::default();eg.parse_and_run_program(None,source()).unwrap();
    let a=eg.virtual_fact_template(vec![VirtualFactRow{function:"A".into(),columns:vec![0,1]}]).unwrap();
    let b=eg.virtual_fact_template(vec![VirtualFactRow{function:"B".into(),columns:vec![0,1]}]).unwrap();
    let x=value(&mut eg,"(V 1)");let av=value(&mut eg,"(A (V 1))");let bv=value(&mut eg,"(B (A (V 1)))");
    let ab=vec![vec![x,av]];let bad=vec![vec![av,x]];
    assert!(eg.virtualize_existing_facts_batch(&[(&a,&ab),(&b,&bad)]).is_err());
    assert!(eg.virtual_fact_stats().is_empty());
    let bb=vec![vec![av,bv]];
    let result=eg.virtualize_existing_facts_batch(&[(&a,&ab),(&b,&bb),(&a,&ab)]).unwrap();
    assert_eq!(result.requested_rows,3);assert_eq!(result.new_virtual_rows,2);assert_eq!(result.affected_tables,2);
    let result=eg.virtualize_unshared_facts_batch(&[(&a,&ab),(&b,&bb)]).unwrap();
    assert_eq!(result.shared_rows_reused,2);assert_eq!(result.new_virtual_rows,0);assert_eq!(result.affected_tables,0);
    // Strict API still rejects wrong payloads, even for shared rows.
    let bad_shared=vec![vec![x,bv]];
    assert!(eg.virtualize_existing_facts_batch(&[(&a,&bad_shared)]).is_err());
    eg.parse_and_run_program(None,"(union (V 1) (V 2)) (run 1) (check (= (B (A (V 1))) (B (A (V 2)))))").unwrap();
}
