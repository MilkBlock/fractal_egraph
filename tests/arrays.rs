use egglog::EGraph;
fn engine() -> EGraph {
    let mut e = EGraph::default();
    e.parse_and_run_program(None, include_str!("../rules/tier2.egg"))
        .unwrap();
    e
}
fn run(e: &mut EGraph, code: &str) {
    e.parse_and_run_program(None, code).unwrap();
    e.parse_and_run_program(None,"(run-schedule (saturate (seq (run endpoint-reduce) (run array-shape) (run array-laws) (run array-eval))))").unwrap();
}
#[test]
fn symbolic_sum_reuses_reduce_without_materializing_elements() {
    let mut e = engine();
    run(
        &mut e,
        r#"
       (let n (ECount "N"))
       (let a (RampArray (FiniteDomain n) (EInt 1) (EInt 1)))
       (let sum (ReduceArray (SumReduction) a))
    "#,
    );
    e.parse_and_run_program(
        None,
        r#"
       (check (= sum (EAdd n (EDiv (EMul n (ESub n (EInt 1))) (EInt 2)))))
       (check (ArrayDomain a (FiniteDomain n)))
    "#,
    )
    .unwrap();
    assert_eq!(e.get_size("ArrayAt"), 0);
    assert_eq!(e.get_size("ReducePrefix"), 0);
}

#[test]
fn binding_scopes_and_index_bounds_are_preserved() {
    let mut e = engine();
    run(
        &mut e,
        r#"
      (let body (BAdd (BParam 0) (BParam 1)))
      (let a (Tabulate (FiniteDomain (EInt 3)) body (BCons (EInt 10) (BNil))))
      (let b (Tabulate (FiniteDomain (EInt 3)) body (BCons (EInt 20) (BNil))))
      (let x (ArrayAt a 1)) (let y (ArrayAt b 1))
      (ArrayAt a -1) (ArrayAt a 3)
      (let invalid (SliceArray a 2 2)) (ArrayAt invalid 0)
      (let short (FillArray (FiniteDomain (EInt 2)) (EInt 1)))
      (let badzip (ZipArray (BinaryLambda (BMul (BParam 0) (BParam 1)) (BNil)) a short))
      (ArrayAt badzip 0)
    "#,
    );
    e.parse_and_run_program(
        None,
        r#"
      (check (= x (EInt 11))) (check (= y (EInt 21)))
      (fail (check (= x y)))
      (fail (check (ArrayIndex a -1))) (fail (check (ArrayIndex a 3)))
      (fail (check (ArrayIndex invalid 0))) (fail (check (ArrayIndex badzip 0)))
    "#,
    )
    .unwrap();
}

#[test]
fn map_fold_fusion_preserves_non_associative_order() {
    let mut e = engine();
    run(
        &mut e,
        r#"
      (let a (RampArray (FiniteDomain (EInt 3)) (EInt 1) (EInt 1)))
      (let inc (ScalarLambda (BAdd (BParam 0) (BInt 1)) (BNil)))
      (let sub (FoldLambda (BSub (BParam 0) (BParam 1)) (BNil)))
      (let mapped (MapArray inc a))
      (let result (FoldArray sub (EInt 10) mapped))
      (TryBlock a 2)
    "#,
    );
    assert_eq!(e.get_size("ArrayAt"), 0);
    assert_eq!(e.get_size("ReduceBlocks"), 0); // no unjustified associativity of subtraction
    e.parse_and_run_program(
        None,
        "(check (= result (FoldArray (MappedFold sub inc) (EInt 10) a)))",
    )
    .unwrap();
    run(&mut e, "(EvaluateFold mapped 3)");
    e.parse_and_run_program(None, "(check (= result (EInt 1)))")
        .unwrap();
}

