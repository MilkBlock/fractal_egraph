use egg_layout::{
    coarse_smooth::*,
    comb_reuse::{self, Atom, Part, Reference},
};
fn add(
    s: &mut LayerStore,
    rule: &str,
    parents: Vec<(usize, usize)>,
    external: Option<usize>,
    out: usize,
) -> usize {
    let mut ps = vec![];
    let mut binding = vec![];
    let mut wanted = vec![];
    for (p, o) in parents {
        let k = ps.iter().position(|x| *x == p).unwrap_or_else(|| {
            ps.push(p);
            ps.len() - 1
        });
        binding.push(RelativeBinding::ParentPort {
            parent: k,
            output: o,
        });
        wanted.push(s.occurrences[p].apply.outputs[o]);
    }
    let mut ext = vec![];
    if let Some(v) = external {
        ext.push(v);
        binding.push(RelativeBinding::External { slot: 0 });
        wanted.push(v);
    }
    let event = s.occurrences.len();
    let required = wanted
        .iter()
        .filter(|v| !ext.contains(v))
        .copied()
        .map(Effect::RowFact)
        .collect();
    s.push(Apply {
        event: event as u64,
        rule: rule.into(),
        parents: ps,
        input_roles: (0..wanted.len()).map(|i| format!("arg{i}")).collect(),
        output_roles: vec!["out".into()],
        binding,
        wanted,
        outputs: vec![out],
        external: ext,
        required,
        external_facts: vec![],
        produced: vec![Effect::RowFact(out)],
    })
    .unwrap()
}
fn check_cover(s: &LayerStore) {
    s.reuse.verify(s).unwrap();
    let r = &s.reuse;
    let mut covered = std::collections::BTreeSet::new();
    for i in &r.active_uses {
        let u = &r.uses[*i];
        assert!(
            r.templates[u.template].learned_at < u.completed_at,
            "no future learning"
        );
        for m in &u.members {
            assert!(covered.insert(*m), "disjoint preferred cover");
            assert_eq!(r.owner[*m].unwrap().0, *i);
        }
    }
    for (i, owner) in r.owner.iter().enumerate() {
        if owner.is_none() {
            assert!(covered.insert(i));
        }
    }
    assert_eq!(covered.len(), s.occurrences.len());
    fn expand(r: &comb_reuse::ReuseStore, u: usize) -> Vec<usize> {
        let mut out = vec![];
        for p in &r.uses[u].parts {
            match p {
                Part::Residual(e) => out.push(*e),
                Part::Use(old) => {
                    assert!(*old < u);
                    out.extend(expand(r, *old));
                }
            }
        }
        out.sort();
        out
    }
    for (i, u) in r.uses.iter().enumerate() {
        let mut want = u.members.clone();
        want.sort();
        assert_eq!(expand(r, i), want);
    }
}
#[test]
fn subsequent_applies_use_macros_and_larger_recipes_reuse_them() {
    let mut s = LayerStore::default();
    let mut prev = add(&mut s, "R", vec![], Some(1000), 1);
    for v in 2..40 {
        prev = add(&mut s, "R", vec![(prev, 0)], None, v);
    }
    check_cover(&s);
    assert!(!s.reuse.uses.is_empty());
    assert!(s.reuse.continuations_through_use > 0);
    assert!(
        s.reuse
            .templates
            .iter()
            .any(|t| t.atoms.iter().any(|a| matches!(a, Atom::Use { .. })))
    );
    assert!(
        s.reuse
            .uses
            .iter()
            .any(|u| u.parts.iter().any(|p| matches!(p, Part::Use(_))))
    );
    assert!(s.reuse.selected_wiring_cost < s.reuse.raw_wiring_cost);
}
#[test]
fn no_layer_gate_and_external_residual_inputs_are_explicit() {
    let mut s = LayerStore::default();
    for n in 0..8 {
        let a = add(&mut s, "A", vec![], Some(100 + n), 1000 + n * 2);
        add(&mut s, "B", vec![(a, 0)], Some(200 + n), 1001 + n * 2);
    }
    check_cover(&s);
    assert!(s.reuse.uses.len() > 2);
    let restricted = comb_reuse::ReuseStore::replay(&s, true);
    assert!(restricted.uses.is_empty());
    let b = add(&mut s, "New", vec![(15, 0)], Some(9999), 99999);
    assert!(s.reuse.residuals[b].coarse);
    assert!(
        s.reuse.residuals[b]
            .binding
            .iter()
            .any(|r| matches!(r, Reference::ResidualPort { .. }))
    );
    assert!(
        s.reuse.residuals[b]
            .binding
            .contains(&Reference::External(9999))
    );
}
#[test]
fn interior_outputs_stay_addressable() {
    let mut s = LayerStore::default();
    for k in 0..3 {
        let a = add(&mut s, "A", vec![], Some(100 + k), 1000 + k * 2);
        add(&mut s, "B", vec![(a, 0)], None, 1001 + k * 2);
    }
    let (u, m) = s.reuse.owner[4].expect("A in repeated A+B");
    assert_ne!(m, s.reuse.templates[s.reuse.uses[u].template].pattern.root);
    let next = add(&mut s, "ReadInterior", vec![(4, 0)], None, 2000);
    assert!(
        matches!(s.reuse.locate(4, 0),Reference::UsePort{instance,member,..} if instance==u&&member==m)
    );
    assert_eq!(
        s.reuse.residuals[next].binding[0],
        Reference::ResidualPort {
            event: 4,
            output: 0
        }
    );
    assert!(s.reuse.interior_port_reads > 0);
    check_cover(&s);
}
#[test]
fn aliases_guards_and_effect_shapes_are_not_erased() {
    let mut s = LayerStore::default();
    let a = add(&mut s, "A", vec![], Some(9), 1);
    add(&mut s, "B guard-one", vec![(a, 0)], None, 2);
    let a = add(&mut s, "A", vec![], Some(10), 3);
    add(&mut s, "B guard-two", vec![(a, 0)], None, 4);
    assert!(s.reuse.uses.is_empty());
    let a = add(&mut s, "A", vec![], Some(11), 5);
    let mut b = s.occurrences[1].apply.clone();
    b.event = 5;
    b.parents = vec![a];
    b.wanted = vec![5];
    b.outputs = vec![6];
    b.required = vec![Effect::RowFact(5)];
    b.produced = vec![Effect::Equal(5, 6)];
    s.push(b).unwrap();
    assert!(s.reuse.uses.is_empty());
}
#[test]
fn union_proof_and_prior_ports_survive_encoding() {
    let mut s = LayerStore::default();
    for k in 0..3 {
        let a = add(&mut s, "A", vec![], Some(100 + k), 10 + k);
        let mut b = s.occurrences[a].apply.clone();
        b.event = s.occurrences.len() as u64;
        b.rule = "Union".into();
        b.parents = vec![a];
        b.binding = vec![RelativeBinding::ParentPort {
            parent: 0,
            output: 0,
        }];
        b.wanted = vec![10 + k];
        b.outputs = vec![20 + k];
        b.external.clear();
        b.required = vec![Effect::RowFact(10 + k)];
        b.produced = vec![Effect::Equal(10 + k, 20 + k)];
        s.push(b).unwrap();
    }
    check_cover(&s);
    assert!(!s.reuse.uses.is_empty());
    let u = s.reuse.uses.last().unwrap();
    assert!(
        s.reuse.templates[u.template]
            .pattern
            .steps
            .iter()
            .any(|m| m.effects.iter().any(|e| matches!(e, Effect::Equal(..))))
    );
}

