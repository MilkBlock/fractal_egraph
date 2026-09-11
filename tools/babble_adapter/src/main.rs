use babble::{
    ast_node::{Arity, AstNode, Expr, Printable, Printer},
    experiments::{BeamExperiment, Experiment},
    extract::{apply_libs, beam::PartialLibCost},
    learn::LibId,
    teachable::{BindingExpr, DeBruijnIndex, Teachable},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fmt};
mod au_reference;
mod from_candidates;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum Op {
    Data(String, usize),
    List,
    Var(usize),
    Ref(usize),
    Lambda,
    Apply,
    Lib(usize),
}
impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl Arity for Op {
    fn min_arity(&self) -> usize {
        match self {
            Self::Data(_, n) => *n,
            Self::List | Self::Var(_) | Self::Ref(_) => 0,
            Self::Lambda => 1,
            _ => 2,
        }
    }
    fn max_arity(&self) -> Option<usize> {
        if matches!(self, Self::List) {
            None
        } else {
            Some(self.min_arity())
        }
    }
}
impl Teachable for Op {
    fn from_binding_expr<T>(b: BindingExpr<T>) -> AstNode<Self, T> {
        match b {
            BindingExpr::Var(i) => AstNode::new(Self::Var(i.0), []),
            BindingExpr::LibVar(i) => AstNode::new(Self::Ref(i.0), []),
            BindingExpr::Lambda(x) => AstNode::new(Self::Lambda, [x]),
            BindingExpr::Apply(x, y) => AstNode::new(Self::Apply, [x, y]),
            BindingExpr::Lib(i, x, y) => AstNode::new(Self::Lib(i.0), [x, y]),
        }
    }
    fn as_binding_expr<T>(n: &AstNode<Self, T>) -> Option<BindingExpr<&T>> {
        Some(match (n.operation(), n.args()) {
            (Self::Var(i), []) => BindingExpr::Var(DeBruijnIndex(*i)),
            (Self::Ref(i), []) => BindingExpr::LibVar(LibId(*i)),
            (Self::Lambda, [x]) => BindingExpr::Lambda(x),
            (Self::Apply, [x, y]) => BindingExpr::Apply(x, y),
            (Self::Lib(i), [x, y]) => BindingExpr::Lib(LibId(*i), x, y),
            _ => return None,
        })
    }
    fn list() -> Self {
        Self::List
    }
}
impl Printable for Op {
    fn precedence(&self) -> u8 {
        0
    }
    fn print_naked<W: fmt::Write>(e: &Expr<Self>, p: &mut Printer<W>) -> fmt::Result {
        write!(p.writer, "({}", e.0.operation())?;
        for a in e.0.args() {
            write!(p.writer, " ")?;
            p.print(a)?;
        }
        write!(p.writer, ")")
    }
}
fn node(op: Op, args: Vec<Expr<Op>>) -> Expr<Op> {
    Expr(AstNode::new(op, args))
}
// Lossless JSON tree encoding. No source token is interpreted as a lambda or union.
#[cfg(test)]
fn encode(v: &Value) -> Expr<Op> {
    encode_mode(v, true)
}
fn encode_mode(v: &Value, compact: bool) -> Expr<Op> {
    let data = |name: String, args: Vec<Expr<Op>>| node(Op::Data(name, args.len()), args);
    if compact {
        if let Some(m) = v.as_object() {
            if m.len() == 3
                && m.contains_key("sort")
                && m.contains_key("op")
                && m.contains_key("args")
            {
                return data(
                    format!("term:{}", json!([v["sort"], v["op"]])),
                    v["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| encode_mode(v, compact))
                        .collect(),
                );
            }
            if m.len() == 2 && m.contains_key("call") && m.contains_key("args") {
                return data(
                    format!("call:{}", v["call"]),
                    v["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| encode_mode(v, compact))
                        .collect(),
                );
            }
            if m.len() == 1 {
                let (k, x) = m.iter().next().unwrap();
                if k == "var" || k == "literal" {
                    return data(format!("leaf:{}", v), vec![]);
                }
                if let Some(a) = x.as_array() {
                    return data(
                        format!("field-array:{}", json!(k)),
                        a.iter().map(|v| encode_mode(v, compact)).collect(),
                    );
                }
            }
        }
    }
    match v {
        Value::Array(a) => data(
            "array".into(),
            a.iter().map(|v| encode_mode(v, compact)).collect(),
        ),
        Value::Object(m) => data(
            "object".into(),
            m.iter()
                .map(|(k, v)| data(format!("key:{k}"), vec![encode_mode(v, compact)]))
                .collect(),
        ),
        _ => data(format!("atom:{}", v), vec![]),
    }
}
fn decode(e: &Expr<Op>) -> Value {
    let Op::Data(label, _) = e.0.operation() else {
        panic!("not expanded data")
    };
    let args = || e.0.args().iter().map(decode).collect::<Vec<_>>();
    if let Some(t) = label.strip_prefix("term:") {
        let p: Value = serde_json::from_str(t).unwrap();
        return json!({"sort":p[0],"op":p[1],"args":args()});
    }
    if let Some(t) = label.strip_prefix("call:") {
        return json!({"call":serde_json::from_str::<Value>(t).unwrap(),"args":args()});
    }
    if let Some(t) = label
        .strip_prefix("leaf:")
        .or_else(|| label.strip_prefix("atom:"))
    {
        return serde_json::from_str(t).unwrap();
    }
    if let Some(t) = label.strip_prefix("field-array:") {
        let key: String = serde_json::from_str(t).unwrap();
        return json!({key:args()});
    }
    if label == "array" {
        return json!(args());
    }
    assert_eq!(label, "object");
    Value::Object(
        e.0.args()
            .iter()
            .map(|e| {
                let Op::Data(k, _) = e.0.operation() else {
                    panic!("not a field")
                };
                (
                    k.strip_prefix("key:").unwrap().to_owned(),
                    decode(&e.0.args()[0]),
                )
            })
            .collect(),
    )
}
fn dump(e: &Expr<Op>) -> Value {
    json!({"op":format!("{}",e.0.operation()),"args":e.0.args().iter().map(dump).collect::<Vec<_>>()})
}

// Independent lexical evaluator for learned libraries. Expansion must equal the
// exact original data AST; thus no learned abstraction is trusted as an effect law.
#[derive(Clone)]
enum Eval {
    Data(Op, Vec<Eval>),
    Closure(Expr<Op>, Vec<Eval>, BTreeMap<usize, Eval>),
}
fn eval(e: &Expr<Op>, vars: &[Eval], libs: &BTreeMap<usize, Eval>, fuel: &mut usize) -> Eval {
    assert!(*fuel > 0, "expansion budget exceeded");
    *fuel -= 1;
    let a = e.0.args();
    match e.0.operation() {
        Op::Var(i) => vars[*i].clone(),
        Op::Ref(i) => libs.get(i).expect("unbound library reference").clone(),
        Op::Lambda => Eval::Closure(a[0].clone(), vars.to_vec(), libs.clone()),
        Op::Apply => {
            let f = eval(&a[0], vars, libs, fuel);
            let x = eval(&a[1], vars, libs, fuel);
            let Eval::Closure(body, mut env, scope) = f else {
                panic!("apply to data")
            };
            env.insert(0, x);
            eval(&body, &env, &scope, fuel)
        }
        Op::Lib(i) => {
            let v = eval(&a[0], vars, libs, fuel);
            let mut scope = libs.clone();
            scope.insert(*i, v);
            eval(&a[1], vars, &scope, fuel)
        }
        op => Eval::Data(
            op.clone(),
            a.iter().map(|x| eval(x, vars, libs, fuel)).collect(),
        ),
    }
}
fn unvalue(v: Eval) -> Expr<Op> {
    match v {
        Eval::Data(op, args) => node(op, args.into_iter().map(unvalue).collect()),
        _ => panic!("unapplied closure in corpus"),
    }
}
fn verify(e: &Expr<Op>, original: &[Expr<Op>]) -> bool {
    let expanded = unvalue(eval(e, &[], &BTreeMap::new(), &mut 10_000_000));
    assert_eq!(
        expanded,
        node(Op::List, original.to_vec()),
        "lossless expansion failed"
    );
    true
}
fn exact_cost(input: &[Expr<Op>]) -> usize {
    let mut unique: Vec<&Expr<Op>> = vec![];
    for e in input {
        if !unique.contains(&e) {
            unique.push(e)
        }
    }
    // One definition wrapper each, one reference per input, one corpus root.
    unique.iter().map(|e| e.len() + 1).sum::<usize>() + input.len() + 1
}
fn definitions(e: &Expr<Op>) -> Vec<Value> {
    let mut out = vec![];
    let mut e = e;
    while let Op::Lib(id) = e.0.operation() {
        out.push(
            json!({"id":id,"definition":dump(&e.0.args()[0]),"ast_nodes":e.0.args()[0].len()}),
        );
        e = &e.0.args()[1];
    }
    out
}
fn corpus_refs(e: &Expr<Op>) -> BTreeMap<usize, usize> {
    let mut body = e;
    while matches!(body.0.operation(), Op::Lib(_)) {
        body = &body.0.args()[1];
    }
    fn visit(e: &Expr<Op>, counts: &mut BTreeMap<usize, usize>) {
        if let Op::Ref(i) = e.0.operation() {
            *counts.entry(*i).or_default() += 1;
        }
        for a in e.0.args() {
            visit(a, counts);
        }
    }
    let mut counts = BTreeMap::new();
    visit(body, &mut counts);
    counts
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        (3..=4).contains(&args.len()),
        "combine-babble corpus.json results.json [json-control]"
    );
    let compact = args.get(3).map_or(true, |x| x != "json-control");
    let corpus: Value = serde_json::from_str(&std::fs::read_to_string(&args[1]).unwrap()).unwrap();
    let entries = corpus["entries"].as_array().unwrap();
    let train: Vec<_> = entries
        .iter()
        .filter(|x| x["split"] == "train")
        .map(|x| encode_mode(&x["program"], compact))
        .collect();
    let test: Vec<_> = entries
        .iter()
        .filter(|x| x["split"] == "test")
        .map(|x| encode_mode(&x["program"], compact))
        .collect();
    assert!(!train.is_empty() && !test.is_empty());
    if args.get(3).is_some_and(|x| x == "au-reference") {
        au_reference::export(&train, &args[2]);
        return;
    }
    for entry in entries {
        assert_eq!(
            decode(&encode_mode(&entry["program"], compact)),
            entry["program"]
        );
    }
    let experiment = BeamExperiment::<Op, ()>::new(vec![], 16, 16, 2, (), false, Some(3), 2);
    let candidate_path = args.get(3).and_then(|s| s.strip_prefix("candidates="));
    let learned = if let Some(path) = candidate_path {
        let patterns: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        from_candidates::run(&train, patterns.as_array().unwrap())
    } else {
        experiment.run_multi(train.iter().cloned().map(|e| vec![e]).collect())
    };
    let train_verified = verify(&learned.final_expr, &train);
    let mut graph = egg::EGraph::<AstNode<Op>, PartialLibCost>::default();
    let roots: Vec<_> = test
        .iter()
        .cloned()
        .map(|e| graph.add_expr(&e.into()))
        .collect();
    graph.rebuild();
    let heldout: Expr<Op> = apply_libs(graph, &roots, &learned.rewrites).into();
    let test_verified = verify(&heldout, &test);
    let metrics = |input: &[Expr<Op>], out: &Expr<Op>| json!({"programs":input.len(),"raw_ast_nodes":1+input.iter().map(Expr::len).sum::<usize>(),"exact_whole_program_dictionary_nodes":exact_cost(input),"babble_corpus_plus_library_nodes":out.len()});
    let report = json!({"engine":if candidate_path.is_some(){"native egglog candidates + upstream beam/extraction"}else{"upstream babble BeamExperiment"},"encoding":if compact {"typed-ast"} else {"json-control"},"domain_equations":0,"settings":{"beam":16,"libraries_per_step":2,"max_arity":3,"library_iterations":2},"train":metrics(&train,&learned.final_expr),"test":metrics(&test,&heldout),"selected_train_libraries":learned.num_libs,"lossless_expansion":{"train":train_verified,"test":test_verified},"libraries":definitions(&learned.final_expr),"corpus_reference_counts":{"train":corpus_refs(&learned.final_expr),"test":corpus_refs(&heldout)},"scope":"offline syntax learning; conditional effects preserved as source syntax, not inferred or certified; heldout classes within one trace, not independent runs; AST counts are not runtime memory savings"});
    std::fs::write(
        format!("{}.programs.json", args[2]),
        serde_json::to_string(&json!({"train":dump(&learned.final_expr),"test":dump(&heldout)}))
            .unwrap(),
    )
    .unwrap();
    std::fs::write(&args[2], serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!(
        "{}",
        json!({"train":report["train"],"test":report["test"],"libraries":learned.num_libs,"verified":report["lossless_expansion"]})
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn learned_lambda_expands_with_repeated_arguments_and_lexical_scope() {
        let atom = encode(&json!("union"));
        let body = node(
            Op::Data("pair".into(), 2),
            vec![node(Op::Var(0), vec![]), node(Op::Var(0), vec![])],
        );
        let library = node(Op::Lambda, vec![body]);
        let call = node(Op::Apply, vec![node(Op::Ref(7), vec![]), atom.clone()]);
        let program = node(Op::Lib(7), vec![library, node(Op::List, vec![call])]);
        assert!(verify(
            &program,
            &[node(Op::Data("pair".into(), 2), vec![atom.clone(), atom])]
        ));
    }
    #[test]
    fn data_cannot_masquerade_as_binding_syntax() {
        let a = encode(&json!({"var":"$0","union":["l0","lambda"]}));
        assert!(verify(&node(Op::List, vec![a.clone()]), &[a]));
        assert_ne!(encode(&json!(1)), encode(&json!("1")));
    }
    #[test]
    #[should_panic(expected = "lossless expansion failed")]
    fn wrong_expansion_is_rejected() {
        verify(
            &node(Op::List, vec![encode(&json!(1))]),
            &[encode(&json!(2))],
        );
    }
}
