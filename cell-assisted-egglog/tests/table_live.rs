use cell_assisted_egglog::table_live::Runtime;
use egglog::{EGraph, ast::Command};
fn setup(code: &str) -> (EGraph, Vec<Command>) {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, code).unwrap();
    let cmds = eg
        .parse_program(None, "(run-schedule (saturate (seq a (saturate b))))")
        .unwrap();
    (eg, cmds)
}
fn worker() -> Option<std::path::PathBuf> {
    std::env::var_os("CELL_MATRIX_WORKER").map(Into::into)
}
fn assert_check(r: &Runtime, code: &str) {
    r.engine()
        .clone()
        .parse_and_run_program(None, code)
        .unwrap();
}
#[test]
fn continuous_schedule_keeps_indexes_and_receipts_across_real_native_updates() {
    let Some(worker) = worker() else {
        return;
    };
    let (eg, cmds) = setup(
        r#"
 (datatype E (N i64)) (relation Edge (E E)) (relation Reach (E))
 (function Cost (E) i64 :merge (min old new)) (ruleset a) (ruleset b)
 (rule ((Reach x) (Edge x y) (= cost (Cost x))) ((Reach y) (set (Cost y) (+ cost 1))) :ruleset a)
 (rule ((Reach (N 3))) ((union (N 3) (N 4))) :ruleset b)
 (Reach (N 0)) (set (Cost (N 0)) 0)
 (Edge (N 0) (N 1)) (Edge (N 1) (N 2)) (Edge (N 2) (N 3)) (Edge (N 4) (N 5))
 "#,
    );
    let mut r = Runtime::new(eg, &cmds, &worker, true).unwrap();
    r.run(&cmds).unwrap();
    assert_check(&r, "(check (Reach (N 5))) (check (= (Cost (N 5)) 4))");
    let before = r.report();
    let dispatches = before["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["stats"]["dispatches"].as_u64().unwrap())
        .sum::<u64>();
    assert!(dispatches > 2);
    r.run(&cmds).unwrap();
    let after = r.report();
    assert_eq!(
        dispatches,
        after["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| q["stats"]["dispatches"].as_u64().unwrap())
            .sum::<u64>()
    );
    assert_eq!(after["runtime"]["shader_compiles"], 1);
    r.input("(Edge (N 5) (N 6))").unwrap();
    r.run(&cmds).unwrap();
    assert_check(&r, "(check (= (Cost (N 6)) 5))");
}
#[test]
fn update_visibility_and_false_guards_are_followed_between_rounds() {
    let Some(worker) = worker() else {
        return;
    };
    let (eg, cmds) = setup(
        r#"
 (datatype E (N i64)) (relation Offer (E i64)) (relation Seen (E))
 (function Cost (E) i64 :merge (min old new)) (ruleset a) (ruleset b)
 (rule ((Offer x c) (= old (Cost x)) (< c old)) ((set (Cost x) c) (Seen x)) :ruleset a)
 (set (Cost (N 0)) 10) (Offer (N 0) 12)
 "#,
    );
    let mut r = Runtime::new(eg, &cmds, &worker, true).unwrap();
    r.run(&cmds).unwrap();
    r.input("(Offer (N 0) 3)").unwrap();
    r.run(&cmds).unwrap();
    assert_check(&r, "(check (= (Cost (N 0)) 3)) (check (Seen (N 0)))");
    r.input("(subsume (Offer (N 0) 3)) (Offer (N 0) 2)")
        .unwrap();
    r.run(&cmds).unwrap();
    assert_check(&r, "(check (= (Cost (N 0)) 2))");
}

#[test]
fn term_cost_function_merge_and_smaller_self_join_run_continuously() {
    let Some(worker) = worker() else {
        return;
    };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,r#"
 (datatype E (Arg i64) (Add E E))
 (datatype T (Leaf i64) (Plus T T))
 (datatype TC (TCPair T i64) (Smaller TC TC))
 (function Best (E) TC :merge (Smaller old new))
 (relation Candidate (E TC)) (ruleset make) (ruleset update) (ruleset choose)
 (rule ((= e (Arg n))) ((Candidate e (TCPair (Leaf n) 1))) :ruleset make)
 (rule ((= e (Add a b)) (= (Best a) (TCPair x i)) (= (Best b) (TCPair y j))) ((Candidate e (TCPair (Plus x y) (+ 1 (+ i j))))) :ruleset make)
 (rule ((Candidate e (TCPair t c)) (= (Best e) (TCPair old oldc)) (< c oldc)) ((set (Best e) (TCPair t c))) :ruleset update)
 (rule ((= p (Smaller (TCPair x i) (TCPair y j))) (<= i j)) ((union p (TCPair x i))) :ruleset choose)
 (rule ((= p (Smaller (TCPair x i) (TCPair y j))) (> i j)) ((union p (TCPair y j))) :ruleset choose)
 (set (Best (Arg 0)) (TCPair (Leaf 9) 100))
 (set (Best (Arg 1)) (TCPair (Leaf 9) 100))
 (set (Best (Add (Arg 0) (Arg 1))) (TCPair (Leaf 9) 100))
 (Add (Arg 0) (Arg 1))
 "#).unwrap();
    let cmds = eg
        .parse_program(
            None,
            "(run-schedule (saturate (seq make (saturate (seq update (saturate choose))))))",
        )
        .unwrap();
    let mut r = Runtime::new(eg, &cmds, &worker, true).unwrap();
    r.run(&cmds).unwrap();
    assert_check(
        &r,
        "(check (= (Best (Add (Arg 0) (Arg 1))) (TCPair (Plus (Leaf 0) (Leaf 1)) 3)))",
    );
}

#[test]
fn handoff_keeps_the_original_seminaive_watermark() {
    let Some(worker) = worker() else {
        return;
    };
    let (mut eg, cmds) = setup(
        r#"
 (datatype E (N i64)) (relation Edge (E E)) (relation Reach (E))
 (ruleset a) (ruleset b)
 (rule ((Reach x) (Edge x y)) ((Reach y)) :ruleset a)
 (Reach (N 0)) (Edge (N 0) (N 1)) (Edge (N 1) (N 2)) (Edge (N 2) (N 3))
 "#,
    );
    eg.parse_and_run_program(None, "(run a 1)").unwrap();
    let mut r = Runtime::new(eg, &cmds, &worker, true).unwrap();
    r.run(&cmds).unwrap();
    assert_check(&r, "(check (Reach (N 3)))");
    let report = r.report();
    let stats = &report["rules"][0]["stats"];
    assert_eq!(
        stats["emitted_bindings"], 2,
        "the already completed 0->1 application must not be replayed"
    );
}

#[test]
fn handback_does_not_replay_completed_native_actions() {
    let Some(worker) = worker() else {
        return;
    };
    let (eg, cmds) = setup(
        r#"
 (datatype E (N i64)) (relation P (E)) (relation Q (E)) (ruleset a) (ruleset b)
 (rule ((P x)) ((Q x)) :ruleset a) (P (N 1))
 "#,
    );
    let mut r = Runtime::new(eg, &cmds, &worker, true).unwrap();
    r.run(&cmds).unwrap();
    let mut eg = r.into_engine();
    let report = eg.step_rules("a").unwrap();
    assert_eq!(report.num_matches_per_rule.values().sum::<usize>(), 0);
    eg.parse_and_run_program(None, "(P (N 2))").unwrap();
    let report = eg.step_rules("a").unwrap();
    assert_eq!(report.num_matches_per_rule.values().sum::<usize>(), 1);
    eg.parse_and_run_program(None, "(check (Q (N 2)))").unwrap();
}

#[test]
fn rules_in_one_run_match_the_same_pre_rhs_state() {
    let Some(worker) = worker() else {
        return;
    };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (relation P (i64)) (relation Q (i64)) (relation R (i64)) (ruleset a)
      (rule ((P x)) ((Q x)) :ruleset a)
      (rule ((Q x)) ((R x)) :ruleset a)
      (P 7)
    "#,
    )
    .unwrap();
    let commands = eg.parse_program(None, "(run a 1)").unwrap();
    let mut r = Runtime::new(eg, &commands, &worker, true).unwrap();
    r.run(&commands).unwrap();
    assert_eq!(r.engine().get_size("Q"), 1);
    assert_eq!(
        r.engine().get_size("R"),
        0,
        "a sibling rule must not see this Run's new RHS"
    );
    r.run(&commands).unwrap();
    assert_eq!(r.engine().get_size("R"), 1);
}

#[test]
fn identical_row_reinsert_is_a_new_occurrence_for_a_stateful_native_merge() {
    let Some(worker) = worker() else {
        return;
    };
    let (eg, commands) = setup(
        r#"
      (relation P (i64)) (function Count (i64) i64 :merge (+ old new))
      (ruleset a) (ruleset b)
      (rule ((P x)) ((set (Count x) 1)) :ruleset a)
      (P 7)
    "#,
    );
    let mut r = Runtime::new(eg, &commands, &worker, true).unwrap();
    r.run(&commands).unwrap();
    assert_check(&r, "(check (= (Count 7) 1))");
    r.input("(delete (P 7)) (P 7)").unwrap();
    r.run(&commands).unwrap();
    assert_check(&r, "(check (= (Count 7) 2))");
    r.run(&commands).unwrap();
    assert_check(&r, "(check (= (Count 7) 2))");
}

#[test]
fn shared_gc_does_not_replay_matches_or_lose_a_delayed_ruleset() {
    let Some(worker) = worker() else {
        return;
    };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (relation P (i64)) (relation Q (i64))
      (function Count (i64) i64 :merge (+ old new))
      (ruleset a) (ruleset b)
      (rule ((P x)) ((set (Count x) 1)) :ruleset a)
      (rule ((Q x)) ((set (Count x) 1)) :ruleset b)
      (P 0) (Q 100)
    "#,
    )
    .unwrap();
    let both = eg.parse_program(None, "(run-schedule (seq a b))").unwrap();
    let a = eg.parse_program(None, "(run a 1)").unwrap();
    let b = eg.parse_program(None, "(run b 1)").unwrap();
    let mut r = Runtime::new(eg, &both, &worker, true).unwrap();
    r.run(&a).unwrap();
    for _ in 0..24 {
        r.input("(delete (P 0)) (P 0)").unwrap();
        r.run(&a).unwrap();
    }
    assert_check(&r, "(check (= (Count 0) 25))");
    r.run(&b).unwrap();
    assert_check(&r, "(check (= (Count 100) 1))");
    r.run(&both).unwrap();
    assert_check(&r, "(check (= (Count 0) 25)) (check (= (Count 100) 1))");
    let report = r.report();
    assert_eq!(report["row_slots"], 2);
    assert_eq!(report["runtime"]["workers"], 1);
    assert!(report["device"]["compactions"].as_u64().unwrap() > 1);
    assert_eq!(report["device"]["retained_matches"], 0);
    assert!(report["device"]["events"].as_u64().unwrap() <= 4);
}

