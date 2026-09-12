use egg_layout::tier1_effects::{execute, external_ports};
const IR: &str = "(include \"experiments/tier1_effects/tier1_rule_comb_ir.egg\")";
#[test]
fn shared_templates_do_not_share_concrete_evidence() {
    let r = execute(include_str!(
        "../experiments/tier1_effects/tier1_rule_comb_example.egg"
    ))
    .unwrap();
    assert_eq!(r["templates"].as_array().unwrap().len(), 2);
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
(let $c (SmoothComb (MoreParents (Basic (Rule "A")) (NoParents)) (Rule "R") (RCons (ParentPort 0 0 "Math") (RNil))))
(let $i (Occurrence 1 $c)) (let $wrong (Occurrence 2 (Basic (Rule "B"))))
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
            include_str!("../experiments/tier1_effects/tier0_math_example.egg"),
        )
        .unwrap();
}

#[test]
fn redundant_support_cannot_use_an_indirectly_removed_instance() {
    let program = format!(
        r#"{IR}
(let $base (Basic (Rule "P")))
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
(let $c (Basic (Rule "opaque-rule")))
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
(let $i (Occurrence 1 (Basic (Rule "R"))))
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
(let $base (Basic (Rule "A")))
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
