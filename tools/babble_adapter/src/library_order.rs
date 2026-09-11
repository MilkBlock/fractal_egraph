//! Give lifted, pure, closed library definitions a valid lexical dependency order.
use super::*;
use std::collections::BTreeSet;
fn refs(e: &Expr<Op>, out: &mut BTreeSet<usize>, depth: usize) {
    match e.0.operation() {
        Op::Ref(id) => {
            out.insert(*id);
        }
        Op::Var(i) => assert!(*i < depth, "free term variable in lifted library"),
        Op::Lib(_) => panic!("nested library binding was not lifted"),
        _ => {}
    }
    let next = depth + usize::from(matches!(e.0.operation(), Op::Lambda));
    for a in e.0.args() {
        refs(a, out, next);
    }
}
pub fn normalize(expr: Expr<Op>) -> (Expr<Op>, bool) {
    let original = expr.clone();
    let mut body = expr;
    let mut definitions = BTreeMap::new();
    let mut original_order = vec![];
    while let Op::Lib(id) = body.0.operation() {
        let id = *id;
        original_order.push(id);
        let args = body.0.args();
        assert!(
            definitions.insert(id, args[0].clone()).is_none(),
            "duplicate lifted library ID"
        );
        body = args[1].clone();
    }
    let dependencies: BTreeMap<_, _> = definitions
        .iter()
        .map(|(id, e)| {
            let mut r = BTreeSet::new();
            refs(e, &mut r, 0);
            (*id, r)
        })
        .collect();
    let mut body_refs = BTreeSet::new();
    refs(&body, &mut body_refs, 0);
    assert!(
        body_refs.iter().all(|id| definitions.contains_key(id)),
        "missing top-level library definition"
    );
    let mut visible = BTreeSet::new();
    if original_order
        .iter()
        .all(|id| dependencies[id].is_subset(&visible) && visible.insert(*id))
    {
        return (original, false);
    }
    let mut done = BTreeSet::new();
    let mut order = vec![];
    while done.len() < definitions.len() {
        let ready = dependencies
            .iter()
            .find(|(id, deps)| !done.contains(*id) && deps.iter().all(|id| done.contains(id)))
            .map(|(id, _)| *id);
        let id = ready.expect("cyclic or missing library dependency; cannot repair by reordering");
        done.insert(id);
        order.push(id);
    }
    for id in order.into_iter().rev() {
        body = node(Op::Lib(id), vec![definitions.remove(&id).unwrap(), body]);
    }
    let changed = body != original;
    (body, changed)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn puts_referenced_library_before_its_user() {
        let a = node(Op::Lambda, vec![node(Op::Var(0), vec![])]);
        let b = node(
            Op::Lambda,
            vec![node(
                Op::Apply,
                vec![node(Op::Ref(1), vec![]), node(Op::Var(0), vec![])],
            )],
        );
        let value = encode(&json!("value"));
        let body = node(
            Op::List,
            vec![node(
                Op::Apply,
                vec![node(Op::Ref(2), vec![]), value.clone()],
            )],
        );
        let wrong = node(Op::Lib(2), vec![b, node(Op::Lib(1), vec![a, body])]);
        let (fixed, changed) = normalize(wrong);
        assert!(changed);
        assert!(verify(&fixed, &[value]));
    }
    #[test]
    #[should_panic(expected = "cyclic or missing")]
    fn does_not_invent_a_missing_library() {
        normalize(node(
            Op::Lib(1),
            vec![
                node(Op::Lambda, vec![node(Op::Ref(2), vec![])]),
                node(Op::List, vec![]),
            ],
        ));
    }
}
