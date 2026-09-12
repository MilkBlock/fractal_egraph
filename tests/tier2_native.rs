#[test]
fn coordinate_composition_and_counterexamples_are_native() {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None, include_str!("../experiments/tier2/ir.egg"))
        .unwrap();
    eg.parse_and_run_program(
        None,
        r#"
      (let t (Translate -2 3))
      (let c (Compose t t)) (let p (Power t 7))
      (Proposed "test" t)
      (Sample "test" 12 0 10 3)
      (Sample "test" 10 3 8 7)
      (CheckLinear t 3 2)
      (run-schedule (saturate (run tier2)))
      (check (= c (Translate -4 6)))
      (check (= p (Translate -14 21)))
      (check (PreservesLinear t 3 2))
      (check (Fits "test" 12 0 10 3))
      (check (Counterexample "test" 10 3 8 7))
    "#,
    )
    .unwrap();
}
#[test]
fn repeats_must_follow_the_same_parent_slot() {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None, include_str!("../experiments/tier2/ir.egg"))
        .unwrap();
    eg.parse_and_run_program(
        None,
        r#"
      (let e (Extend "r" (End) (Schema "effect")))
      (At 0 e) (At 1 e) (At 2 e) (At 3 e)
      (Before 0 1 0) (Before 1 2 1) (Before 1 3 0)
      (run-schedule (saturate (run tier2)))
      (check (Repeat 0 3 e 0 3))
    "#,
    )
    .unwrap();
    let mut mixed = false;
    eg.function_for_each("Repeat", |r| {
        if eg.value_to_base::<i64>(r.vals[0]) == 0 && eg.value_to_base::<i64>(r.vals[1]) == 2 {
            mixed = true;
        }
    })
    .unwrap();
    assert!(
        !mixed,
        "different relative parent slots cannot form a homogeneous run"
    );
}

#[test]
fn higher_rule_counts_actual_steps_and_preserves_initial_context() {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None, include_str!("../experiments/tier2/ir.egg"))
        .unwrap();
    eg.parse_and_run_program(
        None,
        "(datatype Comb (Base) (Step Comb)) (datatype RelativeBinding (Bind))",
    )
    .unwrap();
    eg.parse_and_run_program(None, include_str!("../experiments/tier2/higher_ir.egg"))
        .unwrap();
    eg.parse_and_run_program(
        None,
        r#"
      (let r (Extend "r" (End) (Schema "effect-r")))
      (let s (Extend "r" (End) (Schema "different-effect")))
      (let start (Base)) (let mid (Step start)) (let out (Step mid))
      (UnaryStep start mid r (Bind)) (UnaryStep mid out r (Bind))
      (UnaryStep out (Step out) s (Bind))
      (run-schedule (saturate (run higher)))
      (check (Represents (HigherRule 2 r start (Bind)) out))
    "#,
    )
    .unwrap();
    let mut count = 0;
    eg.function_for_each("HigherRule", |r| {
        assert_eq!(eg.value_to_base::<i64>(r.vals[0]), 2);
        count += 1;
    })
    .unwrap();
    assert_eq!(count, 1);
}