#[test]
fn incomplete_regions_cannot_hide_an_intermediate_dependency() {
    let mut s = LayerStore::default();
    let a = add(&mut s, "A", vec![], Some(50), 1);
    let b = add(&mut s, "B", vec![(a, 0)], None, 2);
    add(&mut s, "C", vec![(a, 0), (b, 0)], None, 3);
    assert!(s.reuse.rejected_nonconvex > 0);
    check_cover(&s);
}

#[test]
fn decoder_detects_corrupted_boundary_and_effect_evidence() {
    let mut s = LayerStore::default();
    for k in 0..4 {
        let a = add(&mut s, "A", vec![], Some(100 + k), 1000 + k * 2);
        add(&mut s, "B", vec![(a, 0)], None, 1001 + k * 2);
    }
    let mut bad = s.reuse.clone();
    assert!(!bad.uses.is_empty());
    bad.uses[0].inputs[0] = Reference::External(usize::MAX);
    assert!(bad.verify(&s).is_err());
    let mut bad = s.reuse.clone();
    let t = bad.uses[0].template;
    bad.templates[t].pattern.steps[0].effects.clear();
    assert!(bad.verify(&s).is_err());
}

#[test]
fn recut_preserves_handles_and_never_increases_selected_codec() {
    let mut s = LayerStore::default();
    for k in 0..12 {
        let a = add(&mut s, "A", vec![], Some(100 + k), 1000 + k * 2);
        add(&mut s, "B", vec![(a, 0)], None, 1001 + k * 2);
    }
    let before = serde_json::to_value(&s.reuse.residuals).unwrap();
    let cost = |s: &LayerStore| {
        let r = s.reuse.report();
        r["stats"]["selected_wiring_units"].as_u64().unwrap()
            + r["stats"]["used_dictionary_units"].as_u64().unwrap()
    };
    let old = cost(&s);
    let mut reuse = std::mem::take(&mut s.reuse);
    reuse.repartition(&s);
    s.reuse = reuse;
    assert!(cost(&s) <= old);
    assert_eq!(before, serde_json::to_value(&s.reuse.residuals).unwrap());
    check_cover(&s);
    add(&mut s, "Continuation", vec![(4, 0)], None, 99999);
    check_cover(&s);
}

#[test]
fn dictionary_charge_splits_an_unamortized_use() {
    let mut s = LayerStore::default();
    for k in 0..3 {
        let a = add(&mut s, "A", vec![], Some(100 + k), 1000 + k * 2);
        add(&mut s, "B", vec![(a, 0)], None, 1001 + k * 2);
    }
    assert!(!s.reuse.active_uses.is_empty());
    let saved = serde_json::to_value(&s.reuse.residuals).unwrap();
    let mut reuse = std::mem::take(&mut s.reuse);
    reuse.repartition(&s);
    s.reuse = reuse;
    assert!(
        s.reuse.active_uses.is_empty(),
        "a lone Use cannot pay for this dictionary entry"
    );
    assert!(s.reuse.cut_removed_uses > 0);
    assert_eq!(saved, serde_json::to_value(&s.reuse.residuals).unwrap());
    check_cover(&s);
}
