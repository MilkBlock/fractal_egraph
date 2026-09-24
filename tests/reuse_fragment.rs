use egg_layout::reuse_graph::planner::{Query, fragment::contains};
use egglog::{EGraph, ast::Literal};
use std::collections::BTreeMap;

#[test]
fn source_fragment_containment_agrees_with_native_join() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,"(datatype E (V i64) (B E) (Absent E)) (relation Extra (E E)) (B (V 1)) (B (V 2)) (Extra (V 1) (V 2)) (Extra (V 1) (V 3)) (union (B (V 1)) (B (V 2)))").unwrap();
    let mut body = Query {
        values: vec![],
        atoms: vec![],
        projection: vec![],
    };
    let mut body_arities = vec![];
    let mut ids = BTreeMap::new();
    for op in ["V", "B", "Extra"] {
        let schema = eg.get_function(op).unwrap().schema().clone();
        eg.function_for_each(op, |row| {
            let mut cols = vec![];
            for (&v, s) in row
                .vals
                .iter()
                .zip(schema.input.iter().chain([&schema.output]))
            {
                let value = eg.get_canonical_value(v, s);
                let key = (s.name().to_string(), value);
                let slot = *ids.entry(key).or_insert_with(|| {
                    let id = body.values.len();
                    let literal =
                        (s.name() == "i64").then(|| eg.value_to_base::<i64>(value).to_string());
                    body.values.push((s.name().to_string(), literal));
                    id
                });
                cols.push(slot);
            }
            body.atoms.push((op.into(), cols));
            body_arities.push(schema.input.len());
        })
        .unwrap();
    }
    let mut positives = 0;
    let mut negatives = 0;
    for n in 0..5 {
        for alias in [false, true] {
            for output in [false, true] {
                let mut b = vec![0];
                if output {
                    b.push(2);
                }
                let query = Query {
                    values: vec![
                        ("E".into(), None),
                        ("E".into(), None),
                        ("E".into(), None),
                        ("i64".into(), Some(n.to_string())),
                    ],
                    atoms: vec![
                        ("B".into(), b),
                        ("Extra".into(), vec![0, if alias { 0 } else { 1 }]),
                        ("V".into(), vec![3, 0]),
                    ],
                    projection: vec![0],
                };
                let matched = contains(&body, &body_arities, &query, &[1, 2, 1]).unwrap();
                let native_values = vec![
                    ("E".into(), None),
                    ("E".into(), None),
                    ("E".into(), None),
                    ("i64".into(), Some(Literal::Int(n))),
                ];
                let q = eg
                    .compile_fact_query(&native_values, &query.atoms, &query.projection)
                    .unwrap();
                assert_eq!(
                    matched,
                    !eg.run_fact_query(&q).unwrap().is_empty(),
                    "{query:?}"
                );
                eg.free_fact_query(q);
                if matched {
                    positives += 1;
                } else {
                    negatives += 1;
                }
            }
        }
    }
    assert!(positives > 0 && negatives > 0);
    // Matching preserves cross-atom aliases, not just operator membership.
    let repeated = Query {
        values: vec![("E".into(), None); 2],
        atoms: vec![("B".into(), vec![0, 1]), ("B".into(), vec![0, 1])],
        projection: vec![],
    };
    assert!(contains(&body, &body_arities, &repeated, &[1, 1]).unwrap());
    assert!(contains(&body, &body_arities, &repeated, &[2, 2]).is_ok_and(|x| !x));
    let mut bad = repeated.clone();
    bad.atoms[0].1.push(99);
    assert!(contains(&body, &body_arities, &bad, &[1, 1]).is_err());
}
