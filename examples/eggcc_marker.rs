//! Source-only, exact materialized views selected from observed RipSat motifs.
use egg_layout::{
    pipeline::Result,
    saturated_rule_composition::{Comparison, compare},
    visual_rule,
};
use egglog::{
    EGraph,
    ast::{Action, Actions, Command, Expr, Fact, GenericRunConfig, GenericSchedule, Rule, Span},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Instant,
};
#[path = "../experiments/eggcc_cell_probe/snapshot.rs"]
mod native_snapshot;
fn sp() -> Span {
    Span::Panic
}
fn var(v: impl Into<String>) -> Expr {
    Expr::Var(sp(), v.into())
}
fn call(op: impl Into<String>, args: Vec<Expr>) -> Expr {
    Expr::Call(sp(), op.into(), args)
}
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Motif {
    Nested {
        outer: String,
        slot: usize,
        inner: String,
    },
    Shared {
        a: String,
        ai: usize,
        b: String,
        bi: usize,
    },
}
#[derive(Clone)]
struct Schema {
    args: Vec<String>,
    out: String,
}
fn runs(s: &egglog::ast::Schedule, out: &mut BTreeSet<String>) {
    match s {
        GenericSchedule::Run(_, r) => {
            out.insert(r.ruleset.clone());
        }
        GenericSchedule::Repeat(_, _, s) | GenericSchedule::Saturate(_, s) => runs(s, out),
        GenericSchedule::Sequence(_, ss) => {
            for s in ss {
                runs(s, out)
            }
        }
    }
}
fn validate_scope(program: &[Command]) -> Result {
    fn schedule(s: &egglog::ast::Schedule) -> Result {
        match s {
            GenericSchedule::Run(_, c) if c.until.is_some() => {
                Err("until schedules need a separate preservation check".into())
            }
            GenericSchedule::Repeat(_, _, s) | GenericSchedule::Saturate(_, s) => schedule(s),
            GenericSchedule::Sequence(_, ss) => {
                for s in ss {
                    schedule(s)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    let mut executed = false;
    for c in program {
        match c {
            Command::RunSchedule(s) => {
                schedule(s)?;
                executed = true;
            }
            Command::Include(..)
            | Command::Push(..)
            | Command::Pop(..)
            | Command::Rewrite(..)
            | Command::BiRewrite(..) => {
                return Err(
                    "use a flattened explicit-rule eggcc export without include/push/pop".into(),
                );
            }
            Command::Rule { .. }
            | Command::Constructor { .. }
            | Command::Function { .. }
            | Command::Relation { .. }
            | Command::Sort { .. }
                if executed =>
            {
                return Err(
                    "late schema/rule installation is outside this prototype's schedule contract"
                        .into(),
                );
            }
            _ => {}
        }
    }
    Ok(())
}
fn schemas(program: &[Command]) -> BTreeMap<String, Schema> {
    program
        .iter()
        .filter_map(|c| match c {
            Command::Constructor { name, schema, .. } => Some((
                name.clone(),
                Schema {
                    args: schema.input.clone(),
                    out: schema.output.clone(),
                },
            )),
            _ => None,
        })
        .collect()
}
fn motif_variables(
    m: &Motif,
    schemas: &BTreeMap<String, Schema>,
    prefix: &str,
) -> (Vec<String>, Vec<String>, Vec<Fact>) {
    let mut fields = vec![];
    let mut types = vec![];
    let mut next = |ty: &String| {
        let name = format!("{prefix}_v{}", fields.len());
        fields.push(name.clone());
        types.push(ty.clone());
        var(name)
    };
    let body = match m {
        Motif::Nested { outer, slot, inner } => {
            let mut outer_args = vec![];
            for (i, ty) in schemas[outer].args.iter().enumerate() {
                if i == *slot {
                    let args = schemas[inner].args.iter().map(&mut next).collect();
                    outer_args.push(call(inner, args));
                } else {
                    outer_args.push(next(ty));
                }
            }
            let root = next(&schemas[outer].out);
            vec![Fact::Eq(sp(), root, call(outer, outer_args))]
        }
        Motif::Shared { a, ai, b, bi } => {
            let aa: Vec<_> = schemas[a].args.iter().map(&mut next).collect();
            let bb = schemas[b]
                .args
                .iter()
                .enumerate()
                .map(|(i, ty)| if i == *bi { aa[*ai].clone() } else { next(ty) })
                .collect();
            vec![Fact::Fact(call(a, aa)), Fact::Fact(call(b, bb))]
        }
    };
    (fields, types, body)
}
fn expr_rewrite(
    e: &mut Expr,
    m: &Motif,
    name: &str,
    prefix: &str,
    added: &mut Vec<Fact>,
    counter: &mut usize,
) {
    let Expr::Call(_, op, args) = e else {
        return;
    };
    for a in args.iter_mut() {
        expr_rewrite(a, m, name, prefix, added, counter);
    }
    if let Motif::Nested { outer, slot, inner } = m {
        if op != outer {
            return;
        }
        let Some(Expr::Call(_, inner_op, inner_args)) = args.get(*slot) else {
            return;
        };
        if inner_op != inner {
            return;
        }
        let mut fields = vec![];
        for (i, arg) in args.iter().enumerate() {
            if i == *slot {
                fields.extend(inner_args.clone());
            } else {
                fields.push(arg.clone());
            }
        }
        let root = var(format!("{prefix}_{}", *counter));
        *counter += 1;
        fields.push(root.clone());
        added.push(Fact::Fact(call(name, fields)));
        *e = root;
    }
}
fn rewrite(rule: &mut Rule, m: &Motif, name: &str, prefix: &str, projected: bool) -> usize {
    let mut n = 0;
    match m {
        Motif::Nested { outer, slot, inner } if projected => {
            for f in &mut rule.body {
                if let Fact::Fact(Expr::Call(_, op, args)) = f {
                    if op == outer {
                        if let Some(Expr::Call(_, inside, inner_args)) = args.get(*slot) {
                            if inside == inner {
                                let mut fields = vec![];
                                for (i, arg) in args.iter().enumerate() {
                                    if i == *slot {
                                        fields.extend(inner_args.clone());
                                    } else {
                                        fields.push(arg.clone());
                                    }
                                }
                                *f = Fact::Fact(call(name, fields));
                                n += 1;
                            }
                        }
                    }
                }
            }
        }
        Motif::Nested { .. } => {
            let mut added = vec![];
            for f in &mut rule.body {
                match f {
                    Fact::Eq(_, a, b) => {
                        expr_rewrite(a, m, name, prefix, &mut added, &mut n);
                        expr_rewrite(b, m, name, prefix, &mut added, &mut n);
                    }
                    Fact::Fact(e) => expr_rewrite(e, m, name, prefix, &mut added, &mut n),
                }
            }
            rule.body.extend(added);
        }
        Motif::Shared { a, ai, b, bi } => loop {
            let mut found = None;
            'outer: for (i, f) in rule.body.iter().enumerate() {
                let Fact::Fact(Expr::Call(_, op, xs)) = f else {
                    continue;
                };
                if op != a {
                    continue;
                }
                for (j, g) in rule.body.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let Fact::Fact(Expr::Call(_, op, ys)) = g else {
                        continue;
                    };
                    if op == b
                        && matches!((&xs[*ai],&ys[*bi]),(Expr::Var(_,x),Expr::Var(_,y)) if x==y)
                    {
                        let mut fields = xs.clone();
                        fields.extend(
                            ys.iter()
                                .enumerate()
                                .filter(|(k, _)| *k != *bi)
                                .map(|(_, e)| e.clone()),
                        );
                        found = Some((i, j, fields));
                        break 'outer;
                    }
                }
            }
            let Some((i, j, fields)) = found else {
                break;
            };
            rule.body.remove(i.max(j));
            rule.body.remove(i.min(j));
            rule.body.push(Fact::Fact(call(name, fields)));
            n += 1;
        },
    };
    n
}
fn before_runs(
    s: egglog::ast::Schedule,
    consumers: &BTreeSet<String>,
    marker: &str,
    shadow_names: &BTreeMap<String, String>,
) -> egglog::ast::Schedule {
    use GenericSchedule::*;
    match s {
        Run(span, c) if consumers.contains(&c.ruleset) => Sequence(
            span.clone(),
            vec![
                Run(
                    sp(),
                    GenericRunConfig {
                        ruleset: marker.into(),
                        until: None,
                    },
                ),
                Run(
                    span,
                    GenericRunConfig {
                        ruleset: shadow_names.get(&c.ruleset).cloned().unwrap_or(c.ruleset),
                        until: c.until,
                    },
                ),
            ],
        ),
        Repeat(span, n, s) => Repeat(
            span,
            n,
            Box::new(before_runs(*s, consumers, marker, shadow_names)),
        ),
        Saturate(span, s) => Saturate(
            span,
            Box::new(before_runs(*s, consumers, marker, shadow_names)),
        ),
        Sequence(span, ss) => Sequence(
            span,
            ss.into_iter()
                .map(|s| before_runs(s, consumers, marker, shadow_names))
                .collect(),
        ),
        s => s,
    }
}
fn transform(
    program: &[Command],
    m: &Motif,
    schemas: &BTreeMap<String, Schema>,
    active: &BTreeSet<String>,
    shadow: bool,
) -> (Vec<Command>, Json) {
    let text = program
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut name = "__RipView".to_string();
    while text.contains(&name) {
        name.push('_');
    }
    let ruleset = format!("{name}Build");
    let prefix = format!("{name}Binding");
    fn matches(e: &Expr, m: &Motif) -> bool {
        matches!((e,m),(Expr::Call(_,op,args),Motif::Nested{outer,slot,inner}) if op==outer && matches!(args.get(*slot),Some(Expr::Call(_,i,_)) if i==inner))
    }
    fn count(e: &Expr, m: &Motif) -> usize {
        usize::from(matches(e, m))
            + match e {
                Expr::Call(_, _, args) => args.iter().map(|a| count(a, m)).sum(),
                _ => 0,
            }
    }
    let (mut total, mut roots) = (0, 0);
    for c in program {
        if let Command::Rule { rule } = c {
            if active.contains(&rule.ruleset) {
                for f in &rule.body {
                    match f {
                        Fact::Fact(e) => {
                            total += count(e, m);
                            roots += usize::from(matches(e, m));
                        }
                        Fact::Eq(_, a, b) => {
                            total += count(a, m) + count(b, m);
                        }
                    }
                }
            }
        }
    }
    let projected = matches!(m, Motif::Nested { .. }) && total > 0 && total == roots;
    let (mut fields, mut types, body) = motif_variables(m, schemas, &name);
    if projected {
        fields.pop();
        types.pop();
    }

    let builder = Rule {
        span: sp(),
        body,
        head: Actions::new(vec![Action::Expr(
            sp(),
            call(&name, fields.into_iter().map(var).collect()),
        )]),
        ruleset: ruleset.clone(),
        name: format!("{name}:materialize"),
        naive: false,
    };
    let mut parser = EGraph::default();
    let declarations = parser
        .parse_program(
            None,
            &format!(
                "(relation {name} ({})) (ruleset {ruleset})",
                types.join(" ")
            ),
        )
        .unwrap();
    let mut result = vec![];
    let mut consumers = BTreeSet::new();
    let mut rewrites = vec![];
    let mut installed = false;
    for c in program {
        let mut c = c.clone();
        if let Command::Rule { rule } = &mut c {
            if active.contains(&rule.ruleset) {
                let n = rewrite(rule, m, &name, &prefix, projected);
                if n > 0 {
                    // Both referenced schemas exist by the first consumer's
                    // original declaration point; later sorts may not exist at
                    // the program's first rule.
                    if !installed && !shadow {
                        result.extend(declarations.clone());
                        result.push(Command::Rule {
                            rule: builder.clone(),
                        });
                        installed = true;
                    }
                    consumers.insert(rule.ruleset.clone());
                    rewrites.push(json!({"name":rule.name,"ruleset":rule.ruleset,"occurrences":n}));
                }
            }
        }
        result.push(c);
    }
    let shadow_names: BTreeMap<String, String> = if shadow {
        consumers
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), format!("{name}_scope_{i}")))
            .collect()
    } else {
        BTreeMap::new()
    };
    if shadow && !consumers.is_empty() {
        let mut extra = declarations.clone();
        extra.push(Command::Rule { rule: builder });
        extra.extend(
            shadow_names
                .values()
                .map(|n| Command::AddRuleset(sp(), n.clone())),
        );
        for (i, c) in result.iter().enumerate() {
            if let Command::Rule { rule } = c {
                if let Some(target) = shadow_names.get(&rule.ruleset) {
                    let mut rule = rule.clone();
                    rule.ruleset = target.clone();
                    rule.name = format!("{name}:shadow:{i}:{}", rule.name);
                    extra.push(Command::Rule { rule });
                }
            }
        }
        result = vec![];
        let mut extra = Some(extra);
        for c in program {
            if matches!(c, Command::RunSchedule(..)) {
                if let Some(extra) = extra.take() {
                    result.extend(extra);
                }
            }
            result.push(c.clone());
        }
    }
    for c in &mut result {
        if let Command::RunSchedule(s) = c {
            *s = before_runs(s.clone(), &consumers, &ruleset, &shadow_names);
        }
    }
    (
        result,
        json!({"motif":m,"marker":name,"arity":types.len(),"projected_unused_result":projected,"rewrites":rewrites,"consumer_rulesets":consumers,"shadow_rulesets":shadow_names,"maintenance":"one native materialization run immediately before each consumer ruleset run; original RHS preserved"}),
    )
}
fn emit(path: &Path, commands: &[Command], meta: &Json) -> Result {
    let text = format!(
        "; Experimental RipSat-selected materialized view; full-run validation required.\n; {}\n{}\n",
        serde_json::to_string(meta)?,
        commands
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
    std::fs::write(path, text)?;
    Ok(())
}
fn execute(program: &[Command], profile: bool) -> Result<(EGraph, Json)> {
    let mut eg = EGraph::default();
    #[cfg(feature = "native-read-profile")]
    eg.reset_join_read_profile(profile);
    #[cfg(not(feature = "native-read-profile"))]
    if profile {
        return Err("build with --features native-read-profile".into());
    }
    let mut setup = 0.;
    let mut execution = 0.;
    let start = Instant::now();
    for c in program {
        let tick = Instant::now();
        eg.run_program(vec![c.clone()])?;
        if matches!(c, Command::RunSchedule(..)) {
            execution += tick.elapsed().as_secs_f64();
        } else {
            setup += tick.elapsed().as_secs_f64();
        }
    }
    #[allow(unused_mut)]
    let mut r = json!({"total_seconds":start.elapsed().as_secs_f64(),"setup_seconds":setup,"execution_seconds":execution});
    #[cfg(feature = "native-read-profile")]
    if profile {
        let tables:Vec<_>=eg.join_read_profile().into_iter().filter_map(|(name,p)|p.map(|p|json!({"table":name,"scan_rows":p.scan_rows,"refine_rows":p.refine_rows,"key_lookups":p.key_lookups,"scan_calls":p.scan_calls}))).collect();
        for key in ["scan_rows", "refine_rows", "key_lookups"] {
            r[key] = json!(tables.iter().map(|t| t[key].as_u64().unwrap()).sum::<u64>());
        }
        r["tables"] = json!(tables);
    }
    Ok((eg, r))
}
fn measure(args: &[String]) -> Result {
    let out = PathBuf::from(&args[2]);
    std::fs::create_dir(&out)?;
    let repeats: usize = args[3].parse()?;
    let mut parser = EGraph::default();
    let programs: Vec<_> = args[4..]
        .iter()
        .map(|f| -> Result<_> {
            Ok(visual_rule::surface_program(parser.parse_program(
                Some(f.clone()),
                &std::fs::read_to_string(f)?,
            )?))
        })
        .collect::<Result<_>>()?;
    let original_names: BTreeSet<_> = programs[0]
        .iter()
        .filter_map(|c| match c {
            Command::Constructor { name, .. }
            | Command::Function { name, .. }
            | Command::Relation { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let ignored: Vec<BTreeSet<String>> = programs
        .iter()
        .map(|p| {
            p.iter()
                .filter_map(|c| match c {
                    Command::Relation { name, .. } if !original_names.contains(name) => {
                        Some(name.clone())
                    }
                    _ => None,
                })
                .collect()
        })
        .collect();
    let (base, base_profile) = execute(&programs[0], true)?;
    let state =
        native_snapshot::snapshot_filtered(&base, &BTreeSet::new(), "measure", &ignored[0])?;
    let diagnostic_base = native_snapshot::snapshot_filtered(
        &base,
        &BTreeSet::new(),
        "diagnostic-only",
        &BTreeSet::from(["to-hoist".into()]),
    )?;
    let mut diagnostic_equal = vec![true];
    let mut profiles = vec![base_profile];
    let mut profile_equal = vec![true];
    let mut marker_rows = vec![0];
    for (i, p) in programs.iter().enumerate().skip(1) {
        let (eg, profile) = execute(p, true)?;
        profiles.push(profile);
        profile_equal.push(matches!(
            compare(
                &state,
                &native_snapshot::snapshot_filtered(&eg, &BTreeSet::new(), "measure", &ignored[i])?
            )?,
            Comparison::Equivalent { .. }
        ));
        let mut diagnostic_ignored = ignored[i].clone();
        diagnostic_ignored.insert("to-hoist".into());
        diagnostic_equal.push(matches!(
            compare(
                &diagnostic_base,
                &native_snapshot::snapshot_filtered(
                    &eg,
                    &BTreeSet::new(),
                    "diagnostic-only",
                    &diagnostic_ignored
                )?
            )?,
            Comparison::Equivalent { .. }
        ));
        marker_rows.push(ignored[i].iter().map(|n| eg.get_size(n)).sum::<usize>());
    }
    let mut samples = vec![];
    for round in 0..repeats {
        for offset in 0..programs.len() {
            let i = (round + offset) % programs.len();
            let (eg, mut sample) = execute(&programs[i], false)?;
            sample["round"] = json!(round);
            sample["variant"] = json!(i);
            sample["same_original_snapshot"] = json!(matches!(
                compare(
                    &state,
                    &native_snapshot::snapshot_filtered(
                        &eg,
                        &BTreeSet::new(),
                        "measure",
                        &ignored[i]
                    )?
                )?,
                Comparison::Equivalent { .. }
            ));
            samples.push(sample);
        }
    }
    let result = json!({"scope":"actual native execution of generated .egg; original facts, literals, equality and visibility compared after excluding new auxiliary relations; no trace/ripen pipeline","files":&args[4..],"profile_same_original_snapshot":profile_equal,"diagnostic_only_equivalent_except_to_hoist":diagnostic_equal,"marker_rows":marker_rows,"profiles":profiles,"samples":samples,"timing":"rotating execution order; profiling disabled during timing; parse, rewrite generation and graph comparison excluded; setup and schedule time reported separately"});
    std::fs::write(
        out.join("measurement.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!(
        "{}",
        json!({"profile_same_original_snapshot":profile_equal,"samples":samples.len()})
    );
    Ok(())
}
fn main() -> Result {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "measure") {
        return measure(&args);
    }

    let source = std::fs::read_to_string(
        args.get(1)
            .ok_or("SOURCE.egg RIPSAT_DIR OUTPUT_DIR [inventory|bench]")?,
    )?;
    let lib = PathBuf::from(&args[2]);
    let out = PathBuf::from(&args[3]);
    std::fs::create_dir(&out)?;
    let mut parser = EGraph::default();
    let program = visual_rule::surface_program(parser.parse_program(None, &source)?);
    validate_scope(&program)?;
    let schemas = schemas(&program);
    let mut active = BTreeSet::new();
    for c in &program {
        if let Command::RunSchedule(s) = c {
            runs(s, &mut active);
        }
    }
    let mut mutable = BTreeSet::new();
    for c in &program {
        if let Command::Rule { rule } = c {
            if active.contains(&rule.ruleset) {
                for a in &rule.head.0 {
                    if let Action::Change(_, _, op, _) = a {
                        mutable.insert(op.clone());
                    }
                }
            }
        }
    }
    for c in &program {
        if let Command::Action(Action::Change(_, _, op, _)) = c {
            mutable.insert(op.clone());
        }
    }
    let mut motifs: BTreeMap<Motif, BTreeSet<String>> = BTreeMap::new();
    if lib.is_file() {
        let frozen: Json = serde_json::from_slice(&std::fs::read(&lib)?)?;
        if frozen["version"] != 1 {
            return Err("unsupported motif dictionary version".into());
        }
        for item in frozen["motifs"].as_array().ok_or("missing frozen motifs")? {
            let m: Motif = serde_json::from_value(item["motif"].clone())?;
            let valid = match &m {
                Motif::Nested { outer, slot, inner } => {
                    schemas
                        .get(outer)
                        .zip(schemas.get(inner))
                        .is_some_and(|(o, i)| o.args.get(*slot) == Some(&i.out))
                        && !mutable.contains(outer)
                        && !mutable.contains(inner)
                }
                Motif::Shared { a, ai, b, bi } => {
                    schemas.get(a).zip(schemas.get(b)).is_some_and(|(a, b)| {
                        a.args
                            .get(*ai)
                            .zip(b.args.get(*bi))
                            .is_some_and(|(a, b)| a == b)
                    }) && !mutable.contains(a)
                        && !mutable.contains(b)
                }
            };
            if valid {
                motifs.insert(
                    m,
                    item["ripsat_evidence"]
                        .as_array()
                        .ok_or("missing evidence")?
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect(),
                );
            }
        }
    } else {
        for path in std::fs::read_dir(&lib)? {
            let path = path?.path();
            if !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("group-")
                || path.extension().is_none_or(|e| e != "json")
            {
                continue;
            }
            let state: egg_layout::saturated_rule_composition::SaturatedRuleComposition =
                serde_json::from_slice(&std::fs::read(&path)?)?;
            let rows: Vec<_> = state
                .rows
                .iter()
                .filter(|r| schemas.contains_key(&r.op) && !mutable.contains(&r.op))
                .collect();
            for a in &rows {
                for b in &rows {
                    if schemas[&a.op].args.is_empty() || schemas[&b.op].args.is_empty() {
                        continue;
                    }
                    for (i, x) in a.args.iter().enumerate() {
                        if *x == b.result {
                            motifs
                                .entry(Motif::Nested {
                                    outer: a.op.clone(),
                                    slot: i,
                                    inner: b.op.clone(),
                                })
                                .or_default()
                                .insert(path.file_name().unwrap().to_string_lossy().into());
                        }
                    }
                    if a.op < b.op {
                        for (i, x) in a.args.iter().enumerate() {
                            for (j, y) in b.args.iter().enumerate() {
                                if x == y {
                                    motifs
                                        .entry(Motif::Shared {
                                            a: a.op.clone(),
                                            ai: i,
                                            b: b.op.clone(),
                                            bi: j,
                                        })
                                        .or_default()
                                        .insert(path.file_name().unwrap().to_string_lossy().into());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Boundary candidates: a source consumer connects a known cell operator
    // to another positive structural atom. This is selection evidence only;
    // the materialized relation still checks the entire exact conjunction.
    let observed: BTreeSet<_> = motifs
        .keys()
        .flat_map(|m| match m {
            Motif::Nested { outer, inner, .. } => vec![outer.clone(), inner.clone()],
            Motif::Shared { a, b, .. } => vec![a.clone(), b.clone()],
        })
        .collect();
    fn nested(
        e: &Expr,
        schemas: &BTreeMap<String, Schema>,
        mutable: &BTreeSet<String>,
        known: &BTreeSet<String>,
        out: &mut BTreeSet<Motif>,
    ) {
        if let Expr::Call(_, op, args) = e {
            if schemas.contains_key(op) && !mutable.contains(op) {
                for (slot, arg) in args.iter().enumerate() {
                    if let Expr::Call(_, inner, _) = arg {
                        if schemas.get(inner).is_some_and(|s| !s.args.is_empty())
                            && !mutable.contains(inner)
                            && (known.contains(op) || known.contains(inner))
                        {
                            out.insert(Motif::Nested {
                                outer: op.clone(),
                                slot,
                                inner: inner.clone(),
                            });
                        }
                    }
                }
            }
            for a in args {
                nested(a, schemas, mutable, known, out);
            }
        }
    }
    let mut boundary = BTreeSet::new();
    for c in &program {
        if let Command::Rule { rule } = c {
            if !active.contains(&rule.ruleset) {
                continue;
            }
            for f in &rule.body {
                match f {
                    Fact::Eq(_, a, b) => {
                        nested(a, &schemas, &mutable, &observed, &mut boundary);
                        nested(b, &schemas, &mutable, &observed, &mut boundary);
                    }
                    Fact::Fact(e) => nested(e, &schemas, &mutable, &observed, &mut boundary),
                }
            }
            for f in &rule.body {
                let Fact::Fact(Expr::Call(_, a, xs)) = f else {
                    continue;
                };
                if !schemas.contains_key(a) || mutable.contains(a) {
                    continue;
                }
                for g in &rule.body {
                    let Fact::Fact(Expr::Call(_, b, ys)) = g else {
                        continue;
                    };
                    if a >= b
                        || !schemas.contains_key(b)
                        || mutable.contains(b)
                        || (!observed.contains(a) && !observed.contains(b))
                    {
                        continue;
                    }
                    for (ai, x) in xs.iter().enumerate() {
                        for (bi, y) in ys.iter().enumerate() {
                            if matches!((x,y),(Expr::Var(_,x),Expr::Var(_,y)) if x==y) {
                                boundary.insert(Motif::Shared {
                                    a: a.clone(),
                                    ai,
                                    b: b.clone(),
                                    bi,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    for m in boundary.into_iter().filter(|_| !lib.is_file()) {
        motifs
            .entry(m)
            .or_default()
            .insert("boundary-consumer-of-observed-cell-operator".into());
    }
    emit(
        &out.join("baseline.egg"),
        &program,
        &json!({"variant":"baseline"}),
    )?;
    let mut candidates = vec![];
    let mut programs = vec![];
    for (m, evidence) in motifs {
        let (p, mut meta) = transform(
            &program,
            &m,
            &schemas,
            &active,
            args.iter().any(|s| s == "--shadow"),
        );
        let occurrences: u64 = meta["rewrites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["occurrences"].as_u64().unwrap())
            .sum();
        if occurrences < 2 {
            continue;
        }
        let id = candidates.len();
        let file = format!("view-{id:03}.egg");
        meta["id"] = json!(id);
        meta["file"] = json!(file);
        meta["ripsat_evidence"] = json!(evidence);
        meta["occurrences"] = json!(occurrences);
        emit(&out.join(file), &p, &meta)?;
        programs.push(p);
        candidates.push(meta);
    }
    let mut report = json!({"scope":"standalone source-to-source experiment; no kernel/planner changes; markers consume actual facts, not predicted closure facts","active_rulesets":active,"excluded_mutable_operators":mutable,"candidates":candidates});
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    eprintln!("[marker] {} candidates", programs.len());
    if args.get(4).is_some_and(|m| m == "bundle") {
        let ids: Vec<usize> = args
            .get(5)
            .ok_or("bundle needs comma-separated candidate IDs")?
            .split(',')
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()?;
        let mut combined = program.clone();
        let mut parts = vec![];
        for id in ids {
            let motif: Motif = serde_json::from_value(
                candidates.get(id).ok_or("invalid candidate ID")?["motif"].clone(),
            )?;
            let mut bundle_active = BTreeSet::new();
            for c in &combined {
                if let Command::RunSchedule(s) = c {
                    runs(s, &mut bundle_active);
                }
            }
            let (next, meta) = transform(
                &combined,
                &motif,
                &schemas,
                &bundle_active,
                args.iter().any(|s| s == "--shadow"),
            );
            combined = next;
            parts.push(meta);
        }
        emit(&out.join("bundle.egg"), &combined, &json!({"parts":parts}))?;
        return Ok(());
    }
    if args.get(4).is_some_and(|m| m == "inventory") {
        return Ok(());
    }
    let (baseline, baseline_profile) = execute(&program, true)?;
    let baseline_state = native_snapshot::snapshot_filtered(
        &baseline,
        &BTreeSet::new(),
        "projected-original-state",
        &BTreeSet::new(),
    )?;
    report["baseline"] = baseline_profile;
    std::fs::write(
        out.join("baseline-state.json"),
        serde_json::to_vec(&baseline_state)?,
    )?;
    let (control, _) = execute(&program, false)?;
    report["baseline_self_equivalent"] = json!(matches!(
        compare(
            &baseline_state,
            &native_snapshot::snapshot_filtered(
                &control,
                &BTreeSet::new(),
                "projected-original-state",
                &BTreeSet::new()
            )?
        )?,
        Comparison::Equivalent { .. }
    ));

    let mut measurements = vec![];
    for (i, p) in programs.iter().enumerate() {
        let (eg, profile) = execute(p, true)?;
        let ignored = BTreeSet::from([candidates[i]["marker"].as_str().unwrap().to_string()]);
        let state = native_snapshot::snapshot_filtered(
            &eg,
            &BTreeSet::new(),
            "projected-original-state",
            &ignored,
        )?;
        let equal = matches!(
            compare(&baseline_state, &state)?,
            Comparison::Equivalent { .. }
        );
        if !equal {
            std::fs::write(
                out.join(format!("view-{i:03}-state.json")),
                serde_json::to_vec(&state)?,
            )?;
        }
        let marker_rows = eg.get_size(candidates[i]["marker"].as_str().unwrap());
        eprintln!(
            "[marker] {i} equal={equal} rows={}+{} marker_rows={marker_rows}",
            profile["scan_rows"], profile["refine_rows"]
        );
        measurements.push(json!({"id":i,"same_original_snapshot":equal,"marker_rows":marker_rows,"profile":profile}));
        report["measurements"] = json!(measurements);
        std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn check(source: &str, m: Motif) {
        let mut parser = EGraph::default();
        let p = visual_rule::surface_program(parser.parse_program(None, source).unwrap());
        let mut active = BTreeSet::new();
        for c in &p {
            if let Command::RunSchedule(s) = c {
                runs(s, &mut active)
            }
        }
        let (optimized, meta) = transform(&p, &m, &schemas(&p), &active, false);
        let mut base = EGraph::default();
        base.run_program(p).unwrap();
        let mut changed = EGraph::default();
        changed.run_program(optimized).unwrap();
        let a =
            native_snapshot::snapshot_filtered(&base, &BTreeSet::new(), "test", &BTreeSet::new())
                .unwrap();
        let b = native_snapshot::snapshot_filtered(
            &changed,
            &BTreeSet::new(),
            "test",
            &BTreeSet::from([meta["marker"].as_str().unwrap().to_string()]),
        )
        .unwrap();
        assert!(matches!(
            compare(&a, &b).unwrap(),
            Comparison::Equivalent { .. }
        ));
    }
    #[test]
    fn shared_marker_sees_union_enabled_matches_in_same_single_step() {
        check(
            r#"
            (sort E) (sort F) (constructor A (i64) E)
            (constructor P (E) F) (constructor Q (E) F)
            (relation Hit (E)) (relation Hit2 (E))
            (rule ((P x) (Q x)) ((Hit x)))
            (rule ((P x) (Q x)) ((Hit2 x)))
            (P (A 0)) (Q (A 1)) (run 1)
            (fail (check (Hit (A 0))))
            (union (A 0) (A 1)) (run 1)
            (check (Hit (A 0)) (Hit2 (A 1)))
        "#,
            Motif::Shared {
                a: "P".into(),
                ai: 0,
                b: "Q".into(),
                bi: 0,
            },
        );
    }
    #[test]
    fn nested_marker_sees_union_enabled_type_shape() {
        check(
            r#"
            (sort E) (sort T) (sort F) (constructor V (i64) E)
            (constructor Unknown () T) (constructor Ty (i64) T) (constructor Has (E T) F)
            (relation Hit (E i64)) (relation Hit2 (E))
            (rule ((Has e (Ty t))) ((Hit e t)))
            (rule ((Has e (Ty 1))) ((Hit2 e)))
            (Has (V 0) (Unknown)) (Ty 1) (run 1)
            (fail (check (Hit2 (V 0))))
            (union (Unknown) (Ty 1)) (run 1) (check (Hit2 (V 0)))
        "#,
            Motif::Nested {
                outer: "Has".into(),
                slot: 1,
                inner: "Ty".into(),
            },
        );
    }
    #[test]
    fn nested_view_retains_outputs_and_alias_constraints() {
        check(
            r#"
            (sort E) (constructor V (i64) E) (constructor Inner (E) E) (constructor Outer (E E) E)
            (relation Hit (E E)) (relation Hit2 (E))
            (rule ((= root (Outer x (Inner x)))) ((Hit root x)))
            (rule ((Outer x (Inner y))) ((Hit2 y)))
            (Outer (V 0) (Inner (V 0))) (Outer (V 1) (Inner (V 2))) (run 1)
            (check (Hit (Outer (V 0) (Inner (V 0))) (V 0)))
            (fail (check (Hit (Outer (V 1) (Inner (V 2))) (V 1))))
        "#,
            Motif::Nested {
                outer: "Outer".into(),
                slot: 1,
                inner: "Inner".into(),
            },
        );
    }
    #[test]
    fn predicate_view_projects_unused_result_and_maintains_late_input() {
        check(
            r#"
            (sort E) (sort T) (sort F) (constructor V (i64) E) (constructor Ty (i64) T)
            (constructor Has (E T) F) (relation Hit (E i64)) (relation Hit2 (E))
            (rule ((Has e (Ty t))) ((Hit e t)))
            (rule ((Has e (Ty 1))) ((Hit2 e)))
            (Has (V 0) (Ty 1)) (run 1) (check (Hit2 (V 0)))
            (Has (V 2) (Ty 3)) (run 1) (check (Hit (V 2) 3))
        "#,
            Motif::Nested {
                outer: "Has".into(),
                slot: 1,
                inner: "Ty".into(),
            },
        );
    }
}
