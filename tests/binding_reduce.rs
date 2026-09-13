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
                site: 0,
                arguments: vec!["shift"],
            },
            Recur {
                name: "right",
                site: 1,
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
      (let left (ERecur 0 (BCons (EInt 7) (BNil))))
      (let right (ERecur 1 (BCons (EInt 0) (BNil))))
      (check (LayerEndpointView f
        (BResult (BCons (EAdd left left) (BCons right (BCons (EAdd (EAdd left left) right) (BNil))))
          (BCons (EApply "union" (BCons left (BCons right (BNil)))) (BNil)))))
      (fail (check (= left right)))
      (fail (check (= (EAdd left left) left)))
    "#)).unwrap();
    // Only two distinct, fully instantiated recursion sites, despite repeated uses.
    let mut calls = std::collections::BTreeSet::new();
    e.function_for_each("ERecur", |r| {
        let (term, _) = e
            .extract_value_to_string(e.get_sort_by_name("EndpointExpr").unwrap(), r.vals[2])
            .unwrap();
        if term == "(ERecur 0 (BCons (EInt 7) (BNil)))"
            || term == "(ERecur 1 (BCons (EInt 0) (BNil)))"
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
      (check (= a (BResult (BCons (EInt 2) (BNil)) (BNil))))
      (check (= b (BResult (BCons (EInt 3) (BNil)) (BNil))))
      (fail (check (= a b)))
      (fail (check (= (EInt 2) (EInt 3))))
    "#
        ),
    )
    .unwrap();
}

#[test]
fn rejects_forward_redefinition_and_ambiguous_sites() {
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
                    site: 0,
                    arguments: vec![]
                },
                Recur {
                    name: "b",
                    site: 0,
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
