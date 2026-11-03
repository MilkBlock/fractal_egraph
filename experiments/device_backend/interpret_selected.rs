//! Execute Tiger's selected program using eggcc's production converter and VM.
//! This is a finite-input check, not a proof that two final egraphs are equal.
use dag_in_context::{
    from_egglog::FromEgglog,
    interpreter::{interpret_dag_prog, Value},
    schema::{BaseType, Constant, Expr, TreeProgram, Type},
};
use indexmap::IndexMap;
use serde_json::{json, Value as Json};

fn input(ty: &Type, args: &[Json]) -> Value {
    let mut pos = 0;
    let mut scalar = |ty: &BaseType| {
        if *ty == BaseType::StateT { return Value::StateV; }
        let arg = &args[pos]; pos += 1;
        Value::Const(match ty {
            BaseType::IntT => Constant::Int(arg.as_i64().expect("integer input")),
            BaseType::FloatT => Constant::Float(arg.as_f64().expect("float input").into()),
            BaseType::BoolT => Constant::Bool(arg.as_bool().expect("boolean input")),
            _ => panic!("input type is not supported by this test driver: {ty:?}"),
        })
    };
    let value = match ty {
        Type::Base(t) => scalar(t),
        Type::TupleT(ts) => Value::Tuple(ts.iter().map(&mut scalar).collect()),
        _ => panic!("unresolved function input type: {ty:?}"),
    };
    assert_eq!(pos, args.len(), "unused input arguments"); value
}
fn output(v: &Value) -> Json {
    match v {
        Value::Const(Constant::Int(x)) => json!({"int":x}),
        Value::Const(Constant::Float(x)) => json!({"float_bits":format!("{:016x}",x.to_bits())}),
        Value::Const(Constant::Bool(x)) => json!({"bool":x}),
        Value::Tuple(xs) => json!({"tuple":xs.iter().map(output).collect::<Vec<_>>()}),
        Value::StateV => json!({"state":true}),
        Value::Ptr(_) => panic!("pointer return requires an observational memory checker"),
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 4, "usage: selected-vm selected.egg entry inputs.json");
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(None, &std::fs::read_to_string(&args[1]).unwrap()).unwrap();
    let (_, terms, dag) = eg.function_to_dag("Function", usize::MAX, true).unwrap();
    // One converter preserves Rc sharing across all selected functions.
    let mut converter = FromEgglog { termdag: &dag, conversion_cache: IndexMap::new() };
    let mut functions = IndexMap::new();
    for term in terms.unwrap() {
        let expr = converter.expr_from_egglog(term);
        let Expr::Function(name, _, _, _) = expr.as_ref() else { panic!("not a Function") };
        assert!(functions.insert(name.clone(), expr).is_none(), "duplicate selected function");
    }
    let entry = functions.shift_remove(&args[2]).expect("entry function");
    let prog = TreeProgram { entry, functions: functions.into_values().collect() };
    let Expr::Function(_, ty, _, _) = prog.entry.as_ref() else { unreachable!() };
    let tests: Vec<Vec<Json>> = serde_json::from_str(&std::fs::read_to_string(&args[3]).unwrap()).unwrap();
    let results: Vec<_> = tests.iter().map(|args| {
        let (value, log) = interpret_dag_prog(&prog, &input(ty, args));
        json!({"input":args,"return":output(&value),"log":log})
    }).collect();
    println!("{}", json!({"entry":args[2],"functions":prog.functions.len()+1,"results":results}));
}
