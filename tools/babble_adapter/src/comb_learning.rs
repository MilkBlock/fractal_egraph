//! Learn sub-combinations; do not relearn the syntax of atomic rules.
use super::*;
use babble::{co_occurrence::COBuilder, experiments::ExperimentResult, learn::LearnedLibrary};
fn has_connection(ast: &egg::PatternAst<AstNode<Op>>) -> bool {
    ast.as_ref().iter().any(|n| match n {
        egg::ENodeOrVar::ENode(n) => match n.operation() {
            Op::Data(label, _) => label
                .strip_prefix("term:")
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .is_some_and(|v| v[0] == "CombEdge"),
            _ => false,
        },
        _ => false,
    })
}
pub fn run(train: &[Expr<Op>]) -> (ExperimentResult<Op>, Value) {
    let mut g = egg::EGraph::<AstNode<Op>, PartialLibCost>::default();
    let roots: Vec<_> = train
        .iter()
        .cloned()
        .map(|e| g.add_expr(&e.into()))
        .collect();
    g.rebuild();
    let co = COBuilder::new(&g, &roots).run();
    let mut library = LearnedLibrary::new(&g, false, Some(3), co);
    let raw = library.size();
    library.deduplicate(&g);
    let filtered: Vec<_> = library
        .rewrites::<PartialLibCost>()
        .filter_map(|r| {
            let ast = r.searcher.get_pattern_ast().unwrap();
            has_connection(ast).then(|| au_reference::pattern(ast))
        })
        .collect();
    let report = json!({"raw_candidates":raw,"deduplicated_candidates":library.size(),"connection_containing_candidates":filtered.len(),"filter":"at least one concrete producer-consumer CombEdge; no specific rule names hardcoded","library_limit":4});
    (from_candidates::run_with_limit(train, &filtered, 4), report)
}
/// Both inputs and learned programs use the same exact-subtree DAG codec.
pub fn dag_metrics(e: &Expr<Op>) -> Value {
    fn visit(
        e: &Expr<Op>,
        ops: &mut BTreeMap<Op, usize>,
        nodes: &mut BTreeMap<(usize, Vec<usize>), usize>,
        rows: &mut Vec<(usize, Vec<usize>)>,
    ) -> usize {
        let args =
            e.0.args()
                .iter()
                .map(|a| visit(a, ops, nodes, rows))
                .collect::<Vec<_>>();
        let n = ops.len();
        let op = *ops.entry(e.0.operation().clone()).or_insert(n);
        let key = (op, args);
        if let Some(&id) = nodes.get(&key) {
            return id;
        }
        let id = rows.len();
        rows.push(key.clone());
        nodes.insert(key, id);
        id
    }
    let mut ops = BTreeMap::new();
    let mut nodes = BTreeMap::new();
    let mut rows = vec![];
    let root = visit(e, &mut ops, &mut nodes, &mut rows);
    let mut symbols = vec![String::new(); ops.len()];
    for (op, id) in ops {
        symbols[id] = op.to_string();
    }
    fn expand(id: usize, rows: &[(usize, Vec<usize>)], symbols: &[String]) -> Value {
        let (op, args) = &rows[id];
        json!({"op":symbols[*op],"args":args.iter().map(|&i|expand(i,rows,symbols)).collect::<Vec<_>>()})
    }
    assert_eq!(
        expand(root, &rows, &symbols),
        dump(e),
        "DAG codec must expand exactly"
    );
    let bytes = serde_json::to_vec(&json!({"symbols":symbols,"nodes":rows,"root":root}))
        .unwrap()
        .len();
    json!({"unique_nodes":rows.len(),"child_references":rows.iter().map(|(_,a)|a.len()).sum::<usize>(),"self_contained_json_codec_bytes":bytes})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_requires_a_connection_not_just_a_rule_wrapper() {
        let ast = |sort: &str, op: &str| {
            let e = encode(&json!({"sort":sort,"op":op,"args":[]}));
            let r: egg::RecExpr<AstNode<Op>> = e.into();
            let p: egg::Pattern<_> = r.as_ref().into();
            p.ast
        };
        assert!(!has_connection(&ast("Rule", "wrapper")));
        assert!(has_connection(&ast(
            "CombEdge",
            "use:ArbitraryA->ArbitraryB:T"
        )));
    }
    #[test]
    fn dag_baseline_already_shares_identical_subtrees() {
        let a = encode(&json!("same"));
        let e = node(Op::Data("pair".into(), 2), vec![a.clone(), a]);
        let m = dag_metrics(&e);
        assert_eq!(m["unique_nodes"], 2);
        assert_eq!(m["child_references"], 2);
    }
}
