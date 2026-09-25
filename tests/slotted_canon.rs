use egg_layout::{
    coarse_smooth::RelativeBinding,
    reuse_graph::{Graft, InstanceId, RuntimeEvent},
    saturated_rule_composition::{Comparison, Row, SaturatedRuleComposition, Vertex, compare},
    slotted_canon::{GraftCandidate, SlotDescriptor, SlottedCanon},
};
use serde_json::json;
use std::collections::BTreeSet;

fn body(offset: usize) -> SaturatedRuleComposition {
    SaturatedRuleComposition {
        version: 2,
        local_ids: vec![],
        scope: json!({"rules":["seed"],"datatype":"E"}),
        values: vec![
            Vertex {
                sort: "E".into(),
                literal: None,
            },
            Vertex {
                sort: "E".into(),
                literal: None,
            },
            Vertex {
                sort: "E".into(),
                literal: None,
            },
        ],
        rows: vec![Row {
            op: "Add".into(),
            args: vec![offset, offset + 1],
            result: offset + 2,
        }],
        subsumed_rows: vec![],
        ports: Default::default(),
    }
}
fn candidate(rule: &str, target: usize, asymmetric: bool) -> GraftCandidate {
    let slots = if asymmetric {
        vec![
            SlotDescriptor {
                sort: "E".into(),
                role: "left".into(),
                upper_degree: 1,
                lower_degree: 0,
                alias_class: 0,
            },
            SlotDescriptor {
                sort: "E".into(),
                role: "right".into(),
                upper_degree: 0,
                lower_degree: 1,
                alias_class: 1,
            },
        ]
    } else {
        vec![
            SlotDescriptor {
                sort: "E".into(),
                role: "input".into(),
                upper_degree: 1,
                lower_degree: 1,
                alias_class: 0,
            },
            SlotDescriptor {
                sort: "E".into(),
                role: "input".into(),
                upper_degree: 1,
                lower_degree: 1,
                alias_class: 0,
            },
        ]
    };
    GraftCandidate {
        rule: rule.into(),
        slots,
        targets: [target].into_iter().collect(),
        upper: [target].into_iter().collect(),
        lower: BTreeSet::new(),
        binding: vec![RelativeBinding::External { slot: target }],
        residual_ops: vec!["C".into(), "D".into()],
        witness: None,
    }
}
#[test]
fn slotted_canon_collapses_symmetric_grafts_but_keeps_ripsat_canon_separate() {
    let a = body(0);
    let mut b = a.clone();
    // Same complete body after a value-slot renaming: RipSat canonicalization
    // identifies it by graph isomorphism, independently of Graft positions.
    b.values.swap(0, 1);
    b.rows[0].args.swap(0, 1);
    assert!(matches!(
        compare(&a, &b).unwrap(),
        Comparison::Equivalent { .. }
    ));

    let mut canon = SlottedCanon::new();
    let first = canon.insert(candidate("graft", 0, false));
    let second = canon.insert(candidate("graft", 1, false));
    let third = canon.insert(candidate("graft", 0, true));
    assert_eq!(
        first, second,
        "symmetric RipenInput positions share one Graft orbit"
    );
    assert_ne!(first, third, "role/connection asymmetry splits the orbit");
    let report = canon.report();
    assert_eq!(report.candidates, 3);
    assert_eq!(report.orbits, 2);
    assert_eq!(report.orbit_sizes, vec![2, 1]);
    assert!(report.compression_ratio < 1.0);
}
#[test]
fn witness_and_runtime_instance_do_not_change_slotted_identity() {
    let mut left = candidate("graft", 0, false);
    let mut right = candidate("graft", 1, false);
    left.witness = Some(egg_layout::slotted_canon::GraftWitness {
        event: 1,
        instance: 7,
    });
    right.witness = Some(egg_layout::slotted_canon::GraftWitness {
        event: 99,
        instance: 8,
    });
    let mut canon = SlottedCanon::new();
    assert_eq!(canon.insert(left), canon.insert(right));
    let runtime = Graft {
        event: RuntimeEvent { run: 4, id: 99 },
        episode: 2,
        instance: InstanceId(8),
        revision: 0,
        rule: "graft".into(),
        upper: [1].into_iter().collect(),
        lower: BTreeSet::new(),
        inner_facts: Default::default(),
    };
    let from = egg_layout::slotted_canon::from_graft(
        &runtime,
        "graft",
        vec![
            SlotDescriptor {
                sort: "E".into(),
                role: "input".into(),
                upper_degree: 1,
                lower_degree: 1,
                alias_class: 0,
            },
            SlotDescriptor {
                sort: "E".into(),
                role: "input".into(),
                upper_degree: 1,
                lower_degree: 1,
                alias_class: 0,
            },
        ],
        vec![RelativeBinding::External { slot: 1 }],
        vec!["C".into(), "D".into()],
    );
    assert_eq!(canon.insert(from), 0);
}

#[test]
fn two_symmetric_slots_do_not_absorb_an_asymmetric_slot() {
    let symmetric = |target: usize| GraftCandidate {
        rule: "three-input".into(),
        slots: vec![
            SlotDescriptor {
                sort: "E".into(),
                role: "input".into(),
                upper_degree: 1,
                lower_degree: 1,
                alias_class: 0,
            },
            SlotDescriptor {
                sort: "E".into(),
                role: "input".into(),
                upper_degree: 1,
                lower_degree: 1,
                alias_class: 0,
            },
            SlotDescriptor {
                sort: "E".into(),
                role: "anchor".into(),
                upper_degree: 0,
                lower_degree: 2,
                alias_class: 1,
            },
        ],
        targets: [target].into_iter().collect(),
        upper: [target].into_iter().collect(),
        lower: BTreeSet::new(),
        binding: vec![RelativeBinding::External { slot: target }],
        residual_ops: vec!["C".into()],
        witness: None,
    };
    let mut canon = SlottedCanon::new();
    let a = canon.insert(symmetric(0));
    let b = canon.insert(symmetric(1));
    let anchor = canon.insert(symmetric(2));
    assert_eq!(a, b);
    assert_ne!(a, anchor);
    assert_eq!(canon.report().orbit_sizes, vec![2, 1]);
}
