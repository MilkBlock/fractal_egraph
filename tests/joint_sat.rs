#[allow(dead_code)]
#[path = "../examples/support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::parameterized_cell::{Runtime, joint::SCHEDULE};
use egglog::{CommandOutput, EGraph};
const SOURCE: &str = include_str!("../experiments/parameterized_cell/terms.egg");
#[path = "../examples/support/cell_transition_case.rs"]
mod transition_case;

#[test]
fn competing_existing_cell_resumes_its_ports_and_preserves_every_effect() {
    transition_case::run().unwrap();
}

#[test]
fn competing_transitions_are_used_by_the_shared_cpu_gpu_hook() {
    use cell_assisted_egglog::table_live;
    let worker = std::env::var_os("CELL_MATRIX_WORKER");
    for gpu in [false, true] {
        if gpu && worker.is_none() {
            continue;
        }
        let (base, observer) = Runtime::new_joint(SOURCE).unwrap().detach_joint().unwrap();
        let commands = base
            .clone()
            .parse_program(None, transition_case::UPDATES)
            .unwrap();
        let schedules = commands
            .iter()
            .filter(|c| matches!(c, egglog::ast::Command::RunSchedule(_)))
            .cloned()
            .collect::<Vec<_>>();
        let selected = ["terms", "terms-helpers", "terms-helpers-helpers"].map(String::from);
        let mut reference = table_live::Runtime::new_cpu(base.clone(), &schedules).unwrap();
        let mut cell = table_live::Runtime::new_hybrid(
            base.clone(),
            &schedules,
            gpu.then(|| std::path::Path::new(worker.as_ref().unwrap())),
            gpu,
            &selected,
        )
        .unwrap();
        cell.set_saturation_hook(Box::new(observer));
        let mut input = String::new();
        for command in commands {
            if !matches!(command, egglog::ast::Command::RunSchedule(_)) {
                input.push_str(&format!("{command}\n"));
                continue;
            }
            reference.input(&input).unwrap();
            cell.input(&input).unwrap();
            input.clear();
            reference.run(std::slice::from_ref(&command)).unwrap();
            cell.run(std::slice::from_ref(&command)).unwrap();
            compare::equal(&base, reference.engine(), cell.engine()).unwrap();
        }
        assert!(input.trim().is_empty());
        let report = cell.report();
        assert!(
            report["saturation_diagnostics"]["port_reuse"]["reused_reads"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(
            report["saturation_diagnostics"]["joint"]["stats"]["macro_instances"],
            0
        );
        let ordinary: u64 = report["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["stats"]["emitted_bindings"].as_u64().unwrap())
            .sum();
        assert_eq!(
            ordinary, 0,
            "the ordinary query dispatcher should not run in this closure"
        );
    }
}
struct Pair {
    base: EGraph,
    native: EGraph,
    cell: Runtime,
}

#[test]
fn certified_comparisons_close_a_deep_minimum_chain_in_one_native_batch() {
    let mut p = Pair::new();
    let mut term = "(TCPair (TermLeaf 0) 1)".to_string();
    for _ in 0..24 { term = format!("(Smaller (TCPair (TermLeaf 1) 9) {term})"); }
    // Existing helper state forces the general joint path, not the fresh-DAG macro.
    p.input(&format!("(PotentialExtractedExpr (V 9) {term})"));
    p.run_ordered(SCHEDULE);
    let report = p.cell.report();
    let c = &report["joint"]["comparison_closure"];
    assert!(c["saturations"].as_u64().unwrap() > 0, "{report}");
    assert!(c["propagated_costs"].as_u64().unwrap() >= 24, "{report}");
    assert!(c["native_batches"].as_u64().unwrap() <= 2, "{report}");
    p.cell.engine().clone().parse_and_run_program(None,
        "(check (= (ExtractedExpr (V 9)) (TCPair (TermLeaf 0) 1)))").unwrap();
    let before = c["native_batches"].clone();
    // A finite Run keeps its usual semantics even after the accelerated closure.
    p.input("(Smaller (TCPair (TermLeaf 2) 2) (TCPair (TermLeaf 3) 3))");
    p.run_ordered("(run terms-helpers-helpers 1)");
    assert_eq!(p.cell.report()["joint"]["comparison_closure"]["native_batches"], before);
}

#[test]
fn foreign_cost_keyed_function_disables_comparison_reordering() {
    for declaration in [
        "(function Foreign (TermAndCost) i64 :merge new)",
        "(sort Costs (Vec TermAndCost)) (function Foreign (Costs) i64 :merge new)",
    ] {
        let source = format!("{SOURCE}\n{declaration}");
        let cell = Runtime::new_joint(&source).unwrap();
        let base = cell.engine().clone();
        let mut p = Pair { native: base.clone(), base, cell };
        p.input("(PotentialExtractedExpr (V 0) (TCPair (TermLeaf 1) 2))");
        p.run_ordered(SCHEDULE);
        assert!(p.cell.report()["joint"]["comparison_closure"].is_null());
    }
}

#[test]
fn comparison_proof_is_not_reused_after_conflicting_cost_aliases() {
    let mut p = Pair::new();
    p.input("(PotentialExtractedExpr (V 0) (TCPair (TermLeaf 0) 3))");
    p.run_ordered(SCHEDULE);
    let before = p.cell.report()["joint"]["comparison_closure"]["saturations"].clone();
    p.input("(union (TCPair (TermLeaf 0) 3) (TCPair (TermLeaf 1) 8)) \
             (PotentialExtractedExpr (V 0) (TCPair (TermLeaf 2) 5))");
    p.run_ordered(SCHEDULE);
    assert_eq!(p.cell.report()["joint"]["comparison_closure"]["saturations"], before);
}

#[test]
fn unrelated_container_sort_does_not_disable_the_cost_closure() {
    let source = format!("{SOURCE}\n(sort Exprs (Set Expr)) \
                         (function Foreign (Exprs) i64 :merge new)");
    let cell = Runtime::new_joint(&source).unwrap();
    let base = cell.engine().clone();
    let mut p = Pair { native: base.clone(), base, cell };
    p.input("(PotentialExtractedExpr (V 0) (TCPair (TermLeaf 1) 2))");
    p.run_ordered(SCHEDULE);
    assert!(p.cell.report()["joint"]["comparison_closure"]["saturations"].as_u64().unwrap() > 0);
}

#[test]
fn pending_comparison_resumes_when_a_child_is_derived_inside_the_closure() {
    let mut p = Pair::new();
    // The outer left argument is allocated before the inner arguments, so its
    // seed is visited before the inner Smaller has a resolved TCPair cost.
    p.input("(PotentialExtractedExpr (V 0) \
               (Smaller (TCPair (TermLeaf 0) 1) \
                 (Smaller (TCPair (TermLeaf 1) 9) (TCPair (TermLeaf 2) 5))))");
    p.run_ordered(SCHEDULE);
    let report = p.cell.report();
    let c = &report["joint"]["comparison_closure"];
    assert!(c["propagated_costs"].as_u64().unwrap() >= 2);
    assert!(c["native_batches"].as_u64().unwrap() <= 2);
    p.cell.engine().clone().parse_and_run_program(None,
        "(check (= (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 1)))").unwrap();
}
impl Pair {
    fn new() -> Self {
        let cell = Runtime::new_joint(SOURCE).unwrap();
        let base = cell.engine().clone();
        Self {
            native: base.clone(),
            base,
            cell,
        }
    }
    fn input(&mut self, s: &str) {
        self.native.parse_and_run_program(None, s).unwrap();
        self.cell.input(s).unwrap();
    }
    fn run(&mut self, s: &str) {
        let commands = self.native.parse_program(None, s).unwrap();
        let updated = self
            .native
            .run_program(commands.clone())
            .unwrap()
            .into_iter()
            .any(|o| matches!(o,CommandOutput::RunSchedule(r) if r.updated));
        self.cell.run(&commands).unwrap();
        self.cell.audit_mirror().unwrap();
        assert_eq!(updated, self.cell.last_updated, "{s}");
        compare::equal(&self.base, &self.native, self.cell.engine()).unwrap();
    }
    // The production gate uses the same sorted native commit protocol. A
    // fused native Run can create a different orientation of Smaller records,
    // even with retirement disabled; it is not the oracle for this optimization.
    fn run_ordered(&mut self, s: &str) {
        let commands = self.native.parse_program(None, s).unwrap();
        let mut ordered =
            cell_assisted_egglog::table_live::Runtime::new_cpu(self.native.clone(), &commands)
                .unwrap();
        ordered.run(&commands).unwrap();
        self.native = ordered.into_engine();
        self.cell.run(&commands).unwrap();
        self.cell.audit_mirror().unwrap();
        compare::equal(&self.base, &self.native, self.cell.engine()).unwrap();
    }
    fn emitted(&self) -> u64 {
        self.cell.report()["programs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["emitted_bindings"].as_u64().unwrap())
            .sum()
    }
}
#[test]
fn fresh_chain_publishes_joint_closure_without_smooth_matches() {
    let mut p = Pair::new();
    p.input("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3))
 (Bop (Add) (V 0) (V 1)) (Bop (Mul) (Bop (Add) (V 0) (V 1)) (V 0))
 (Bop (Add) (Bop (Mul) (Bop (Add) (V 0) (V 1)) (V 0)) (V 1))");
    p.run(SCHEDULE);
    assert_eq!(p.emitted(), 0, "{}", p.cell.report());
    assert_eq!(p.cell.report()["joint"]["stats"]["macro_instances"], 3);
    assert!(
        p.native
            .get_overall_run_report()
            .num_matches_per_rule
            .values()
            .sum::<usize>()
            > 0
    );
    p.input("(Bop (Mul) (Bop (Add) (V 0) (V 1)) (V 1))");
    p.run(SCHEDULE);
    assert_eq!(p.cell.report()["joint"]["stats"]["macro_instances"], 4);
    assert_eq!(p.emitted(), 0);
    p.run(SCHEDULE);
    assert_eq!(p.emitted(), 0);
    p.run(
        "(run-schedule (saturate terms (saturate terms-helpers (saturate terms-helpers-helpers))))",
    );
    assert_eq!(p.emitted(), 0);
    p.run("(run terms 1) (run terms-helpers 1) (run terms-helpers-helpers 1)");
    assert_eq!(p.emitted(), 0);
}

#[test]
fn destructive_changes_and_cycles_cannot_replay_stale_closures() {
    let mut p = Pair::new();
    p.input("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (Bop (Add) (V 0) (V 0))");
    p.run(SCHEDULE);
    p.input("(delete (PotentialExtractedExpr (Bop (Add) (V 0) (V 0)) (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 0)) 5)))");
    p.run(SCHEDULE);
    assert_eq!(p.cell.engine().get_size("PotentialExtractedExpr"), 0);
    p.input("(subsume (Bop (Add) (V 0) (V 0)))");
    p.run(SCHEDULE);
    let mut q = Pair::new();
    q.input("(union (V 0) (Bop (Add) (V 0) (V 0)))");
    q.run(SCHEDULE);
    assert_eq!(q.cell.engine().get_size("PotentialExtractedExpr"), 0);
    // A consumed producer whose candidate was deleted is not a new Cell simply
    // because an independent fresh root is appended after the fallback closes.
    let mut q = Pair::new();
    q.input("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (Bop (Add) (V 0) (V 0))");
    q.run("(run terms 1)");
    q.input("(delete (PotentialExtractedExpr (Bop (Add) (V 0) (V 0)) (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 0)) 5)))");
    q.run(SCHEDULE);
    q.input("(Bop (Mul) (V 0) (V 0))");
    q.run(SCHEDULE);
    assert_eq!(q.cell.engine().get_size("PotentialExtractedExpr"), 1);
}

#[test]
fn unsafe_numeric_paths_fall_back_before_any_macro_and_keep_native_errors() {
    for c in [-10, 9223372036854775807i64] {
        let mut p = Pair::new();
        p.input(&format!("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) {c})) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 1)) (Bop (Add) (V 0) (V 1))"));
        let s = p.native.parse_program(None, SCHEDULE).unwrap();
        let a = p.native.run_program(s.clone()).unwrap_err().to_string();
        let b = p.cell.run(&s).unwrap_err().to_string();
        assert_eq!(a, b);
        assert_eq!(p.cell.report()["joint"]["stats"]["macro_instances"], 0);
        assert!(p.cell.run(&s).is_err());
    }
    let bad = SOURCE.replace(":merge (Smaller old new)", ":merge new");
    assert!(Runtime::new_joint(&bad).is_err());
    let bad = SOURCE.replace("(< cost oldcost)", "(<= cost oldcost)");
    assert!(Runtime::new_joint(&bad).is_err());
}

#[test]
fn aliased_roots_and_another_schedule_are_not_given_the_joint_certificate() {
    let mut p = Pair::new();
    p.input("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 5)) (union (Bop (Add) (V 0) (V 0)) (Bop (Mul) (V 0) (V 1)))");
    p.run(SCHEDULE);
    assert_eq!(p.cell.report()["joint"]["stats"]["macro_instances"], 0);
    let mut p = Pair::new();
    p.input("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (Bop (Add) (V 0) (V 0))");
    p.run("(run-schedule (saturate (seq terms-helpers-helpers terms-helpers terms)))");
    assert_eq!(
        p.cell.report()["joint"]["stats"]["skipped_joint_saturations"],
        0
    );
    assert!(p.emitted() > 0);
}
#[test]
fn finite_run_partial_history_and_external_updates_do_not_get_macroed() {
    let mut p = Pair::new();
    p.input("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3)) (Bop (Add) (V 0) (V 1))");
    p.run("(run terms 1)");
    assert_eq!(p.cell.engine().get_size("ExtractedExpr"), 2);
    p.run(SCHEDULE);
    assert!(
        p.cell.report()["joint"]["stats"]["fallback_boundaries"]
            .as_u64()
            .unwrap()
            > 0
    );
    p.input("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 5) 1))");
    p.run(SCHEDULE);
    p.input("(union (V 0) (V 1))");
    p.run(SCHEDULE);
    p.input("(union (TCPair (TermLeaf 5) 1) (TCPair (TermLeaf 6) 1))");
    p.run(SCHEDULE);
    p.run(SCHEDULE);
}

#[test]
fn a_new_instance_cannot_reuse_a_subsumed_candidate_as_a_visible_proof() {
    let mut p = Pair::new();
    p.input("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3)) (Bop (Add) (V 0) (V 1))");
    p.run(SCHEDULE);
    p.input("(subsume (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 1)) 6))");
    p.run(SCHEDULE);
    p.input("(set (ExtractedExpr (V 2)) (TCPair (TermLeaf 0) 2)) (Bop (Add) (V 2) (V 1))");
    p.run(SCHEDULE);
    assert_eq!(p.cell.report()["joint"]["stats"]["macro_instances"], 1);
}
#[test]
fn blocked_inputs_and_sentinel_branches_are_preserved() {
    let mut p = Pair::new();
    p.input("(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 10000000000000000)) (Bop (Add) (V 0) (V 1))");
    p.run(SCHEDULE);
    assert_eq!(p.cell.engine().get_size("PotentialExtractedExpr"), 0);
    p.input("(set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 2))");
    p.run(SCHEDULE);
    assert_eq!(p.emitted(), 0);
    assert_eq!(p.cell.engine().get_size("Smaller"), 0);
}

#[test]
fn canonical_key_replacement_before_the_first_boundary_is_not_a_hidden_fact() {
    let mut p = Pair::new();
    p.input("(Bop (Add) (V 0) (V 2)) (union (V 2) (V 1)) (set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3))");
    p.cell.audit_mirror().unwrap();
    p.run(SCHEDULE);
    assert_eq!(p.cell.report()["joint"]["stats"]["macro_instances"], 1);
    assert_eq!(p.emitted(), 0);
}

#[test]
fn shared_cpu_and_gpu_receive_joint_macro_rows_and_handback() {
    use cell_assisted_egglog::table_live;
    let worker = std::env::var_os("CELL_MATRIX_WORKER");
    for gpu in [false, true] {
        if gpu && worker.is_none() {
            continue;
        }
        let source = format!(
            "{SOURCE}\n(relation Seen (Expr)) (ruleset observe) (rule ((PotentialExtractedExpr e t)) ((Seen e)) :ruleset observe)"
        );
        let (mut base, observer) = Runtime::new_joint(&source).unwrap().detach_joint().unwrap();
        let schedule = format!("{SCHEDULE}\n(run observe 1)");
        let commands = base.parse_program(None, &schedule).unwrap();
        let mut native = base.clone();
        let mut r = if gpu {
            table_live::Runtime::new(
                base.clone(),
                &commands,
                std::path::Path::new(worker.as_ref().unwrap()),
                true,
            )
            .unwrap()
        } else {
            table_live::Runtime::new_cpu(base.clone(), &commands).unwrap()
        };
        r.set_saturation_hook(Box::new(observer));
        for input in [
            "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3)) (Bop (Add) (V 0) (V 1)) (Bop (Mul) (V 0) (V 1))",
            "(Bop (Add) (Bop (Mul) (V 0) (V 1)) (V 0))",
            "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 8) 1))",
            "(union (V 0) (V 1))",
            "",
        ] {
            native.parse_and_run_program(None, input).unwrap();
            r.input(input).unwrap();
            native.run_program(commands.clone()).unwrap();
            r.run(&commands).unwrap();
            compare::equal(&base, &native, r.engine()).unwrap();
        }
        assert!(
            r.report()["runtime"]["skipped_saturations"]
                .as_u64()
                .unwrap()
                >= 2,
            "{}",
            r.report()
        );
        assert!(
            r.report()["saturation_diagnostics"]["joint"]["stats"]["dataflow_boundaries"]
                .as_u64()
                .unwrap()
                > 0,
            "{}",
            r.report()
        );
        let noops: u64 = r.report()["saturation_diagnostics"]["programs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["identity_noop_seeds"].as_u64().unwrap())
            .sum();
        let closed_noops = r.report()["saturation_diagnostics"]["joint"]["comparison_closure"]
            ["identity_noop_seeds"].as_u64().unwrap_or(0);
        assert!(noops + closed_noops > 0, "{}", r.report());
        let mut handed = r.into_engine();
        for set in ["terms", "terms-helpers", "terms-helpers-helpers"] {
            assert_eq!(
                handed
                    .step_rules(set)
                    .unwrap()
                    .num_matches_per_rule
                    .values()
                    .sum::<usize>(),
                0
            );
        }
    }
}

#[test]
fn dominated_candidates_reopen_after_external_delete_or_cost_alias() {
    for alias in [false, true] {
        let mut p = Pair::new();
        p.input("(set (ExtractedExpr (V 9)) (TCPair (TermLeaf 9) 3)) (PotentialExtractedExpr (V 9) (TCPair (TermLeaf 8) 5))");
        p.run_ordered(SCHEDULE);
        let report = p.cell.report();
        let retired: u64 = report["programs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["retired_guard_seeds"].as_u64().unwrap())
            .sum();
        assert!(retired > 0, "{report}");
        let uniform = report["joint"]["stats"]["uniform_cost_boundaries"]
            .as_u64()
            .unwrap();
        if alias {
            p.input("(union (TCPair (TermLeaf 9) 3) (TCPair (TermLeaf 19) 9))");
        } else {
            p.input("(delete (ExtractedExpr (V 9))) (PotentialExtractedExpr (V 9) (TCPair (TermLeaf 7) 7))");
        }
        let before = p.emitted();
        p.run_ordered(SCHEDULE);
        assert!(p.emitted() > before);
        if alias {
            assert_eq!(
                p.cell.report()["joint"]["stats"]["uniform_cost_boundaries"],
                uniform
            );
        } else {
            p.cell
                .engine()
                .clone()
                .parse_and_run_program(
                    None,
                    "(check (= (ExtractedExpr (V 9)) (TCPair (TermLeaf 8) 5)))",
                )
                .unwrap();
        }
    }
}

#[test]
fn a_pending_smaller_alias_cannot_certify_an_immutable_cost() {
    let mut p = Pair::new();
    p.input("(set (ExtractedExpr (V 9)) (TCPair (TermLeaf 3) 3)) (union (TCPair (TermLeaf 3) 3) (Smaller (TCPair (TermLeaf 8) 8) (TCPair (TermLeaf 10) 10))) (PotentialExtractedExpr (V 9) (TCPair (TermLeaf 5) 5))");
    p.run_ordered(SCHEDULE);
    assert_eq!(
        p.cell.report()["joint"]["stats"]["uniform_cost_boundaries"],
        0
    );
}

#[test]
fn full_gpu_dispatch_keeps_joint_skip_and_observes_macro_effects() {
    use cell_assisted_egglog::table_live;
    let Some(worker)=std::env::var_os("CELL_MATRIX_WORKER") else{return};
    let source=format!("{SOURCE}\n(relation Seen (Expr i64)) (ruleset boundary)\n(rule ((= tc (ExtractedExpr e)) (= tc (TCPair t n)) (> n 0) (= m (+ n 1))) ((Seen e m)) :ruleset boundary)");
    let (base,observer)=Runtime::new_joint(&source).unwrap().detach_joint().unwrap();
    let commands=base.clone().parse_program(None,&format!("{SCHEDULE} (run boundary 1)")).unwrap();
    let mut reference=table_live::Runtime::new_full(base.clone(),&commands,None,false,None).unwrap();
    let mut cell=table_live::Runtime::new_full(base.clone(),&commands,Some(std::path::Path::new(&worker)),true,None).unwrap();
    cell.set_saturation_hook(Box::new(observer));
    for input in [
        "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2)) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3)) (Bop (Add) (V 0) (V 1))",
        "(Bop (Mul) (Bop (Add) (V 0) (V 1)) (V 0))",
        "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 9) 1))",
        "(union (V 0) (V 1))",
        "",
    ] {
        reference.input(input).unwrap();cell.input(input).unwrap();
        reference.run(&commands).unwrap();cell.run(&commands).unwrap();
        compare::equal(&base,reference.engine(),cell.engine()).unwrap();
    }
    let r=cell.report();
    assert!(r["runtime"]["skipped_saturations"].as_u64().unwrap()>0);
    assert!(r["rules"].as_array().unwrap().iter().any(|q|q["ruleset"]=="boundary" && q["stats"]["emitted_bindings"].as_u64().unwrap()>0));
    assert_eq!(r["extended_queries"],true);
    assert_eq!(r["cell_query_reuse"],true);
}

#[test]
fn credit_baseline_keeps_dataflow_and_ports_while_only_joint_proofs_change() {
    let mut yes=Runtime::new_joint(SOURCE).unwrap();
    let mut no=Runtime::new_joint(SOURCE).unwrap();
    yes.set_joint_proofs(true).unwrap();no.set_joint_proofs(false).unwrap();
    let base=yes.engine().clone();
    let schedule=base.clone().parse_program(None,SCHEDULE).unwrap();
    for input in [
        "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 8)) (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3)) (Bop (Add) (V 0) (V 1))",
        "(PotentialExtractedExpr (V 0) (TCPair (TermLeaf 3) 2))",
        "(Bop (Mul) (Bop (Add) (V 0) (V 1)) (V 1))",
        "(union (TCPair (TermLeaf 1) 3) (TCPair (TermLeaf 7) 3))",
    ] {
        yes.input(input).unwrap();no.input(input).unwrap();
        yes.run(&schedule).unwrap();no.run(&schedule).unwrap();
        compare::equal(&base,yes.engine(),no.engine()).unwrap();
        yes.audit_mirror().unwrap();no.audit_mirror().unwrap();
    }
    let a=yes.report();let b=no.report();
    assert_eq!(a["port_reuse"]["enabled"],true);assert_eq!(b["port_reuse"]["enabled"],true);
    assert!(b["joint"]["stats"]["dataflow_boundaries"].as_u64().unwrap()>0);
    assert_eq!(b["joint"]["stats"]["macro_instances"],0);
    assert_eq!(b["joint"]["stats"]["uniform_cost_boundaries"],0);
    assert_eq!(b["joint"]["comparison_closure"]["saturations"],0);
    assert_eq!(b["programs"].as_array().unwrap().iter().map(|p|p["retired_guard_seeds"].as_u64().unwrap()).sum::<u64>(),0);
    assert!(a["joint"]["comparison_closure"]["saturations"].as_u64().unwrap()>0);
    // Turning the proof back on cannot revive a stale closure certificate.
    no.set_joint_proofs(true).unwrap();no.run(&schedule).unwrap();
    compare::equal(&base,yes.engine(),no.engine()).unwrap();
}

#[test]
fn selection_closure_is_discovered_after_renaming_every_domain_role() {
    use cell_assisted_egglog::parameterized_cell::Runtime;
    let mut source=include_str!("../experiments/parameterized_cell/terms.egg").to_string();
    let mut input=include_str!("../experiments/parameterized_cell/competing_updates.egg").to_string();
    for (a,b) in [("terms-helpers-helpers","select-pass"),("terms-helpers","update-pass"),("terms","produce-pass"),
        ("PotentialExtractedExpr","Proposal"),("ExtractedExpr","Chosen"),("TCPair","Scored"),("Smaller","SelectLow"),("NoTerm","Unresolved")] {
        source=source.replace(a,b);input=input.replace(a,b);
    }
    let mut runtime=Runtime::new_joint(&source).unwrap();
    let commands=runtime.engine().clone().parse_program(None,&input).unwrap();
    let mut pending=String::new();
    for c in commands {
        if matches!(c,egglog::ast::Command::RunSchedule(_)) {
            runtime.input(&pending).unwrap();pending.clear();runtime.run(&[c]).unwrap();runtime.audit_mirror().unwrap();
        } else {pending.push_str(&format!("{c}\n"));}
    }
    if !pending.is_empty(){runtime.input(&pending).unwrap();}
    assert_eq!(runtime.report()["joint"]["roles"]["compare"],"SelectLow");
    assert_eq!(runtime.report()["joint"]["roles"]["sets"][0],"produce-pass");
}
