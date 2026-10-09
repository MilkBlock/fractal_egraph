//! Compare complete Math structure and equality partitions on actual egglog.
//! Usage: structural_cell_verify MANIFEST.json REPORT.json
//! The compiled path executes constructor/union actions without a rule schedule.
//! Inspection looks up existing Math rows; it never evaluates a Math expression.

use egg_layout::pipeline::Result;
use egglog::{
    EGraph, Value,
    ast::{Action, Command, Expr, Literal, Parser},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    environment_source: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    actions: Vec<String>,
    expected_nodes: Vec<String>,
    expected_classes: Vec<Vec<String>>,
    compiled_actions: Vec<String>,
    #[serde(default)]
    replay_source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct Row {
    op: String,
    args: Vec<String>,
    result: String,
    subsumed: bool,
}

#[derive(Clone)]
struct Resolved {
    sort: String,
    value: Value,
    row: Option<Row>,
}

fn path(base: &Path, text: &str) -> Result<PathBuf> {
    Ok(base.join(text).canonicalize()?)
}

fn canonical(eg: &EGraph, value: Value, sort: &egglog::ArcSort) -> String {
    let value = eg.get_canonical_value(value, sort);
    if sort.name() == "i64" {
        format!("i64:{}", eg.value_to_base::<i64>(value))
    } else {
        format!("{}:{value:?}", sort.name())
    }
}

/// Resolve only ground Math constructors and i64 literals. eval_expr is safe
/// here only for the literal branch; calls use non-creating table lookup.
fn resolve(
    eg: &mut EGraph,
    expr: &Expr,
    memo: &mut BTreeMap<String, Resolved>,
) -> Result<Resolved> {
    let text = expr.to_string();
    if let Some(result) = memo.get(&text) {
        return Ok(result.clone());
    }
    let resolved = match expr {
        Expr::Lit(_, Literal::Int(_)) => {
            let (sort, value) = eg.eval_expr(expr)?;
            Resolved {
                sort: sort.name().into(),
                value,
                row: None,
            }
        }
        Expr::Call(_, op, args) => {
            let function = eg
                .get_function(op)
                .ok_or_else(|| format!("unknown operator: {op}"))?;
            if !function.is_constructor() || function.is_let_binding() {
                return Err(format!("expected a constructor, got {op}").into());
            }
            let schema = function.schema().clone();
            if schema.output.name() != "Math"
                || schema
                    .input
                    .iter()
                    .any(|s| !matches!(s.name(), "Math" | "i64"))
                || args.len() != schema.input.len()
            {
                return Err(format!("unsupported Math constructor schema: {op}").into());
            }
            let mut key = Vec::new();
            for (arg, sort) in args.iter().zip(&schema.input) {
                let resolved = resolve(eg, arg, memo)?;
                if resolved.sort != sort.name() {
                    return Err(format!("sort mismatch in {expr}: expected {}", sort.name()).into());
                }
                key.push(eg.get_canonical_value(resolved.value, sort));
            }
            let found = eg
                .lookup_function_row(op, &key)
                .ok_or_else(|| format!("expected constructor row is absent: {expr}"))?;
            let value = eg.get_canonical_value(found[args.len()], &schema.output);
            let row = Row {
                op: op.clone(),
                args: key
                    .iter()
                    .zip(&schema.input)
                    .map(|(v, s)| canonical(eg, *v, s))
                    .collect(),
                result: canonical(eg, value, &schema.output),
                subsumed: false,
            };
            Resolved {
                sort: "Math".into(),
                value,
                row: Some(row),
            }
        }
        _ => return Err(format!("expected a ground Math term with i64 literals: {expr}").into()),
    };
    memo.insert(text, resolved.clone());
    Ok(resolved)
}

/// Internal let bindings are aliases, not datatype enodes. Include subsumed
/// constructor rows in the comparison so no hidden structure is overlooked.
fn snapshot(eg: &EGraph) -> Result<(BTreeSet<Row>, BTreeMap<String, usize>)> {
    let mut rows = BTreeSet::new();
    let mut sizes = BTreeMap::new();
    for op in eg.get_function_names() {
        let function = eg.get_function(&op).ok_or("missing function")?;
        let schema = function.schema();
        if schema.output.name() != "Math" || function.is_let_binding() {
            continue;
        }
        let supported = function.is_constructor()
            && schema
                .input
                .iter()
                .all(|s| matches!(s.name(), "Math" | "i64"));
        let arity = schema.input.len();
        let mut count = 0;
        eg.function_for_each(&op, |row| {
            count += 1;
            if !supported {
                return;
            }
            rows.insert(Row {
                op: op.clone(),
                args: row.vals[..arity]
                    .iter()
                    .zip(&schema.input)
                    .map(|(v, s)| canonical(eg, *v, s))
                    .collect(),
                result: canonical(eg, row.vals[arity], &schema.output),
                subsumed: row.subsumed,
            });
        })?;
        if count != 0 && !supported {
            return Err(format!("unsupported populated function in Math universe: {op}").into());
        }
        sizes.insert(op, count);
    }
    Ok((rows, sizes))
}

fn verify(eg: &mut EGraph, case: &Case) -> Result<Json> {
    let before = snapshot(eg)?;
    let mut parser = Parser::default();
    let mut memo = BTreeMap::new();
    let mut expected = BTreeSet::new();
    let mut terms = BTreeMap::new();
    for text in &case.expected_nodes {
        let expr = parser.get_expr_from_string(None, text)?;
        let resolved = resolve(eg, &expr, &mut memo)?;
        let row = resolved
            .row
            .ok_or("expected_nodes must contain Math constructor terms")?;
        if terms.insert(expr.to_string(), row.result.clone()).is_some() {
            return Err(format!("duplicate expected term: {expr}").into());
        }
        expected.insert(row);
    }
    if before.0 != expected {
        let unexpected: Vec<_> = before.0.difference(&expected).collect();
        let missing: Vec<_> = expected.difference(&before.0).collect();
        return Err(format!(
            "Math universe mismatch: unexpected={unexpected:?}, missing={missing:?}"
        )
        .into());
    }
    let mut covered = BTreeSet::new();
    let mut class_roots = BTreeSet::new();
    for group in &case.expected_classes {
        if group.is_empty() {
            return Err("expected equality classes must be nonempty".into());
        }
        let mut root = None;
        for text in group {
            let normalized = parser.get_expr_from_string(None, text)?.to_string();
            let actual = terms
                .get(&normalized)
                .ok_or_else(|| format!("class contains an unlisted term: {text}"))?;
            if !covered.insert(normalized) {
                return Err(format!("term appears in multiple class positions: {text}").into());
            }
            if root.as_ref().is_some_and(|value| value != actual) {
                return Err(format!("expected equality is absent in group {group:?}").into());
            }
            root = Some(actual.clone());
        }
        if !class_roots.insert(root.unwrap()) {
            return Err("distinct expected classes were merged by the runtime".into());
        }
    }
    if covered.len() != terms.len() {
        return Err(
            "expected_classes must partition every expected_nodes term exactly once".into(),
        );
    }
    if snapshot(eg)? != before {
        return Err("verification changed the Math universe".into());
    }
    let roots: BTreeSet<_> = before.0.iter().map(|r| &r.result).collect();
    if roots.len() != class_roots.len() {
        return Err("equality partition does not cover every Math eclass".into());
    }
    Ok(json!({
        "status": "passed",
        "matched_constructor_rows": before.0.len(),
        "physical_constructor_rows": before.1.values().sum::<usize>(),
        "math_eclasses": roots.len(),
        "expected_symbolic_terms": terms.len(),
        "constructor_table_rows": before.1,
        "inspection_created_math_nodes": false,
    }))
}

fn install_environment(source: &str, source_name: &str) -> Result<EGraph> {
    let mut eg = EGraph::default();
    let commands = eg.parse_program(Some(source_name.into()), source)?;
    for command in &commands {
        if !matches!(
            command,
            Command::Sort { .. }
                | Command::Datatype { .. }
                | Command::Datatypes { .. }
                | Command::Constructor { .. }
                | Command::Rule { .. }
                | Command::Rewrite(..)
                | Command::BiRewrite(..)
                | Command::AddRuleset(..)
        ) {
            return Err(
                "environment may contain only sort/constructor and rule declarations".into(),
            );
        }
    }
    eg.run_program(commands)?;
    if !snapshot(&eg)?.0.is_empty() {
        return Err("environment must not seed Math rows".into());
    }
    Ok(eg)
}

fn actions(eg: &mut EGraph, source: &[String]) -> Result<usize> {
    let commands = eg.parse_program(None, &source.join("\n"))?;
    for command in &commands {
        if !matches!(
            command,
            Command::Action(Action::Expr(..) | Action::Union(..) | Action::Let(..))
        ) {
            return Err("case actions permit only constructor expressions, unions, and let bindings; no schedules".into());
        }
    }
    let count = commands.len();
    eg.run_program(commands)?;
    Ok(count)
}

fn saturate(eg: &mut EGraph) -> Result<usize> {
    let mut sweeps = 0;
    loop {
        sweeps += 1;
        if !eg.step_rules("")?.updated {
            return Ok(sweeps);
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err(format!("usage: {} MANIFEST.json REPORT.json", args[0]).into());
    }
    let manifest_path = Path::new(&args[1]).canonicalize()?;
    let base = manifest_path
        .parent()
        .ok_or("manifest has no parent directory")?;
    let manifest: Manifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest.cases.is_empty() {
        return Err("manifest requires at least one case".into());
    }
    let environment_path = path(base, &manifest.environment_source)?;
    let source = fs::read_to_string(&environment_path)?;
    EGraph::set_num_threads(1);
    let mut names = BTreeSet::new();
    let mut reports = Vec::new();
    for case in &manifest.cases {
        if case.name.is_empty() || !names.insert(&case.name) {
            return Err("case names must be nonempty and unique".into());
        }
        let execute = || -> Result<Json> {
            let mut native = install_environment(&source, &environment_path.display().to_string())?;
            let seed_actions = actions(&mut native, &case.actions)?;
            let sweeps = saturate(&mut native)?;
            let native_result = verify(&mut native, case)?;
            let mut compiled =
                install_environment(&source, &environment_path.display().to_string())?;
            actions(&mut compiled, &case.actions)?;
            let compiled_actions = actions(&mut compiled, &case.compiled_actions)?;
            let compiled_result = verify(&mut compiled, case)?;
            let replay = if let Some(file) = &case.replay_source {
                let file = path(base, file)?;
                let mut eg = EGraph::default();
                eg.parse_and_run_program(
                    Some(file.display().to_string()),
                    &fs::read_to_string(&file)?,
                )?;
                let replay_sweeps = saturate(&mut eg)?;
                Some(
                    json!({"source": file, "saturation_sweeps": replay_sweeps, "verification": verify(&mut eg, case)?}),
                )
            } else {
                None
            };
            Ok(json!({
                "name": case.name, "seed_action_count": seed_actions,
                "native_saturation_sweeps": sweeps, "native": native_result,
                "compiled_action_count": compiled_actions,
                "compiled_rule_schedules": 0, "compiled": compiled_result,
                "source_replay": replay,
            }))
        };
        reports.push(execute().map_err(|error| format!("case {}: {error}", case.name))?);
    }
    let report = json!({
        "schema_version": 1, "runtime": "actual egglog runtime",
        "scope": "complete Math constructor rows and full equality partition for supplied finite cases; no timings",
        "inspection": "ground Math terms use non-creating lookup; only i64 literals are evaluated; let aliases are excluded",
        "manifest_source": manifest_path, "environment_source": environment_path,
        "case_count": reports.len(), "cases": reports,
    });
    fs::write(
        &args[2],
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!(
        "verified {} structural cases; report={}",
        manifest.cases.len(),
        args[2]
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("structural_cell_verify: {error}");
        std::process::exit(1);
    }
}
