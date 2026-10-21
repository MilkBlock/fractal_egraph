use cell_assisted_egglog::table_live::Runtime;
use egglog::EGraph;
fn check(runtime: &Runtime, code: &str) {
    runtime
        .engine()
        .clone()
        .parse_and_run_program(None, code)
        .unwrap();
}
fn worker() -> Option<std::path::PathBuf> {
    std::env::var_os("CELL_MATRIX_WORKER").map(Into::into)
}
#[test]
fn domain_refutation_keeps_aliases_new_bridges_and_rehashed_buckets() {
    let Some(worker)=worker() else {return};
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None,r#"
      (datatype E (N i64))
      (relation Frame (E E)) (relation L (E E)) (relation R (E E))
      (relation Hit (E)) (ruleset s)
      (rule ((Frame x z) (L x y) (R y z)) ((Hit y)) :ruleset s)
      ; Another query supplies the existing R(_,z) projection. The negative
      ; filter deliberately does not create missing tuple indexes itself.
      (relation NeedR (E)) (relation Seen (E))
      (rule ((NeedR z) (R y z)) ((Seen y)) :ruleset s)
      (Frame (N 1) (N 2)) (L (N 1) (N 3)) (R (N 4) (N 2))
    "#).unwrap();
    let commands=eg.parse_program(None,"(run s 1)").unwrap();
    let mut r=Runtime::new_full(eg,&commands,Some(&worker),true,None).unwrap();
    if std::env::var("CELL_REFUTATION_DOMAINS").as_deref()==Ok("1") {
        assert!(r.report()["cells"]["domain_checks"].as_u64().unwrap()>0);
    }
    r.run(&commands).unwrap();
    check(&r,"(fail (check (Hit (N 3))))");
    r.input("(union (N 3) (N 4))").unwrap();
    r.run(&commands).unwrap();
    check(&r,"(check (Hit (N 3)))");
    r.input("(delete (R (N 4) (N 2))) (L (N 1) (N 5))").unwrap();
    let unrelated=(100..230).map(|i|format!("(L (N {i}) (N {})) (R (N {}) (N {}))",i+1,i+2,i+3)).collect::<String>();
    r.input(&unrelated).unwrap();
    r.run(&commands).unwrap();
    check(&r,"(fail (check (Hit (N 5))))");
    r.input("(R (N 5) (N 2))").unwrap();
    r.run(&commands).unwrap();
    check(&r,"(check (Hit (N 5)))");
}
#[test]
fn refutation_is_invalidated_by_insert_union_rebuild_and_reinsert() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, r#"
      (datatype E (N i64))
      (relation P (E)) (relation Q (E E)) (relation R (E E))
      (relation Hit (E E E)) (ruleset s)
      (rule ((P x) (Q x y) (R x z)) ((Hit x y z)) :ruleset s)
      (P (N 1)) (Q (N 1) (N 2))
    "#).unwrap();
    let commands = eg.parse_program(None, "(run s 1)").unwrap();
    // Audit checks complete incremental binding sets at every original Run.
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands).unwrap();
    check(&r, "(fail (check (Hit (N 1) (N 2) (N 3))))");
    r.input("(R (N 4) (N 3))").unwrap();
    r.run(&commands).unwrap();
    r.input("(union (N 1) (N 4))").unwrap();
    r.run(&commands).unwrap();
    check(&r, "(check (Hit (N 1) (N 2) (N 3)))");
    r.input("(delete (R (N 1) (N 3))) (Q (N 1) (N 5))").unwrap();
    r.run(&commands).unwrap();
    check(&r, "(fail (check (Hit (N 1) (N 5) (N 3))))");
    r.input("(R (N 1) (N 3))").unwrap();
    r.run(&commands).unwrap();
    check(&r, "(check (Hit (N 1) (N 5) (N 3)))");
}
#[test]
fn refutation_uses_exact_computed_integer_keys_not_interned_ids() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, r#"
      (relation P (i64)) (relation Q (i64 i64)) (relation R (i64 i64))
      (relation Hit (i64 i64 i64)) (ruleset s)
      (rule ((P x) (Q x y) (R (+ x 1) z)) ((Hit x y z)) :ruleset s)
      (P 4294967296) (Q 4294967296 17) (R 4294967297 18)
      (P -4294967296) (Q -4294967296 19)
    "#).unwrap();
    let commands = eg.parse_program(None, "(run s 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    assert!(r.report()["cells"]["refutation_checks"].as_u64().unwrap() > 0);
    r.run(&commands).unwrap();
    check(&r, "(check (Hit 4294967296 17 18))");
    r.input("(R -4294967295 20)").unwrap();
    r.run(&commands).unwrap();
    check(&r, "(check (Hit -4294967296 19 20))");
}
#[test]
fn backtracking_restores_bound_slots_after_failed_and_successful_siblings() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, r#"
      (relation P (i64)) (relation Q (i64 i64)) (relation R (i64 i64))
      (relation Hit (i64 i64 i64)) (ruleset s)
      (rule ((P x) (Q x y) (R (+ y 1) z) (> z x) (!= (% z y) 0))
            ((Hit x y z)) :ruleset s)
    "#).unwrap();
    for x in [-1, 0, 1, 3] {
        eg.parse_and_run_program(None, &format!("(P {x})")).unwrap();
        for y in [0, 1, 2, 3, i64::MAX] {
            eg.parse_and_run_program(None, &format!("(Q {x} {y})")).unwrap();
        }
    }
    for y in 0..5 {
        for z in -2..9 {
            eg.parse_and_run_program(None, &format!("(R {y} {z})")).unwrap();
        }
    }
    let commands = eg.parse_program(None, "(run s 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands).unwrap();
    check(&r, "(check (Hit 1 2 3)) (fail (check (Hit 1 2 4)))");
    r.input("(P 5) (Q 5 2) (R 3 11) (delete (R 3 3))").unwrap();
    r.run(&commands).unwrap();
    check(&r, "(check (Hit 5 2 11))");
    r.run(&commands).unwrap();
}
#[test]
fn computed_keys_and_late_aliases_use_the_complete_native_frontier() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype E (N i64)) (relation Input (E i64)) (relation At (E i64 E)) (relation Hit (E E))
      (ruleset s)
      (rule ((Input ctx i) (At ctx (+ i 1) out) (!= ctx out)) ((Hit ctx out)) :ruleset s)
      (Input (N 0) 8) (At (N 0) 9 (N 9)) (At (N 0) 10 (N 10))
      (Input (N 1) 9223372036854775807) (At (N 1) -9223372036854775808 (N 2))
    "#,
    )
    .unwrap();
    let commands = eg.parse_program(None, "(run s 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands).unwrap();
    check(
        &r,
        "(check (Hit (N 0) (N 9))) (fail (check (Hit (N 0) (N 10)))) (fail (check (Hit (N 1) (N 2))))",
    );
    r.input("(Input (N 0) 9) (union (N 0) (N 10))").unwrap();
    r.run(&commands).unwrap();
    check(&r, "(fail (check (Hit (N 0) (N 10))))");
}
#[test]
fn remainder_and_derived_guards_match_exact_signed_i64_semantics() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (relation Input (i64 i64)) (relation Result (i64 i64 i64)) (ruleset s)
      (rule ((Input a b) (= r (% a b)) (!= r 0)) ((Result a b r)) :ruleset s)
    "#,
    )
    .unwrap();
    for a in [i64::MIN, i64::MAX, -5, -4, -1, 0, 1, 4, 5] {
        for b in [i64::MIN, i64::MAX, -4, -1, 0, 1, 4] {
            eg.parse_and_run_program(None, &format!("(Input {a} {b})"))
                .unwrap();
        }
    }
    let commands = eg.parse_program(None, "(run s 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands).unwrap();
    check(
        &r,
        "(check (Result -5 4 -1)) (fail (check (Result -9223372036854775808 -1 0)))",
    );
}
#[test]
fn combined_run_globals_and_subsume_keep_one_frozen_boundary() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype E (N i64)) (let G (N 0))
      (relation P (E)) (relation Q (E)) (relation R (E)) (relation Global (E E))
      (function Count () i64 :merge (+ old new))
      (ruleset a) (ruleset b) (unstable-combined-ruleset ab a b)
      (rule () ((set (Count) 1)) :ruleset a)
      (rule ((P x)) ((Q x) (subsume (P x))) :ruleset a)
      (rule ((Q x)) ((R x) (Global x G)) :ruleset b)
      (P (N 1))
    "#,
    )
    .unwrap();
    let commands = eg.parse_program(None, "(run ab 1) (run a 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands[..1]).unwrap();
    check(
        &r,
        "(check (Q (N 1))) (fail (check (R (N 1)))) (check (= (Count) 1))",
    );
    r.run(&commands).unwrap();
    check(&r, "(check (Global (N 1) (N 0))) (check (= (Count) 1))");
    r.input("(union (N -1) (N 0)) (P (N 2))").unwrap();
    r.run(&commands).unwrap();
    r.run(&commands).unwrap();
    check(&r, "(check (Global (N 2) (N -1))) (check (= (Count) 1))");
}
#[test]
fn empty_graph_unit_query_fires_once() {
    let Some(worker) = worker() else { return };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,"(function Count () i64 :merge (+ old new)) (ruleset s) (rule () ((set (Count) 1)) :ruleset s)").unwrap();
    let commands = eg.parse_program(None, "(run s 1)").unwrap();
    let mut r = Runtime::new_full(eg, &commands, Some(&worker), true, None).unwrap();
    r.run(&commands).unwrap();
    r.run(&commands).unwrap();
    check(&r, "(check (= (Count) 1))");
}

