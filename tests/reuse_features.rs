//! Differential checks against actual native queries over a completed ripen body.
use egg_layout::reuse_graph::{
    RipSatEgraph,
    features::{Features, Probe, Region, TypedAtom},
    planner::Query,
};
use egglog::{EGraph, ast::Literal};
use std::{collections::BTreeMap, fs, process::Command};

#[test]
fn typed_features_are_conservative_against_native_matches() {
    let root = std::env::temp_dir().join(format!("typed-features-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.egg");
    let code = r#"(datatype E (V i64) (Pair E E) (Text String) (Absent E) (Wide E E E E E))
        (Pair (V 1) (V 1)) (Pair (V 1) (V 2)) (Text "kept")
        (Wide (V 1) (V 2) (V 1) (V 2) (V 1))"#;
    fs::write(&source, code).unwrap();
    let out = root.join("ripen");
    let run = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["ripen", source.to_str().unwrap(), out.to_str().unwrap()])
        .env("EGG_LAYOUT_GRAPHS", "0")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let report = serde_json::from_slice(&fs::read(out.join("ripen.json")).unwrap()).unwrap();
    let state =
        serde_json::from_slice(&fs::read(out.join("saturated-rule-composition.json")).unwrap())
            .unwrap();
    let body = RipSatEgraph::from_native_export(state, &report).unwrap();
    let features = Features::bake(&body);
    assert_eq!(std::mem::size_of::<Features>(), Features::STORAGE_BYTES);
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, code).unwrap();
    let arities: BTreeMap<_, _> = [("Pair", 2), ("V", 1), ("Text", 1), ("Absent", 1)]
        .into_iter()
        .collect();
    let mut hits = 0;
    let mut rejects = 0;
    for literal in -3..8 {
        for same in [false, true] {
            for result_column in [false, true] {
                let mut pair = vec![0, if same { 0 } else { 1 }];
                if result_column {
                    pair.push(2);
                }
                let q = Query {
                    values: vec![
                        ("E".into(), None),
                        ("E".into(), None),
                        ("E".into(), None),
                        ("i64".into(), Some(literal.to_string())),
                    ],
                    atoms: vec![("Pair".into(), pair), ("V".into(), vec![3, 0])],
                    projection: vec![0],
                };
                let atoms: Vec<_> = q
                    .atoms
                    .iter()
                    .enumerate()
                    .map(|(i, (op, _))| {
                        TypedAtom::from_query_atom(&q, i, arities[op.as_str()]).unwrap()
                    })
                    .collect();
                let values = vec![
                    ("E".into(), None),
                    ("E".into(), None),
                    ("E".into(), None),
                    ("i64".into(), Some(Literal::Int(literal))),
                ];
                let compiled = eg
                    .compile_fact_query(&values, &q.atoms, &q.projection)
                    .unwrap();
                let matched = !eg.run_fact_query(&compiled).unwrap().is_empty();
                eg.free_fact_query(compiled);
                let probe = features.probe_typed(&atoms, Region::BodyOnly);
                if matched {
                    hits += 1;
                    assert_eq!(probe, Probe::Maybe, "{q:?}");
                }
                if probe == Probe::No {
                    rejects += 1;
                    assert!(!matched);
                }
                for region in [
                    Region::ThroughBoundary,
                    Region::UnknownConditions,
                    Region::StaleConnections,
                ] {
                    assert_eq!(features.probe_typed(&atoms, region), Probe::Maybe);
                }
            }
        }
    }
    assert!(hits > 0 && rejects > 0);
    let mut wrong_sort = TypedAtom {
        op: "Text".into(),
        arity: 1,
        columns: vec![("i64".into(), None)],
        equal_columns: vec![],
    };
    assert_eq!(
        features.probe_typed(&[wrong_sort.clone()], Region::BodyOnly),
        Probe::No
    );
    wrong_sort.columns[0].0 = "String".into();
    wrong_sort.columns[0].1 = Some("\"missing\"".into());
    assert_eq!(
        features.probe_typed(&[wrong_sort], Region::BodyOnly),
        Probe::No
    );
    // A result column aliased to an argument is absent for Pair in this snapshot.
    let alias = TypedAtom {
        op: "Pair".into(),
        arity: 2,
        columns: vec![("E".into(), None); 3],
        equal_columns: vec![(0, 2)],
    };
    assert_eq!(features.probe_typed(&[alias], Region::BodyOnly), Probe::No);
    let unknown = TypedAtom {
        op: "Absent".into(),
        arity: 1,
        columns: vec![],
        equal_columns: vec![],
    };
    assert_eq!(
        features.probe_typed(&[unknown], Region::BodyOnly),
        Probe::Maybe
    );
    // An updated instance cannot use a negative from the old immutable body.
    eg.parse_and_run_program(None, "(Absent (V 1))").unwrap();
    let absent = TypedAtom {
        op: "Absent".into(),
        arity: 1,
        columns: vec![("E".into(), None)],
        equal_columns: vec![],
    };
    assert_eq!(
        features.probe_typed(&[absent.clone()], Region::BodyOnly),
        Probe::No
    );
    assert_eq!(
        features.probe_typed(&[absent], Region::StaleConnections),
        Probe::Maybe
    );
    // Larger positional constraints are unsupported hints, never negatives.
    let wide = TypedAtom {
        op: "Wide".into(),
        arity: 5,
        columns: vec![("E".into(), None); 5],
        equal_columns: vec![(3, 4)],
    };
    assert_eq!(
        features.probe_typed(&[wide], Region::BodyOnly),
        Probe::Maybe
    );
    let mut literal_query = Query {
        values: vec![("i64".into(), Some(" 1 ".into()))],
        atoms: vec![("V".into(), vec![0])],
        projection: vec![0],
    };
    let atom = TypedAtom::from_query_atom(&literal_query, 0, 1).unwrap();
    assert_eq!(
        features.probe_typed(&[atom], Region::BodyOnly),
        Probe::Maybe
    );
    literal_query.values[0].1 = Some("(+ 1 2)".into());
    assert!(TypedAtom::from_query_atom(&literal_query, 0, 1).is_none());
    fs::remove_dir_all(root).unwrap();
}
