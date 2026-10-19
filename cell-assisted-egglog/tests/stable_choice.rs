use cell_assisted_egglog::stable_choice;
use egglog::EGraph;
fn setup(facts: &str) -> EGraph {
    let mut eg = EGraph::default();
    let commands = eg
        .parse_program(
            None,
            &format!(
                r#"
      (datatype Expr (N i64) (Pair Expr Expr))
      (function Expr-size (Expr) i64 :merge (max old new))
      (function to-hoist (Expr Expr) Expr :merge new)
      (relation Candidate (Expr))
      (ruleset boundary-analysis)
      (rule ((Candidate e)) ((set (to-hoist (N 0) (N 0)) e)) :ruleset boundary-analysis)
      (N 0) {facts}
    "#
            ),
        )
        .unwrap();
    stable_choice::setup(&mut eg, &commands).unwrap();
    eg
}
fn step(eg: &mut EGraph) {
    let c = eg.parse_program(None, "(run boundary-analysis 1)").unwrap();
    let c = stable_choice::schedule(eg, &c).unwrap();
    eg.run_program(c).unwrap();
}
#[test]
fn tie_is_structural_not_insertion_order_and_larger_size_still_wins() {
    for facts in ["(N 1) (N 2)", "(N 2) (N 1)"] {
        let mut eg = setup(&format!(
            "{facts} (Candidate (N 1)) (Candidate (N 2)) (set (Expr-size (N 1)) 1) (set (Expr-size (N 2)) 1)"
        ));
        step(&mut eg);
        eg.parse_and_run_program(None, "(check (= (to-hoist (N 0) (N 0)) (N 1)))")
            .unwrap();
        eg.parse_and_run_program(
            None,
            "(Candidate (Pair (N 2) (N 2))) (set (Expr-size (Pair (N 2) (N 2))) 3)",
        )
        .unwrap();
        step(&mut eg);
        eg.parse_and_run_program(
            None,
            "(check (= (to-hoist (N 0) (N 0)) (Pair (N 2) (N 2))))",
        )
        .unwrap();
    }
}
#[test]
fn union_and_cyclic_constructor_keys_converge() {
    let mut eg = setup(
        "(Candidate (N 1)) (Candidate (N 2)) (set (Expr-size (N 1)) 1) (set (Expr-size (N 2)) 1) (union (N 2) (Pair (N 2) (N 2)))",
    );
    step(&mut eg);
    eg.parse_and_run_program(
        None,
        "(check (= (to-hoist (N 0) (N 0)) (N 1))) (union (N 1) (N 2))",
    )
    .unwrap();
    step(&mut eg);
    eg.parse_and_run_program(None, "(check (= (to-hoist (N 0) (N 0)) (N 2)))")
        .unwrap();
}
#[test]
fn newly_created_equal_size_candidates_do_not_use_their_allocated_ids() {
    for facts in [
        "(Pair (N 1) (N 2)) (Pair (N 2) (N 1))",
        "(Pair (N 2) (N 1)) (Pair (N 1) (N 2))",
    ] {
        let mut eg = setup("(N 1) (N 2)");
        eg.parse_and_run_program(None,&format!("{facts} (Candidate (Pair (N 1) (N 2))) (Candidate (Pair (N 2) (N 1))) (set (Expr-size (Pair (N 1) (N 2))) 2) (set (Expr-size (Pair (N 2) (N 1))) 2)")).unwrap();
        step(&mut eg);
        eg.parse_and_run_program(
            None,
            "(check (= (to-hoist (N 0) (N 0)) (Pair (N 1) (N 2))))",
        )
        .unwrap();
    }
}