#[test]
fn unrelated_tables_do_not_multiply_index_storage_and_query_shapes_stay_exact() {
    let Some(worker)=worker()else{return};
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None,"(ruleset s)").unwrap();
    for i in 0..24 {
        eg.parse_and_run_program(None,&format!("(relation P{i} (i64 i64)) (relation Q{i} (i64 i64)) (relation Out{i} (i64 i64)) (rule ((P{i} x y) (Q{i} y z)) ((Out{i} x z)) :ruleset s)")).unwrap();
        for x in 0..(i+2){eg.parse_and_run_program(None,&format!("(P{i} {x} {}) (Q{i} {} {})",x+1,x+1,x+2)).unwrap();}
    }
    eg.parse_and_run_program(None,"(relation Link (i64 i64)) (relation Reach (i64 i64)) (rule ((Link a b) (Link b c) (Link c d) (Link d e)) ((Reach a e)) :ruleset s) (Link 0 1) (Link 1 2) (Link 2 3) (Link 3 4)").unwrap();
    let commands=eg.parse_program(None,"(run s 1)").unwrap();
    let mut r=Runtime::new_full(eg,&commands,Some(&worker),true,None).unwrap();
    r.run(&commands).unwrap();check(&r,"(check (Reach 0 4)) (check (Out23 24 26))");
    // Rehash one small table, reuse a deleted row slot, and wake a delayed query.
    r.input("(delete (P0 0 1)) (P0 4 5) (Q0 5 6) (Link 4 5)").unwrap();r.run(&commands).unwrap();
    check(&r,"(check (Out0 4 6)) (check (Reach 1 5))");
    let report=r.report();let d=&report["device"];
    assert_eq!(d["sparse_indexes"],true);assert_eq!(d["compact_bindings"],true);
    assert!(d["query_shapes"].as_u64().unwrap()>1);
    let dense=d["events"].as_u64().unwrap()*d["index_count"].as_u64().unwrap();
    assert!(d["index_link_words"].as_u64().unwrap()*8<dense,"{d}");
}