#[test]
fn unchanged_journal_notifications_do_not_repeat_stateful_merge() {
    let Some(worker) = worker() else {
        return;
    };
    let (eg, cmds) = setup(
        r#"
      (relation P (i64)) (function Count (i64) i64 :merge (+ old new)) (ruleset a) (ruleset b)
      (rule ((P x)) ((set (Count x) 1)) :ruleset a) (P 7)
    "#,
    );
    let mut r = Runtime::new(eg, &cmds, &worker, true).unwrap();
    r.run(&cmds).unwrap();
    r.input("(P 7)").unwrap();
    r.run(&cmds).unwrap();
    assert_check(&r, "(check (= (Count 7) 1))");
}

#[test]
fn shared_cartesian_output_grows_and_gc_does_not_replay_the_old_product() {
    let Some(worker) = worker() else {
        return;
    };
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (relation P (i64)) (relation Q (i64))
      (function Count (i64) i64 :merge (+ old new)) (ruleset a)
      (rule ((P x) (Q y)) ((set (Count x) 1)) :ruleset a)
    "#,
    )
    .unwrap();
    for i in 0..32 {
        eg.parse_and_run_program(None, &format!("(P {i}) (Q {i})"))
            .unwrap();
    }
    let commands = eg
        .parse_program(None, "(run-schedule (saturate a))")
        .unwrap();
    let mut cpu = Runtime::new_cpu(eg.clone(), &commands).unwrap();
    cpu.run(&commands).unwrap();
    let mut gpu = Runtime::new(eg, &commands, &worker, true).unwrap();
    gpu.run(&commands).unwrap();
    assert_check(&cpu, "(check (= (Count 0) 32))");
    assert_check(&gpu, "(check (= (Count 0) 32))");
    assert!(
        gpu.report()["runtime"]["output_high_water"]
            .as_u64()
            .unwrap()
            >= 1024
    );
    gpu.input("(Q 32)").unwrap();
    gpu.run(&commands).unwrap();
    assert_check(&gpu, "(check (= (Count 0) 33))");
    gpu.run(&commands).unwrap();
    assert_check(&gpu, "(check (= (Count 0) 33))");
}

#[test]
fn noop_rhs_does_not_count_private_worklist_cleanup_as_graph_growth() {
    let Some(worker) = worker() else {
        return;
    };
    let (mut eg, commands) = setup(
        r#"
      (relation P (i64)) (ruleset a) (ruleset b)
      (rule ((P x)) ((P x)) :ruleset a) (P 1)
    "#,
    );
    let commands = eg
        .parse_program(None, "(run-schedule (saturate a))")
        .unwrap();
    let mut cpu = Runtime::new_cpu(eg.clone(), &commands).unwrap();
    cpu.run(&commands).unwrap();
    let mut gpu = Runtime::new(eg, &commands, &worker, true).unwrap();
    gpu.run(&commands).unwrap();
    assert_eq!(cpu.report()["runtime"]["ruleset_runs"], 1);
    assert_eq!(gpu.report()["runtime"]["ruleset_runs"], 1);
}
