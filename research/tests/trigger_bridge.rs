use egg_layout::{
    binding_program::Term,
    effect_program::{Fact, Summary},
    trigger_bridge::{BindingRelation, BridgeAction, search},
};
use serde_json::json;
use std::collections::BTreeMap;
fn port(i: usize) -> Term {
    Term::new("i64", format!("in:{i}"), vec![])
}
fn val(i: i64) -> Term {
    Term::new("i64", format!("literal:{i}"), vec![])
}
fn rel(name: &str, args: Vec<Term>) -> Fact {
    Fact::Relation(name.into(), args)
}
fn empty() -> Summary {
    Summary::identity(BTreeMap::new())
}
fn trigger() -> BindingRelation {
    BindingRelation {
        sorts: vec!["i64".into(), "i64".into()],
        inputs: vec![0],
        outputs: vec![1],
        facts: vec![rel("Ready", vec![port(0), port(1)])],
        equalities: vec![],
    }
}
fn actions(x: i64, y: i64) -> Vec<BridgeAction> {
    [
        ("seed-to-link", "Seed", "Link"),
        ("link-to-ready", "Link", "Ready"),
    ]
    .into_iter()
    .map(|(name, from, to)| {
        let mut effect = empty();
        effect.requires.insert(rel(from, vec![val(x), val(y)]));
        effect.adds.insert(rel(to, vec![val(x), val(y)]));
        BridgeAction {
            name: name.into(),
            effect,
        }
    })
    .collect()
}
#[test]
fn native_starts_share_template_but_need_different_bridges() {
    let mut rows = vec![];
    for (k, initial, expected) in [(0, "Ready", 0), (1, "Link", 1), (2, "Seed", 2)] {
        let (x, y) = (10 + k, 20 + k);
        let mut state = empty();
        state.requires.insert(rel(initial, vec![val(x), val(y)]));
        let bindings = BTreeMap::from([(0, val(x)), (1, val(y))]);
        let before = trigger().gap(&state, &bindings).unwrap().json();
        let candidates = actions(x, y);
        let result = search(state, &trigger(), &bindings, &candidates, 16).unwrap();
        let report = result.json(&trigger(), &bindings, &candidates).unwrap();
        let plan = result.plan.as_ref().unwrap();
        assert_eq!(plan.len(), expected);
        let mut eg = egglog::EGraph::default();
        eg.parse_and_run_program(
            None,
            &format!(
                r#"
            (relation Seed (i64 i64)) (relation Link (i64 i64)) (relation Ready (i64 i64))
            (ruleset first) (ruleset second)
            (rule ((Seed x y)) ((Link x y)) :ruleset first)
            (rule ((Link x y)) ((Ready x y)) :ruleset second)
            ({initial} {x} {y})
        "#
            ),
        )
        .unwrap();
        for &step in plan {
            eg.parse_and_run_program(
                None,
                if step == 0 {
                    "(run first 1)"
                } else {
                    "(run second 1)"
                },
            )
            .unwrap();
        }
        eg.parse_and_run_program(None, &format!("(check (Ready {x} {y}))"))
            .unwrap();
        rows.push(json!({"initial":initial,"binding":[x,y],"template_key":trigger().canonical_key().unwrap(),"initial_gap":before,"bridge":report,"native_check":true}));
    }
    assert!(
        rows.iter()
            .all(|r| r["template_key"] == rows[0]["template_key"])
    );
    if let Ok(path) = std::env::var("TRIGGER_BRIDGE_REPORT") {
        std::fs::write(path,serde_json::to_string_pretty(&json!({"scope":"symbolic bridge search with independently executed native egglog fixtures; no trace-driven extraction or scheduler replacement","cases":rows,"concrete_bindings":3,"shared_relation_templates":1})).unwrap()).unwrap();
    }
}
#[test]
fn relation_composition_keeps_existential_and_repeated_port_constraints() {
    let mut a = trigger();
    a.outputs = vec![1, 1];
    let b = BindingRelation {
        sorts: vec!["i64".into(); 3],
        inputs: vec![0, 1],
        outputs: vec![2],
        facts: vec![rel("Next", vec![port(0), port(1), port(2)])],
        equalities: vec![],
    };
    let c = a.then(&b).unwrap();
    assert_eq!(c.sorts.len(), 5);
    assert_eq!(c.outputs, vec![4]);
    assert_eq!(c.equalities, vec![(port(1), port(2)), (port(1), port(3))]);
    let gap = c.gap(&empty(), &BTreeMap::from([(0, val(1))])).unwrap();
    assert_eq!(
        gap.unbound.into_iter().collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
}
#[test]
fn alpha_renaming_shares_key_but_aliasing_does_not() {
    let a = trigger();
    let b = BindingRelation {
        sorts: a.sorts.clone(),
        inputs: vec![1],
        outputs: vec![0],
        facts: vec![rel("Ready", vec![port(1), port(0)])],
        equalities: vec![],
    };
    assert_eq!(a.canonical_key().unwrap(), b.canonical_key().unwrap());
    let mut c = a.clone();
    c.facts = vec![rel("Ready", vec![port(0), port(0)])];
    assert_ne!(a.canonical_key().unwrap(), c.canonical_key().unwrap());
    let mut c = a.clone();
    c.sorts[1] = "String".into();
    assert!(c.validate().is_err());
}
#[test]
fn union_can_enable_trigger_without_inserting_its_relation_again() {
    // Use a genuine equality sort: unions on primitive i64 are not native operations.
    let term = |name: &str| Term::new("E", name, vec![]);
    let mut graph = empty();
    graph
        .requires
        .insert(rel("Ready", vec![term("A"), term("B")]));
    let mut t = trigger();
    t.sorts = vec!["E".into(); 2];
    t.facts = vec![rel(
        "Ready",
        vec![
            Term::new("E", "in:0", vec![]),
            Term::new("E", "in:1", vec![]),
        ],
    )];
    let binding = BTreeMap::from([(0, term("A")), (1, term("C"))]);
    assert!(!t.gap(&graph, &binding).unwrap().satisfied());
    let mut effect = empty();
    effect.record_equality(term("B"), term("C"), false);
    let result = search(
        graph,
        &t,
        &binding,
        &[BridgeAction {
            name: "join".into(),
            effect,
        }],
        8,
    )
    .unwrap();
    assert_eq!(result.plan, Some(vec![0]));
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None,"(datatype E (A) (B) (C)) (relation Ready (E E)) (Ready (A) (B)) (union (B) (C)) (check (Ready (A) (C)))").unwrap();
}
#[test]
fn missing_preconditions_are_not_injected_and_budget_is_not_impossibility() {
    let b = BTreeMap::from([(0, val(1)), (1, val(2))]);
    let a = actions(1, 2);
    let absent = search(empty(), &trigger(), &b, &a, 8).unwrap();
    assert!(absent.plan.is_none());
    assert_eq!(absent.status, "no_plan_in_supplied_actions");
    let mut graph = empty();
    graph.requires.insert(rel("Seed", vec![val(1), val(2)]));
    let bounded = search(graph, &trigger(), &b, &a, 1).unwrap();
    assert_eq!(bounded.status, "budget_unknown");
    let unresolved = search(empty(), &trigger(), &BTreeMap::new(), &a, 8).unwrap();
    assert_eq!(unresolved.status, "binding_unresolved");
}
#[test]
fn equality_of_constructors_does_not_unify_their_arguments() {
    let wrap = |t| Term::new("i64", "Wrap", vec![t]);
    let mut graph = empty();
    graph.record_equality(wrap(val(1)), wrap(val(2)), false);
    let mut t = trigger();
    t.facts.clear();
    t.equalities = vec![(port(0), port(1))];
    assert!(
        !t.gap(&graph, &BTreeMap::from([(0, val(1)), (1, val(2))]))
            .unwrap()
            .satisfied()
    );
}