#[test]
fn shared_queries_preserve_wiring_predicates_and_independent_watermarks() {
    let Some(worker)=worker()else{return};
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None,r#"
      (datatype E (N i64)) (relation P (E E))
      (function A (E E) i64 :merge (+ old new))
      (function B (E E) i64 :merge (+ old new))
      (relation Same (E)) (relation Different (E E))
      (ruleset a) (ruleset b) (unstable-combined-ruleset ab a b)
      (rule ((P x y)) ((set (A x y) 1)) :ruleset a :name "left")
      (rule ((P v u)) ((set (B u v) 1)) :ruleset b :name "right")
      (rule ((P z z)) ((Same z)) :ruleset b :name "alias")
      (rule ((P x y) (!= x y)) ((Different x y)) :ruleset b :name "neq")
      (P (N 0) (N 1))
    "#).unwrap();
    let commands=eg.parse_program(None,"(run a 1) (run ab 1)").unwrap();
    let mut r=Runtime::new_full(eg,&commands,Some(&worker),true,None).unwrap();
    r.set_query_sharing(true);
    r.run(&commands).unwrap();
    check(&r,"(check (= (A (N 0) (N 1)) 1)) (check (= (B (N 1) (N 0)) 1)) (fail (check (Same (N 0))))");
    r.input("(P (N 2) (N 3))").unwrap();r.run(&commands[1..]).unwrap();
    check(&r,"(check (= (A (N 2) (N 3)) 1)) (check (= (B (N 3) (N 2)) 1)) (fail (check (Same (N 2))))");
    r.input("(union (N 2) (N 3))").unwrap();r.run(&commands[1..]).unwrap();check(&r,"(check (Same (N 2)))");
    assert!(r.report()["runtime"]["shared_query_bindings"].as_u64().unwrap()>0);
    let report=r.report();
    if report["query_timing_profile"]==true {
        let s=&report["runtime"];
        let delta=s["device_seconds"].as_f64().unwrap()-s["gpu_matching_seconds"].as_f64().unwrap()-s["gpu_maintenance_seconds"].as_f64().unwrap();
        assert!(delta.abs()<1e-6,"stage timers do not add up: {s}");
    }
}

