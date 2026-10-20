use cell_assisted_egglog::stable_choice;
use egglog::EGraph;
const BASE: &str = include_str!("../../experiments/parameterized_cell/terms.egg");
fn setup() -> EGraph {
    let mut eg = EGraph::default();
    let source = format!(
        "{BASE}\n(function Expr-size (Expr) i64 :merge (min old new))\n(function to-hoist (Expr Expr) Expr :merge new)\n(TermLeaf 1) (TermLeaf 2) (V 0)"
    );
    let commands = eg.parse_program(None, &source).unwrap();
    stable_choice::setup_full(&mut eg, &commands).unwrap();
    eg
}
fn run(eg: &mut EGraph) {
    let cs = eg
        .parse_program(
            None,
            "(run-schedule (saturate terms-helpers (saturate terms-helpers-helpers)))",
        )
        .unwrap();
    let cs = stable_choice::schedule_full(eg, &cs).unwrap();
    eg.run_program(cs).unwrap();
}
#[test]
fn late_equal_cost_candidate_can_replace_an_earlier_choice() {
    for ids in [[2, 1], [1, 2]] {
        let mut eg = setup();
        for i in ids {
            eg.parse_and_run_program(
                None,
                &format!("(PotentialExtractedExpr (V 0) (TCPair (TermLeaf {i}) 1))"),
            )
            .unwrap();
            run(&mut eg);
        }
        eg.parse_and_run_program(
            None,
            "(check (= (ExtractedExpr (V 0)) (TCPair (TermLeaf 1) 1)))",
        )
        .unwrap();
        assert!(
            stable_choice::coverage(&eg)
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["missing"] == 0)
        );
    }
}
#[test]
fn all_constructor_sorts_include_non_expr_roots() {
    let eg = setup();
    let coverage = stable_choice::coverage(&eg).unwrap();
    assert!(
        coverage
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["sort"] == "Term"
                && r["constructor_eclasses"].as_u64().unwrap() > 0
                && r["missing"] == 0)
    );
}
#[test]
fn ivt_equal_length_uses_the_whole_candidate_key() {
    let mut eg = EGraph::default();
    let source = format!(
        r#"{BASE}
      (function Expr-size (Expr) i64 :merge (min old new))
      (function to-hoist (Expr Expr) Expr :merge new)
      (datatype TypeList (TNil))
      (datatype IVTRes (IVTAnalysisRes Expr Expr TypeList i64) (IVTMin IVTRes IVTRes))
      (ruleset ivt-analysis)
      (rule ((= lhs (IVTMin (IVTAnalysisRes a b c n) (IVTAnalysisRes d e f m))) (<= n m))
            ((union lhs (IVTAnalysisRes a b c n))) :ruleset ivt-analysis :name "left")
      (rule ((= lhs (IVTMin (IVTAnalysisRes a b c n) (IVTAnalysisRes d e f m))) (> n m))
            ((union lhs (IVTAnalysisRes d e f m))) :ruleset ivt-analysis :name "right")
      (let a (IVTAnalysisRes (V 1) (V 0) (TNil) 0))
      (let b (IVTAnalysisRes (V 2) (V 0) (TNil) 0))
      (IVTMin a b) (IVTMin b a)
    "#
    );
    let cs = eg.parse_program(None, &source).unwrap();
    stable_choice::setup_full(&mut eg, &cs).unwrap();
    let cs = eg
        .parse_program(None, "(run-schedule (saturate ivt-analysis))")
        .unwrap();
    let cs = stable_choice::schedule_full(&mut eg, &cs).unwrap();
    eg.run_program(cs).unwrap();
    eg.parse_and_run_program(
        None,
        "(check (= (IVTMin a b) a)) (check (= (IVTMin b a) a))",
    )
    .unwrap();
}
#[test]
fn unused_container_declaration_is_allowed_but_actual_instances_fail_closed() {
    let mut eg = EGraph::default();
    let source = format!(
        "{BASE} (function Expr-size (Expr) i64 :merge (min old new)) (function to-hoist (Expr Expr) Expr :merge new) (sort ExprBag (Set Expr)) (datatype Box (Items ExprBag))"
    );
    let cs = eg.parse_program(None, &source).unwrap();
    stable_choice::setup_full(&mut eg, &cs).unwrap();
    eg.parse_and_run_program(None, "(Items (set-of (V 1)))")
        .unwrap();
    let cs = eg.parse_program(None, "(run terms 1)").unwrap();
    let cs = stable_choice::schedule_full(&mut eg, &cs).unwrap();
    assert!(
        eg.run_program(cs)
            .unwrap_err()
            .to_string()
            .contains("unsupported instantiated constructor: Items")
    );
}
