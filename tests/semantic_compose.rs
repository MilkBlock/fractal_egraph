use egg_layout::semantic_compose::{self as s, Substitution, Term};
fn v(id: usize, sort: &str) -> Term {
    Term::Var {
        id,
        sort: sort.into(),
    }
}
fn app(op: &str, args: Vec<Term>) -> Term {
    Term::App {
        op: op.into(),
        args,
        sort: "M".into(),
    }
}
fn count(e: &egglog::EGraph, op: &str) -> usize {
    let mut n = 0;
    e.function_for_each(op, |_| n += 1).unwrap();
    n
}
const DECL: &str = "(datatype M (V String) (Add M M))\n";
const RULES: &str = "(rewrite (Add a (Add b c)) (Add (Add a b) c) :name \"assoc\")\n(rewrite (Add a b) (Add b a) :name \"comm\")";
#[test]
fn typed_unification_occurs_alias_and_constructor_conflicts() {
    assert!(
        Substitution::default()
            .unify(&v(0, "M"), &app("F", vec![v(0, "M")]))
            .is_err()
    );
    assert!(
        Substitution::default()
            .unify(&v(0, "M"), &v(1, "i64"))
            .is_err()
    );
    let mut u = Substitution::default();
    assert!(
        u.unify(
            &app("Add", vec![v(0, "M"), v(0, "M")]),
            &app("Add", vec![app("A", vec![]), app("B", vec![])])
        )
        .is_err()
    );
    let mut u = Substitution::default();
    let a = app("Add", vec![v(0, "M"), v(1, "M")]);
    let b = app("Add", vec![v(2, "M"), v(2, "M")]);
    u.unify(&a, &b).unwrap();
    assert_eq!(u.apply(&a), u.apply(&b));
}
#[test]
fn source_composition_runs_natively_on_fresh_bindings() {
    let book = s::import(&(DECL.to_string() + RULES)).unwrap();
    let c = s::compose(&book["assoc"], &book["comm"], &[]).unwrap();
    assert_eq!(c.rule.effects.len(), 2);
    assert!(c.right_variable_offset > 0);
    for names in [["a", "b", "c"], ["x", "y", "z"]] {
        let seed = format!(
            "(let seed (Add (V \"{}\") (Add (V \"{}\") (V \"{}\"))))",
            names[0], names[1], names[2]
        );
        let mut e = egglog::EGraph::default();
        e.parse_and_run_program(None,&format!("{DECL}\n{}\n{seed}\n(run 1)\n(check (= seed (Add (V \"{}\") (Add (V \"{}\") (V \"{}\")))))",c.rule.egg(),names[2],names[0],names[1])).unwrap();
        assert_eq!(count(&e, "Add"), 5); // all initial + both intermediate and final rows
    }
}
#[test]
fn double_swap_keeps_effects_and_matches_sequential_execution() {
    let book = s::import(&(DECL.to_string() + RULES)).unwrap();
    let c = s::compose(&book["comm"], &book["comm"], &[]).unwrap();
    assert_eq!(c.rule.lhs, c.rule.rhs);
    assert_eq!(c.rule.effects.len(), 2);
    let seed = "(let seed (Add (V \"a\") (V \"b\")))";
    let mut e = egglog::EGraph::default();
    e.parse_and_run_program(None, &format!("{DECL}\n{}\n{seed}\n(run 1)", c.rule.egg()))
        .unwrap();
    let mut seq = egglog::EGraph::default();
    seq.parse_and_run_program(
        None,
        &format!("{DECL}\n(rewrite (Add a b) (Add b a))\n{seed}\n(run 2)"),
    )
    .unwrap();
    assert_eq!(count(&e, "Add"), 2);
    assert_eq!(count(&e, "Add"), count(&seq, "Add"));
}
#[test]
fn external_guard_is_retained_and_required() {
    let source = format!(
        "{DECL}\n(relation Ready (M))\n(rewrite (Add a b) (Add b a) :name \"first\")\n(rewrite (Add a b) (Add b a) :when ((Ready a)) :name \"second\")"
    );
    let book = s::import(&source).unwrap();
    let c = s::compose(&book["first"], &book["second"], &[]).unwrap();
    assert_eq!(c.rule.guards.len(), 1);
    let mut e = egglog::EGraph::default();
    e.parse_and_run_program(
        None,
        &format!(
            "{DECL}\n(relation Ready (M))\n{}\n(let seed (Add (V \"a\") (V \"b\")))\n(run 1)",
            c.rule.egg()
        ),
    )
    .unwrap();
    assert_eq!(count(&e, "Add"), 1);
    e.parse_and_run_program(None, "(Ready (V \"b\"))\n(run 1)")
        .unwrap();
    assert_eq!(count(&e, "Add"), 2);
}
#[test]
fn selected_inner_position_changes_result() {
    let book = s::import(&(DECL.to_string() + RULES)).unwrap();
    let root = s::compose(&book["assoc"], &book["comm"], &[]).unwrap();
    let inner = s::compose(&book["assoc"], &book["comm"], &[0]).unwrap();
    assert_ne!(root.rule.rhs, inner.rule.rhs);
    assert!(s::compose(&book["assoc"], &book["comm"], &[2]).is_err());
}

#[test]
fn existing_concat_nodes_gain_semantic_candidates_without_claiming_observed_positions() {
    use egg_layout::packet_library::{Library, Packet};
    let mut library = Library::default();
    let root = library
        .build([
            Packet::normalize("assoc".into(), vec![]),
            Packet::normalize("comm".into(), vec![]),
        ])
        .unwrap();
    let ids = library.learn_semantics(root, &(DECL.to_string() + RULES));
    assert_eq!(ids.len(), 1);
    let candidate = &library.semantic_compositions[ids[0]];
    assert_eq!(candidate["observed_position_verified"], false);
    assert!(candidate["egg"].as_str().unwrap().contains("union"));
}

#[test]
fn integration_by_parts_self_composes_at_recursive_position() {
    let decl = "(datatype M (V String) (Mul M M) (Sub M M) (Diff M M) (Integral M M))";
    let rewrite = "(rewrite (Integral (Mul a b) x) (Sub (Mul a (Integral b x)) (Integral (Mul (Diff x a) (Integral b x)) x)) :name \"ibp\")";
    let book = s::import(&format!("{decl}\n{rewrite}")).unwrap();
    let c = s::compose(&book["ibp"], &book["ibp"], &[1]).unwrap();
    let seed = "(let seed (Integral (Mul (V \"u\") (V \"v\")) (V \"x\")))";
    let mut combined = egglog::EGraph::default();
    combined
        .parse_and_run_program(None, &format!("{decl}\n{}\n{seed}\n(run 1)", c.rule.egg()))
        .unwrap();
    let mut sequential = egglog::EGraph::default();
    sequential
        .parse_and_run_program(None, &format!("{decl}\n{rewrite}\n{seed}\n(run 2)"))
        .unwrap();
    for op in ["V", "Mul", "Sub", "Diff", "Integral"] {
        assert_eq!(count(&combined, op), count(&sequential, op), "{op}");
    }
    assert_eq!(count(&combined, "Diff"), 2);
    assert_eq!(count(&combined, "Integral"), 5);
}