#[test]
fn complete_cell_connections_refute_and_refresh_without_claiming_query_frontiers() {
    let Some(worker)=worker() else{return};
    if std::env::var("CELL_BINDING_CONNECTIONS").as_deref()!=Ok("1"){return}
    use cell_assisted_egglog::{cell_connection, table_live::SaturationHook};
    struct Hook(cell_connection::Library);
    impl SaturationHook for Hook {
        fn binding_connections(&self)->cell_connection::Library {self.0.clone()}
        fn try_boundary(&mut self,_:&mut EGraph,_:&egglog::ast::Schedule)->cell_assisted_egglog::Result<Option<serde_json::Value>>{Ok(None)}
        fn take_commits(&mut self,_:&mut EGraph)->cell_assisted_egglog::Result<(Vec<String>,Vec<egglog::cell_events::CellCommitBatch>)>{Ok((vec![],vec![]))}
    }
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None,r#"
      (datatype E (N i64) (Root E i64))
      (function Cost (E) i64 :merge (min old new))
      (relation Fan (E i64)) (relation Hit (E i64)) (relation CellMark (E))
      (ruleset bake) (ruleset s)
      (rule ((= r (Root x limit)) (= cost (Cost x))) ((CellMark r)) :ruleset bake :name "cell-port")
      (rule ((= r (Root x limit)) (= cost (Cost x)) (Fan x y) (> cost limit)) ((Hit r y)) :ruleset s :name "consumer")
      (Root (N 0) 10) (Root (N 1) 10) (Root (N 2) 10)
      (set (Cost (N 0)) 5) (set (Cost (N 1)) 20)
      (Fan (N 0) 1) (Fan (N 1) 2) (Fan (N 2) 3)
    "#).unwrap();
    let q=eg.prepare_gpu_cell_action("bake","cell-port").unwrap();
    let lib=cell_connection::Library::from_cell_programs(&[q]);assert!(!lib.is_empty());
    let commands=eg.parse_program(None,"(run s 1)").unwrap();
    let mut r=Runtime::new_full_assisted(eg,&commands,Some(&worker),true,None,Some(Box::new(Hook(lib)))).unwrap();
    assert_eq!(r.report()["cells"]["binding_connections"],true);
    r.run(&commands).unwrap();
    check(&r,"(check (Hit (Root (N 1) 10) 2)) (fail (check (Hit (Root (N 0) 10) 1)))");
    // Missing -> present, function update, deletion, fresh alias key, and subsume.
    r.input("(set (Cost (N 2)) 15) (set (Cost (N 1)) 3) (Fan (N 1) 4)").unwrap();
    r.run(&commands).unwrap();
    check(&r,"(check (Hit (Root (N 2) 10) 3)) (fail (check (Hit (Root (N 1) 10) 4)))");
    r.input("(delete (Cost (N 2))) (Fan (N 2) 5)").unwrap();r.run(&commands).unwrap();
    check(&r,"(fail (check (Hit (Root (N 2) 10) 5)))");
    r.input("(set (Cost (N 3)) 17) (union (N 2) (N 3))").unwrap();r.run(&commands).unwrap();
    check(&r,"(check (Hit (Root (N 2) 10) 5))");
    r.input("(subsume (Root (N 2) 10)) (Fan (N 2) 6)").unwrap();r.run(&commands).unwrap();
    check(&r,"(fail (check (Hit (Root (N 2) 10) 6)))");
    let report=r.report();
    assert_eq!(report["runtime"]["cell_query_bindings"],0);
    if std::env::var("CELL_SEARCH_COUNTERS").as_deref()==Ok("1") {
        assert!(report["cell_work"]["connection_refutations"].as_u64().unwrap()>0);
        assert!(report["cell_work"]["watch_wakes"].as_u64().unwrap()>0);
    }
}

