//! Analyze's committed history -> portable parameterized port programs.
//! This export does not run ripen candidate discovery or save a full trace.
use super::*;
use cell_assisted_egglog::parameterized_cell::artifact::Bundle;

fn input_role(rule: &RuleInfo, input: &Input) -> String {
    match input {
        Input::Var(n) => format!("var:{n}"),
        Input::Read(span) => format!(
            "read:{}",
            rule.calls.get(span).map(|x| x.0.as_str()).unwrap_or(span)
        ),
    }
}
fn output_role(rule: &RuleInfo, output: &Output) -> String {
    match output {
        Output::Var(n) => format!("var:{n}"),
        Output::Row(span) => format!(
            "row:{}",
            rule.calls.get(span).map(|x| x.0.as_str()).unwrap_or(span)
        ),
        Output::Column(span, col) => format!(
            "column:{}:{col}",
            rule.calls.get(span).map(|x| x.0.as_str()).unwrap_or(span)
        ),
    }
}
pub fn bake_text(name: &str, text: &str) -> Result<Bundle> {
    let c = capture_text_with_sink(name, text, None, None)?;
    from_capture(&c)
}
pub(super) fn from_capture(c: &Captured) -> Result<Bundle> {
    let effects = c
        .records
        .iter()
        .filter(|r| !r.produced.is_empty() || !r.unions.is_empty() || r.updated_rows)
        .count();
    if effects == 0 {
        return Err("transfer export needs observed committed changes".into());
    }
    let sets = c
        .rules
        .iter()
        .map(|r| r.rule.ruleset.clone())
        .collect::<BTreeSet<_>>();
    let environment = format!(
        "{}\n{}\n{}",
        c.datatype,
        sets.iter()
            .filter(|s| !s.is_empty())
            .map(|s| format!("(ruleset {s})"))
            .collect::<Vec<_>>()
            .join("\n"),
        c.rules
            .iter()
            .map(|r| r.rule.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut parser = EGraph::default();
    let schedules = parser
        .parse_program(None, &c.preview_source)?
        .into_iter()
        .filter(|c| matches!(c, Command::RunSchedule(_)))
        .map(|c| c.to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut edges = BTreeMap::<(String, String, String, String), usize>::new();
    for r in &c.records {
        for (i, port) in r.ports.iter().enumerate() {
            if let Port::Parent(parent, output) = port {
                let p = &c.records[*parent];
                let key = (
                    c.rules[p.rule].rule.name.clone(),
                    output_role(&c.rules[p.rule], &p.outputs[*output]),
                    c.rules[r.rule].rule.name.clone(),
                    input_role(&c.rules[r.rule], &r.inputs[i]),
                );
                *edges.entry(key).or_default() += 1;
            }
        }
    }
    let evidence = json!({
        "source":"native_analyze committed read/write/union records",
        "accepted_records":c.records.len(),"records_with_committed_changes":effects,"rejected_records":c.rejected,
        "rules":c.rules.iter().enumerate().map(|(i,rule)| {
            let rs=c.records.iter().filter(|r|r.rule==i).collect::<Vec<_>>();
            json!({"rule":rule.rule.name,"ruleset":rule.rule.ruleset,"records":rs.len(),
                "inserted_outputs":rs.iter().map(|r|r.produced.len()).sum::<usize>(),
                "union_witnesses":rs.iter().map(|r|r.unions.len()).sum::<usize>(),
                "function_update_records":rs.iter().filter(|r|r.updated_rows).count(),
                "set_witnesses":rs.iter().map(|r|r.sets.len()).sum::<usize>()})
        }).collect::<Vec<_>>(),
        "connections":edges.into_iter().map(|((pr,po,cr,ci),n)|json!({"producer":pr,"output":po,"consumer":cr,"input":ci,"observations":n})).collect::<Vec<_>>(),
        "scope":"History identifies observed interactions. Complete typed port programs and original guards/actions are validated from source, including unobserved rule branches. Training values and graph instances are not exported."
    });
    Bundle::bake_terms(&environment, schedules, evidence)
}
pub fn export_file(source: &Path, target: &Path) -> Result<Json> {
    if target.exists() {
        return Err("transfer output already exists".into());
    }
    let b = bake_text(&source.to_string_lossy(), &std::fs::read_to_string(source)?)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target, b.text()?)?;
    Ok(b.report())
}

/// Inputs contain only new facts/checks and schedules. No original source,
/// history file, analyzer or planner is consulted after the bundle is loaded.
pub fn run_file(bundle: &Path, input: &Path, out: &Path) -> Result<Json> {
    let library = Bundle::load(bundle)?;
    let mut runtime = library.instantiate()?;
    let mut parser = runtime.engine().clone();
    let commands = parser.parse_program(
        Some(input.to_string_lossy().into()),
        &std::fs::read_to_string(input)?,
    )?;
    if commands.iter().any(|c| {
        !matches!(
            c,
            Command::Action(_) | Command::Check(..) | Command::RunSchedule(_)
        )
    }) {
        return Err("transfer input accepts only facts/checks/schedules".into());
    }
    std::fs::create_dir(out)?;
    let mut actions = String::new();
    let mut boundaries = vec![];
    for command in commands {
        if !matches!(command, Command::RunSchedule(_)) {
            actions.push_str(&format!("{command}\n"));
            continue;
        }
        runtime.input(&actions)?;
        actions.clear();
        runtime.run(std::slice::from_ref(&command))?;
        let rows = runtime.audit_mirror()?;
        boundaries.push(json!({"boundary":boundaries.len(),"checked_mirror_rows":rows,"runtime":runtime.report()}));
    }
    if !actions.trim().is_empty() {
        runtime.input(&actions)?;
    }
    let result = json!({"passed":true,"transfer_run_complete":true,"library":library.report(),"boundaries":boundaries,
        "runtime":runtime.report(),"scope":"Fresh runtime, serialized typed wiring, original native effects. Input checks passed; independent full-graph comparison is performed by the roundtrip tests."});
    std::fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}
