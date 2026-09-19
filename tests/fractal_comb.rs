use egglog::EGraph;
#[test]
fn smooth_segments_stack_across_coarse_injection_and_keep_positioned_effects() {
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
    e.parse_and_run_program(None, include_str!("../rules/fractal_views.egg"))
        .unwrap();
    e.parse_and_run_program(
        None,
        r#"
      (let r (Extend "r" (End) (Schema "finite-stable-interface")))
      (let seed (CoarseComb (MoreParents (Empty) (NoParents)) (Rule "inject") (PNil)))
      (let a (SmoothComb (MoreParents seed (NoParents)) (Rule "r") (RNil)))
      (let b (SmoothComb (MoreParents a (NoParents)) (Rule "r") (RNil)))
      (let boundary (CoarseComb (MoreParents b (NoParents)) (Rule "other") (PNil)))
      (let c (SmoothComb (MoreParents boundary (NoParents)) (Rule "r") (RNil)))
      (let d (SmoothComb (MoreParents c (NoParents)) (Rule "r") (RNil)))
      (UnaryStep seed a r (RNil)) (UnaryStep a b r (RNil))
      (UnaryStep b boundary r (RNil)) ; deliberately malformed certificate
      (UnaryStep boundary c r (RNil)) (UnaryStep c d r (RNil))
      (let i0 (Occurrence 0 seed)) (let i1 (Occurrence 1 a)) (let i2 (Occurrence 2 b))
      (let i3 (Occurrence 3 boundary)) (let i4 (Occurrence 4 c)) (let i5 (Occurrence 5 d))
      (ObservedStep i0 i1 r (RNil)) (ObservedStep i1 i2 r (RNil))
      (ObservedStep i2 i3 r (RNil)) (ObservedStep i3 i4 r (RNil)) (ObservedStep i4 i5 r (RNil))
      (let f1 (FractalComb (Depth 2) r seed (RNil)))
      (let packed_boundary (CoarseComb (MoreParents f1 (NoParents)) (Rule "other") (PNil)))
      (let f2 (FractalComb (Depth 2) r packed_boundary (RNil)))
      (OriginalComb seed) (OriginalComb boundary)
      (SelectedView i2 f1) (SelectedView i3 packed_boundary) (SelectedView i5 f2)
      (ParentAt i1 0 i0) (ParentAt i2 0 i1)
      (ParentAt i3 0 i2) (ParentAt i4 0 i3) (ParentAt i5 0 i4)
      (let injected (V "Fact:T" "external-effect"))
      (Produced i3 (RowFact injected))
      (Requires i4 (ECons (RowFact injected) (ENil)))
      (let outsider (Occurrence 88 (SmoothComb (MoreParents a (NoParents)) (Rule "consumer") (RNil))))
      (ParentAt outsider 0 i1)
      (let fake (Occurrence 99 b)) (SelectedView fake f1)
      (let row (V "Fact:T" "scope0:write1"))
      (OutputAt i1 0 row) (Produced i1 (RowFact row))
      (Produced i1 (Equal (V "Math" "x") (V "Math" "y")))
      (OutputAt i4 0 (V "Fact:T" "scope0:write4"))
      (run-schedule (saturate (run tier1)))
      (run-schedule (saturate (run higher)))
      (run-schedule (saturate (run fractal-views)))
      (check (VerifiedView i3 packed_boundary))
      (check (VerifiedView i5 f2))
      (check (SupportsUse i4 i4))
      (check (FractalProvides i5 f2 (RowFact injected)))
      (fail (check (FractalEffect i5 f2 1 (RowFact injected))))
      (check (FractalOccurrence i2 f1 i0 2))
      (check (FractalOccurrence i5 f2 i3 2))
      (check (FractalOutput i2 f1 1 0 row))
      (check (FractalEffect i2 f1 1 (RowFact row)))
      (check (FractalEffect i2 f1 1 (Equal (V "Math" "x") (V "Math" "y"))))
      (check (FractalParent i3 0 i2 f1))
      (check (FractalInputAddress outsider 0 i2 f1 1))
      (fail (check (FractalOccurrence fake f1 i0 2)))
      (fail (check (FractalOutput fake f1 1 0 row)))
      (fail (check (FractalOutput i5 f2 1 0 row)))
      (fail (check (PowerPrefix b c r (RNil) 2)))
      (fail (check (PowerPrefix seed d r (RNil) 5)))
    "#,
    )
    .unwrap();
}