#[test]
fn complete_port_ranges_cover_all_aliases_and_extreme_integer_values() {
    let Some(worker)=worker() else{return};
    if std::env::var("CELL_BINDING_RANGES").as_deref()!=Ok("1"){return}
    use cell_assisted_egglog::{cell_connection, table_live::SaturationHook};
    struct Hook(cell_connection::Library);
    impl SaturationHook for Hook {
        fn binding_connections(&self)->cell_connection::Library {self.0.clone()}
        fn try_boundary(&mut self,_:&mut EGraph,_:&egglog::ast::Schedule)->cell_assisted_egglog::Result<Option<serde_json::Value>>{Ok(None)}
        fn take_commits(&mut self,_:&mut EGraph)->cell_assisted_egglog::Result<(Vec<String>,Vec<egglog::cell_events::CellCommitBatch>)>{Ok((vec![],vec![]))}
    }
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None,r#"
      (datatype E (N i64) (Pair E i64))
      (relation Anchor (E i64)) (relation Fan (E i64)) (relation Hit (E i64 i64 i64)) (relation CellMark (E))
      (ruleset bake) (ruleset s)
      (rule ((= p (Pair term cost))) ((CellMark p)) :ruleset bake :name "cell-cost-port")
    "#).unwrap();
    for (op,i) in ["<",">","<=",">=","!="].into_iter().zip(0..){
        eg.parse_and_run_program(None,&format!("(rule ((Anchor p limit) (= p (Pair term cost)) (Fan p token) ({op} cost limit)) ((Hit p cost token {i})) :ruleset s)")).unwrap();
    }
    let values=[i64::MIN,i64::MIN+1,-4294967296,-2147483648,-11,0,7,20,2147483647,4294967296,i64::MAX-1,i64::MAX];
    for (i,cost) in values.iter().enumerate(){for limit in values {
        eg.parse_and_run_program(None,&format!("(Anchor (Pair (N {i}) {cost}) {limit}) (Fan (Pair (N {i}) {cost}) 1)")).unwrap();
    }}
    let q=eg.prepare_gpu_cell_action("bake","cell-cost-port").unwrap();
    let commands=eg.parse_program(None,"(run s 1)").unwrap();
    let mut r=Runtime::new_full_assisted(eg,&commands,Some(&worker),true,None,Some(Box::new(Hook(cell_connection::Library::from_cell_programs(&[q]))))).unwrap();
    assert!(r.report()["cells"]["range_checks"].as_u64().unwrap()>0);
    r.run(&commands).unwrap();
    // New candidate in the same output fiber must widen, not reuse the old
    // positive sample as an upper bound. Join retains full tuple correlations.
    r.input("(union (Pair (N 7) 20) (Pair (N 42) 3)) (Fan (Pair (N 7) 20) 2)").unwrap();
    r.run(&commands).unwrap();
    check(&r,"(check (Hit (Pair (N 7) 20) 3 2 0))");
    r.input("(subsume (Pair (N 42) 3)) (Fan (Pair (N 7) 20) 3)").unwrap();r.run(&commands).unwrap();
    check(&r,"(fail (check (Hit (Pair (N 7) 20) 3 3 0)))");
    // Forces index rebase; summary reset must include all surviving rows.
    let fresh=(100..600).map(|i|format!("(Anchor (Pair (N {i}) {i}) 0) (Fan (Pair (N {i}) {i}) 4)")).collect::<String>();
    r.input(&fresh).unwrap();r.run(&commands).unwrap();
    if std::env::var("CELL_SEARCH_COUNTERS").as_deref()==Ok("1") {
        assert!(r.report()["cell_work"]["range_refutations"].as_u64().unwrap()>0);
    }
}

