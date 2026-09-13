use egg_layout::tier1_effects::{execute as native_execute, external_ports};
const IR: &str = include_str!("../../experiments/tier1_effects/tier1_rule_comb_ir.egg");
// Inline the fixture include: Cargo runs research tests from a different directory.
fn execute(source: &str) -> Result<serde_json::Value, String> {
    native_execute(&source.replace("(include \"experiments/tier1_effects/tier1_rule_comb_ir.egg\")", IR))
}
#[test]
fn shared_templates_do_not_share_concrete_evidence() {
    let r = execute(include_str!(
        "../../experiments/tier1_effects/tier1_rule_comb_example.egg"
    ))
    .unwrap();
    assert_eq!(r["templates"].as_array().unwrap().len(), 3);
    assert_eq!(r["instances"].as_array().unwrap().len(), 4);
    assert!(
        r["native_egraph"]["nodes"]
            .as_object()
            .unwrap()
            .values()
            .all(|n| n["op"] != "Entry")
    );
    if let Ok(path) = std::env::var("TIER1_EFFECT_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&r).unwrap()).unwrap();
    }
}
#[test]
fn external_names_normalize_but_aliases_and_sorts_do_not_disappear() {
    let ports = |a: &str, b: &str| vec![(a.into(), "Math".into()), (b.into(), "Math".into())];
    assert_eq!(
        external_ports(&ports("x", "x")).unwrap(),
        external_ports(&ports("y", "y")).unwrap()
    );
    assert_ne!(
        external_ports(&ports("x", "x")).unwrap(),
        external_ports(&ports("x", "y")).unwrap()
    );
    assert!(external_ports(&[("x".into(), "A".into()), ("x".into(), "B".into())]).is_err());
}
#[test]
fn constructed_routes_need_materialized_witnesses() {
    execute(&format!(
        r#"{IR}
(let $ports (PCons (MakePartial "Diff" (PCons (External 0 "Math") (PNil)) "Math") (PNil)))
(let $c (CoarseComb (NoParents) (Rule "R") $ports))
(let $i (Occurrence 1 $c))
(ExternalAt $i 0 (V "Math" "x"))
(run-schedule (saturate (run tier1)))
(fail (check (Binding $i args)))
(Materialized $i "Diff" (ACons (V "Math" "x") (ANil)) (V "Math" "Dx"))
(run-schedule (saturate (run tier1)))
(check (Binding $i (ACons (V "Math" "Dx") (ANil))))
"#
    ))
    .unwrap();
}
#[test]
fn mismatched_parent_templates_do_not_supply_bindings() {
    execute(&format!(r#"{IR}
(let $c (SmoothComb (MoreParents (CoarseComb (MoreParents (Empty) (NoParents)) (Rule "A") (PNil)) (NoParents)) (Rule "R") (RCons (ParentPort 0 0 "Math") (RNil))))
(let $i (Occurrence 1 $c)) (let $wrong (Occurrence 2 (CoarseComb (MoreParents (Empty) (NoParents)) (Rule "B") (PNil))))
(ParentAt $i 0 $wrong) (OutputAt $wrong 0 (V "Math" "x"))
(run-schedule (saturate (run tier1)))
(fail (check (Binding $i args)))
"#)).unwrap();
}
#[test]
fn tier0_example_still_runs() {
    egglog::EGraph::default()
        .parse_and_run_program(
            None,
            include_str!("../../experiments/tier1_effects/tier0_math_example.egg"),
        )
        .unwrap();
}

#[test]
fn redundant_support_cannot_use_an_indirectly_removed_instance() {
    let program = format!(
        r#"{IR}
(let $base (CoarseComb (MoreParents (Empty) (NoParents)) (Rule "P") (PNil)))
(let $child (SmoothComb (MoreParents $base (NoParents)) (Rule "Q") (RNil)))
(let $p (Occurrence 1 $base)) (let $q (Occurrence 2 $child))
(ParentAt $q 0 $p)
(Independent $q $p)
"#
    );
    assert!(execute(&program).unwrap_err().contains("independence"));
}
#[test]
fn requirements_cannot_pool_facts_across_shared_template_instances() {
    execute(&format!(
        r#"{IR}
(let $c (CoarseComb (MoreParents (Empty) (NoParents)) (Rule "opaque-rule") (PNil)))
(let $a (Occurrence 1 $c)) (let $b (Occurrence 2 $c))
(let $p (HasFact "P" (ANil))) (let $q (HasFact "Q" (ANil)))
(Produced $a $p) (Produced $b $q)
(Requires $a (ECons $p (ECons $q (ENil))))
(run-schedule (saturate (run tier1)))
(fail (check (SupportsUse $a $a)))
(fail (check (SupportsUse $b $a)))
"#
    ))
    .unwrap();
}

#[test]
fn one_occurrence_port_cannot_have_two_different_assignments() {
    let source = format!(
        r#"{IR}
(let $i (Occurrence 1 (CoarseComb (MoreParents (Empty) (NoParents)) (Rule "R") (PNil))))
(ExternalAt $i 0 (V "Math" "x"))
(ExternalAt $i 0 (V "Math" "y"))
"#
    );
    assert!(
        execute(&source)
            .unwrap_err()
            .contains("conflicting ExternalAt")
    );
}

#[test]
fn native_types_reject_external_routes_in_smooth_combinations() {
    let bad = [
        r#"(SmoothComb (NoParents) (Rule "R") (PCons (External 0 "Math") (PNil)))"#,
        r#"(SmoothComb (NoParents) (Rule "R") (RCons (External 0 "Math") (RNil)))"#,
        r#"(SmoothComb (NoParents) (Rule "R") (RCons (Make "F" (PCons (External 0 "Math") (PNil)) "Math") (RNil)))"#,
        r#"(SmoothComb (NoParents) "R" (RNil))"#,
    ];
    for expr in bad {
        let mut eg = egglog::EGraph::default();
        eg.parse_and_run_program(None, IR).unwrap();
        assert!(
            matches!(
                eg.parse_and_run_program(None, expr),
                Err(egglog::Error::TypeError(..))
            ),
            "must be rejected by native type checker: {expr}"
        );
    }
}

#[test]
fn smooth_nested_make_resolves_only_from_parent_ports_and_materialization() {
    execute(&format!(
        r#"{IR}
(let $base (CoarseComb (MoreParents (Empty) (NoParents)) (Rule "A") (PNil)))
(let $parents (MoreParents $base (NoParents)))
(let $local (RCons (ParentPort 0 0 "Math") (RNil)))
(let $c (SmoothComb $parents (Rule "B") (RCons (Make "F" $local "Math") (RNil))))
(let $p (Occurrence 1 $base)) (let $i (Occurrence 2 $c))
(ParentAt $i 0 $p) (OutputAt $p 0 (V "Math" "x"))
(run-schedule (saturate (run tier1)))
(fail (check (Binding $i args)))
(Materialized $i "F" (ACons (V "Math" "x") (ANil)) (V "Math" "Fx"))
(run-schedule (saturate (run tier1)))
(check (Binding $i (ACons (V "Math" "Fx") (ANil))))
"#
    ))
    .unwrap();
}

#[test]
fn local_binding_view_preserves_coarse_effect_boundary_and_instances() {
    execute(&format!(r#"{IR}
(let $parent (CoarseComb (MoreParents (Empty) (NoParents)) (Rule "P") (PCons (External 0 "Math") (PNil))))
(let $parents (MoreParents $parent (NoParents)))
(let $local (RCons (ParentPort 0 0 "Math") (RNil)))
(let $partial (PCons (Local (ParentPort 0 0 "Math")) (PNil)))
(let $c (CoarseComb $parents (Rule "R") $partial))
(let $s (SmoothComb $parents (Rule "R") $local))
(let $p (Occurrence 1 $parent)) (ExternalAt $p 0 (V "Math" "x")) (OutputAt $p 0 (V "Math" "x"))
(let $a (Occurrence 2 $c)) (let $b (Occurrence 3 $s))
(ParentAt $a 0 $p) (ParentAt $b 0 $p)
(let $effect (HasFact "only-a" (ANil))) (Produced $a $effect)
(run-schedule (saturate (run tier1)))
(fail (check (= $c $s)))
(check (Binding $a (ACons (V "Math" "x") (ANil))))
(run-schedule (saturate (run tier1_equivalences)))
(run-schedule (saturate (run tier1)))
(fail (check (= $c $s)))
(check (LocalBindingView $c $local))
(check (Binding $a (ACons (V "Math" "x") (ANil))))
(check (Binding $b (ACons (V "Math" "x") (ANil))))
(check (Provides $a $effect))
(fail (check (Provides $b $effect)))
(let $external (CoarseComb $parents (Rule "R") (PCons (External 0 "Math") (PNil))))
(run-schedule (saturate (run tier1_equivalences)))
(fail (check (= $external $s)))
"#)).unwrap();
}
#[test]
fn nested_partial_make_can_normalize_without_changing_its_witness() {
    execute(&format!(
        r#"{IR}
(let $ps (MoreParents (Empty) (NoParents)))
(let $partial (PCons (MakePartial "Const" (PNil) "Math") (PNil)))
(let $local (RCons (Make "Const" (RNil) "Math") (RNil)))
(let $c (CoarseComb $ps (Rule "R") $partial))
(let $s (SmoothComb $ps (Rule "R") $local))
(let $canonical (CoarseComb $ps (Rule "R") (PCons (Local (Make "Const" (RNil) "Math")) (PNil))))
(fail (check (= $c $canonical)))
(let $i (Occurrence 1 $c))
(Materialized $i "Const" (ANil) (V "Math" "constant"))
(run-schedule (saturate (run tier1)))
(check (Binding $i (ACons (V "Math" "constant") (ANil))))
(run-schedule (saturate (run tier1_equivalences)))
(run-schedule (saturate (run tier1)))
(fail (check (= $c $s)))
(check (LocalBindingView $c $local))
(check (= $c $canonical))
(check (Binding $i (ACons (V "Math" "constant") (ANil))))
"#
    ))
    .unwrap();
}

#[test]
fn empty_cannot_be_used_as_a_data_entry_occurrence() {
    assert!(
        execute(&format!("{IR} (Occurrence 1 (Empty))"))
            .unwrap_err()
            .contains("context unit")
    );
}

#[test]
fn streaming_import_preserves_native_templates_and_bindings() {
    let program=include_str!("../../tests/fixtures/bridge_program.egg");
    let include=format!("(include \"{}/../experiments/tier1_effects/tier1_rule_comb_ir.egg\")",env!("CARGO_MANIFEST_DIR"));
    let program=program.replace("(include \"experiments/tier1_effects/tier1_rule_comb_ir.egg\")",&include);
    let full=egg_layout::tier1_effects::execute_mode(&program,true).unwrap();
    let stream=egg_layout::tier1_effects::execute_stream(std::io::Cursor::new(program),true).unwrap();
    assert_eq!(full,stream);
}

#[test]
fn unrelated_binding_templates_do_not_expand_an_instances_route_space() {
    let mut program = format!(r#"{IR}
(let $root (CoarseComb (NoParents) (Rule "root") (PNil)))
(let $parent (Occurrence 0 $root))
(let $used (RCons (ParentPort 0 0 "Math") (RNil)))
(let $child (Occurrence 1 (SmoothComb (MoreParents $root (NoParents)) (Rule "child") $used)))
(ParentAt $child 0 $parent)
"#);
    for slot in 0..32 {
        program += &format!("(OutputAt $parent {slot} (V \"Math\" \"{slot}\"))\n(RCons (ParentPort 0 {slot} \"Math\") (RNil))\n");
    }
    program += "(run-schedule (saturate (run tier1)))\n(check (Binding $child (ACons (V \"Math\" \"0\") (ANil))))";
    let result = execute(&program).unwrap();
    assert_eq!(result["relation_sizes"]["LocalArgs"],2);
    assert_eq!(result["relation_sizes"]["LocalResolved"],1);
}
