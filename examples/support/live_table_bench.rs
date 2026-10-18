//! Full schedule benchmark. Exact graph equality is relative to the shared
//! checkpoint, so newly allocated eclass IDs need not coincide across engines.
use cell_assisted_egglog::table_live::Runtime;
use egglog::{ArcSort, CommandOutput, EGraph, Value, ast::Command};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Atom {
    Eq(String, Value),
    Base(String, String),
}
type Row = (String, Vec<Atom>, Atom, bool);
fn atom(eg: &EGraph, v: Value, s: &ArcSort) -> Result<Atom> {
    if s.is_eq_sort() {
        return Ok(Atom::Eq(s.name().into(), eg.get_canonical_value(v, s)));
    }
    let text = match s.name() {
        "i64" => eg.value_to_base::<i64>(v).to_string(),
        "f64" => format!("{:016x}", eg.value_to_base::<egglog::sort::F>(v).to_bits()),
        "bool" => eg.value_to_base::<bool>(v).to_string(),
        "String" => eg.value_to_base::<egglog::sort::S>(v).to_string(),
        "Unit" => "()".into(),
        other => {
            return Err(format!(
                "exact graph comparison does not support primitive/container sort {other}"
            )
            .into());
        }
    };
    Ok(Atom::Base(s.name().into(), text))
}
fn snapshot(eg: &EGraph) -> Result<Vec<Row>> {
    let mut rows = vec![];
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        if f.is_let_binding() || name.starts_with('@') {
            continue;
        }
        eg.function_for_each(&name, |row| {
            let values = row
                .vals
                .iter()
                .zip(f.schema().input.iter().chain([&f.schema().output]))
                .map(|(v, s)| atom(eg, *v, s))
                .collect::<Result<Vec<_>>>();
            rows.push(values.map(|mut v| {
                let result = v.pop().unwrap();
                (name.clone(), v, result, row.subsumed)
            }));
        })?;
    }
    rows.into_iter().collect()
}
fn pair(
    a: Atom,
    b: Atom,
    map: &mut BTreeMap<Atom, Atom>,
    inverse: &mut BTreeMap<Atom, Atom>,
) -> Result<()> {
    if map.get(&a).is_some_and(|old| old != &b) || inverse.get(&b).is_some_and(|old| old != &a) {
        return Err(format!("different equality partition at {a:?} -> {b:?}").into());
    }
    map.insert(a.clone(), b.clone());
    inverse.insert(b, a);
    Ok(())
}
fn equal(base: &EGraph, a: &EGraph, b: &EGraph) -> Result<Json> {
    equal_projected(base, a, b, false)
}
fn equal_projected(base: &EGraph, a: &EGraph, b: &EGraph, omit_smaller: bool) -> Result<Json> {
    let mut map = BTreeMap::new();
    let mut inverse = BTreeMap::new();
    // Shared old IDs anchor cycles and pre-existing open inputs. Never assume IDs
    // of terms created by differently ordered RHS applications are the same.
    for (_, args, result, _) in snapshot(base)? {
        for v in args.into_iter().chain([result]) {
            if let Atom::Eq(sort, value) = v {
                let s = a.get_sort_by_name(&sort).ok_or("missing equality sort")?;
                pair(
                    atom(a, value, &s)?,
                    atom(b, value, &s)?,
                    &mut map,
                    &mut inverse,
                )?;
            }
        }
    }
    let left = snapshot(a)?
        .into_iter()
        .filter(|r| !omit_smaller || r.0 != "Smaller")
        .collect::<Vec<_>>();
    let right = snapshot(b)?
        .into_iter()
        .filter(|r| !omit_smaller || r.0 != "Smaller")
        .collect::<Vec<_>>();
    let counts = |rows: &[Row]| {
        let mut out = BTreeMap::<String, usize>::new();
        for (op, ..) in rows {
            *out.entry(op.clone()).or_default() += 1;
        }
        out
    };
    let lc = counts(&left);
    let rc = counts(&right);
    if lc != rc {
        let diff = lc
            .keys()
            .chain(rc.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|k| lc.get(*k) != rc.get(*k))
            .map(|k| (k, lc.get(k), rc.get(k)))
            .collect::<Vec<_>>();
        return Err(format!("different final table counts: {diff:?}").into());
    }
    let index = right
        .into_iter()
        .map(|(op, args, r, h)| ((op, args), (r, h)))
        .collect::<BTreeMap<_, _>>();
    let total = left.len();
    if index.len() != total {
        return Err("duplicate native canonical keys".into());
    }
    let mut pending = left;
    while !pending.is_empty() {
        let before = pending.len();
        let mut rest = vec![];
        for (op, args, result, hidden) in pending {
            let key = args
                .iter()
                .map(|v| match v {
                    Atom::Eq(..) => map.get(v).cloned(),
                    _ => Some(v.clone()),
                })
                .collect::<Option<Vec<_>>>();
            let Some(key) = key else {
                rest.push((op, args, result, hidden));
                continue;
            };
            let (target, h) = index
                .get(&(op.clone(), key))
                .ok_or_else(|| format!("missing final fact {op} {args:?}"))?;
            if *h != hidden {
                return Err(format!("different visibility {op} {args:?}").into());
            }
            match (&result, target) {
                (Atom::Eq(s, _), Atom::Eq(t, _)) if s == t => {
                    pair(result, target.clone(), &mut map, &mut inverse)?
                }
                (Atom::Base(..), Atom::Base(..)) if &result == target => (),
                _ => {
                    return Err(
                        format!("different final value {op}: {result:?} vs {target:?}").into(),
                    );
                }
            }
        }
        if rest.len() == before {
            return Err("unmapped newly created cycle in graph comparison".into());
        }
        pending = rest;
    }
    Ok(
        json!({"exact_rows_values_equivalence_visibility":true,"rows":total,"eclass_mapping":map.len(),"tables":lc}),
    )
}
fn graph_sizes(eg: &EGraph) -> Result<Json> {
    let mut nodes = BTreeMap::<String, usize>::new();
    let mut classes = BTreeMap::<String, BTreeSet<Value>>::new();
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        let sort = f.schema().output.name();
        if !f.is_constructor() || !["Expr", "Term", "TermAndCost"].contains(&sort) {
            continue;
        }
        eg.function_for_each(&name, |row| {
            if !row.subsumed {
                *nodes.entry(sort.into()).or_default() += 1;
                classes
                    .entry(sort.into())
                    .or_default()
                    .insert(eg.get_canonical_value(*row.vals.last().unwrap(), &f.schema().output));
            }
        })?;
    }
    Ok(
        json!({"table_rows":eg.num_tuples(),"sorts":nodes.into_iter().map(|(s,n)|{let c=classes[&s].len();(s,json!({"enodes":n,"eclasses":c}))}).collect::<BTreeMap<_,_>>()}),
    )
}

