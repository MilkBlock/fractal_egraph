use egglog::EGraph;
#[test]
fn exact_rows_keep_identity_and_generic_effects_keep_equality() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        include_str!("../experiments/tier1_effects/tier1_rule_comb_ir.egg"),
    )
    .unwrap();
    eg.parse_and_run_program(
        None,
        r#"
      (let i (Occurrence 1 (Empty)))
      (let x (V "Math" "x")) (let y (V "Math" "y"))
      (let row1 (V "Fact:T" "scope0:write1"))
      (let row2 (V "Fact:T" "scope1:write1"))
      (Produced i (RowFact row1))
      (Produced i (Equal x y))
      (Produced i (HasFact "read-row" (ACons x (ANil))))
      (NeedEffect i (RowFact row1))
      (NeedEffect i (RowFact row2))
      (NeedEffect i (HasFact "read-row" (ACons y (ANil))))
      (run-schedule (saturate (run tier1)))
      (check (Satisfies i (RowFact row1)))
      (check (Satisfies i (HasFact "read-row" (ACons y (ANil)))))
      (fail (check (Satisfies i (RowFact row2))))
      (fail (check (NeedArgs i (ACons row1 (ANil)) (ACons row2 (ANil)))))
    "#,
    )
    .unwrap();
}
