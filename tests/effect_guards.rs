#[allow(dead_code)]
#[path = "../examples/support/live_table_bench.rs"]
mod compare;
use egglog::{EGraph, cell_assist::CellEffectRuleset};
const DECL: &str = r#"
(datatype N (V i64))
(relation A (N N)) (relation Q (N N)) (relation B (N)) (relation C (N))
(relation Edge (N N)) (relation Out (N N)) (relation Wipe (N))
(function F (N) N :merge old) (ruleset r)
"#;
struct Pair {
    base: EGraph,
    native: EGraph,
    guarded: EGraph,
    plan: CellEffectRuleset,
}
impl Pair {
    fn new(rules: &str) -> Self {
        let mut base = EGraph::default();
        base.parse_and_run_program(None, &format!("{DECL}\n{rules}"))
            .unwrap();
        let plan = base.prepare_cell_effect_ruleset("r").unwrap().unwrap();
        Self {
            native: base.clone(),
            guarded: base.clone(),
            base,
            plan,
        }
    }
    fn input(&mut self, s: &str) {
        self.native.parse_and_run_program(None, s).unwrap();
        self.guarded.parse_and_run_program(None, s).unwrap();
    }
    fn run(&mut self) -> (usize, usize) {
        let a = self.native.step_rules("r").unwrap();
        let b = self
            .guarded
            .step_rules_with_cell_effects(&self.plan)
            .unwrap();
        assert_eq!(a.updated, b.updated);
        compare::equal(&self.base, &self.native, &self.guarded).unwrap();
        (
            a.num_matches_per_rule.values().sum(),
            b.num_matches_per_rule.values().sum(),
        )
    }
}
#[test]
fn a_present_head_rejects_queries_and_a_deleted_head_is_not_a_new_trigger() {
    let mut p = Pair::new("(rule ((A x y) (Q y z)) ((B x)) :ruleset r :name \"derive\")");
    p.input("(A (V 0) (V 1)) (Q (V 1) (V 2)) (B (V 0))");
    assert_eq!(p.run(), (1, 0));
    p.input("(A (V 3) (V 1))");
    assert_eq!(p.run(), (1, 1));
    p.input("(delete (B (V 0)))");
    assert_eq!(p.run(), (0, 0));
    p.guarded
        .parse_and_run_program(None, "(fail (check (B (V 0))))")
        .unwrap();
    p.input("(Q (V 1) (V 4))");
    assert_eq!(p.run(), (2, 1));
    p.guarded
        .parse_and_run_program(None, "(check (B (V 0))) (check (B (V 3)))")
        .unwrap();
    // Handback keeps the ORIGINAL cursors, not just the private guarded ones.
    assert_eq!(
        p.guarded
            .step_rules("r")
            .unwrap()
            .num_matches_per_rule
            .values()
            .sum::<usize>(),
        0
    );
}
#[test]
fn every_head_lookup_must_exist_and_union_witnesses_must_be_equal() {
    let mut p = Pair::new("(rule ((A x y)) ((B x) (C y) (union x y)) :ruleset r :name \"multi\")");
    p.input("(A (V 0) (V 1)) (B (V 0))");
    assert_eq!(p.run(), (1, 1));
    p.run();
    p.input("(A (V 2) (V 2)) (B (V 2)) (C (V 2))");
    let (a, b) = p.run();
    assert!(a > b);
}
#[test]
fn head_deletion_in_the_same_native_batch_is_not_undone() {
    let mut p = Pair::new(
        "(rule ((Wipe x)) ((delete (B x))) :ruleset r :name \"wipe\") (rule ((A x y)) ((B x)) :ruleset r :name \"emit\")",
    );
    assert_eq!(p.plan.guarded_rules(), ["emit"]);
    p.input("(A (V 0) (V 1)) (Wipe (V 0)) (B (V 0))");
    p.run();
    p.guarded
        .parse_and_run_program(None, "(fail (check (B (V 0))))")
        .unwrap();
}
#[test]
fn primitive_rhs_errors_keep_native_execution() {
    let mut p = Pair::new(
        "(rule ((A x y)) ((B x)) :ruleset r :name \"emit\") (rule ((A x y) (= x (V n))) ((C (V (+ n 1)))) :ruleset r :name \"overflow\")",
    );
    assert_eq!(p.plan.guarded_rules(), ["emit"]);
    p.input("(A (V 9223372036854775807) (V 1)) (B (V 9223372036854775807))");
    let a = p.native.step_rules("r").unwrap_err().to_string();
    let b = p
        .guarded
        .step_rules_with_cell_effects(&p.plan)
        .unwrap_err()
        .to_string();
    assert_eq!(a, b);
}

