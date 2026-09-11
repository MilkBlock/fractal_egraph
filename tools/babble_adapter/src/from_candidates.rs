//! Native candidates connected to upstream beam selection and extraction.
use super::*;
use babble::{ast_node::PartialExpr, experiments::ExperimentResult};
type Ast = egg::PatternAst<AstNode<Op>>;
fn push(ast: &mut Ast, op: Op, args: Vec<egg::Id>) -> egg::Id {
    ast.add(egg::ENodeOrVar::ENode(AstNode::new(op, args)))
}
fn search(p: &Value, ops: &BTreeMap<String, Op>, ast: &mut Ast) -> egg::Id {
    if let Some(h) = p.get("hole") {
        return ast.add(egg::ENodeOrVar::Var(
            format!("?x{}", h.as_u64().unwrap()).parse().unwrap(),
        ));
    }
    let op = ops
        .get(p["op"].as_str().unwrap())
        .expect("unknown source operator")
        .clone();
    let args = p["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| search(p, ops, ast))
        .collect();
    push(ast, op, args)
}
fn definition(p: &Value, ops: &BTreeMap<String, Op>) -> Expr<Op> {
    if let Some(h) = p.get("hole") {
        return node(Op::Var(h.as_u64().unwrap() as usize), vec![]);
    }
    node(
        ops[p["op"].as_str().unwrap()].clone(),
        p["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| definition(p, ops))
            .collect(),
    )
}
fn append(e: &Expr<Op>, ast: &mut Ast) -> egg::Id {
    let args = e.0.args().iter().map(|a| append(a, ast)).collect();
    push(ast, e.0.operation().clone(), args)
}
pub fn run(train: &[Expr<Op>], patterns: &[Value]) -> ExperimentResult<Op> {
    fn collect(e: &Expr<Op>, ops: &mut BTreeMap<String, Op>) {
        assert!(
            matches!(e.0.operation(), Op::Data(..)),
            "external candidates require binder-free syntax"
        );
        ops.insert(e.0.operation().to_string(), e.0.operation().clone());
        for a in e.0.args() {
            collect(a, ops);
        }
    }
    let mut ops = BTreeMap::new();
    for e in train {
        collect(e, &mut ops);
    }
    // Preserve upstream candidate order, avoiding beam changes caused by JSON order.
    let mut ordered = BTreeMap::<PartialExpr<Op, egg::Var>, Value>::new();
    for p in patterns {
        let mut ast = Ast::default();
        search(p, &ops, &mut ast);
        let ptn: egg::Pattern<_> = ast.into();
        ordered.insert(ptn.into(), p.clone());
    }
    let rewrites: Vec<egg::Rewrite<AstNode<Op>, PartialLibCost>> = ordered
        .values()
        .enumerate()
        .map(|(id, p)| {
            let mut ast = Ast::default();
            search(p, &ops, &mut ast);
            let lhs: egg::Pattern<_> = ast.into();
            let arity = lhs.vars().len();
            assert!((1..=3).contains(&arity));
            for i in 0..arity {
                assert!(
                    lhs.vars().contains(&format!("?x{i}").parse().unwrap()),
                    "holes must be contiguous and canonical"
                );
            }
            let mut fun = definition(p, &ops);
            for _ in 0..arity {
                fun = node(Op::Lambda, vec![fun]);
            }
            let mut rhs = Ast::default();
            let fun = append(&fun, &mut rhs);
            let mut call = push(&mut rhs, Op::Ref(id), vec![]);
            for i in (0..arity).rev() {
                let arg = rhs.add(egg::ENodeOrVar::Var(format!("?x{i}").parse().unwrap()));
                call = push(&mut rhs, Op::Apply, vec![call, arg]);
            }
            push(&mut rhs, Op::Lib(id), vec![fun, call]);
            egg::Rewrite::new(format!("native-au-{id}"), lhs, egg::Pattern::new(rhs)).unwrap()
        })
        .collect();
    let mut g = egg::EGraph::new(PartialLibCost::new(16, 16, 2));
    let roots: Vec<_> = train
        .iter()
        .cloned()
        .map(|e| g.add_expr(&e.into()))
        .collect();
    g.rebuild();
    let runner = egg::Runner::<_, _, ()>::new(PartialLibCost::new(16, 16, 2))
        .with_egraph(g.clone())
        .with_iter_limit(2)
        .with_node_limit(1_000_000)
        .with_time_limit(std::time::Duration::from_secs(120))
        .run(rewrites.iter());
    let mut selected_graph = runner.egraph;
    let root = selected_graph.add(AstNode::new(Op::List, roots.iter().copied()));
    let mut costs = selected_graph[selected_graph.find(root)].data.clone();
    costs.set.sort_unstable_by_key(|x| x.full_cost);
    let selected: Vec<_> = costs.set[0]
        .libs
        .iter()
        .map(|lib| rewrites[lib.0 .0].clone())
        .collect();
    let expr = apply_libs(g, &roots, &selected).into();
    ExperimentResult {
        final_expr: expr,
        num_libs: selected.len(),
        rewrites: selected,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_hole_expands_exactly() {
        let train: Vec<_> = (0..6)
            .map(|i| {
                node(
                    Op::Data("pair".into(), 2),
                    vec![
                        node(
                            Op::Data("payload".into(), 1),
                            vec![node(Op::Data("payload".into(), 1), vec![encode(&json!(i))])]
                        );
                        2
                    ],
                )
            })
            .collect();
        let patterns = vec![
            json!({"op":Op::Data("pair".into(),2).to_string(),"args":[{"hole":0},{"hole":0}]}),
        ];
        let result = run(&train, &patterns);
        assert_eq!(result.num_libs, 1);
        assert!(verify(&result.final_expr, &train));
    }
}
