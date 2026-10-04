use dag_in_context::{
    from_egglog::program_from_egglog,
    interpreter::{interpret_dag_prog, Value},
    schema::Constant,
};
use serde_json::json;
fn load(path: &str) -> (dag_in_context::schema::TreeProgram, bool) {
    let term = std::fs::read_to_string(path).unwrap();
    let mut eg = egglog::new_experimental_egraph();
    eg.parse_and_run_program(
        None,
        &(dag_in_context::prologue() + "\n(let validation_program " + &term + ")"),
    )
    .unwrap();
    let (_, value) = eg
        .eval_expr(&egglog::ast::Expr::Var(
            egglog::ast::Span::Panic,
            "validation_program".into(),
        ))
        .unwrap();
    let sort = eg.get_sort_by_name("ProgramType").unwrap();
    let (dag, id, _) = eg.extract_value(sort, value).unwrap();
    let prog = program_from_egglog(id, &dag);
    let typed = prog.with_arg_types_and_cache().0;
    let changed = typed != prog;
    (typed, changed)
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        5,
        "INPUT.term BASELINE.term CANDIDATE.term REPORT.json"
    );
    let (original, input_changed) = load(&args[1]);
    let (baseline, baseline_changed) = load(&args[2]);
    let (candidate, candidate_changed) = load(&args[3]);
    let mut cases = vec![];
    for n in [0, 1, 2, 3, 5, 10] {
        let input = Value::Tuple(vec![Value::Const(Constant::Int(n)), Value::StateV]);
        let a = interpret_dag_prog(&original, &input);
        let b = interpret_dag_prog(&baseline, &input);
        let c = interpret_dag_prog(&candidate, &input);
        let expected = (1..=n).product::<i64>().to_string();
        let ok = a == b && a == c && a.1 == vec![expected];
        cases.push(json!({"input":n,"equal":ok,"input_result":format!("{a:?}"),"baseline_result":format!("{b:?}"),"candidate_result":format!("{c:?}")}));
    }
    let passed = cases.iter().all(|c| c["equal"] == true);
    std::fs::write(&args[4],serde_json::to_vec_pretty(&json!({"typecheck":"eggcc TreeProgram::with_arg_types_and_cache passed","argument_annotations_changed":{"input":input_changed,"baseline":baseline_changed,"candidate":candidate_changed},"conversion":"eggcc FromEgglog preserves Rc sharing and removes context nodes","interpreter":"actual eggcc DAG interpreter; factorial/loopfactorial inputs","passed":passed,"cases":cases,"scope":"native tree extraction imported into eggcc; not the production Tiger/DAG extractor and not a universal program-equivalence proof"})).unwrap()).unwrap();
    if !passed {
        std::process::exit(1);
    }
}