#[test]
fn union_can_enable_new_bindings_after_a_guarded_run() {
    let mut p =
        Pair::new("(rule ((A x middle) (Q middle y)) ((Out x y)) :ruleset r :name \"join\")");
    p.input("(A (V 0) (V 1)) (Q (V 2) (V 3))");
    assert_eq!(p.run(), (0, 0));
    p.input("(union (V 1) (V 2))");
    assert_eq!(p.run(), (1, 1));
    p.input("(A (V 4) (V 2))");
    assert_eq!(p.run(), (1, 1));
}
#[test]
fn function_updates_and_panic_are_not_discarded_as_equal_value_writes() {
    let mut p = Pair::new(
        "(rule ((A x y)) ((B x)) :ruleset r :name \"emit\") (rule ((A x y)) ((set (F x) y)) :ruleset r :name \"set\")",
    );
    assert_eq!(p.plan.guarded_rules(), ["emit"]);
    p.input("(A (V 0) (V 1)) (B (V 0))");
    p.run();
    let mut e = EGraph::default();
    e.parse_and_run_program(
        None,
        &format!("{DECL} (rule ((A x y)) ((panic \"must run\")) :ruleset r :name \"panic\")"),
    )
    .unwrap();
    assert!(e.prepare_cell_effect_ruleset("r").unwrap().is_none());
}
#[test]
fn hidden_constructor_heads_keep_the_original_visibility() {
    let mut p = Pair::new("(rule ((A x y)) ((B x)) :ruleset r :name \"emit\")");
    p.input("(A (V 0) (V 1)) (B (V 0)) (subsume (B (V 0)))");
    p.run();
}
#[test]
fn new_merge_tables_invalidate_the_compiled_effect_environment() {
    let mut p = Pair::new("(rule ((A x y)) ((B x)) :ruleset r :name \"emit\")");
    let before = p.guarded.cell_rule_environment_stamp();
    p.guarded
        .parse_and_run_program(None, "(function Foreign (N) N :merge (V 42))")
        .unwrap();
    assert_ne!(before, p.guarded.cell_rule_environment_stamp());
    assert!(p.guarded.step_rules_with_cell_effects(&p.plan).is_err());
}

#[test]
fn a_completed_head_prunes_before_the_remaining_triangle_join() {
    let mut p =
        Pair::new("(rule ((A x y) (Q y z) (Edge z x)) ((B x)) :ruleset r :name \"triangle\")");
    let mut input = String::from("(A (V 0) (V 1)) (A (V 9) (V 1)) (B (V 0))");
    for i in 20..84 {
        input += &format!(" (Q (V 1) (V {i})) (Edge (V {i}) (V 0)) (Edge (V {i}) (V 9))");
    }
    p.input(&input);
    let enabled = egglog::set_probe_counts_enabled(true);
    let counts = || {
        egglog::probe_counts()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let before = counts();
    p.native.step_rules("r").unwrap();
    let middle = counts();
    p.guarded.step_rules_with_cell_effects(&p.plan).unwrap();
    let after = counts();
    egglog::set_probe_counts_enabled(enabled);
    compare::equal(&p.base, &p.native, &p.guarded).unwrap();
    assert!(
        after["effect_prefix_early_rejections"] > middle["effect_prefix_early_rejections"],
        "{before:?} {middle:?} {after:?}"
    );
    assert!(
        after["probes"] - middle["probes"] < middle["probes"] - before["probes"],
        "{before:?} {middle:?} {after:?}"
    );
}

#[test]
fn prefix_backtracking_cannot_reuse_a_previous_branches_output_binding() {
    let mut p =
        Pair::new("(rule ((A x y) (Q y z) (Edge z x)) ((Out x z)) :ruleset r :name \"branch\")");
    p.input("(A (V 0) (V 1)) (Q (V 1) (V 2)) (Q (V 1) (V 3)) (Edge (V 2) (V 0)) (Edge (V 3) (V 0)) (Out (V 0) (V 2))");
    p.run();
    p.guarded
        .parse_and_run_program(None, "(check (Out (V 0) (V 3)))")
        .unwrap();
    p.input("(A (V 9) (V 1)) (Edge (V 2) (V 9)) (Edge (V 3) (V 9))");
    p.run();
    p.guarded
        .parse_and_run_program(None, "(check (Out (V 9) (V 2))) (check (Out (V 9) (V 3)))")
        .unwrap();
}

#[test]
fn multiple_potential_effects_keep_native_batch_boundaries() {
    let p = Pair::new("(rule ((A x y)) ((B x) (C y)) :ruleset r :name \"two\")");
    assert!(p.plan.prefix_rules().is_empty());
    assert_eq!(p.plan.guarded_rules(), ["two"]);
}

#[test]
fn rhs_lookups_already_in_the_lhs_do_not_count_as_mutations() {
    let p = Pair::new("(rule ((A (V n) y)) ((B (V n))) :ruleset r :name \"nested\")");
    assert_eq!(p.plan.prefix_rules(), ["nested"]);
}