#[test]
fn generic_empty_table_gate_resumes_after_insert_delete_union_and_subsume() {
    let Some(worker)=worker() else{return};
    for direct in [false,true] {
        let mut eg=EGraph::default();
        eg.parse_and_run_program(None,r#"
          (datatype E (N i64) (Gate E))
          (relation Need (E)) (relation Hit (E)) (relation Boot ()) (ruleset s)
          (rule ((Need x) (= g (Gate x))) ((Hit x)) :ruleset s :name "join")
          (Need (N 1))
        "#).unwrap();
        eg.parse_and_run_program(None,if direct {"(Boot)"} else {"(rule () ((Boot)) :ruleset s :name \"unit\")"}).unwrap();
        let commands=eg.parse_program(None,"(run s 1)").unwrap();
        let mut r=if direct {Runtime::new(eg,&commands,&worker,true).unwrap()} else {Runtime::new_full(eg,&commands,Some(&worker),true,None).unwrap()};
        r.run(&commands).unwrap();check(&r,"(check (Boot)) (fail (check (Hit (N 1))))");
        r.run(&commands).unwrap(); // Repeated empty query still acknowledges its frontier.
        r.input("(Gate (N 1))").unwrap();r.run(&commands).unwrap();check(&r,"(check (Hit (N 1)))");
        r.input("(delete (Gate (N 1))) (Need (N 2))").unwrap();r.run(&commands).unwrap();
        check(&r,"(fail (check (Hit (N 2))))");
        r.input("(Gate (N 3)) (union (N 2) (N 3))").unwrap();r.run(&commands).unwrap();
        check(&r,"(check (Hit (N 2)))");
        // The final snapshot is nonempty; do not prune on an intermediate delete.
        r.input("(delete (Gate (N 3))) (Gate (N 4)) (Need (N 4))").unwrap();r.run(&commands).unwrap();
        check(&r,"(check (Hit (N 4)))");
        r.input("(subsume (Gate (N 4))) (Need (N 5))").unwrap();r.run(&commands).unwrap();
        check(&r,"(fail (check (Hit (N 5))))");
        r.input("(Gate (N 5))").unwrap();r.run(&commands).unwrap();check(&r,"(check (Hit (N 5)))");
        let report=r.report();assert!(report["saturation_diagnostics"].is_null());assert_eq!(report["runtime"]["cell_query_rules"],0);
        if report["gpu_optimizations"]["empty_table_pruning"]==true {
            assert!(report["runtime"]["gpu_empty_table_prunes"].as_u64().unwrap()>=3);
            if !direct {
                let unit=report["rules"].as_array().unwrap().iter().find(|q|q["rule"]=="unit").unwrap();
                assert_eq!(unit["stats"]["empty_table_prunes"],0);
            }
        }
    }
}

#[test]
fn empty_nullary_relation_is_required_and_pruning_preserves_frozen_run_boundaries() {
    let Some(worker)=worker() else{return};
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None,r#"
      (relation Ready ()) (relation Seed (i64)) (relation Gate (i64)) (relation Hit (i64)) (ruleset s)
      (rule ((Seed x)) ((Gate x)) :ruleset s :name "produce")
      (rule ((Ready) (Gate x)) ((Hit x)) :ruleset s :name "consume")
      (Seed 1) (Ready)
    "#).unwrap();
    let commands=eg.parse_program(None,"(run s 1)").unwrap();
    let mut r=Runtime::new_full(eg,&commands,Some(&worker),true,None).unwrap();
    r.run(&commands).unwrap();check(&r,"(check (Gate 1)) (fail (check (Hit 1)))");
    r.run(&commands).unwrap();check(&r,"(check (Hit 1))");
    r.input("(delete (Ready)) (Gate 2)").unwrap();r.run(&commands).unwrap();check(&r,"(fail (check (Hit 2)))");
    r.input("(Ready)").unwrap();r.run(&commands).unwrap();check(&r,"(check (Hit 2))");
}

#[test]
fn analyzed_structural_projection_coexists_with_full_numeric_queries() {
    let Some(worker) = worker() else { return };
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None, r#"
      (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
      (relation Out (Expr)) (relation Num (i64)) (relation Done (i64))
      (ruleset s)
      (rule ((= y (Single x)) (= z (Get y 0))) ((Out z)) :ruleset s :name "structure")
      (rule ((Num n) (> n 0)) ((Done (+ n 1))) :ruleset s :name "numeric")
      (union (Get (Single (Leaf 9)) 0) (Leaf 9)) (Num 4)
    "#).unwrap();
    let cmds=eg.parse_program(None,"(run s 1)").unwrap();
    let lib=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    for plan_only in [false,true] {
        let mut r=Runtime::new_full_configured(eg.clone(),&cmds,Some(&worker),true,None,None,
            cell_assisted_egglog::table_live::Options{analysis:Some(&lib),plan_only,..Default::default()}).unwrap();
        let report=r.report();
        assert_eq!(report["cells"]["compact_binding_workspace"],true);
        assert!(!report["cells"]["projection_queries"].as_array().unwrap().is_empty());
        assert!(!report["cells"]["projection_fallback_queries"].as_array().unwrap().is_empty());
        r.run(&cmds).unwrap();check(&r,"(check (Out (Leaf 9))) (check (Done 5))");
        r.input("(subsume (Get (Single (Leaf 9)) 0)) (Num 5)").unwrap();r.run(&cmds).unwrap();
        r.input("(union (Get (Single (Leaf 8)) 0) (Leaf 8)) (union (Leaf 9) (Leaf 8))").unwrap();r.run(&cmds).unwrap();
        check(&r,"(check (Out (Leaf 8))) (check (Done 6))");
    }
}
