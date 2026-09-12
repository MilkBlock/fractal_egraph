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

#[test]
fn concrete_factor_then_product_derivative_matches_native_math_rules() {
    use egglog::{EGraph, ast::Command};
    let source = include_str!("../experiments/annotated_export/math_microbenchmark/combined.egg");
    let mut eg = EGraph::default();
    let commands = eg.parse_program(None, source).unwrap();
    eg.run_program(vec![commands[0].clone()]).unwrap();
    eg.parse_and_run_program(None, "(ruleset factor) (ruleset derivative)")
        .unwrap();
    for (id, rs) in [("R10", "factor"), ("R15", "derivative")] {
        let mut rule = commands
            .iter()
            .find_map(|c| match c {
                Command::Rule { rule } if rule.name == id => Some(rule.clone()),
                _ => None,
            })
            .unwrap();
        rule.ruleset = rs.into();
        eg.run_program(vec![Command::Rule { rule }]).unwrap();
    }
    eg.parse_and_run_program(None,"(let $seed (Diff (Var \"x\") (Add (Mul (Var \"x\") (Const 2)) (Mul (Var \"x\") (Const 3)))))").unwrap();
    fn check(eg: &mut EGraph, q: &str) -> bool {
        match eg.parse_and_run_program(None, &format!("(check {q})")) {
            Ok(_) => true,
            Err(egglog::Error::CheckError(..)) => false,
            Err(e) => panic!("{e}"),
        }
    }
    let lhs = "(= d (Diff (Var \"x\") (Mul (Var \"x\") (Add (Const 2) (Const 3)))))";
    assert!(!check(&mut eg, lhs));
    let mut shape_only = eg.clone();
    shape_only
        .parse_and_run_program(None, "(Mul (Var \"x\") (Add (Const 2) (Const 3)))")
        .unwrap();
    assert!(!check(&mut shape_only, lhs));
    eg.parse_and_run_program(None, "(run factor 1)").unwrap();
    assert!(check(&mut eg, lhs));
    eg.parse_and_run_program(None, "(run derivative 1)")
        .unwrap();
    assert!(check(
        &mut eg,
        "(= $seed (Add (Mul (Var \"x\") (Diff (Var \"x\") (Add (Const 2) (Const 3)))) (Mul (Add (Const 2) (Const 3)) (Diff (Var \"x\") (Var \"x\")))))"
    ));

    let named = |s: &str| Term::new("Math", s, vec![]);
    let (x, two, three, u, vv, p, s, q, d) = (
        named("x"),
        named("2"),
        named("3"),
        named("u"),
        named("v"),
        named("p"),
        named("s"),
        named("q"),
        named("d"),
    );
    let f = |name: &str, args: Vec<Term>| Fact::Relation(name.into(), args);
    let initial = vec![
        f("Mul", vec![x.clone(), two.clone(), u.clone()]),
        f("Mul", vec![x.clone(), three.clone(), vv.clone()]),
        f("Add", vec![u.clone(), vv.clone(), p.clone()]),
        f("Diff", vec![x.clone(), p.clone(), d.clone()]),
    ];
    let contract = |facts: Vec<Fact>| BindingRelation {
        sorts: vec![],
        inputs: vec![],
        outputs: vec![],
        facts,
        equalities: vec![],
    };
    let r10 = contract(initial[..3].to_vec());
    let r15 = contract(vec![
        f("Mul", vec![x.clone(), s.clone(), q.clone()]),
        f("Diff", vec![x.clone(), q.clone(), d.clone()]),
    ]);
    let r10_effects = vec![
        Effect::Fact(f("Add", vec![two, three, s.clone()])),
        Effect::Fact(f("Mul", vec![x.clone(), s.clone(), q.clone()])),
        Effect::Equal(p, q),
    ];
    let (dxs, dxx, l, r, z) = (
        named("dxs"),
        named("dxx"),
        named("left"),
        named("right"),
        named("z"),
    );
    let r15_effects = vec![
        Effect::Fact(f("Diff", vec![x.clone(), s.clone(), dxs.clone()])),
        Effect::Fact(f("Diff", vec![x.clone(), x.clone(), dxx.clone()])),
        Effect::Fact(f("Mul", vec![x, dxs, l.clone()])),
        Effect::Fact(f("Mul", vec![s, dxx, r.clone()])),
        Effect::Fact(f("Add", vec![l, r, z.clone()])),
        Effect::Equal(d, z),
    ];
    let mut t = Tier1::new().unwrap();
    let c0 = t
        .entry(&initial.into_iter().map(Effect::Fact).collect::<Vec<_>>())
        .unwrap();
    let blocked = t
        .apply(
            c0,
            "R15: product derivative",
            &r15,
            &BTreeMap::new(),
            &r15_effects,
        )
        .unwrap();
    let factor = t
        .apply(c0, "R10: factor", &r10, &BTreeMap::new(), &r10_effects)
        .unwrap();
    let enabled = t
        .apply(
            factor,
            "R15: product derivative",
            &r15,
            &BTreeMap::new(),
            &r15_effects,
        )
        .unwrap();
    t.saturate().unwrap();
    assert!(!t.ready(blocked).unwrap());
    assert!(t.ready(enabled).unwrap());
    emit(
        "concrete",
        json!({"analysis":t.report().unwrap(),"native_tier0_checks":{"R15_before_factor":false,"R15_after_shape_only":false,"R15_after_R10":true,"expected_result_after_R15":true},"labels":{"u":"x*2","v":"x*3","p":"x*2+x*3","s":"2+3","q":"x*(2+3)","d":"Diff(x,p)","dxs":"Diff(x,s)","dxx":"Diff(x,x)","left":"x*dxs","right":"s*dxx","z":"left+right"},"scope":"fixed ground contracts for actual R10/R15; independently checked in native tier-0, not an automatic trace importer"}),
    );
}

#[test]
fn standalone_concrete_egg_files_execute_all_embedded_checks() {
    for path in [
        "experiments/tier1_effects/concrete_math.egg",
        "experiments/tier1_effects/concrete_tier1.egg",
    ] {
        let mut eg = egglog::EGraph::default();
        eg.parse_and_run_program(Some(path.into()), &std::fs::read_to_string(path).unwrap())
            .unwrap();
    }
}
