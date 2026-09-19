use egg_layout::coarse_smooth::*;
fn seed(event: u64, rule: &str, input: usize, output: usize) -> Apply {
    Apply {
        event,
        rule: rule.into(),
        input_roles: vec!["input".into()],
        output_roles: vec!["output".into()],
        parents: vec![],
        binding: vec![RelativeBinding::External { slot: 0 }],
        wanted: vec![input],
        outputs: vec![output],
        external: vec![input],
        required: vec![],
        external_facts: vec![],
        produced: vec![Effect::RowFact(output)],
    }
}
fn smooth(s: &LayerStore, event: u64, rule: &str, parents: &[usize], out: usize) -> Apply {
    let values: Vec<_> = parents
        .iter()
        .map(|p| s.occurrences[*p].apply.outputs[0])
        .collect();
    Apply {
        event,
        rule: rule.into(),
        input_roles: (0..parents.len()).map(|i| format!("input{i}")).collect(),
        output_roles: vec!["output".into()],
        parents: parents.to_vec(),
        binding: (0..parents.len())
            .map(|parent| RelativeBinding::ParentPort { parent, output: 0 })
            .collect(),
        wanted: values.clone(),
        outputs: vec![out],
        external: vec![],
        required: values.into_iter().map(Effect::RowFact).collect(),
        external_facts: vec![],
        produced: vec![Effect::RowFact(out)],
    }
}
#[test]
fn shared_constant_does_not_merge_but_joint_consumer_does() {
    let mut s = LayerStore::default();
    for i in 0..100 {
        s.push(seed(i as u64, "C", 0, i + 1)).unwrap();
    }
    assert_eq!(s.coarse_layers.len(), 100);
    assert_eq!(s.combs.len(), 1, "different values share one definition");
    s.push(smooth(&s, 100, "S", &[0], 101)).unwrap();
    s.push(smooth(&s, 101, "S", &[1], 102)).unwrap();
    assert_eq!(
        s.coarse_layers.len(),
        100,
        "independent smooth branches do not fuse seeds"
    );
    s.push(smooth(&s, 102, "Join", &[100, 101], 103)).unwrap();
    let layer = &s.coarse_layers[s.occurrences[102].coarse_layer];
    assert_eq!(layer.members, vec![0, 1]);
    assert_eq!(layer.witness, Some(102));
    assert_eq!(layer.children.len(), 2);
    assert_eq!(s.coarse_layers.len(), 101);
}
#[test]
fn three_members_are_valid_and_repeated_support_reuses_layer() {
    let mut s = LayerStore::default();
    for i in 0..3 {
        s.push(seed(i as u64, "C", 0, i + 1)).unwrap();
    }
    s.push(smooth(&s, 3, "Join", &[0, 1, 2], 4)).unwrap();
    s.push(smooth(&s, 4, "Again", &[3], 5)).unwrap();
    assert_eq!(s.coarse_layers.len(), 4);
    assert_eq!(s.occurrences[3].coarse_layer, s.occurrences[4].coarse_layer);
    assert_eq!(s.coarse_layers[3].members.len(), 3);
}
#[test]
fn interleaving_independent_branches_and_parent_order_share_definitions() {
    fn build(reverse: bool) -> LayerStore {
        let mut s = LayerStore::default();
        let a = s.push(seed(10, "A", 1, 10)).unwrap();
        let (b, sa) = if reverse {
            let sa = s.push(smooth(&s, 30, "SA", &[a], 30)).unwrap();
            (s.push(seed(20, "B", 2, 20)).unwrap(), sa)
        } else {
            let b = s.push(seed(20, "B", 2, 20)).unwrap();
            (b, s.push(smooth(&s, 30, "SA", &[a], 30)).unwrap())
        };
        let sb = s.push(smooth(&s, 40, "SB", &[b], 40)).unwrap();
        let mut join = smooth(&s, 50, "Join", &[sa, sb], 50);
        if reverse {
            join.parents.reverse();
            join.binding = vec![
                RelativeBinding::ParentPort {
                    parent: 1,
                    output: 0,
                },
                RelativeBinding::ParentPort {
                    parent: 0,
                    output: 0,
                },
            ];
        }
        s.push(join).unwrap();
        s
    }
    fn signature(s: &LayerStore, id: usize) -> serde_json::Value {
        let mut key = serde_json::to_value(&s.combs[id]).unwrap();
        key["parents"] = s.combs[id]
            .parents
            .iter()
            .map(|p| signature(s, *p))
            .collect();
        key
    }
    let a = build(false);
    let b = build(true);
    assert_eq!(
        signature(&a, a.occurrences[4].comb),
        signature(&b, b.occurrences[4].comb)
    );
    let members = |s: &LayerStore| {
        s.coarse_layers[s.occurrences[4].coarse_layer]
            .members
            .iter()
            .map(|i| s.occurrences[*i].apply.event)
            .collect::<Vec<_>>()
    };
    assert_eq!(members(&a), members(&b));
}
#[test]
fn alias_and_effect_shapes_do_not_false_deduplicate() {
    let mut s = LayerStore::default();
    let mut a = seed(0, "C", 1, 2);
    a.outputs.push(2);
    a.output_roles.push("second".into());
    s.push(a).unwrap();
    let mut b = seed(1, "C", 3, 4);
    b.outputs.push(5);
    b.output_roles.push("second".into());
    s.push(b).unwrap();
    assert_ne!(s.occurrences[0].comb, s.occurrences[1].comb);
    let mut c = seed(2, "C", 6, 7);
    c.produced = vec![Effect::Equal(6, 7)];
    s.push(c).unwrap();
    assert_ne!(s.occurrences[0].comb, s.occurrences[2].comb);
}
#[test]
fn equality_is_transitive_but_rows_keep_identity_and_preconditions_precede_effects() {
    let mut s = LayerStore::default();
    let mut a = seed(0, "Union", 0, 1);
    a.produced
        .extend([Effect::Equal(10, 11), Effect::Equal(11, 12)]);
    s.push(a).unwrap();
    assert!(s.supports(&[0], &Effect::Equal(10, 12)));
    assert!(!s.supports(&[0], &Effect::RowFact(12)));
    let mut b = smooth(&s, 1, "B", &[0], 2);
    b.required.push(Effect::RowFact(2));
    assert!(s.push(b).unwrap_err().contains("unprovided"));
    let mut b = smooth(&s, 1, "B", &[0], 2);
    b.required.push(Effect::Equal(10, 12));
    s.push(b).unwrap();
    assert_eq!(s.occurrences.len(), 2, "failed insert did not mutate store");
}
#[test]
fn external_effect_restarts_coarse_and_ancestor_overlap_does_not_hoist() {
    let mut s = LayerStore::default();
    s.push(seed(0, "C1", 0, 1)).unwrap();
    s.push(smooth(&s, 1, "S1", &[0], 2)).unwrap();
    let mut c = smooth(&s, 2, "C2", &[1], 3);
    c.required.push(Effect::RowFact(99));
    c.external_facts.push(Effect::RowFact(99));
    s.push(c).unwrap();
    assert!(matches!(
        s.combs[s.occurrences[2].comb].kind,
        CombKind::CoarseComb
    ));
    s.push(smooth(&s, 3, "S2", &[0, 2], 4)).unwrap();
    assert!(s.occurrences[3].boundary_restart);
    assert_eq!(
        s.coarse_layers[s.occurrences[3].coarse_layer].members,
        vec![3]
    );
    assert_eq!(
        s.occurrences[3].apply.parents,
        vec![0, 2],
        "context remains explicit"
    );
}
#[test]
fn binding_validation_rejects_wrong_port_and_forward_parent() {
    let mut s = LayerStore::default();
    s.push(seed(0, "A", 0, 1)).unwrap();
    let mut a = smooth(&s, 1, "B", &[0], 2);
    a.wanted[0] = 42;
    assert!(s.push(a).is_err());
    let mut a = smooth(&s, 1, "B", &[0], 2);
    a.parents[0] = 1;
    assert!(s.push(a).is_err());
}

