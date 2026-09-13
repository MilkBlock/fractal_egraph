use egg_layout::binding_reduce::{Definition::*, compile};
use egglog::EGraph;

fn engine() -> EGraph {
    let mut e = EGraph::default();
    e.parse_and_run_program(
        None,
        include_str!("../experiments/tier1_effects/tier1_rule_comb_ir.egg"),
    )
    .unwrap();
    e.parse_and_run_program(None, include_str!("../rules/tier2.egg"))
        .unwrap();
    e.parse_and_run_program(None, include_str!("../rules/higher.egg"))
        .unwrap();
    e.parse_and_run_program(None, include_str!("../rules/recursive_patterns.egg"))
        .unwrap();
    e
}

#[test]
fn two_sites_shared_uses_and_effects_reach_native_reduce() {
    let dag = compile(
        &["x", "extra"],
        &[
            Let {
                name: "shift",
                expression: "(EAdd x 0)",
            },
            Recur {
                name: "left",
                callee: "F",
                output: 0,
                arguments: vec!["shift"],
            },
            Recur {
                name: "right",
                callee: "F",
                output: 1,
                arguments: vec!["(EMul extra 0)"],
            },
            Let {
                name: "twice",
                expression: "(EAdd left left)",
            },
        ],
        &["twice", "right", "(EAdd twice right)"],
        &["(union left right)"],
    )
    .unwrap();
    let mut e = engine();
    e.parse_and_run_program(None, &format!(r#"
      (let step (RefinedExtension (Extend "r" (End) (Schema "explicit-interface")) "aliases"))
      (let pattern (RecursivePattern step (RecursivePort 0 step (RecursivePort 1 step (NoRecursivePorts)))))
      (let f (FractalComb (SparseExtent (NoExpansionNodes)) pattern (Empty) (RNil)))
      (LayerReductionInput f {dag} (BCons (EInt 7) (BCons (EInt 9) (BNil))))
      (run-schedule (saturate (run higher)))
      (run-schedule (saturate (run endpoint-reduce)))
      (let left (ERecur "F" 0 (BCons (EInt 7) (BNil))))
      (let right (ERecur "F" 1 (BCons (EInt 0) (BNil))))
      (check (LayerEndpointView f
        (BResult (BCons (EAdd left left) (BCons right (BCons (EAdd (EAdd left left) right) (BNil))))
          (BNil) (BCons (EApply "union" (BCons left (BCons right (BNil)))) (BNil)))))
      (fail (check (= left right)))
      (fail (check (= (EAdd left left) left)))
    "#)).unwrap();
    // Only two distinct, fully instantiated recursion sites, despite repeated uses.
    let mut calls = std::collections::BTreeSet::new();
    e.function_for_each("ERecur", |r| {
        let (term, _) = e
            .extract_value_to_string(e.get_sort_by_name("EndpointExpr").unwrap(), r.vals[3])
            .unwrap();
        if term == "(ERecur \"F\" 0 (BCons (EInt 7) (BNil)))"
            || term == "(ERecur \"F\" 1 (BCons (EInt 0) (BNil)))"
        {
            calls.insert(term);
        }
    })
    .unwrap();
    assert_eq!(calls.len(), 2);
}

#[test]
fn alpha_renaming_shares_templates_but_environments_do_not_leak() {
    let a = compile(
        &["x"],
        &[Let {
            name: "y",
            expression: "(EAdd x 0)",
        }],
        &["y"],
        &[],
    )
    .unwrap();
    let b = compile(
        &["a"],
        &[Let {
            name: "b",
            expression: "(EAdd a 0)",
        }],
        &["b"],
        &[],
    )
    .unwrap();
    assert_eq!(a, b);
    let mut e = engine();
    e.parse_and_run_program(
        None,
        &format!(
            r#"
      (let a (ReduceDag {a} (BCons (EInt 2) (BNil))))
      (let b (ReduceDag {b} (BCons (EInt 3) (BNil))))
      (run-schedule (saturate (run endpoint-reduce)))
      (check (= a (BResult (BCons (EInt 2) (BNil)) (BNil) (BNil))))
      (check (= b (BResult (BCons (EInt 3) (BNil)) (BNil) (BNil))))
      (fail (check (= a b)))
      (fail (check (= (EInt 2) (EInt 3))))
    "#
        ),
    )
    .unwrap();
}

#[test]
fn rejects_forward_redefinition_and_invalid_recursion() {
    assert!(
        compile(
            &[],
            &[Let {
                name: "x",
                expression: "y"
            }],
            &["x"],
            &[]
        )
        .is_err()
    );
    assert!(
        compile(
            &["x"],
            &[Let {
                name: "x",
                expression: "1"
            }],
            &["x"],
            &[]
        )
        .is_err()
    );
    assert!(
        compile(
            &[],
            &[
                Recur {
                    name: "a",
                    callee: "F",
                    output: 0,
                    arguments: vec![]
                },
                Recur {
                    name: "b",
                    callee: "F",
                    output: -1,
                    arguments: vec![]
                },
            ],
            &["a", "b"],
            &[]
        )
        .is_err()
    );
}

#[test]
fn repeated_shared_bindings_have_linear_encoding_size() {
    let names: Vec<_> = (0..40).map(|i| format!("v{i}")).collect();
    let expressions: Vec<_> = (0..40)
        .map(|i| {
            let prev = if i == 0 { "x" } else { &names[i - 1] };
            format!("(EAdd {prev} {prev})")
        })
        .collect();
    let defs: Vec<_> = (0..40)
        .map(|i| Let {
            name: &names[i],
            expression: &expressions[i],
        })
        .collect();
    let dag = compile(&["x"], &defs, &[&names[39]], &[]).unwrap();
    assert_eq!(dag.matches("(BLet ").count(), 40);
    assert!(dag.len() < 2500, "must not expand a shared DAG into a tree");
}

#[test]
fn independent_schedules_and_names_have_one_canonical_dag() {
    let a = compile(
        &["x", "y"],
        &[
            Let {
                name: "a",
                expression: "(EAdd x 1)",
            },
            Let {
                name: "b",
                expression: "(EMul y 2)",
            },
        ],
        &["(ESub a b)"],
        &[],
    )
    .unwrap();
    let b = compile(
        &["u", "v"],
        &[
            Let {
                name: "right",
                expression: "(EMul v 2)",
            },
            Let {
                name: "left",
                expression: "(EAdd u 1)",
            },
        ],
        &["(ESub left right)"],
        &[],
    )
    .unwrap();
    assert_eq!(a, b);
    assert_ne!(
        compile(&["x", "y"], &[], &["(ESub x x)"], &[]).unwrap(),
        compile(&["x", "y"], &[], &["(ESub x y)"], &[]).unwrap()
    );
    // Pure repeated calls share; changing either callee or output port does not.
    let one = compile(
        &["x"],
        &[Recur {
            name: "a",
            callee: "F",
            output: 0,
            arguments: vec!["x"],
        }],
        &["a", "a"],
        &[],
    )
    .unwrap();
    let two = compile(
        &["x"],
        &[
            Recur {
                name: "a",
                callee: "F",
                output: 0,
                arguments: vec!["x"],
            },
            Recur {
                name: "b",
                callee: "F",
                output: 0,
                arguments: vec!["x"],
            },
        ],
        &["a", "b"],
        &[],
    )
    .unwrap();
    assert_eq!(one, two);
    let mut e = engine();
    e.parse_and_run_program(
        None,
        r#"
      (let xs (BCons (EInt 1) (BNil)))
      (ERecur "F" 0 xs) (ERecur "G" 0 xs) (ERecur "F" 1 xs)
      (run-schedule (saturate (run endpoint-reduce)))
      (fail (check (= (ERecur "F" 0 xs) (ERecur "G" 0 xs))))
      (fail (check (= (ERecur "F" 0 xs) (ERecur "F" 1 xs))))
    "#,
    )
    .unwrap();
}

#[test]
fn result_wrapper_is_not_completion_and_contracts_are_not_executed() {
    let mut e = engine();
    e.parse_and_run_program(
        None,
        r#"
      (let bad (ReduceDag (BDag (BXCons (BParam 8) (BXNil)) (BXNil) (BXNil)) (BNil)))
      (run-schedule (saturate (run endpoint-reduce)))
      (check (= bad (BResult (BCons (ParameterAt 8 (BNil)) (BNil)) (BNil) (BNil))))
      (fail (check (CompleteBinding bad)))
    "#,
    )
    .unwrap();
    let dag = egg_layout::binding_reduce::compile_contract(
        &["x", "y"],
        &[],
        &["x"],
        &["(require-eq x y)", "(require-predicate (positive x))"],
        &["(effect-union x y)"],
    )
    .unwrap();
    e.parse_and_run_program(None,&format!(r#"
      (let ok (ReduceDag {dag} (BCons (EInt 2) (BCons (EInt 3) (BNil)))))
      (run-schedule (saturate (run endpoint-reduce)))
      (check (CompleteBinding ok))
      (check (= ok (BResult (BCons (EInt 2) (BNil))
        (BCons (EApply "require-eq" (BCons (EInt 2) (BCons (EInt 3) (BNil))))
          (BCons (EApply "require-predicate" (BCons (EApply "positive" (BCons (EInt 2) (BNil))) (BNil))) (BNil)))
        (BCons (EApply "effect-union" (BCons (EInt 2) (BCons (EInt 3) (BNil)))) (BNil)))))
      (fail (check (= (EInt 2) (EInt 3))))
    "#)).unwrap();
}