fn extraction_costs(a: &EGraph, b: &EGraph) -> Result<Json> {
    fn values(eg: &EGraph) -> Result<BTreeMap<Vec<Value>, BTreeSet<i64>>> {
        let pair = eg
            .get_function("TCPair")
            .ok_or("no TCPair quality metric")?;
        let extracted = eg
            .get_function("ExtractedExpr")
            .ok_or("no ExtractedExpr quality metric")?;
        let mut costs = BTreeMap::<Value, BTreeSet<i64>>::new();
        eg.function_for_each("TCPair", |row| {
            if !row.subsumed {
                costs
                    .entry(eg.get_canonical_value(row.vals[2], &pair.schema().output))
                    .or_default()
                    .insert(eg.value_to_base::<i64>(row.vals[1]));
            }
        })?;
        let mut out = BTreeMap::new();
        eg.function_for_each("ExtractedExpr", |row| {
            if !row.subsumed {
                let key = row
                    .vals
                    .iter()
                    .zip(&extracted.schema().input)
                    .map(|(v, s)| eg.get_canonical_value(*v, s))
                    .collect();
                let root =
                    eg.get_canonical_value(*row.vals.last().unwrap(), &extracted.schema().output);
                out.insert(key, costs.get(&root).cloned().unwrap_or_default());
            }
        })?;
        Ok(out)
    }
    let left = values(a)?;
    let right = values(b)?;
    let different = left
        .keys()
        .chain(right.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|k| left.get(*k) != right.get(*k))
        .count();
    Ok(
        json!({"same_extracted_costs":left==right,"native_expr_keys":left.len(),"gpu_expr_keys":right.len(),"different_keys":different,
        "native_unresolved":left.values().filter(|x|x.is_empty()).count(),"gpu_unresolved":right.values().filter(|x|x.is_empty()).count(),
        "scope":"Every ExtractedExpr key uses its unchanged checkpoint Expr identity; compare the complete TCPair cost set. Equal cost is not identical selected Term syntax."}),
    )
}