#[test]
fn blocks_have_exact_tails_and_reductions_agree() {
    let mut e = engine();
    for n in [0_i64, 1, 5, 8, 17] {
        for size in [1_i64, 3, 8] {
            let a = format!("(RampArray (FiniteDomain (EInt {n})) (EInt 3) (EInt 2))");
            let blocks = format!("(Blocks {a} {size})");
            run(
                &mut e,
                &format!(
                    "(TryBlock {a} {size})\n(ReduceArray (SumReduction) {a})\n(FlattenBlocks {blocks})"
                ),
            );
            let count = (n + size - 1) / size;
            e.parse_and_run_program(None,&format!("(check (BlockCount {blocks} {count}))\n(check (= (FlattenBlocks {blocks}) {a}))\n(check (= (ReduceBlocks (SumReduction) {blocks}) (EInt {})))", n*3+n*(n-1))).unwrap();
            for i in 0..count {
                let block = format!("(BlockAt {blocks} {i})");
                let len = size.min(n - i * size);
                run(
                    &mut e,
                    &format!("(EvaluateFold {block} {size})\n(ReduceArray (SumReduction) {block})"),
                );
                let expected: i64 = (i * size..i * size + len).map(|j| 3 + 2 * j).sum();
                e.parse_and_run_program(None,&format!("(check (ArrayLength {block} {len}))\n(check (= (ReduceArray (SumReduction) {block}) (EInt {expected})))")).unwrap();
            }
        }
    }
    run(
        &mut e,
        "(TryBlock (FillArray (FiniteDomain (EInt 3)) (EInt 1)) 0)",
    );
    e.parse_and_run_program(
        None,
        "(fail (check (BlockCount (Blocks (FillArray (FiniteDomain (EInt 3)) (EInt 1)) 0) 1)))",
    )
    .unwrap();
}

#[test]
fn streams_require_finite_slices_and_symbolic_exp_sum_is_compact() {
    let mut e = engine();
    run(
        &mut e,
        r#"
      (let stream (RampArray (StreamDomain) (EInt 1) (EInt 1)))
      (let whole (ReduceArray (SumReduction) stream))
      (EvaluateFold stream 100)
      (let prefix (SliceArray stream 0 5))
      (let finite (ReduceArray (SumReduction) prefix)) (EvaluateFold prefix 5)
      (let n (ECount "N"))
      (let expfn (ScalarLambda (BExp (BParam 0)) (BNil)))
      (let sumexp (ReduceArray (SumReduction) (MapArray expfn (FillArray (FiniteDomain n) (EInt 0)))))
      (let bs (Blocks (InputArray "x" (FiniteDomain n)) 16))
    "#,
    );
    e.parse_and_run_program(
        None,
        r#"
      (check (= finite (EInt 15))) (check (= sumexp n))
      (check (BlockDomain bs (FiniteDomain (ECeilDiv n 16))))
      (fail (check (= whole (EInt 15))))
      (fail (check (ArrayLength stream 100)))
    "#,
    )
    .unwrap();
}

#[test]
fn fractal_endpoint_contract_reaches_arrays_and_reduce() {
    let mut e = EGraph::default();
    e.parse_and_run_program(
        None,
        include_str!("../research/legacy_tier1.egg"),
    )
    .unwrap();
    e.parse_and_run_program(None, include_str!("../rules/tier2.egg"))
        .unwrap();
    e.parse_and_run_program(None, include_str!("../rules/higher.egg"))
        .unwrap();
    run(
        &mut e,
        r#"
      (let f (FractalComb (Depth 5) (Extend "accumulate" (End) (Schema "caller-certified-array-summary")) (Empty) (RNil)))
      (let a (RampArray (FiniteDomain (EInt 5)) (EInt 1) (EInt 1)))
      (ArrayReductionSummary f (SumReduction) a)
    "#,
    );
    e.parse_and_run_program(None, "(run-schedule (saturate (run higher)))")
        .unwrap();
    run(&mut e, "");
    e.parse_and_run_program(None, "(check (EndpointView f (EInt 15)))")
        .unwrap();
}

