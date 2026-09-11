//! Export a differential fixture from the actual upstream candidate generator.
use super::*;
use babble::{co_occurrence::COBuilder, learn::LearnedLibrary};
use std::collections::BTreeSet;
pub(crate) fn pattern(ast: &egg::PatternAst<AstNode<Op>>) -> Value {
    fn go(
        ast: &egg::PatternAst<AstNode<Op>>,
        i: usize,
        vars: &mut BTreeMap<String, usize>,
    ) -> Value {
        match &ast.as_ref()[i] {
            egg::ENodeOrVar::Var(v) => {
                let n = vars.len();
                let id = *vars.entry(v.to_string()).or_insert(n);
                json!({"hole":id})
            }
            egg::ENodeOrVar::ENode(n) => {
                json!({"op":n.operation().to_string(),"args":n.args().iter().map(|i|go(ast,usize::from(*i),vars)).collect::<Vec<_>>()})
            }
        }
    }
    go(ast, ast.as_ref().len() - 1, &mut BTreeMap::new())
}
fn tree_size(p: &Value) -> usize {
    1 + p
        .get("args")
        .and_then(Value::as_array)
        .map_or(0, |a| a.iter().map(tree_size).sum())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_cost_counts_expanded_pattern_not_compacted_storage() {
        let ast: egg::PatternAst<AstNode<Op>> = vec![
            egg::ENodeOrVar::Var("?x".parse().unwrap()),
            egg::ENodeOrVar::ENode(AstNode::new(
                Op::Data("pair".into(), 2),
                [0.into(), 0.into()],
            )),
        ]
        .into();
        assert_eq!(ast.as_ref().len(), 2);
        assert_eq!(tree_size(&pattern(&ast)), 3);
    }
}
pub fn export(train: &[Expr<Op>], path: &str) {
    let mut g = egg::EGraph::<AstNode<Op>, PartialLibCost>::default();
    let roots: Vec<_> = train
        .iter()
        .cloned()
        .map(|x| g.add_expr(&x.into()))
        .collect();
    g.rebuild();
    let co = COBuilder::new(&g, &roots).run();
    let nodes: BTreeMap<_, _> = g
        .classes()
        .map(|c| {
            assert_eq!(
                c.nodes.len(),
                1,
                "fixture restricted to unsaturated syntax DAG"
            );
            (usize::from(c.id), c.nodes[0].clone())
        })
        .collect();
    // All same-operator pairs can produce nontrivial patterns. Other pairs are
    // needed only when reached as their children; flags below are reference only.
    let mut pairs = BTreeSet::new();
    let mut pending: Vec<_> = nodes
        .iter()
        .flat_map(|(&a, x)| {
            nodes
                .iter()
                .filter(move |(_, y)| x.operation() == y.operation())
                .map(move |(&b, _)| (a, b))
        })
        .collect();
    while let Some((a, b)) = pending.pop() {
        if !pairs.insert((a, b)) {
            continue;
        }
        if nodes[&a].operation() == nodes[&b].operation() {
            pending.extend(
                nodes[&a]
                    .args()
                    .iter()
                    .zip(nodes[&b].args())
                    .map(|(a, b)| (usize::from(*a), usize::from(*b))),
            );
        }
    }
    let mut lib = LearnedLibrary::new(&g, false, Some(3), co.clone());
    let candidates: BTreeSet<_> = lib
        .rewrites::<PartialLibCost>()
        .map(|r| pattern(r.searcher.get_pattern_ast().unwrap()).to_string())
        .collect();
    lib.deduplicate(&g);
    let signatures: Vec<_> = lib
        .rewrites::<PartialLibCost>()
        .map(|r| {
            let p: egg::Pattern<_> = r.searcher.get_pattern_ast().unwrap().clone().into();
            let vars = p.vars();
            let mut matches = vec![];
            for m in r.searcher.search(&g) {
                for s in m.substs {
                    let mut actuals: Vec<_> = vars.iter().map(|v| usize::from(s[*v])).collect();
                    actuals.sort();
                    matches.push((usize::from(m.eclass), actuals));
                }
            }
            matches.sort();
            json!({"matches":matches,"size":tree_size(&pattern(&p.ast))})
        })
        .collect();
    let data = json!({"scope":"reference only; co-occurrence flags must not feed native inference", "max_arity":3,"roots":roots.iter().map(|i|usize::from(*i)).collect::<Vec<_>>(),
        "nodes":nodes.iter().map(|(i,n)|json!({"id":i,"op":n.operation().to_string(),"children":n.args().iter().map(|x|usize::from(*x)).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "pairs":pairs.iter().map(|&(a,b)|json!([a,b,co.may_co_occur(a.into(),b.into())])).collect::<Vec<_>>(),
        "expected":candidates.iter().map(|s|serde_json::from_str::<Value>(s).unwrap()).collect::<Vec<_>>(),"expected_dedup_signatures":signatures});
    std::fs::write(path, serde_json::to_string(&data).unwrap()).unwrap();
    println!(
        "{}",
        json!({"reference_nodes":nodes.len(),"pair_frontier":pairs.len(),"raw_candidates":candidates.len()})
    );
}
