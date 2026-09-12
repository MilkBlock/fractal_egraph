use egg_layout::{
    binding_program::Term, effect_program::Fact, tier1_effects::*, trigger_bridge::BindingRelation,
};
use serde_json::json;
use std::collections::BTreeMap;
fn v(n: usize) -> Term {
    Term::new("E", format!("literal:{n}"), vec![])
}
fn p(n: usize) -> Term {
    Term::new("E", format!("in:{n}"), vec![])
}
fn fact(n: &str, args: Vec<Term>) -> Effect {
    Effect::Fact(Fact::Relation(n.into(), args))
}
fn req(facts: Vec<Fact>) -> BindingRelation {
    BindingRelation {
        sorts: vec!["E".into(); 2],
        inputs: vec![0, 1],
        outputs: vec![0, 1],
        facts,
        equalities: vec![],
    }
}
fn binds() -> BTreeMap<usize, Term> {
    BTreeMap::from([(0, v(0)), (1, v(1))])
}
#[test]
fn contextual_union_enables_a_joint_binding_without_leaking_to_other_contexts() {
    let mut t = Tier1::new().unwrap();
    let base = t
        .entry(&[fact("P", vec![v(0)]), fact("Q", vec![v(2), v(1)])])
        .unwrap();
    let r = req(vec![
        Fact::Relation("P".into(), vec![p(0)]),
        Fact::Relation("Q".into(), vec![p(0), p(1)]),
    ]);
    let old = t
        .apply(base, "R", &r, &binds(), &[fact("T", vec![p(1)])])
        .unwrap();
    t.saturate().unwrap();
    assert!(!t.ready(old).unwrap());
    assert!(!t.satisfies(old, &fact("T", vec![v(1)])).unwrap());
    let eq = t.entry(&[Effect::Equal(v(0), v(2))]).unwrap();
    let merged = t.join(base, eq).unwrap();
    let enabled = t
        .apply(merged, "R", &r, &binds(), &[fact("T", vec![p(1)])])
        .unwrap();
    t.saturate().unwrap();
    assert!(t.ready(enabled).unwrap());
    assert!(t.satisfies(enabled, &fact("T", vec![v(1)])).unwrap());
    assert!(!t.ready(old).unwrap());
    emit(
        "union",
        json!({"analysis":t.report().unwrap(),"blocked_context":old.0,"enabled_context":enabled.0}),
    );
}
#[test]
fn redundant_support_is_local_and_cannot_use_the_removed_producer_indirectly() {
    let mut t = Tier1::new().unwrap();
    let a = t.entry(&[fact("P", vec![v(0)])]).unwrap();
    let r = req(vec![Fact::Relation("P".into(), vec![p(0)])]);
    let b = t
        .apply(a, "produce-Q", &r, &binds(), &[fact("Q", vec![v(0)])])
        .unwrap();
    let joined = t.join(a, b).unwrap();
    let old = t.apply(joined, "use-P", &r, &binds(), &[]).unwrap();
    let new = t.without_join_parent(old, b).unwrap();
    t.saturate().unwrap();
    assert!(t.redundant(old, new, b).unwrap());
    assert!(t.satisfies(old, &fact("Q", vec![v(0)])).unwrap());
    assert!(!t.satisfies(new, &fact("Q", vec![v(0)])).unwrap());
    let q = req(vec![Fact::Relation("Q".into(), vec![p(0)])]);
    let c = t.apply(b, "forward-Q", &q, &binds(), &[]).unwrap();
    let bc = t.join(b, c).unwrap();
    let use_q = t.apply(bc, "use-Q", &q, &binds(), &[]).unwrap();
    let shortcut = t.without_join_parent(use_q, b).unwrap();
    t.saturate().unwrap();
    assert!(t.ready(shortcut).unwrap());
    assert!(!t.redundant(use_q, shortcut, b).unwrap());
    emit("redundancy", t.report().unwrap());
}
#[test]
fn unresolved_binding_does_not_manufacture_an_effect() {
    let mut t = Tier1::new().unwrap();
    let c = t.entry(&[fact("P", vec![v(0)])]).unwrap();
    let r = req(vec![Fact::Relation("P".into(), vec![p(0)])]);
    let a = t
        .apply(
            c,
            "R",
            &r,
            &BTreeMap::from([(0, v(0))]),
            &[fact("T", vec![p(1)])],
        )
        .unwrap();
    t.saturate().unwrap();
    assert!(!t.ready(a).unwrap());
    let b = t
        .apply(c, "R", &r, &binds(), &[fact("T", vec![p(1)])])
        .unwrap();
    t.saturate().unwrap();
    assert!(t.ready(b).unwrap());
    assert!(!t.ready(a).unwrap());
}

#[test]
fn arriving_effect_reclassifies_same_application_and_updates_use_dominance() {
    let mut t = Tier1::new().unwrap();
    let a = t.entry(&[fact("P", vec![v(0)])]).unwrap();
    let b = t.entry(&[fact("Q", vec![v(1)])]).unwrap();
    let r = req(vec![
        Fact::Relation("P".into(), vec![p(0)]),
        Fact::Relation("Q".into(), vec![p(1)]),
    ]);
    let app = t
        .apply(a, "R", &r, &binds(), &[fact("T", vec![p(1)])])
        .unwrap();
    t.saturate().unwrap();
    assert!(!t.ready(app).unwrap());
    assert!(!t.dominates_for_use(a, app).unwrap());
    let before = t.report().unwrap();
    t.add_entry_effects(a, &[fact("Q", vec![v(1)])]).unwrap();
    t.saturate().unwrap();
    assert!(t.ready(app).unwrap());
    assert!(t.dominates_for_use(a, app).unwrap());
    assert!(!t.dominates_for_use(b, app).unwrap());
    assert!(t.add_entry_effects(app, &[]).is_err());
    emit(
        "dynamic",
        json!({"before":before,"after":t.report().unwrap(),"same_application":app.0}),
    );
}
#[test]
fn equal_constructor_values_do_not_unify_children_and_sorts_are_checked() {
    let mut t = Tier1::new().unwrap();
    let f = |x| Term::new("E", "F", vec![x]);
    let c = t
        .entry(&[Effect::Equal(f(v(0)), f(v(1))), fact("P", vec![v(0)])])
        .unwrap();
    assert!(!t.satisfies(c, &fact("P", vec![v(1)])).unwrap());
    assert!(
        t.entry(&[Effect::Equal(v(0), Term::new("Other", "literal:0", vec![]))])
            .is_err()
    );
    let r = req(vec![]);
    assert!(
        t.apply(c, "bad-output", &r, &binds(), &[fact("T", vec![p(99)])])
            .is_err()
    );
}

fn emit(name: &str, value: serde_json::Value) {
    if let Ok(dir) = std::env::var("TIER1_EFFECT_REPORT_DIR") {
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.json")),
            serde_json::to_string_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}
