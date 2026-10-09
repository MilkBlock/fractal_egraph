//! Differential verification of extracted Expr-size operators on saved native Cells.
//! Usage: cell_operator_verify MANIFEST.json REPORT.json
//! This checks supplied sample inputs, not an admission certificate or a benchmark.

use egg_layout::pipeline::Result;
use egglog::{
    CommandOutput, EGraph,
    ast::{
        Action, Command, Expr, GenericRunConfig, GenericSchedule, Literal, Parser, RustSpan, Span,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    cells: Vec<Cell>,
    inputs: Vec<i64>,
    domain_max: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cell {
    id: usize,
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    isolation_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compiled_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compiled_ruleset: Option<String>,
    rulesets: Vec<String>,
    input_term: String,
    outputs: Vec<Output>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Output {
    term: String,
    /// The extracted affine expression is coefficients[0] * input + coefficients[1].
    coefficients: [i64; 2],
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct Row {
    values: Vec<String>,
    subsumed: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Table {
    sorts: Vec<String>,
    rows: Vec<Row>,
}

type Snapshot = BTreeMap<String, Table>;

fn snapshot(eg: &EGraph) -> Result<Snapshot> {
    let mut result = BTreeMap::new();
    for name in eg.get_function_names() {
        let schema = eg.get_function(&name).ok_or("missing function")?.schema();
        let sorts: Vec<_> = schema.input.iter().chain([&schema.output]).collect();
        let mut raw = Vec::new();
        eg.function_for_each(&name, |row| raw.push((row.vals.to_vec(), row.subsumed)))?;
        let mut rows = Vec::new();
        for (values, subsumed) in raw {
            if values.len() != sorts.len() {
                return Err(format!("unexpected row arity in {name}").into());
            }
            rows.push(Row {
                values: values
                    .into_iter()
                    .zip(&sorts)
                    .map(|(value, sort)| {
                        let value = eg.get_canonical_value(value, sort);
                        if sort.name() == "i64" {
                            eg.value_to_base::<i64>(value).to_string()
                        } else {
                            format!("{value:?}")
                        }
                    })
                    .collect(),
                subsumed,
            });
        }
        rows.sort();
        result.insert(
            name,
            Table {
                sorts: sorts.iter().map(|s| s.name().to_owned()).collect(),
                rows,
            },
        );
    }
    Ok(result)
}

fn unchanged_non_size(before: &Snapshot, after: &Snapshot, context: &str) -> Result<()> {
    if !before
        .iter()
        .filter(|(name, _)| name.as_str() != "Expr-size")
        .eq(after
            .iter()
            .filter(|(name, _)| name.as_str() != "Expr-size"))
    {
        return Err(format!("{context}: a non-Expr-size table changed").into());
    }
    Ok(())
}

fn same_snapshot(before: &Snapshot, after: &Snapshot, context: &str) -> Result<()> {
    if before != after {
        let tables: Vec<_> = before
            .keys()
            .chain(after.keys())
            .filter(|name| before.get(*name) != after.get(*name))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        return Err(format!("{context}: tables differ: {tables:?}").into());
    }
    Ok(())
}

fn one_step(ruleset: String) -> Command {
    Command::RunSchedule(GenericSchedule::Run(
        egglog::span!(),
        GenericRunConfig {
            ruleset,
            until: None,
        },
    ))
}

fn saturate(eg: &mut EGraph, rulesets: &[String]) -> Result<usize> {
    let mut ordered = rulesets.to_vec();
    ordered.sort();
    ordered.dedup();
    let commands: Vec<_> = ordered.into_iter().map(one_step).collect();
    let mut sweeps = 0;
    loop {
        sweeps += 1;
        let outputs = eg.run_program(commands.clone())?;
        if !outputs
            .iter()
            .any(|o| matches!(o, CommandOutput::RunSchedule(r) if r.updated))
        {
            return Ok(sweeps);
        }
    }
}

fn affine(coefficients: [i64; 2], input: i64) -> Result<i64> {
    let [a, b] = coefficients;
    let result = i128::from(a)
        .checked_mul(i128::from(input))
        .and_then(|value| value.checked_add(i128::from(b)))
        .ok_or("affine evaluation overflows i128")?;
    Ok(i64::try_from(result).map_err(|_| "affine result is outside i64")?)
}

fn validate(manifest: &Manifest) -> Result<()> {
    if manifest.domain_max < 0 || manifest.cells.is_empty() || manifest.inputs.is_empty() {
        return Err("nonempty cells/inputs and a nonnegative domain_max are required".into());
    }
    for input in &manifest.inputs {
        if !(0..=manifest.domain_max).contains(input) {
            return Err(format!("input {input} is outside [0, {}]", manifest.domain_max).into());
        }
    }
    let mut ids = std::collections::BTreeSet::new();
    for cell in &manifest.cells {
        match (&cell.compiled_source, &cell.compiled_ruleset) {
            (None, None) => {}
            (Some(_), Some(name)) if !name.is_empty() && !cell.rulesets.contains(name) => {}
            _ => {
                return Err(
                    "compiled_source and a separate nonempty compiled_ruleset are required together"
                        .into(),
                );
            }
        }
        if !ids.insert(cell.id) || cell.outputs.is_empty() || cell.rulesets.is_empty() {
            return Err(
                "unique cell ids, nonempty outputs, and saved rulesets are required".into(),
            );
        }
        for output in &cell.outputs {
            // Affine extrema are at the endpoints. This proves representability of
            // the compiled result, not definedness of every native intermediate.
            affine(output.coefficients, 0)?;
            affine(output.coefficients, manifest.domain_max)?;
            for input in &manifest.inputs {
                affine(output.coefficients, *input)?;
            }
        }
    }
    Ok(())
}

fn size_set(term: &Expr, value: i64) -> Command {
    Command::Action(Action::Set(
        egglog::span!(),
        "Expr-size".into(),
        vec![term.clone()],
        Expr::Lit(egglog::span!(), Literal::Int(value)),
    ))
}

fn verify_cell(cell: &Cell, inputs: &[i64]) -> Result<Json> {
    let source = Path::new(&cell.source).canonicalize()?;
    let source_text = std::fs::read_to_string(&source)?;
    let mut parent = EGraph::default();
    let commands = parent.parse_program(Some(source.display().to_string()), &source_text)?;
    parent.run_program(commands)?;
    let parent_sweeps = saturate(&mut parent, &cell.rulesets)?;
    let baseline = snapshot(&parent)?;
    let size_schema = parent
        .get_function("Expr-size")
        .ok_or("Expr-size is absent from the saved source")?
        .schema();
    if size_schema.input.len() != 1
        || size_schema.input[0].name() != "Expr"
        || size_schema.output.name() != "i64"
    {
        return Err("Expr-size must have schema (Expr) -> i64".into());
    }
    let mut parser = Parser::default();
    let input_expr = parser.get_expr_from_string(None, &cell.input_term)?;
    let mut expressions = vec![input_expr.clone()];
    for output in &cell.outputs {
        expressions.push(parser.get_expr_from_string(None, &output.term)?);
    }
    let mut keys = Vec::new();
    for expr in &expressions {
        let (sort, value) = parent.eval_expr(expr)?;
        if sort.name() != "Expr" {
            return Err(format!("interface term must have Expr sort: {expr}").into());
        }
        let value = parent.get_canonical_value(value, &sort);
        if parent.lookup_function_row("Expr-size", &[value]).is_some() {
            return Err(format!("unseeded saved parent already has Expr-size for {expr}").into());
        }
        keys.push(format!("{value:?}"));
    }
    same_snapshot(
        &baseline,
        &snapshot(&parent)?,
        "resolving the saved interface must not create or change rows",
    )?;

    // The generated diagnostic weakens every size-sensitive structural rule's
    // body and fails if any remaining structural precondition can match. Run it
    // against the full native tables, including containers absent from exported
    // graph projections. Its declarations and observer facts stay in this fork.
    let isolation_source = if let Some(path) = &cell.isolation_source {
        let path = Path::new(path).canonicalize()?;
        let text = std::fs::read_to_string(&path)?;
        let mut diagnostic = parent.fork_shared();
        let result = diagnostic.parse_and_run_program(Some(path.display().to_string()), &text);
        same_snapshot(
            &baseline,
            &snapshot(&parent)?,
            "frozen parent after structural isolation diagnostic",
        )?;
        result.map_err(|error| {
            format!(
                "cell {} structural isolation check failed: {error}",
                cell.id
            )
        })?;
        Some(path)
    } else {
        None
    };

    let (compiled_source, mut shortcut_base) = if let Some(path) = &cell.compiled_source {
        let path = Path::new(path).canonicalize()?;
        let text = std::fs::read_to_string(&path)?;
        let name = cell
            .compiled_ruleset
            .as_ref()
            .ok_or("missing shortcut ruleset")?;
        let mut base = parent.fork_shared();
        let commands = base.parse_program(Some(path.display().to_string()), &text)?;
        let mut declarations = 0;
        let mut rules = 0;
        for command in &commands {
            match command {
                Command::AddRuleset(_, declared) if declared == name => declarations += 1,
                Command::Rule { rule } if &rule.ruleset == name => rules += 1,
                _ => {
                    return Err(
                        "shortcut source may only declare its ruleset and define its one rule; no top-level actions or schedules"
                            .into(),
                    );
                }
            }
        }
        if declarations != 1 || rules != 1 {
            return Err("shortcut source requires exactly one ruleset declaration and rule".into());
        }
        base.run_program(commands)?;
        same_snapshot(&baseline, &snapshot(&base)?, "shortcut installation")?;
        same_snapshot(
            &baseline,
            &snapshot(&parent)?,
            "parent after shortcut installation",
        )?;
        (Some(path), Some(base))
    } else {
        (None, None)
    };

    let mut cases = Vec::new();
    for &input in inputs {
        let mut native = parent.fork_shared();
        let mut compiled = parent.fork_shared();
        let seed = size_set(&input_expr, input);
        native.run_program(vec![seed.clone()])?;
        compiled.run_program(vec![seed])?;
        let native_sweeps = saturate(&mut native, &cell.rulesets)?;
        let mut proposed = Vec::new();
        let mut compiled_commands = Vec::new();
        for (output, expr) in cell.outputs.iter().zip(&expressions[1..]) {
            let value = affine(output.coefficients, input)?;
            proposed.push(value);
            compiled_commands.push(size_set(expr, value));
        }
        // This path executes only the input/set actions and their native rebuilds.
        // In particular, it never runs any rule or schedule after the fork.
        compiled.run_program(compiled_commands)?;
        let native_state = snapshot(&native)?;
        let compiled_state = snapshot(&compiled)?;
        unchanged_non_size(&baseline, &native_state, "native path")?;
        unchanged_non_size(&baseline, &compiled_state, "compiled path")?;
        same_snapshot(&native_state, &compiled_state, "native versus compiled")?;
        let shortcut_matches = if let Some(base) = &mut shortcut_base {
            let mut shortcut = base.fork_shared();
            shortcut.run_program(vec![size_set(&input_expr, input)])?;
            let outputs = shortcut.run_program(vec![one_step(
                cell.compiled_ruleset
                    .clone()
                    .ok_or("missing shortcut ruleset")?,
            )])?;
            let matches: usize = outputs
                .iter()
                .filter_map(|output| match output {
                    CommandOutput::RunSchedule(report) => {
                        Some(report.num_matches_per_rule.values().sum::<usize>())
                    }
                    _ => None,
                })
                .sum();
            if matches != 1 {
                return Err(format!(
                    "shortcut must execute exactly one rule match, observed {matches}"
                )
                .into());
            }
            let shortcut_state = snapshot(&shortcut)?;
            unchanged_non_size(&baseline, &shortcut_state, "shortcut path")?;
            same_snapshot(
                &native_state,
                &shortcut_state,
                "native versus shortcut rule",
            )?;
            same_snapshot(&baseline, &snapshot(base)?, "frozen shortcut base")?;
            Some(matches)
        } else {
            None
        };
        same_snapshot(&baseline, &snapshot(&parent)?, "frozen parent")?;
        cases.push(json!({
            "input":input,
            "proposed_outputs":proposed,
            "native_sweeps":native_sweeps,
            "all_tables_equal":true,
            "shortcut_all_tables_equal":shortcut_matches.map(|_| true),
            "shortcut_rule_executed":shortcut_matches.map(|_| true),
            "shortcut_rule_matches":shortcut_matches,
            "non_size_tables_unchanged":true,
            "parent_unchanged":true,
            "expr_size":native_state.get("Expr-size"),
        }));
    }
    Ok(json!({
        "id":cell.id,
        "source":source,
        "parent_sweeps":parent_sweeps,
        "unseeded_interface_sizes_absent":true,
        "interface_resolution_preserved_all_tables":true,
        "isolation_checked":isolation_source.is_some(),
        "isolation_source":isolation_source,
        "compiled_source":compiled_source,
        "compiled_ruleset":cell.compiled_ruleset,
        "shortcut_installation_preserved_all_tables":shortcut_base.as_ref().map(|_| true),
        "interface_keys":keys,
        "observable_tables":baseline.len(),
        "cases":cases,
    }))
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: cell_operator_verify MANIFEST.json REPORT.json".into());
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    // Validate the entire numerical contract before creating or mutating an engine.
    validate(&manifest)?;
    let mut cells = Vec::new();
    for cell in &manifest.cells {
        cells.push(verify_cell(cell, &manifest.inputs)?);
    }
    let report = json!({
        "schema":"cell-operator-native-verification/v1",
        "manifest":manifest,
        "cells":cells,
        "status":"passed",
        "scope":"actual native egglog forks of each saved baked Cell contract; sampled Expr-size subsystem equivalence for direct sets and optional one-step generated shortcut rule, with all non-size tables unchanged; no benchmark, global-rule or full-domain equivalence claim, or runtime admission certificate",
    });
    std::fs::write(&args[2], serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
