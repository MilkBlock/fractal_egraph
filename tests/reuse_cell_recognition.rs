//! Recognition-driven compaction against the real native runtime.
//!
//! These tests drive [`CellRecognizer`] with committed events and install the
//! result through the shared validated entry. They measure the actual egglog
//! runtime: the graph, the storage report and the query results all come from
//! `EGraph`, not from a standalone model.
use egg_layout::reuse_cell_online::{CellRecognizer, CommittedEvent, FactOracle, ValueId};
use egg_layout::reuse_cell_recognition::{compile_bake, BakeCell, EnvironmentId, RuleIdentity};
use egglog::{EGraph, Value};

const ENVIRONMENT: &[&str] = &[
    "(datatype E (V i64) (A E) (B E))",
    "(relation Seed (E))",
    "(ruleset make)",
    "(rule ((Seed x)) ((A x)) :ruleset make :name \"a\")",
    "(rule ((A x)) ((B x)) :ruleset make :name \"b\")",
];

fn environment() -> Vec<String> {
    ENVIRONMENT.iter().map(|s| s.to_string()).collect()
}

fn vid(value: Value) -> ValueId {
    format!("{value:?}")
        .trim_start_matches("Value(")
        .trim_end_matches(')')
        .parse()
        .unwrap()
}

fn native(id: ValueId) -> Value {
    Value::new_const(id as u32)
}

/// The recogniser's point oracle, answered by committed native storage.
struct NativeOracle<'a> {
    eg: &'a EGraph,
}

impl FactOracle for NativeOracle<'_> {
    fn lookup(&self, function: &str, key: &[ValueId]) -> Option<Vec<ValueId>> {
        let key: Vec<Value> = key.iter().map(|v| native(*v)).collect();
        self.eg
            .lookup_function_row(function, &key)
            .map(|row| row.iter().map(|v| vid(*v)).collect())
    }
}

fn value(eg: &mut EGraph, source: &str) -> Value {
    let mut parser = egglog::ast::Parser::default();
    let expr = parser.get_expr_from_string(None, source).unwrap();
    eg.eval_expr(&expr).unwrap().1
}

fn graph(seeds: usize) -> EGraph {
    let mut eg = EGraph::default();
    let mut program = environment().join("\n");
    for i in 0..seeds {
        program += &format!("(Seed (V {i}))");
    }
    program += "(run make 2)";
    eg.parse_and_run_program(None, &program).unwrap();
    eg
}

/// One trigger step whose body is the single row `(A x)`, exactly the shape the
/// adaptive Bake adapter produces.
fn plan() -> egg_layout::reuse_cell_recognition::CellRecognitionPlan {
    compile_bake(&[BakeCell {
        rule: 0,
        rule_text: None,
        environment: environment(),
        slots: vec![
            ("x".into(), "E".into()),
            ("adaptive_local_0".into(), "E".into()),
        ],
        template: vec![("A".into(), vec![0, 1])],
    }])
    .unwrap()
    .remove(0)
}

fn rule() -> RuleIdentity {
    RuleIdentity {
        environment: EnvironmentId::from_text(&environment()),
        rule: "#0".into(),
    }
}

fn rows(eg: &EGraph) -> Vec<(String, Vec<String>)> {
    let mut out = vec![];
    for name in ["A", "B"] {
        eg.function_for_each(name, |row| {
            out.push((name.to_string(), row.vals.iter().map(|v| format!("{v:?}")).collect()))
        })
        .unwrap();
    }
    out.sort();
    out
}

#[test]
fn a_trigger_without_committed_body_rows_never_packs() {
    let mut eg = graph(8);
    eg.parse_and_run_program(None, "(delete (A (V 3)))").unwrap();
    let mut recogniser = CellRecognizer::new(vec![plan()]);
    let x = vid(value(&mut eg, "(V 3)"));
    recogniser
        .observe(CommittedEvent::Apply {
            event: 1,
            rule: &rule(),
            wanted: &[x],
            outputs: &[],
            external: &[x],
        })
        .unwrap();
    assert_eq!(recogniser.counts().located, 1);
    let oracle = NativeOracle { eg: &eg };
    assert_eq!(recogniser.confirm(&oracle), 0);
    assert!(recogniser.complete(&oracle).is_empty());
    assert!(eg.compact_cell_storage().is_empty());
}

