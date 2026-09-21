use egg_layout::{coarse_smooth::*, layer_patterns};
fn apply(
    s: &LayerStore,
    rule: &str,
    parent: Option<(usize, usize)>,
    outputs: Vec<usize>,
    extra: Option<usize>,
) -> Apply {
    let (parents, binding, wanted, external, mut required) = if let Some((p, port)) = parent {
        let v = s.occurrences[p].apply.outputs[port];
        (
            vec![p],
            vec![RelativeBinding::ParentPort {
                parent: 0,
                output: port,
            }],
            vec![v],
            vec![],
            vec![Effect::RowFact(v)],
        )
    } else {
        (
            vec![],
            vec![RelativeBinding::External { slot: 0 }],
            vec![0],
            vec![0],
            vec![],
        )
    };
    let external_facts = extra.into_iter().map(Effect::RowFact).collect::<Vec<_>>();
    required.extend(external_facts.clone());
    Apply {
        event: s.occurrences.len() as u64,
        rule: rule.into(),
        input_roles: vec!["input".into()],
        output_roles: (0..outputs.len()).map(|i| format!("out{i}")).collect(),
        parents,
        binding,
        wanted,
        external,
        required,
        external_facts,
        produced: outputs.iter().copied().map(Effect::RowFact).collect(),
        outputs,
    }
}
#[test]
fn repeats_inside_one_smooth_layer_and_retains_trigger() {
    let mut s = LayerStore::default();
    for i in 0usize..7 {
        let a = apply(&s, "R", i.checked_sub(1).map(|p| (p, 0)), vec![i + 1], None);
        s.push(a).unwrap();
    }
    assert_eq!(s.smooth_layers.len(), 1);
    let a = layer_patterns::analyze(&s);
    let f = a
        .fractals
        .iter()
        .find(|f| f.max_observed_depth >= 3)
        .expect("intra-layer recursion");
    assert_eq!(a.templates[f.template].interface.returns.len(), 1);
    assert!(!f.triggers.is_empty());
    assert!(f.status.contains("unbounded induction unknown"));
    assert!(a.coverage.iter().any(|c| c["status"] == "TemplateCoverage"));
    assert!(
        a.coverage
            .iter()
            .all(|c| c["fractal_dominance"] != "proved")
    );
}
#[test]
fn two_return_ports_form_a_branching_family() {
    let mut s = LayerStore::default();
    let mut value = 1;
    s.push(apply(&s, "A", None, vec![value, value + 1], None))
        .unwrap();
    value += 2;
    let mut frontier = vec![0];
    for _ in 0..4 {
        let mut next = vec![];
        for a in frontier {
            for port in 0..2 {
                let b = s
                    .push(apply(&s, "B", Some((a, port)), vec![value], None))
                    .unwrap();
                value += 1;
                let a = s
                    .push(apply(&s, "A", Some((b, 0)), vec![value, value + 1], None))
                    .unwrap();
                value += 2;
                next.push(a);
            }
        }
        frontier = next;
    }
    let a = layer_patterns::analyze(&s);
    let f = a
        .fractals
        .iter()
        .find(|f| {
            a.templates[f.template].interface.returns.len() == 2
                && a.templates[f.template].interface.members[0].rule == "A"
        })
        .expect("two-return family");
    let t = &a.templates[f.template].interface;
    assert_ne!(t.returns[0], t.returns[1]);
    assert!(f.recursive_edges.iter().any(|(_, slot, _)| *slot == 1));
}
#[test]
fn external_requirements_are_in_the_template_and_each_unit() {
    let mut s = LayerStore::default();
    for i in 0usize..6 {
        let a = apply(
            &s,
            "R",
            i.checked_sub(1).map(|p| (p, 0)),
            vec![i + 1],
            Some(i + 100),
        );
        s.push(a).unwrap();
    }
    let a = layer_patterns::analyze(&s);
    assert!(a.fractals.iter().any(|f| f.external_each_unit));
    for f in &a.fractals {
        assert!(
            a.templates[f.template]
                .interface
                .members
                .iter()
                .any(|m| !m.external_facts.is_empty())
        );
    }
}
#[test]
fn aliases_and_different_guards_are_not_erased() {
    let mut s = LayerStore::default();
    let first = s
        .push(apply(&s, "R with guard1", None, vec![1, 1], None))
        .unwrap();
    let second = s
        .push(apply(&s, "R with guard1", None, vec![2, 3], None))
        .unwrap();
    s.push(apply(&s, "R with guard2", None, vec![4, 4], None))
        .unwrap();
    let a = layer_patterns::analyze(&s);
    assert!(
        !a.templates
            .iter()
            .any(|t| t.witnesses.contains(&vec![first]) && t.witnesses.contains(&vec![second]))
    );
    assert!(a.fractals.is_empty());
}

