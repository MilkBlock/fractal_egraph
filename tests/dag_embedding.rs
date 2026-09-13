use egg_layout::dag_embedding::*;
use serde_json::json;
fn graph(labels: &[&str], edges: &[(usize, usize, &str)]) -> Dag {
    Dag {
        root: 0,
        nodes: labels
            .iter()
            .map(|s| Node {
                label: json!(s),
                constraints: json!(null),
                ports: vec!["input".into()],
            })
            .collect(),
        edges: edges
            .iter()
            .map(|(a, b, l)| Edge {
                from: *a,
                to: *b,
                label: json!(l),
            })
            .collect(),
        interface: vec![Interface {
            name: "entry".into(),
            node: 0,
            port: "input".into(),
        }],
    }
}
#[test]
fn extra_root_and_extra_edges_do_not_block_embedding() {
    let a = graph(&["A", "B", "C"], &[(0, 1, "x"), (0, 2, "y")]);
    let b = graph(
        &["NewRoot", "A", "B", "C", "Extra"],
        &[(0, 1, "input"), (1, 2, "x"), (1, 3, "y"), (1, 4, "z")],
    );
    assert!(matches!(
        embed(&a, &b, 1000).unwrap(),
        Outcome::Found { root_image: 1, .. }
    ));
}
#[test]
fn sharing_is_global_and_distinct_applies_cannot_collapse() {
    let diamond = graph(
        &["A", "B", "C", "D"],
        &[(0, 1, "b"), (0, 2, "c"), (1, 3, "d"), (2, 3, "d")],
    );
    let copies = graph(
        &["A", "B", "C", "D", "D"],
        &[(0, 1, "b"), (0, 2, "c"), (1, 3, "d"), (2, 4, "d")],
    );
    assert!(matches!(
        embed(&diamond, &copies, 1000).unwrap(),
        Outcome::Absent { .. }
    ));
    assert!(matches!(
        embed(&copies, &diamond, 1000).unwrap(),
        Outcome::Absent { .. }
    ));
}
#[test]
fn ports_constraints_and_budget_are_not_silently_ignored() {
    let mut a = graph(&["A", "B"], &[(0, 1, "input-x")]);
    let mut b = graph(&["A", "B"], &[(0, 1, "input-y")]);
    assert!(matches!(
        embed(&a, &b, 1000).unwrap(),
        Outcome::Absent { .. }
    ));
    b.edges[0].label = json!("input-x");
    a.nodes[0].constraints = json!({"aliases":[0,0]});
    b.nodes[0].constraints = json!({"aliases":[0,1]});
    assert!(matches!(
        embed(&a, &b, 1000).unwrap(),
        Outcome::Absent { .. }
    ));
    b.nodes[0].constraints = a.nodes[0].constraints.clone();
    assert!(matches!(
        embed(&a, &b, 0).unwrap(),
        Outcome::UnknownBudget { .. }
    ));
    b.edges.push(Edge {
        from: 1,
        to: 0,
        label: json!("cycle"),
    });
    assert!(embed(&a, &b, 1000).is_err());
}

#[test]
fn exhaustive_small_dags_agree_with_independent_permutation_oracle() {
    fn make(n: usize, mask: usize) -> Dag {
        let labels = vec!["N"; n];
        let mut edges = vec![];
        let mut bit = 0;
        for i in 0..n {
            for j in i + 1..n {
                if mask & (1 << bit) != 0 {
                    edges.push((i, j, "e"));
                }
                bit += 1;
            }
        }
        graph(&labels, &edges)
    }
    fn brute(a: &Dag, b: &Dag) -> bool {
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    if i == j || i == k || j == k {
                        continue;
                    }
                    let m = [i, j, k];
                    if a.edges.iter().all(|e| {
                        b.edges
                            .iter()
                            .any(|f| f.from == m[e.from] && f.to == m[e.to] && f.label == e.label)
                    }) {
                        return true;
                    }
                }
            }
        }
        false
    }
    for a in 0..8 {
        for b in 0..64 {
            let a = make(3, a);
            let b = make(4, b);
            assert_eq!(
                matches!(embed(&a, &b, 100_000).unwrap(), Outcome::Found { .. }),
                brute(&a, &b)
            );
        }
    }
}