pub fn benchmark(
    base: &EGraph,
    commands: &[Command],
    label: &str,
    worker: &Path,
    checked: bool,
    dest: &Path,
) -> Result<()> {
    let t = Instant::now();
    let mut native = base.clone();
    let native_clone = t.elapsed().as_secs_f64();
    let old = egglog::set_probe_counts_enabled(false);
    let tick = Instant::now();
    let messages = native.run_program(commands.to_vec())?;
    let native_seconds = tick.elapsed().as_secs_f64();
    let mut report = egglog_reports::RunReport::default();
    for m in messages {
        if let CommandOutput::RunSchedule(r) = m {
            report.union(r);
        }
    }
    let mut ordered = Runtime::new_cpu(base.clone(), commands)?;
    ordered.run(commands)?;
    let sorted_stats = ordered.report();
    let t = Instant::now();
    let copy = base.clone();
    let gpu_clone = t.elapsed().as_secs_f64();
    let mut runtime = Runtime::new(copy, commands, worker, checked)?;
    runtime.run(commands)?;
    egglog::set_probe_counts_enabled(old);
    let stats = runtime.report();
    let verification = equal(base, &native, runtime.engine());
    let same_commit_verification = equal(base, ordered.engine(), runtime.engine());
    let costs = extraction_costs(&native, runtime.engine());
    let projected = equal_projected(base, &native, runtime.engine(), true);
    let mut continued = runtime.engine().clone();
    let t = Instant::now();
    continued.run_program(commands.to_vec())?;
    let closure_seconds = t.elapsed().as_secs_f64();
    let closure = equal(runtime.engine(), runtime.engine(), &continued);

    let output = json!({"complete":verification.is_ok(),"benchmark_finished":true,"label":label,"schedule":commands.iter().map(ToString::to_string).collect::<Vec<_>>(),
  "scope":"Full original schedule; GPU persistent table joins drive original native RHS/merge/rebuild. Explicit tables, not certified Cell shortcuts. Same native fork, not pristine. Preparation included separately; no repeated cold queries.",
  "native":{"run_seconds":native_seconds,"iterations":report.iterations.len(),"logical_matches":report.num_matches_per_rule.values().sum::<usize>(),
   "search_apply_seconds":report.search_and_apply_time_per_ruleset.values().map(|t|t.as_secs_f64()).sum::<f64>(),
   "insert_merge_seconds":report.merge_time_per_ruleset.values().map(|t|t.as_secs_f64()).sum::<f64>(),
   "rebuild_seconds":report.rebuild_time_per_ruleset.values().map(|t|t.as_secs_f64()).sum::<f64>()},
  "sorted_commit_cpu":sorted_stats,"same_commit_verification":same_commit_verification.as_ref().ok(),"same_commit_verification_error":same_commit_verification.as_ref().err().map(ToString::to_string),
  "gpu":stats,"native_clone_seconds_excluded":native_clone,"gpu_clone_seconds_excluded":gpu_clone,
  "projection_without_smaller":projected.as_ref().ok(),"projection_error":projected.as_ref().err().map(ToString::to_string),"native_graph":graph_sizes(&native)?,"gpu_graph":graph_sizes(runtime.engine())?,"verification":verification.as_ref().ok(),"quality":costs.as_ref().ok(),"quality_error":costs.as_ref().err().map(ToString::to_string),
        "native_continuation_unchanged":closure.is_ok(),"continuation_error":closure.as_ref().err().map(ToString::to_string),"continuation_seconds_excluded":closure_seconds,"verification_error":verification.as_ref().err().map(ToString::to_string),
  "extraction":"not inside this packet; exact final fact/equality comparison is the correctness gate"});
    std::fs::write(dest, serde_json::to_vec_pretty(&output)?)?;
    Ok(())
}

/// Four executions from the identical checkpoint. Plan-only isolates query
/// ordering from actual reuse; work counters are a separate diagnostic run.
pub fn benchmark_cells(
    base: &EGraph,
    commands: &[Command],
    label: &str,
    worker: &Path,
    analysis: &Path,
    checked: bool,
    profile: bool,
    dest: &Path,
) -> Result<()> {
    use cell_assisted_egglog::table_live::Options;
    let mut cpu = Runtime::new_cpu(base.clone(), commands)?;
    cpu.run(commands)?;
    let mut variants = vec![];
    for (name, options) in [
        (
            "full_gpu",
            Options {
                profile,
                ..Options::default()
            },
        ),
        (
            "cell_order_only",
            Options {
                analysis: Some(analysis),
                plan_only: true,
                profile,
            },
        ),
        (
            "cell_certificates",
            Options {
                analysis: Some(analysis),
                plan_only: false,
                profile,
            },
        ),
    ] {
        let mut run = Runtime::configured(base.clone(), commands, worker, checked, options)?;
        run.run(commands)?;
        let verification = equal(base, cpu.engine(), run.engine())?;
        variants.push(json!({"name":name,"report":run.report(),"verification":verification}));
    }
    std::fs::write(
        dest,
        serde_json::to_vec_pretty(&json!({"label":label,"complete":true,
        "analysis":analysis,"audit":checked,"profile":profile,"cpu":cpu.report(),"variants":variants,
        "scope":"Actual native schedule and RHS, same sorted commit; analyzed positive projections plus exhaustive current singleton checks. No pristine or full-eggcc speedup claim. All unknown/ambiguous instances retain full queries."}))?,
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equal_modulo_allocation_order_and_detects_value_or_partition_changes() {
        let mut base = EGraph::default();
        base.parse_and_run_program(
            None,
            "(datatype E (N i64) (Pair E E)) (function Cost (E) i64 :merge (min old new)) (N 0)",
        )
        .unwrap();
        let mut a = base.clone();
        let mut b = base.clone();
        a.parse_and_run_program(None, "(N 1) (N 2) (Pair (N 1) (N 2)) (set (Cost (N 1)) 3)")
            .unwrap();
        b.parse_and_run_program(None, "(N 2) (N 1) (Pair (N 1) (N 2)) (set (Cost (N 1)) 3)")
            .unwrap();
        equal(&base, &a, &b).unwrap();
        let mut bad = b.clone();
        bad.parse_and_run_program(None, "(union (N 1) (N 2))")
            .unwrap();
        assert!(equal(&base, &a, &bad).is_err());
        b.parse_and_run_program(None, "(set (Cost (N 1)) 2)")
            .unwrap();
        assert!(equal(&base, &a, &b).is_err());
    }
}
