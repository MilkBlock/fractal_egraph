//! Full schedule benchmark. Exact graph equality is relative to the shared
//! checkpoint, so newly allocated eclass IDs need not coincide across engines.
use cell_assisted_egglog::table_live::Runtime;
use egglog::{ArcSort, CommandOutput, EGraph, Value, ast::Command};
use serde_json::{Value as Json, json};
use serde::{Serialize,Deserialize};
use egglog_numeric_id::NumericId;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
enum Atom {
    Eq(String, u32),
    Base(String, String),
}
type Row = (String, Vec<Atom>, Atom, bool);
fn atom(eg: &EGraph, v: Value, s: &ArcSort) -> Result<Atom> {
    if s.is_eq_sort() {
        return Ok(Atom::Eq(s.name().into(), eg.get_canonical_value(v, s).rep()));
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
pub fn equal(base: &EGraph, a: &EGraph, b: &EGraph) -> Result<Json> {
    equal_projected(base, a, b, false)
}
fn equal_projected(base: &EGraph, a: &EGraph, b: &EGraph, omit_smaller: bool) -> Result<Json> {
    equal_except(base, a, b, if omit_smaller { &["Smaller"] } else { &[] })
}
/// Diagnostic projection only. The strict benchmark gate always uses `equal`.
pub fn equal_except(base: &EGraph, a: &EGraph, b: &EGraph, omitted: &[&str]) -> Result<Json> {
    let mut map = BTreeMap::new();
    let mut inverse = BTreeMap::new();
    // Shared old IDs anchor cycles and pre-existing open inputs. Never assume IDs
    // of terms created by differently ordered RHS applications are the same.
    for (_, args, result, _) in snapshot(base)? {
        for v in args.into_iter().chain([result]) {
            if let Atom::Eq(sort, value) = v {
                let s = a.get_sort_by_name(&sort).ok_or("missing equality sort")?;
                pair(
                    atom(a, Value::new_const(value), &s)?,
                    atom(b, Value::new_const(value), &s)?,
                    &mut map,
                    &mut inverse,
                )?;
            }
        }
    }
    let left = snapshot(a)?
        .into_iter()
        .filter(|r| !omitted.contains(&r.0.as_str()))
        .collect::<Vec<_>>();
    let right = snapshot(b)?
        .into_iter()
        .filter(|r| !omitted.contains(&r.0.as_str()))
        .collect::<Vec<_>>();
    equal_rows(left,right,map,inverse)
}
fn equal_rows(left:Vec<Row>,right:Vec<Row>,mut map:BTreeMap<Atom,Atom>,mut inverse:BTreeMap<Atom,Atom>)->Result<Json>{
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
                    pair(result, target.clone(), &mut map, &mut inverse)
                        .map_err(|e| format!("result mapping conflict in {op} {args:?}: {e}"))?
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
#[derive(Serialize,Deserialize)]
struct ExactControl { initial:Vec<Row>, anchors:Vec<(Atom,Atom)>, rows:Vec<Row> }
fn save_control(path:&Path,base:&EGraph,eg:&EGraph)->Result<()> {
    let mut initial=snapshot(base)?;initial.sort();
    let mut anchors=BTreeMap::new();
    for (_,args,result,_) in &initial {for v in args.iter().chain([result]) {if let Atom::Eq(sort,id)=v {
        anchors.insert(v.clone(),atom(eg,Value::new_const(*id),eg.get_sort_by_name(sort).ok_or("missing control sort")?)?);
    }}}
    let packet=ExactControl{initial,anchors:anchors.into_iter().collect(),rows:snapshot(eg)?};
    let mut writer=std::io::BufWriter::new(std::fs::File::create(path)?);
    serde_json::to_writer(&mut writer,&packet)?;std::io::Write::flush(&mut writer)?;Ok(())
}
fn equal_control(path:&Path,base:&EGraph,eg:&EGraph)->Result<Json>{
    let saved:ExactControl=serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path)?))?;
    let mut initial=snapshot(base)?;initial.sort();
    if initial!=saved.initial{return Err("cached control has a different frozen input".into())}
    let (mut map,mut inverse)=(BTreeMap::new(),BTreeMap::new());
    for (old,then) in saved.anchors {
        let Atom::Eq(sort,id)=old else{return Err("invalid control anchor".into())};
        let now=atom(eg,Value::new_const(id),eg.get_sort_by_name(&sort).ok_or("missing control sort")?)?;
        pair(then,now,&mut map,&mut inverse)?;
    }
    equal_rows(saved.rows,snapshot(eg)?,map,inverse)
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

pub fn extraction_costs(base: &EGraph, a: &EGraph, b: &EGraph) -> Result<Json> {
    let mut anchors = BTreeSet::new();
    for (_, args, result, _) in snapshot(base)? {
        for atom in args.into_iter().chain([result]) {
            if let Atom::Eq(sort, v) = atom {
                if sort == "Expr" {
                    anchors.insert(Value::new_const(v));
                }
            }
        }
    }
    fn values(
        eg: &EGraph,
        anchors: &BTreeSet<Value>,
    ) -> Result<BTreeMap<Value, Option<BTreeSet<i64>>>> {
        let pair = eg
            .get_function("TCPair")
            .ok_or("no TCPair quality metric")?;
        let extracted = eg
            .get_function("ExtractedExpr")
            .ok_or("no ExtractedExpr quality metric")?;
        if extracted.schema().input.len() != 1 || extracted.schema().input[0].name() != "Expr" {
            return Err("unexpected extraction key schema".into());
        }
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
        for anchor in anchors {
            let value = eg.cell_visible_row("ExtractedExpr", &[*anchor]).map(|row| {
                let root = eg.get_canonical_value(*row.last().unwrap(), &extracted.schema().output);
                costs.get(&root).cloned().unwrap_or_default()
            });
            out.insert(*anchor, value);
        }
        Ok(out)
    }
    let left = values(a, &anchors)?;
    let right = values(b, &anchors)?;
    let different = left
        .keys()
        .chain(right.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|k| left.get(*k) != right.get(*k))
        .count();
    Ok(
        json!({"same_extracted_costs":left==right,"input_expr_anchors":anchors.len(),"different_anchors":different,
        "native_unresolved":left.values().filter(|x|x.as_ref().is_none_or(|v|v.is_empty())).count(),"gpu_unresolved":right.values().filter(|x|x.as_ref().is_none_or(|v|v.is_empty())).count(),
        "new_expr_keys_compared":false,
        "scope":"Each initial Expr anchor is canonicalized independently in each final graph before looking up ExtractedExpr/TCPair. Newly allocated IDs are never compared directly. Equal cost is not identical selected Term syntax or a new Tiger extraction."}),
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
    benchmark_impl(
        base, commands, label, worker, checked, dest, None, true, None, None, false,
    )
}
pub fn benchmark_hybrid(
    base: &EGraph,
    commands: &[Command],
    label: &str,
    worker: &Path,
    checked: bool,
    dest: &Path,
    selected: &[String],
) -> Result<()> {
    benchmark_impl(
        base,
        commands,
        label,
        worker,
        checked,
        dest,
        Some(selected),
        false,
        None,
        None,
        false,
    )
}
pub fn benchmark_hybrid_export(
    base: &EGraph,
    commands: &[Command],
    label: &str,
    worker: &Path,
    dest: &Path,
    selected: &[String],
    exports: &Path,
) -> Result<()> {
    benchmark_impl(
        base,
        commands,
        label,
        worker,
        false,
        dest,
        Some(selected),
        false,
        Some(exports),
        None,
        false,
    )
}
/// The extra GPU-without-hook lane isolates Cell proof/maintenance from the
/// existing GPU join backend. Each lane runs once, sequentially.
pub fn benchmark_hybrid_export_joint(
    base: &EGraph,
    commands: &[Command],
    label: &str,
    worker: &Path,
    dest: &Path,
    selected: &[String],
    exports: &Path,
    hook: Box<dyn cell_assisted_egglog::table_live::SaturationHook>,
) -> Result<()> {
    benchmark_impl(
        base,
        commands,
        label,
        worker,
        false,
        dest,
        Some(selected),
        false,
        Some(exports),
        Some(hook),
        false,
    )
}
pub fn benchmark_all_export(base:&EGraph,commands:&[Command],worker:&Path,dest:&Path,selected:&[String],exports:&Path,checked:bool,hook:Option<Box<dyn cell_assisted_egglog::table_live::SaturationHook>>)->Result<()>{
    if let Some(path)=std::env::var_os("CELL_BENCH_REUSE_CONTROLS") {
        return benchmark_reusing_controls(base,commands,worker,dest,selected,exports,checked,hook,Path::new(&path));
    }
    benchmark_impl(base,commands,"complete Pass 3, all original rules on GPU",worker,checked,dest,Some(selected),false,Some(exports),hook,true)
}
fn benchmark_reusing_controls(base:&EGraph,commands:&[Command],worker:&Path,dest:&Path,selected:&[String],exports:&Path,checked:bool,hook:Option<Box<dyn cell_assisted_egglog::table_live::SaturationHook>>,directory:&Path)->Result<()>{
    let directory=directory.canonicalize()?;
    let mut report:Json=serde_json::from_reader(std::fs::File::open(directory.join("report.json"))?)?;
    if report["schedule"]!=json!(commands.iter().map(ToString::to_string).collect::<Vec<_>>()) || report["assisted_rulesets"]!=json!(selected){return Err("cached control schedule/partition differs".into())}
    if !report["same_commit_verification"].is_object(){return Err("cached CPU/GPU control was not validated".into())}
    let joint=hook.is_some();
    if report["gpu_without_joint"].is_object()!=joint{return Err("cached control Cell mode differs".into())}
    for name in ["native","cpu"].into_iter().chain(joint.then_some("gpu-off")) {if !directory.join(format!("graphs/{name}.control.json")).is_file(){return Err("cached control lacks exact facts/anchors; generate one complete reference first".into())}}
    let mut runtime=Runtime::new_full(base.clone(),commands,Some(worker),checked,Some(selected))?;
    if let Some(hook)=hook {runtime.set_saturation_hook(hook);}
    runtime.run(commands)?;
    for (name,field) in [("native","verification"),("cpu","same_commit_verification")].into_iter().chain(joint.then_some(("gpu-off","joint_verification"))) {
        let proof=equal_control(&directory.join(format!("graphs/{name}.control.json")),base,runtime.engine());
        report[field]=proof.as_ref().map_or(Json::Null,|v|v.clone());
        let error=if field=="verification"{"verification_error".to_string()}else{format!("{field}_error")};
        report[error]=proof.err().map_or(Json::Null,|e|json!(e.to_string()));
    }
    report["complete"]=json!(report["verification"].is_object());report["benchmark_finished"]=json!(true);
    report["gpu"]=runtime.report();report["gpu_graph"]=graph_sizes(runtime.engine())?;
    report["reused_controls"]=json!({"directory":directory,"lanes":if joint{vec!["native","cpu","gpu-off"]}else{vec!["native","cpu"]},"times":"historical unchanged controls; candidate and all exact graph checks are fresh"});
    for k in ["quality","quality_error","same_commit_quality","same_commit_quality_error","projection_without_smaller","projection_error","native_continuation_unchanged","continuation_error","continuation_seconds_excluded","gpu_clone_seconds_excluded"]{report.as_object_mut().unwrap().remove(k);}
    std::fs::write(dest,serde_json::to_vec_pretty(&report)?)?;
    std::fs::create_dir_all(exports)?;
    for name in ["native","cpu"] {
        #[cfg(unix)] std::os::unix::fs::symlink(directory.join(format!("graphs/{name}.json")),exports.join(format!("{name}.json")))?;
        #[cfg(not(unix))] std::fs::copy(directory.join(format!("graphs/{name}.json")),exports.join(format!("{name}.json")))?;
    }
    let graph=runtime.engine().serialize(egglog::SerializeConfig::default());if !graph.is_complete(){return Err("truncated candidate graph".into())}
    let mut writer=std::io::BufWriter::new(std::fs::File::create(exports.join("gpu.json"))?);
    serde_json::to_writer_pretty(&mut writer,&graph.egraph)?;std::io::Write::flush(&mut writer)?;
    Ok(())
}
fn benchmark_impl(
    base: &EGraph,
    commands: &[Command],
    label: &str,
    worker: &Path,
    checked: bool,
    dest: &Path,
    selected: Option<&[String]>,
    fixed_point: bool,
    exports: Option<&Path>,
    hook: Option<Box<dyn cell_assisted_egglog::table_live::SaturationHook>>,
    full_queries: bool,
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
    let mut ordered = if full_queries {
        Runtime::new_full(base.clone(),commands,None,false,selected)?
    } else if let Some(sets) = selected {
        Runtime::new_hybrid(base.clone(), commands, None, false, sets)?
    } else {
        Runtime::new_cpu(base.clone(), commands)?
    };
    ordered.run(commands)?;
    let sorted_stats = ordered.report();
    let gpu_without_joint = if hook.is_some() {
        let mut r = if full_queries {
            Runtime::new_full(base.clone(),commands,Some(worker),false,selected)?
        } else {Runtime::new_hybrid(
            base.clone(),
            commands,
            Some(worker),
            false,
            selected.ok_or("joint comparison requires an explicit partition")?,
        )?};
        r.run(commands)?;
        Some((r.report(), r.into_engine()))
    } else {
        None
    };
    let t = Instant::now();
    let copy = base.clone();
    let gpu_clone = t.elapsed().as_secs_f64();
    let mut runtime = if full_queries {
        Runtime::new_full(copy,commands,Some(worker),checked,selected)?
    } else if let Some(sets) = selected {
        Runtime::new_hybrid(copy, commands, Some(worker), checked, sets)?
    } else {
        Runtime::new(copy, commands, worker, checked)?
    };
    if let Some(hook) = hook {
        runtime.set_saturation_hook(hook);
    }
    runtime.run(commands)?;
    egglog::set_probe_counts_enabled(old);
    let stats = runtime.report();
    let verification = equal(base, &native, runtime.engine());
    let same_commit_verification = equal(base, ordered.engine(), runtime.engine());
    let joint_verification = gpu_without_joint
        .as_ref()
        .map(|(_, eg)| equal(base, eg, runtime.engine()));
    let gpu_without_joint_report = gpu_without_joint.as_ref().map(|(report, _)| report);
    let costs = extraction_costs(base, &native, runtime.engine());
    let same_commit_costs = extraction_costs(base, ordered.engine(), runtime.engine());
    let projected = equal_projected(base, &native, runtime.engine(), true);
    let (closure_seconds, closure, continuation_error) = if fixed_point {
        let mut continued = runtime.engine().clone();
        let t = Instant::now();
        continued.run_program(commands.to_vec())?;
        let seconds = t.elapsed().as_secs_f64();
        let r = equal(runtime.engine(), runtime.engine(), &continued);
        (seconds, Some(r.is_ok()), r.err().map(|e| e.to_string()))
    } else {
        (0.0, None, None)
    };

    let output = json!({"complete":verification.is_ok(),"benchmark_finished":true,"label":label,"assisted_rulesets":selected,"schedule":commands.iter().map(ToString::to_string).collect::<Vec<_>>(),
  "scope":"Full original schedule, same native fork and selection policy, not pristine. Optional joint hook is measured against a separate GPU-without-hook lane; explicit original tables, native commit/rebuild. Preparation and verification excluded from inference timings.",
  "native":{"run_seconds":native_seconds,"iterations":report.iterations.len(),"logical_matches":report.num_matches_per_rule.values().sum::<usize>(),
   "search_apply_seconds":report.search_and_apply_time_per_ruleset.values().map(|t|t.as_secs_f64()).sum::<f64>(),
   "insert_merge_seconds":report.merge_time_per_ruleset.values().map(|t|t.as_secs_f64()).sum::<f64>(),
   "rebuild_seconds":report.rebuild_time_per_ruleset.values().map(|t|t.as_secs_f64()).sum::<f64>(),
   "search_apply_per_ruleset":report.search_and_apply_time_per_ruleset.iter().map(|(s,t)|(s.to_string(),t.as_secs_f64())).collect::<BTreeMap<_,_>>()},
  "native_per_rule":report.search_and_apply_time_per_rule.iter().map(|(s,t)|json!({"rule":s,"search_apply_seconds":t.as_secs_f64(),"matches":report.num_matches_per_rule.get(s).copied().unwrap_or(0)})).collect::<Vec<_>>(),
  "sorted_commit_cpu":sorted_stats,"same_commit_verification":same_commit_verification.as_ref().ok(),"same_commit_verification_error":same_commit_verification.as_ref().err().map(ToString::to_string),
  "gpu_without_joint":gpu_without_joint_report,"joint_verification":joint_verification.as_ref().and_then(|r|r.as_ref().ok()),"joint_verification_error":joint_verification.as_ref().and_then(|r|r.as_ref().err()).map(ToString::to_string),
  "gpu":stats,"native_clone_seconds_excluded":native_clone,"gpu_clone_seconds_excluded":gpu_clone,
  "projection_without_smaller":projected.as_ref().ok(),"projection_error":projected.as_ref().err().map(ToString::to_string),"native_graph":graph_sizes(&native)?,"gpu_graph":graph_sizes(runtime.engine())?,"verification":verification.as_ref().ok(),"quality":costs.as_ref().ok(),"quality_error":costs.as_ref().err().map(ToString::to_string),"same_commit_quality":same_commit_costs.as_ref().ok(),"same_commit_quality_error":same_commit_costs.as_ref().err().map(ToString::to_string),
        "native_continuation_unchanged":closure,"continuation_error":continuation_error,"continuation_seconds_excluded":closure_seconds,"verification_error":verification.as_ref().err().map(ToString::to_string),
  "extraction":"not inside this packet; exact final fact/equality comparison is the correctness gate"});
    std::fs::write(dest, serde_json::to_vec_pretty(&output)?)?;
    if let Some(directory) = exports {
        std::fs::create_dir_all(directory)?;
        if full_queries {
            for (name,e) in [("native",&native),("cpu",ordered.engine())] {save_control(&directory.join(format!("{name}.control.json")),base,e)?;}
            if let Some((_,e))=&gpu_without_joint {save_control(&directory.join("gpu-off.control.json"),base,e)?;}
        }
        for (name, engine) in [
            ("native", &native),
            ("cpu", ordered.engine()),
            ("gpu", runtime.engine()),
        ] {
            let start = Instant::now();
            let snapshot = engine.serialize(egglog::SerializeConfig::default());
            if !snapshot.is_complete() {
                return Err("truncated extraction input".into());
            }
            let mut file = std::io::BufWriter::new(std::fs::File::create(
                directory.join(format!("{name}.json")),
            )?);
            // Production Tiger's reader requires whitespace-delimited JSON.
            serde_json::to_writer_pretty(&mut file, &snapshot.egraph)?;
            std::io::Write::flush(&mut file)?;
            eprintln!("[export] {name} {:.3} s", start.elapsed().as_secs_f64());
        }
    }
    Ok(())
}

/// Five executions from the identical checkpoint. Plan-only isolates query
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
                cache_only: false,
                profile,
                port_locality: Default::default(),
                query_routing: Default::default(),
            },
        ),
        (
            "cell_cached",
            Options {
                analysis: Some(analysis),
                plan_only: false,
                cache_only: true,
                profile,
                port_locality: Default::default(),
                query_routing: Default::default(),
            },
        ),
        (
            "cell_certificates",
            Options {
                analysis: Some(analysis),
                plan_only: false,
                cache_only: false,
                profile,
                port_locality: Default::default(),
                query_routing: Default::default(),
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
        "scope":"Actual native schedule and RHS, same sorted commit; analyzed positive fully keyed projections, cached and commit-driven controls. No pristine or full-eggcc speedup claim. All unknown/ambiguous instances retain full queries."}))?,
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn order_sensitive_function_choice_is_not_a_changed_union_partition() {
        let mut base = EGraph::default();
        base.parse_and_run_program(
            None,
            "(datatype E (N i64)) (function Pick () E :merge new) (N 1) (N 2)",
        )
        .unwrap();
        let mut a = base.clone();
        let mut b = base.clone();
        a.parse_and_run_program(None, "(set (Pick) (N 1)) (set (Pick) (N 2))")
            .unwrap();
        b.parse_and_run_program(None, "(set (Pick) (N 2)) (set (Pick) (N 1))")
            .unwrap();
        let error = equal(&base, &a, &b).unwrap_err().to_string();
        assert!(error.contains("result mapping conflict in Pick"), "{error}");
        assert!(equal_except(&base, &a, &b, &["Pick"]).is_ok());
    }
    #[test]
    fn extraction_quality_uses_input_anchors_not_fresh_numeric_ids() {
        let mut base = EGraph::default();
        base.parse_and_run_program(None,"(datatype Expr (E i64)) (datatype Term (T i64)) (datatype Pair (TCPair Term i64)) (function ExtractedExpr (Expr) Pair :merge old) (E 1)").unwrap();
        let mut a = base.clone();
        let mut b = base.clone();
        a.parse_and_run_program(None,"(E 2) (E 3) (set (ExtractedExpr (E 1)) (TCPair (T 0) 7)) (set (ExtractedExpr (E 2)) (TCPair (T 2) 40))").unwrap();
        b.parse_and_run_program(None,"(E 3) (E 2) (set (ExtractedExpr (E 1)) (TCPair (T 0) 7)) (set (ExtractedExpr (E 2)) (TCPair (T 2) 400))").unwrap();
        let q = extraction_costs(&base, &a, &b).unwrap();
        assert_eq!(q["input_expr_anchors"], 1);
        assert_eq!(q["same_extracted_costs"], true);
        let mut wrong = base.clone();
        wrong
            .parse_and_run_program(None, "(set (ExtractedExpr (E 1)) (TCPair (T 0) 8))")
            .unwrap();
        assert_eq!(
            extraction_costs(&base, &a, &wrong).unwrap()["same_extracted_costs"],
            false
        );
        b.parse_and_run_program(None, "(union (E 1) (E 3))")
            .unwrap();
        assert_eq!(
            extraction_costs(&base, &a, &b).unwrap()["same_extracted_costs"],
            true
        );
    }
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
        let cache=std::env::temp_dir().join(format!("cell-control-{}-{}.json",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        save_control(&cache,&base,&a).unwrap();
        equal_control(&cache,&base,&b).unwrap();
        let mut other=base.clone();other.parse_and_run_program(None,"(N 9)").unwrap();
        assert!(equal_control(&cache,&other,&b).is_err());
        let mut bad = b.clone();
        bad.parse_and_run_program(None, "(union (N 1) (N 2))")
            .unwrap();
        assert!(equal(&base, &a, &bad).is_err());
        assert!(equal_control(&cache,&base,&bad).is_err());
        b.parse_and_run_program(None, "(set (Cost (N 1)) 2)")
            .unwrap();
        assert!(equal(&base, &a, &b).is_err());
        assert!(equal_control(&cache,&base,&b).is_err());
        std::fs::remove_file(cache).unwrap();
    }
    #[test]
    fn cached_controls_recheck_a_fresh_gpu_without_running_old_lanes() {
        let Some(worker)=std::env::var_os("CELL_MATRIX_WORKER") else{return};
        let mut base=EGraph::default();
        base.parse_and_run_program(None,"(datatype E (N i64) (P E E)) (ruleset s) (rewrite (P a b) (P b a) :ruleset s) (P (N 0) (N 1))").unwrap();
        let commands=base.parse_program(None,"(run s 2)").unwrap();
        let path=std::env::temp_dir().join(format!("cell-control-run-{}",std::process::id()));
        std::fs::create_dir(&path).unwrap();let first=path.join("first");let second=path.join("second");
        std::fs::create_dir(&first).unwrap();std::fs::create_dir(&second).unwrap();
        let selected=vec!["s".to_string()];
        benchmark_impl(&base,&commands,"cache correctness fixture",Path::new(&worker),false,&first.join("report.json"),Some(&selected),false,Some(&first.join("graphs")),None,true).unwrap();
        benchmark_reusing_controls(&base,&commands,Path::new(&worker),&second.join("report.json"),&selected,&second.join("graphs"),false,None,&first).unwrap();
        let a:Json=serde_json::from_reader(std::fs::File::open(first.join("report.json")).unwrap()).unwrap();
        let b:Json=serde_json::from_reader(std::fs::File::open(second.join("report.json")).unwrap()).unwrap();
        assert_eq!(a["native"],b["native"]);assert_eq!(a["sorted_commit_cpu"],b["sorted_commit_cpu"]);
        assert!(b["same_commit_verification"].is_object());assert!(b["verification"].is_object());assert!(b["reused_controls"].is_object());
        std::fs::remove_dir_all(path).unwrap();
    }

}