#[test]
fn different_output_roles_do_not_share_a_definition() {
    let mut s = LayerStore::default();
    s.push(seed(0, "C", 1, 2)).unwrap();
    let mut b = seed(1, "C", 3, 4);
    b.output_roles[0] = "another-action-row".into();
    s.push(b).unwrap();
    assert_ne!(s.occurrences[0].comb, s.occurrences[1].comb);
}

#[test]
fn directly_dependent_coarse_members_can_share_a_layer() {
    let mut s = LayerStore::default();
    s.push(seed(0, "C1", 0, 1)).unwrap();
    let mut c = smooth(&s, 1, "C2", &[0], 2);
    c.required.push(Effect::RowFact(99));
    c.external_facts.push(Effect::RowFact(99));
    s.push(c).unwrap();
    s.push(smooth(&s, 2, "Join", &[0, 1], 3)).unwrap();
    assert!(!s.occurrences[2].boundary_restart);
    assert_eq!(
        s.coarse_layers[s.occurrences[2].coarse_layer].members,
        vec![0, 1]
    );
}

#[test]
fn omitting_an_intermediate_coarse_member_does_not_create_a_layer_cycle() {
    let mut s = LayerStore::default();
    s.push(seed(0, "C1", 0, 1)).unwrap();
    for i in 1..=2 {
        let mut c = smooth(&s, i as u64, "Coarse", &[i - 1], i + 1);
        c.required.push(Effect::RowFact(90 + i));
        c.external_facts.push(Effect::RowFact(90 + i));
        s.push(c).unwrap();
    }
    s.push(smooth(&s, 3, "Join", &[0, 2], 4)).unwrap();
    assert!(s.occurrences[3].boundary_restart);
}