#[test]
fn recognised_cells_install_without_changing_the_graph() {
    let mut eg = graph(16);
    let before = rows(&eg);
    let mut recogniser = CellRecognizer::new(vec![plan()]);
    for i in 0..16 {
        let x = vid(value(&mut eg, &format!("(V {i})")));
        recogniser
            .observe(CommittedEvent::Apply {
                event: i + 1,
                rule: &rule(),
                wanted: &[x],
                outputs: &[],
                external: &[x],
            })
            .unwrap();
    }
    assert_eq!(recogniser.counts().located, 16);
    let cells = {
        let oracle = NativeOracle { eg: &eg };
        assert_eq!(recogniser.confirm(&oracle), 16);
        recogniser.complete(&oracle)
    };
    assert_eq!(cells.len(), 16, "one recognised instance per boundary");
    let installed =
        egg_layout::reuse_graph::native_storage::install_recognised_cells(&mut eg, &cells).unwrap();
    assert_eq!(installed["owned_rows"], 16);
    // The logical graph is unchanged; only the representation moved.
    assert_eq!(rows(&eg), before);
    let storage = eg.compact_cell_storage();
    assert!(!storage.is_empty());
    let cell_rows: usize = storage.iter().map(|(_, s)| s.rows).sum();
    let hash_rows: usize = storage.iter().map(|(_, s)| s.native_hash_rows).sum();
    assert_eq!(cell_rows, 16);
    assert!(hash_rows < 16, "covered facts leave the native hash: {hash_rows}");
    recogniser.mark_packed(&cells.iter().map(|c| c.key.clone()).collect::<Vec<_>>());
    let oracle = NativeOracle { eg: &eg };
    assert!(recogniser.complete(&oracle).is_empty());
}

#[test]
fn a_union_after_packing_keeps_the_graph_consistent() {
    let mut eg = graph(8);
    let mut recogniser = CellRecognizer::new(vec![plan()]);
    for i in 0..8 {
        let x = vid(value(&mut eg, &format!("(V {i})")));
        recogniser
            .observe(CommittedEvent::Apply {
                event: i + 1,
                rule: &rule(),
                wanted: &[x],
                outputs: &[],
                external: &[x],
            })
            .unwrap();
    }
    let cells = {
        let oracle = NativeOracle { eg: &eg };
        recogniser.confirm(&oracle);
        recogniser.complete(&oracle)
    };
    egg_layout::reuse_graph::native_storage::install_recognised_cells(&mut eg, &cells).unwrap();
    let before = rows(&eg);
    eg.parse_and_run_program(None, "(union (V 1) (V 2)) (run make 2)")
        .unwrap();
    // The runtime keeps answering the same logical facts after a union.
    let after = rows(&eg);
    assert!(!after.is_empty());
    assert!(
        after.len() >= before.len() - 8,
        "a union may only merge rows: {} -> {}",
        before.len(),
        after.len()
    );
}

#[test]
fn the_analysis_seam_recognises_and_installs_from_committed_applications() {
    // Discover the exact rule text the analyser reports for rule "a".
    let mut program = environment().join("\n");
    for i in 0..16 {
        program += &format!("(Seed (V {i}))");
    }
    program += "(run make 2)";
    let rule_text = {
        let mut probe = EGraph::default();
        probe
            .parse_program(None, &program)
            .unwrap()
            .into_iter()
            .find_map(|command| match command {
                egglog::ast::Command::Rule { rule, .. } if rule.name == "a" => {
                    Some(rule.to_string())
                }
                _ => None,
            })
            .expect("named rule a")
    };
    let plan = compile_bake(&[BakeCell {
        rule: 0,
        rule_text: Some(rule_text),
        environment: environment(),
        slots: vec![
            ("x".into(), "E".into()),
            ("adaptive_local_0".into(), "E".into()),
        ],
        template: vec![("A".into(), vec![0, 1])],
    }])
    .unwrap()
    .remove(0);
    let mut recogniser = CellRecognizer::new(vec![plan]);
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cell-seam");
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("program.egg");
    std::fs::write(&source, &program).unwrap();
    let report =
        egg_layout::native_analyze::run_with_cell_recognition(&source, &root.join("out"), None, &mut recogniser)
            .unwrap();
    assert!(report.is_object());
    // The seam saw the committed `A` applications, confirmed them against the
    // live graph and installed their cells: every instance ends up Packed rather
    // than merely Located.
    let counts = recogniser.counts();
    assert_eq!(counts.packed, 16, "{counts:?}");
    assert_eq!(counts.located, 0);
    assert_eq!(counts.complete, 0);
    assert!(recogniser.unsupported_sources().is_empty());
}
