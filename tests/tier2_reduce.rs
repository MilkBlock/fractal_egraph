use egglog::EGraph;
fn setup() -> EGraph {
    let mut e = EGraph::default();
    e.parse_and_run_program(None, include_str!("../experiments/tier2/reduce.egg"))
        .unwrap();
    e
}
#[test]
fn native_extraction_prefers_closed_forms_without_deleting_reduce() {
    let mut e = setup();
    let mut before = std::collections::BTreeMap::new();
    e.function_for_each("ReduceExample", |r| {
        let name = e
            .value_to_base::<egglog::sort::S>(r.vals[0])
            .as_str()
            .to_owned();
        let (_, c) = e
            .extract_value_to_string(e.get_sort_by_name("EndpointExpr").unwrap(), r.vals[1])
            .unwrap();
        before.insert(name, c);
    })
    .unwrap();
    e.parse_and_run_program(None, "(run-schedule (saturate (run endpoint-reduce)))")
        .unwrap();
    e.function_for_each("ReduceExample", |r| {
        let name = e.value_to_base::<egglog::sort::S>(r.vals[0]);
        let (term, c) = e
            .extract_value_to_string(e.get_sort_by_name("EndpointExpr").unwrap(), r.vals[1])
            .unwrap();
        if ["constant-add", "arithmetic-add", "constant-multiply"].contains(&name.as_str()) {
            assert!(!term.contains("(Reduce "));
            assert!(c < before[name.as_str()]);
        } else {
            assert!(term.contains("(Reduce "));
        }
    })
    .unwrap();
    let mut residuals = 0;
    e.function_for_each("Reduce", |_| residuals += 1).unwrap();
    assert_eq!(residuals, 6);
}
#[test]
fn arithmetic_closed_form_agrees_with_explicit_finite_sums() {
    let mut e = setup();
    e.parse_and_run_program(
        None,
        r#"
 (relation Eval (EndpointExpr i64))
 (ruleset eval)
 (rule ((= e (EInt n))) ((Eval e n)) :ruleset eval)
 (rule ((= e (EAdd a b)) (Eval a x) (Eval b y)) ((Eval e (+ x y))) :ruleset eval)
 (rule ((= e (ESub a b)) (Eval a x) (Eval b y)) ((Eval e (- x y))) :ruleset eval)
 (rule ((= e (EMul a b)) (Eval a x) (Eval b y)) ((Eval e (* x y))) :ruleset eval)
 (rule ((= e (EDiv a b)) (Eval a x) (Eval b y) (!= y 0)) ((Eval e (/ x y))) :ruleset eval)
 "#,
    )
    .unwrap();
    for first in [-4, 0, 3] {
        for step in [-2, 0, 5] {
            for k in 0..9 {
                let term = format!(
                    "(Reduce (AddArithmetic (EInt {first}) (EInt {step})) (EInt {k}) (EInt 7))"
                );
                let expected = 7 + (0..k).map(|i| first + i * step).sum::<i64>();
                e.parse_and_run_program(None,&format!("{term}\n(run-schedule (saturate (run endpoint-reduce)))\n(run-schedule (saturate (run eval)))\n(check (Eval {term} {expected}))")).unwrap();
            }
        }
    }
}
#[test]
fn higher_rule_receives_explicit_reduce_endpoint() {
    let mut e = EGraph::default();
    e.parse_and_run_program(None, include_str!("../experiments/tier2/ir.egg"))
        .unwrap();
    e.parse_and_run_program(
        None,
        include_str!("../experiments/tier1_effects/tier1_rule_comb_ir.egg"),
    )
    .unwrap();
    e.parse_and_run_program(None, include_str!("../experiments/tier2/higher_ir.egg"))
        .unwrap();
    e.parse_and_run_program(
        None,
        r#"
 (let h (FractalComb 3 (Extend "r" (End) (Schema "supplied-additive-summary")) (Empty) (RNil)))
 (let t (ECall "f" (ESymbol "terminal")))
 (ReductionInput h (AddConstant (ESymbol "m")) t)
 (run-schedule (saturate (run higher)))
 (check (EndpointView h (Reduce (AddConstant (ESymbol "m")) (EInt 3) t)))
 (run-schedule (saturate (run endpoint-reduce)))
 (check (EndpointView h (EAdd t (EMul (EInt 3) (ESymbol "m")))))
 "#,
    )
    .unwrap();
}
