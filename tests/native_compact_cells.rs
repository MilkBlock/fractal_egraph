use egglog::{EGraph, Value, VirtualFactRow};
fn value(eg: &mut EGraph, s: &str) -> Value {
    let mut p = egglog::ast::Parser::default();
    eg.eval_expr(&p.get_expr_from_string(None, s).unwrap())
        .unwrap()
        .1
}
fn graph(n: usize) -> EGraph {
    let mut e = EGraph::default();
    let mut s="(datatype E (V i64) (A E) (B E)) (relation Seed (E)) (ruleset build) (rule ((Seed x)) ((B (A x))) :ruleset build)".to_owned();
    for i in 0..n {
        s += &format!("(Seed (V {i}))");
    }
    s += "(run build 1)";
    e.parse_and_run_program(None, &s).unwrap();
    e
}
fn compact(e: &mut EGraph, n: usize) -> egglog::CompactCellsReport {
    let t = e
        .virtual_fact_template(vec![
            VirtualFactRow {
                function: "A".into(),
                columns: vec![0, 1],
            },
            VirtualFactRow {
                function: "B".into(),
                columns: vec![1, 2],
            },
        ])
        .unwrap();
    let b = (0..n)
        .map(|i| {
            vec![
                value(e, &format!("(V {i})")),
                value(e, &format!("(A (V {i}))")),
                value(e, &format!("(B (A (V {i})))")),
            ]
        })
        .collect::<Vec<_>>();
    e.compact_cell_facts(&[(&t, &b)]).unwrap()
}
fn query(e: &mut EGraph) -> Vec<Vec<Value>> {
    let q = e
        .compile_fact_query(
            &vec![("E".into(), None); 3],
            &[("A".into(), vec![0, 1]), ("B".into(), vec![1, 2])],
            &[0, 1, 2],
        )
        .unwrap();
    let mut r = e.run_fact_query(&q).unwrap();
    e.free_fact_query(q);
    r.sort();
    r
}
#[test]
fn shared_body_has_no_per_fact_native_hash_entries() {
    let mut e = graph(128);
    let before = query(&mut e);
    let report = compact(&mut e, 128);
    assert_eq!(report.cell_instances, 128);
    assert_eq!(report.cell_batches, 1);
    assert_eq!(report.owned_rows, 256);
    let stats = e.compact_cell_storage();
    assert_eq!(stats.len(), 2);
    for (_, s) in &stats {
        assert_eq!(s.rows, 128);
        assert_eq!(s.native_hash_rows, 0);
        assert_eq!(s.projections, 1);
        assert_eq!(s.bindings.len(), 1);
    }
    assert_eq!(stats[0].1.bindings[0].0, stats[1].1.bindings[0].0);
    assert_eq!(query(&mut e), before);
    assert_eq!(compact(&mut e, 128).owned_rows, 0);
    assert_eq!(query(&mut e), before);
}
#[test]
fn boundary_union_subsume_and_repacking_preserve_native_facts() {
    let mut e = graph(64);
    let mut reference = e.clone();
    compact(&mut e, 64);
    for action in [
        "(relation Ext (E)) (relation Hit (E)) (ruleset use) (rule ((Ext x) (= y (B (A x)))) ((Hit y)) :ruleset use) (Ext (V 4)) (run use 1)",
        "(union (V 4) (V 5)) (run use 1)",
        "(subsume (B (A (V 6))))",
        "(Seed (V 99)) (run build 1)",
    ] {
        e.parse_and_run_program(None, action).unwrap();
        reference.parse_and_run_program(None, action).unwrap();
        let mut a = query(&mut e);
        let mut b = query(&mut reference);
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
    let stats = e.compact_cell_storage();
    assert!(!stats.is_empty());
    assert!(stats.iter().all(|(_, s)| s.native_hash_rows < 10));
    let t = e
        .virtual_fact_template(vec![
            VirtualFactRow {
                function: "A".into(),
                columns: vec![0, 1],
            },
            VirtualFactRow {
                function: "B".into(),
                columns: vec![1, 2],
            },
        ])
        .unwrap();
    let b = vec![vec![
        value(&mut e, "(V 99)"),
        value(&mut e, "(A (V 99))"),
        value(&mut e, "(B (A (V 99)))"),
    ]];
    e.compact_cell_facts(&[(&t, &b)]).unwrap();
    assert_eq!(query(&mut e), query(&mut reference));
}
#[test]
fn invalid_payload_does_not_partially_compact() {
    let mut e = graph(2);
    let t = e
        .virtual_fact_template(vec![VirtualFactRow {
            function: "A".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let b = vec![
        vec![value(&mut e, "(V 0)"), value(&mut e, "(A (V 0))")],
        vec![value(&mut e, "(V 1)"), value(&mut e, "(V 0)")],
    ];
    assert!(e.compact_cell_facts(&[(&t, &b)]).is_err());
    assert!(e.compact_cell_storage().is_empty());
    assert_eq!(query(&mut e).len(), 2);
}

#[test]
fn function_updates_delete_and_snapshot_keep_hashcons_identity() {
    let mut e = EGraph::default();
    e.parse_and_run_program(None,"(datatype E (X) (Y)) (function score (E) i64 :merge (max old new)) (set (score (X)) 1) (set (score (Y)) 2)").unwrap();
    let t = e
        .virtual_fact_template(vec![VirtualFactRow {
            function: "score".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let b = vec![
        vec![value(&mut e, "(X)"), value(&mut e, "(score (X))")],
        vec![value(&mut e, "(Y)"), value(&mut e, "(score (Y))")],
    ];
    e.compact_cell_facts(&[(&t, &b)]).unwrap();
    let mut snapshot = e.clone();
    for action in [
        "(set (score (X)) 3)",
        "(union (X) (Y))",
        "(delete (score (X)))",
        "(set (score (Y)) 4)",
    ] {
        e.parse_and_run_program(None, action).unwrap();
    }
    e.parse_and_run_program(None, "(check (= (score (X)) 4) (= (X) (Y)))")
        .unwrap();
    snapshot
        .parse_and_run_program(None, "(check (= (score (X)) 1) (= (score (Y)) 2))")
        .unwrap();
    assert_eq!(snapshot.compact_cell_storage()[0].1.native_hash_rows, 0);
}

#[test]
fn compaction_preserves_producers_after_row_positions_change() {
    let mut e = EGraph::default();
    let mut source="(datatype E (V i64) (A E) (B E)) (relation Seed (E)) (ruleset make) (ruleset use) (rule ((Seed x)) ((A x)) :ruleset make :name \"make\") (rule ((= a (A x))) ((B a)) :ruleset use :name \"use\")".to_owned();
    for i in 0..16 {
        source += &format!("(Seed (V {i}))");
    }
    e.parse_and_run_program(None, &source).unwrap();
    let trace = egglog::TraceSession::with_dependencies();
    let cmds = e.parse_program(None, "(run make 1)").unwrap();
    e.run_program_with_trace(cmds, &trace).unwrap();
    let produced = trace.drain_completed();
    let template = e
        .virtual_fact_template(vec![VirtualFactRow {
            function: "A".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    let bindings = (0..16)
        .rev()
        .map(|i| {
            vec![
                value(&mut e, &format!("(V {i})")),
                value(&mut e, &format!("(A (V {i}))")),
            ]
        })
        .collect::<Vec<_>>();
    e.compact_cell_facts(&[(&template, &bindings)]).unwrap();
    let cmds = e.parse_program(None, "(run use 1)").unwrap();
    e.run_program_with_trace(cmds, &trace).unwrap();
    let used = trace.drain_completed();
    let reads = used
        .reads
        .iter()
        .filter(|r| r.table_name.as_deref() == Some("A"))
        .collect::<Vec<_>>();
    assert!(!reads.is_empty());
    for r in reads {
        assert!(r.producer_write_event_id.is_some_and(|id| {
            produced
                .writes
                .iter()
                .any(|w| w.event_id == id && w.outcome == egglog::WriteOutcome::Inserted)
        }));
    }
    assert_eq!(e.get_size("B"), 16);
}
#[test]
fn explicit_materialization_is_observable_and_keeps_cached_queries_valid() {
    let mut e = graph(128);
    compact(&mut e, 128);
    let before = query(&mut e);
    assert_eq!(e.materialize_virtual_facts(), 2);
    assert_eq!(query(&mut e), before);
    for (_, s) in e.compact_cell_storage() {
        assert_eq!(s.materialized_fallbacks, 1);
        assert_eq!(s.rows, 0);
        assert_eq!(s.native_hash_rows, 128);
    }
    e.parse_and_run_program(None, "(Seed (V 999)) (run build 1)")
        .unwrap();
    assert_eq!(query(&mut e).len(), 129);
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MathArg {
    Id(Value),
    Literal(String, String),
}
fn math_rows(e: &EGraph) -> Vec<(String, Vec<MathArg>, Value)> {
    let mut rows = vec![];
    for name in e.get_function_names() {
        let f = e.get_function(&name).unwrap();
        if f.is_let_binding() || f.schema().output.name() != "Math" {
            continue;
        }
        e.function_for_each(&name, |r| {
            let args = r
                .vals
                .iter()
                .zip(&f.schema().input)
                .map(|(v, s)| match s.name() {
                    "Math" => MathArg::Id(e.get_canonical_value(*v, s)),
                    "i64" => MathArg::Literal("i64".into(), e.value_to_base::<i64>(*v).to_string()),
                    "String" => MathArg::Literal(
                        "String".into(),
                        e.value_to_base::<egglog::sort::S>(*v).to_string(),
                    ),
                    _ => panic!("unexpected Math sort"),
                })
                .collect();
            rows.push((
                name.clone(),
                args,
                e.get_canonical_value(*r.vals.last().unwrap(), &f.schema().output),
            ));
        })
        .unwrap();
    }
    rows
}
fn exact_math(a: &EGraph, b: &EGraph) {
    use std::collections::BTreeMap;
    let mut pending = math_rows(a);
    let right = math_rows(b);
    assert_eq!(pending.len(), right.len());
    let index = right
        .into_iter()
        .map(|(op, args, out)| ((op, args), out))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(index.len(), pending.len());
    let mut forward = BTreeMap::new();
    let mut reverse = BTreeMap::new();
    while !pending.is_empty() {
        let n = pending.len();
        let mut next = vec![];
        for (op, args, out) in pending {
            let mapped = args
                .iter()
                .map(|a| match a {
                    MathArg::Id(v) => forward.get(v).copied().map(MathArg::Id),
                    v => Some(v.clone()),
                })
                .collect::<Option<Vec<_>>>();
            if let Some(mapped) = mapped {
                let target = index[&(op, mapped)];
                if let Some(old) = forward.insert(out, target) {
                    assert_eq!(old, target);
                }
                if let Some(old) = reverse.insert(target, out) {
                    assert_eq!(old, out);
                }
            } else {
                next.push((op, args, out));
            }
        }
        assert!(next.len() < n);
        pending = next;
    }
}
#[test]
fn unchanged_math_rules_continue_across_compact_cells() {
    let mut e = EGraph::default();
    e.parse_and_run_program(None, include_str!("../experiments/math_mini3/source.egg"))
        .unwrap();
    let mut reference = e.clone();
    let q = e
        .compile_fact_query(
            &vec![("Math".into(), None); 3],
            &[("Add".into(), vec![0, 1, 2]), ("Add".into(), vec![1, 0, 2])],
            &[0, 1, 2],
        )
        .unwrap();
    let bindings = e.run_fact_query(&q).unwrap();
    e.free_fact_query(q);
    let t = e
        .virtual_fact_template(vec![
            VirtualFactRow {
                function: "Add".into(),
                columns: vec![0, 1, 2],
            },
            VirtualFactRow {
                function: "Add".into(),
                columns: vec![1, 0, 2],
            },
        ])
        .unwrap();
    assert!(e.compact_cell_facts(&[(&t, &bindings)]).unwrap().owned_rows > 0);
    exact_math(&e, &reference);
    for _ in 0..2 {
        e.parse_and_run_program(None, "(run 1)").unwrap();
        reference.parse_and_run_program(None, "(run 1)").unwrap();
        exact_math(&e, &reference);
    }
}