#[test]
fn independent_sibling_schedules_share_the_same_interface() {
    fn build(reverse: bool) -> layer_patterns::Interface {
        let mut s = LayerStore::default();
        s.push(apply(&s, "A", None, vec![1, 2], None)).unwrap();
        let mut bs = [0, 0];
        for port in if reverse { [1, 0] } else { [0, 1] } {
            bs[port] = s
                .push(apply(&s, "B", Some((0, port)), vec![3 + port], None))
                .unwrap();
        }
        let mut j = apply(&s, "Join", Some((bs[0], 0)), vec![5], None);
        j.parents.push(bs[1]);
        j.binding.push(RelativeBinding::ParentPort {
            parent: 1,
            output: 0,
        });
        j.input_roles.push("other".into());
        j.wanted.push(4);
        j.required.push(Effect::RowFact(4));
        s.push(j).unwrap();
        layer_patterns::analyze(&s)
            .templates
            .into_iter()
            .find(|t| t.interface.members.len() == 4)
            .unwrap()
            .interface
    }
    assert_eq!(build(false), build(true));
}
#[test]
fn candidate_budget_is_reported_instead_of_claiming_absence() {
    let mut s = LayerStore::default();
    s.push(apply(&s, "R", None, vec![1], None)).unwrap();
    for i in 0..70 {
        s.push(apply(&s, "R", Some((0, 0)), vec![i + 2], None))
            .unwrap();
    }
    let a = layer_patterns::analyze(&s);
    assert!(a.truncated_candidates > 0);
    assert!(a.coverage_queries <= 512);
}

#[test]
fn caller_output_position_is_not_part_of_the_interface_template() {
    let mut s = LayerStore::default();
    s.push(apply(&s, "Caller", None, vec![1, 2], None)).unwrap();
    let left = s.push(apply(&s, "S", Some((0, 0)), vec![3], None)).unwrap();
    let right = s.push(apply(&s, "S", Some((0, 1)), vec![4], None)).unwrap();
    let a = layer_patterns::analyze(&s);
    assert!(
        a.templates
            .iter()
            .any(|t| t.witnesses.contains(&vec![left]) && t.witnesses.contains(&vec![right]))
    );
    assert_ne!(
        s.occurrences[left].apply.binding, s.occurrences[right].apply.binding,
        "instances retain their exact caller ports"
    );
}

#[test]
fn cached_coverage_matches_fresh_analysis_after_extension() {
    let mut s = LayerStore::default();
    for i in 0usize..6 {
        s.push(apply(
            &s,
            "R",
            i.checked_sub(1).map(|p| (p, 0)),
            vec![i + 1],
            None,
        ))
        .unwrap();
    }
    let mut analyzer = layer_patterns::Analyzer::default();
    analyzer.analyze(&s);
    let repeated = analyzer.analyze(&s);
    assert!(repeated.coverage_cache_hits > 0);
    s.push(apply(&s, "R", Some((5, 0)), vec![7], None)).unwrap();
    let mut cached = serde_json::to_value(analyzer.analyze(&s)).unwrap();
    let mut fresh = serde_json::to_value(layer_patterns::analyze(&s)).unwrap();
    cached
        .as_object_mut()
        .unwrap()
        .remove("coverage_cache_hits");
    fresh.as_object_mut().unwrap().remove("coverage_cache_hits");
    assert_eq!(cached, fresh);
}

#[test]
fn a_globally_smooth_member_can_require_extra_fragment_inputs() {
    let mut s = LayerStore::default();
    let mut a = s.push(apply(&s, "A", None, vec![1], None)).unwrap();
    for n in 0..4 {
        let x = s
            .push(apply(&s, "Outside", None, vec![100 + n], None))
            .unwrap();
        let mut b = apply(&s, "B", Some((a, 0)), vec![10 + n], None);
        b.parents.push(x);
        b.input_roles.push("extra".into());
        b.binding.push(RelativeBinding::ParentPort {
            parent: 1,
            output: 0,
        });
        b.wanted.push(100 + n);
        b.required.push(Effect::RowFact(100 + n));
        let b = s.push(b).unwrap();
        a = s
            .push(apply(&s, "A", Some((b, 0)), vec![20 + n], None))
            .unwrap();
    }
    let result = layer_patterns::analyze(&s);
    assert!(result.fractals.iter().any(|f| {
        f.external_each_unit
            && result.templates[f.template]
                .interface
                .members
                .iter()
                .all(|m| m.kind == "SmoothRuleComposition")
    }));
}
