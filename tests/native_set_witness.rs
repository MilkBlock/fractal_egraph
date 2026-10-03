use egglog::{EGraph, TraceSession, WriteOutcome};

#[test]
fn committed_set_records_absence_and_old_value_even_when_merge_does_not_change_it() {
    for old in [None, Some(10_i64), Some(3_i64)] {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
            (datatype E (V i64))
            (function Cost (E) i64 :merge (min old new))
            (ruleset update)
            (rule ((= x (V 0))) ((set (Cost x) 7)) :ruleset update)
            (V 0)
        "#,
        )
        .unwrap();
        if let Some(old) = old {
            eg.parse_and_run_program(None, &format!("(set (Cost (V 0)) {old})"))
                .unwrap();
        }
        let trace = TraceSession::with_dependencies();
        eg.step_rules_with_trace("update", &trace).unwrap();
        let tables = trace.table_names();
        let cost = tables
            .iter()
            .find(|(_, name)| name.as_ref() == "Cost")
            .unwrap()
            .0;
        let writes = trace.write_events();
        let write = writes.iter().find(|w| w.table == cost).unwrap();
        let before = write
            .before
            .as_ref()
            .expect("commit must distinguish unknown from absent");
        assert_eq!(
            before.as_ref().map(|row| eg.value_to_base::<i64>(row[1])),
            old
        );
        assert_eq!(eg.value_to_base::<i64>(write.proposed[1]), 7);
        assert_eq!(
            eg.value_to_base::<i64>(write.actual[1]),
            old.map_or(7, |v| v.min(7))
        );
        assert_eq!(
            write.outcome,
            match old {
                None => WriteOutcome::Inserted,
                Some(10) => WriteOutcome::Updated,
                _ => WriteOutcome::Deduplicated,
            }
        );
    }
}

#[test]
fn automatic_ripen_uses_set_witnesses_and_does_not_globally_reject_other_rules() {
    use std::{fs, process::Command};
    let dir = std::env::temp_dir().join(format!("set-pipeline-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.egg");
    fs::write(
        &source,
        r#"
        (datatype E (V i64) (Done E))
        (function Cost (E) i64 :merge (min old new))
        (ruleset update)
        (rule ((= x (V 0))) ((set (Cost x) 7)) :ruleset update)
        (rewrite (V 0) (Done (V 1)))
        (set (Cost (V 0)) 10)
        (run update 1) (run 1)
    "#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--source"])
        .arg(source)
        .arg("--output")
        .arg(dir.join("run"))
        .env("EGG_LAYOUT_GRAPHS", "0")
        .env_remove("EGG_LAYOUT_COMPOSITION_DISCOVERY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("run/analysis.json")).unwrap()).unwrap();
    let jobs = report["saturated_rule_composition"]["jobs"]
        .as_array()
        .unwrap();
    assert!(
        jobs.iter().any(|j| j["origin"]["set_update_witnesses"]
            .as_array()
            .is_some_and(|v| !v.is_empty())),
        "{report:#}"
    );
    assert!(jobs.iter().all(|j| j["state"] == "Saturated"), "{report:#}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn virtual_table_update_preserves_the_same_before_witness() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
        (datatype E (A))
        (function Cost (E) i64 :merge (min old new))
        (ruleset update)
        (rule ((= x (A))) ((set (Cost x) 7)) :ruleset update)
        (set (Cost (A)) 10)
    "#,
    )
    .unwrap();
    let mut rows = vec![];
    eg.function_for_each("Cost", |row| rows.push(vec![row.vals[0], row.vals[1]])).unwrap();
    let template = eg
        .virtual_fact_template(vec![egglog::VirtualFactRow {
            function: "Cost".into(),
            columns: vec![0, 1],
        }])
        .unwrap();
    eg.virtualize_existing_facts(&template, &rows).unwrap();
    let trace = TraceSession::with_dependencies();
    eg.step_rules_with_trace("update", &trace).unwrap();
    let tables = trace.table_names();
    let table = tables
        .iter()
        .find(|(_, name)| name.as_ref() == "Cost")
        .unwrap()
        .0;
    let writes = trace.write_events();
    let w = writes.iter().find(|w| w.table == table).unwrap();
    assert_eq!(
        eg.value_to_base::<i64>(w.before.as_ref().unwrap().as_ref().unwrap()[1]),
        10
    );
    assert_eq!(eg.value_to_base::<i64>(w.actual[1]), 7);
    assert_eq!(w.outcome, WriteOutcome::Updated);
}
