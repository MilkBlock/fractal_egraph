//! Native evidence for paper illustrations; restricted fragments, not full math.egg.
use egglog::{EGraph, TraceSession, ast::Command};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
fn rows(eg: &EGraph) -> Vec<Value> {
    let mut result = vec![];
    for op in ["Add", "Mul", "Sub", "Integral", "Diff", "Var", "Const"] {
        let schema = eg.get_function(op).unwrap().schema();
        eg.function_for_each(op,|r|{result.push(json!({"op":op,"args":r.vals.iter().zip(&schema.input).map(|(v,s)|eg.value_to_class_id(s,*v).to_string()).collect::<Vec<_>>(),"result":eg.value_to_class_id(&schema.output,*r.vals.last().unwrap()).to_string(),"subsumed":r.subsumed}));}).unwrap();
    }
    result
}
#[test]
fn native_math_fragments_verify_paper_figures() {
    let original = include_str!("../egglog/tests/web-demo/math.egg");
    assert!(original.contains("(rewrite (Add a (Add b c)) (Add (Add a b) c))"));
    let mut eg = EGraph::default();
    let trace = TraceSession::with_dependencies();
    let cmds = eg
        .parse_program(None, include_str!("../docs/papers/examples/math-ac.egg"))
        .unwrap();
    eg.run_program_with_trace(cmds, &trace).unwrap();
    let ac = rows(&eg);
    assert_eq!(ac.iter().filter(|r| r["op"] == "Add").count(), 12);
    assert_eq!(ac.iter().filter(|r| r["op"] == "Var").count(), 3);
    assert_eq!(
        ac.iter()
            .map(|r| r["result"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        7
    );
    let mut parser = egglog::ast::Parser::default();
    let (_, ab) = eg
        .eval_expr(
            &parser
                .get_expr_from_string(None, "(Add (Var \"a\") (Var \"b\"))")
                .unwrap(),
        )
        .unwrap();
    let (_, cv) = eg
        .eval_expr(&parser.get_expr_from_string(None, "(Var \"c\")").unwrap())
        .unwrap();
    let math = eg.get_arcsort_by(|s| s.name() == "Math");
    let key = [
        eg.value_to_class_id(&math, ab).to_string(),
        eg.value_to_class_id(&math, cv).to_string(),
    ];
    let names: BTreeMap<_, _> = trace
        .matches()
        .iter()
        .map(|m| (m.event_id, m.rule.to_string()))
        .collect();
    let links=trace.row_reads().iter().filter_map(|r|r.producer_match_event_id.and_then(|p| {
  (r.table_name.as_deref()==Some("Add") && r.key.len()==2 && r.key.iter().map(|v|eg.value_to_class_id(&math,*v).to_string()).collect::<Vec<_>>()==key && names.get(&p).is_some_and(|n|n=="paper-assoc")&&names.get(&r.match_event_id).is_some_and(|n|n=="paper-comm")).then(||json!({"producer":p,"consumer":r.match_event_id,"producer_rule":names[&p],"consumer_rule":names[&r.match_event_id],"table":r.table_name,"producer_write":r.producer_write_event_id}))
 })).collect::<Vec<_>>();
    assert!(
        !links.is_empty(),
        "association must provide a committed row read by commutation"
    );
    let mut guard = EGraph::default();
    guard
        .parse_and_run_program(None, include_str!("../docs/papers/examples/math-guard.egg"))
        .unwrap();
    let mut ibp = EGraph::default();
    let cmds = ibp
        .parse_program(None, include_str!("../docs/papers/examples/math-ibp.egg"))
        .unwrap();
    ibp.run_program(
        cmds.into_iter()
            .filter(|c| !matches!(c, Command::RunSchedule(_)))
            .collect(),
    )
    .unwrap();
    let mut prefixes = vec![];
    for k in 0..=4 {
        if k > 0 {
            ibp.parse_and_run_program(None, "(run ibp 1)").unwrap();
        }
        let rs = rows(&ibp);
        let count = |name: &str| rs.iter().filter(|r| r["op"] == name).count();
        assert_eq!(rs.len(), 6 * k + 5);
        assert_eq!(count("Integral"), 2 * k + 1);
        assert_eq!(count("Diff"), k);
        prefixes.push(json!({"steps":k,"math_nodes":rs.len(),"integral":count("Integral"),"diff":count("Diff"),"mul":count("Mul"),"sub":count("Sub")}));
    }
    let mut ccss = EGraph::default();
    ccss.parse_and_run_program(None, include_str!("../docs/papers/examples/math-ccss.egg"))
        .unwrap();
    let ccss_rows = rows(&ccss);
    for op in ["Add", "Mul"] {
        assert_eq!(ccss_rows.iter().filter(|r| r["op"] == op).count(), 12);
    }
    let mut cscs = EGraph::default();
    cscs.parse_and_run_program(None, include_str!("../docs/papers/examples/math-cscs.egg"))
        .unwrap();
    let mut finite = EGraph::default();
    let cmds = finite
        .parse_program(
            None,
            include_str!("../docs/papers/examples/math-fold-chain.egg"),
        )
        .unwrap();
    finite
        .run_program(
            cmds.into_iter()
                .filter(|c| !matches!(c, Command::RunSchedule(_) | Command::Check(..)))
                .collect(),
        )
        .unwrap();
    let mut finite_steps = vec![];
    for k in 0..=4 {
        if k > 0 {
            finite.parse_and_run_program(None, "(run fold 1)").unwrap();
        }
        let rs = rows(&finite);
        assert_eq!(rs.iter().filter(|r| r["op"] == "Add").count(), 4);
        assert_eq!(rs.iter().filter(|r| r["op"] == "Const").count(), k + 1);
        finite_steps
            .push(json!({"steps":k,"remaining_frontier":4-k,"add_rows":4,"const_rows":k+1}));
    }
    finite
        .parse_and_run_program(None, "(check (= seed (Const 5.0)))")
        .unwrap();
    let mut visibility = EGraph::default();
    visibility
        .parse_and_run_program(
            None,
            include_str!("../docs/papers/examples/math-visibility.egg"),
        )
        .unwrap();
    let vr = rows(&visibility);
    assert_eq!(
        vr.iter()
            .filter(|r| r["op"] == "Add" && r["subsumed"] == true)
            .count(),
        1
    );
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("out/paper-math-examples");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("evidence.json"),serde_json::to_vec_pretty(&json!({"source":"egglog/tests/web-demo/math.egg","scope":"native execution of extracted rule fragments; full optimizer not saturated","ac_rows":ac,"ccss_rows":ccss_rows,"cscs_checks":"staged outer context and nested distribution checked","assoc_to_comm_witnesses":links,"ibp_prefixes":prefixes,"finite_fold":finite_steps,"visibility_rows":vr,"guard_checks":"missing guard blocks rule; supplied guard enables it"})).unwrap()).unwrap();
}