#[test]
fn native_cli_exports_symbolic_arrays_without_element_expansion() {
    let out = std::env::temp_dir().join(format!("array-dsl-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["arrays", "experiments/arrays/basic.egg"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(report["element_queries"], 0);
    assert_eq!(report["fold_prefixes"], 0);
    assert_eq!(
        report["examples"]["sum-exp-zero"]["expression"],
        "(ECount \"N\")"
    );
    assert!(
        report["examples"]["fused-exp-sum"]["expression"]
            .as_str()
            .unwrap()
            .contains("FoldMap")
    );
    std::fs::remove_file(out).unwrap();
}

#[test]
fn binding_compiler_accepts_exp_and_max_in_the_array_theory() {
    let code =
        egg_layout::binding_reduce::compile(&["x"], &[], &["(EExp (EMax x 0))"], &[]).unwrap();
    let mut e = engine();
    run(
        &mut e,
        &format!("(let result (ReduceDag {code} (BCons (EInt 0) (BNil))))"),
    );
    e.parse_and_run_program(
        None,
        "(check (= result (BResult (BCons (EInt 1) (BNil)) (BNil) (BNil))))",
    )
    .unwrap();
}

#[test]
fn zip_reduce_fuses_and_empty_max_has_an_identity() {
    let mut e = engine();
    run(
        &mut e,
        r#"
      (let a (RampArray (FiniteDomain (EInt 3)) (EInt 1) (EInt 1)))
      (let b (FillArray (FiniteDomain (EInt 3)) (EInt 2)))
      (let mul (BinaryLambda (BMul (BParam 0) (BParam 1)) (BNil)))
      (let zipped (ZipArray mul a b))
      (let dot (ReduceArray (SumReduction) zipped))
      (let empty (InputArray "empty" (FiniteDomain (EInt 0))))
      (let maxempty (ReduceArray (MaxReduction) empty))
    "#,
    );
    assert_eq!(e.get_size("ArrayAt"), 0);
    e.parse_and_run_program(
        None,
        "(check (= dot (FoldZip (SumReduction) mul a b)))\n(check (= maxempty (ENegInf)))",
    )
    .unwrap();
    run(&mut e, "(EvaluateFold zipped 3)");
    e.parse_and_run_program(None, "(check (= dot (EInt 12)))")
        .unwrap();
}

#[test]
fn symbolic_index_and_captured_array_keep_distinct_binders() {
    let mut e = engine();
    run(
        &mut e,
        r#"
      (let i (EParam 0))
      (let a (Tabulate (FiniteDomain (EInt 4)) (BAdd (BParam 0) (BParam 1)) (BCons (EInt 10) (BNil))))
      (let symbolic (ArrayGet a i))
      (let gather (Tabulate (FiniteDomain (EInt 3)) (BArrayGet a (BAdd (BParam 0) (BInt 1))) (BNil)))
      (let concrete (ArrayAt gather 1))
    "#,
    );
    e.parse_and_run_program(
        None,
        "(check (= concrete (EInt 12)))\n(fail (check (= symbolic (EAdd i (EInt 10)))))",
    )
    .unwrap();
    run(&mut e, "(ArrayIndexProof a i)");
    e.parse_and_run_program(None, "(check (= symbolic (EAdd i (EInt 10))))")
        .unwrap();
}

#[test]
fn a_fractal_output_sequence_is_a_tabulated_array() {
    let mut e = engine();
    run(
        &mut e,
        r#"
      (let a (Tabulate (FiniteDomain (EInt 4))
        (BRecur "baked-fractal" 2 (BXCons (BParam 0) (BXCons (BParam 1) (BXNil))))
        (BCons (ESymbol "initial") (BNil))))
      (let x (ArrayAt a 3))
      (let stream (RampArray (StreamDomain) (EInt 1) (EInt 1)))
      (let block (BlockAt (Blocks stream 3) 2))
      (let sum (ReduceArray (SumReduction) block))
      (EvaluateFold block 3)
    "#,
    );
    e.parse_and_run_program(
        None,
        r#"
      (check (= x (ERecur "baked-fractal" 2 (BCons (EInt 3) (BCons (ESymbol "initial") (BNil))))))
      (check (BlockDomain (Blocks stream 3) (StreamDomain)))
      (check (= sum (EInt 24)))
    "#,
    )
    .unwrap();
}

#[test]
fn arrays_can_be_passed_as_binding_arguments() {
    let mut e = engine();
    let program = egg_layout::binding_reduce::compile(
        &["array", "index"],
        &[],
        &["(EArrayGet array index)"],
        &[],
    )
    .unwrap();
    run(
        &mut e,
        &format!(
            r#"
      (let a (RampArray (FiniteDomain (EInt 4)) (EInt 10) (EInt 2)))
      (let result (ReduceDag {program} (BCons (EArrayValue a) (BCons (EInt 2) (BNil)))))
    "#
        ),
    );
    e.parse_and_run_program(None,"(check (= result (BResult (BCons (EInt 14) (BNil)) (BNil) (BNil))))\n(check (CompleteBinding result))").unwrap();
}

#[test]
fn unrepresentable_constant_folds_and_stream_offsets_stay_symbolic() {
    let mut e = engine();
    run(
        &mut e,
        r#"
      (let sum (EAdd (EInt 9223372036854775807) (EInt 1)))
      (let product (EMul (EInt 9223372036854775807) (EInt 2)))
      (let difference (ESub (EInt -9223372036854775808) (EInt 1)))
      (let stream (FillArray (StreamDomain) (EInt 7)))
      (let block (BlockAt (Blocks stream 9223372036854775807) 2))
      (let value (ArrayAt block 0))
    "#,
    );
    e.parse_and_run_program(
        None,
        r#"
      (fail (check (= sum (EInt -9223372036854775808))))
      (fail (check (= product (EInt -2))))
      (fail (check (= difference (EInt 9223372036854775807))))
      (fail (check (= value (EInt 7))))
    "#,
    )
    .unwrap();
}
