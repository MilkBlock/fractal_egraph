//! Whole-schema choice policy. Acceptance is selected-program equivalence;
//! full intermediate graph determinism is explicitly not guaranteed.
use super::*;
use egglog::ast::{Expr, Fact};
use serde_json::{Value as Json, json};

pub fn setup_full(eg: &mut EGraph, commands: &[Command]) -> Result<()> {
    super::setup_impl(eg, commands, true, true, &BTreeSet::new())
}
pub const POLICY_HEADER: &str = "; egg-layout structural-keys/eggcc-v1";
/// Install a program that has already passed rewrite_ties. Do not duplicate
/// equal-cost improvement rules when replaying recorded normalized source.
pub fn setup_prepared_full(eg: &mut EGraph, commands: &[Command]) -> Result<()> {
    super::setup_impl(eg, commands, true, false, &BTreeSet::new())
}
/// Synthetic boundary constructors are unknown input ports, not real program
/// shapes. Do not synthesize keys from their names or parameter indexes.
pub fn setup_prepared_with_opaque(
    eg: &mut EGraph,
    commands: &[Command],
    opaque: &BTreeSet<String>,
) -> Result<()> {
    super::setup_impl(eg, commands, true, false, opaque)
}
fn encode_key(k: &Key) -> Json {
    json!([
        k.0.anchor,
        k.0.label.as_ref(),
        k.0.children.iter().map(encode_key).collect::<Vec<_>>()
    ])
}
pub(super) fn decode_key(text: &str) -> Option<Key> {
    fn decode(j: &Json) -> Option<Key> {
        let xs = j.as_array()?;
        if xs.len() != 3 {
            return None;
        }
        let anchor = xs[0].as_bool()?;
        let label = xs[1].as_str()?.to_owned();
        let children = xs[2]
            .as_array()?
            .iter()
            .map(decode)
            .collect::<Option<Vec<_>>>()?;
        if anchor && (label != "\0input-anchor" || children.len() != 1) {
            return None;
        }
        let mut key = Key::new(label, children);
        if anchor {
            let n = Arc::get_mut(&mut key.0)?;
            n.anchor = true;
            n.height = 0;
        }
        Some(key)
    }
    decode(&serde_json::from_str::<Json>(text).ok()?)
}
pub fn key_literal(eg: &EGraph, value: Value) -> String {
    let k = eg.value_to_base::<Stored>(value);
    format!(
        "(cell-key-decode {})",
        serde_json::to_string(&encode_key(&k).to_string()).unwrap()
    )
}
pub(super) fn install_base(eg: &mut EGraph) -> Result<()> {
    add_base_sort(eg, KeySort, Span::Panic)?;
    let key = eg.get_sort_by_name(SORT).unwrap().clone();
    let int = eg.get_sort_by_name("i64").unwrap().clone();
    let string = eg.get_sort_by_name("String").unwrap().clone();
    for (name, op, types) in [
        ("cell-key-min", Op::Min, vec![key.clone(); 3]),
        ("cell-key-anchor", Op::Anchor, vec![key.clone(); 2]),
        (
            "cell-key-cmp",
            Op::Compare,
            vec![key.clone(), key.clone(), int],
        ),
        ("cell-key-decode", Op::Decode, vec![string, key]),
    ] {
        eg.add_pure_primitive(
            Prim {
                name: name.into(),
                types,
                op,
            },
            None,
        );
    }
    Ok(())
}
// Only the three known eggcc selection shapes are rewritten. An unfamiliar
// shape is not guessed to have the same selection intent.
fn call_in<'a>(e: &'a Expr, name: &str) -> Option<&'a [Expr]> {
    if let Expr::Call(_, n, args) = e {
        if n == name {
            return Some(args);
        }
        return args.iter().find_map(|e| call_in(e, name));
    }
    None
}
fn fact_call<'a>(f: &'a Fact, name: &str) -> Option<&'a [Expr]> {
    match f {
        Fact::Eq(_, a, b) => call_in(a, name).or_else(|| call_in(b, name)),
        Fact::Fact(e) => call_in(e, name),
    }
}
fn parse_fact(text: &str) -> Result<Fact> {
    let mut p = egglog::ast::Parser::default();
    let cs = p.get_program_from_string(None, &format!("(rule ({text}) ())"))?;
    let Command::Rule { rule, .. } = &cs[0] else {
        unreachable!()
    };
    Ok(rule.body[0].clone())
}
pub fn rewrite_ties(commands: &[Command]) -> Result<(Vec<Command>, Json)> {
    let mut result = vec![];
    let (mut smaller, mut ivt, mut improve, mut hoist) = (0, 0, 0, 0);
    for command in commands {
        if let Command::Function { name, .. } = command {
            if name == "to-hoist" {
                let mut c = command.clone();
                let Command::Function { merge, .. } = &mut c else {
                    unreachable!()
                };
                *merge=Some(egglog::ast::Parser::default().get_expr_from_string(None,"(cell-hoist-best old new (Expr-size old) (Expr-size new) (CellStructuralKey-Expr old) (CellStructuralKey-Expr new))")?);
                result.push(c);
                hoist += 1;
                continue;
            }
        }
        let Command::Rule { rule } = command else {
            result.push(command.clone());
            continue;
        };
        let name = &rule.name;
        let pair = rule.body.iter().find_map(|f| fact_call(f, "Smaller"));
        if rule.ruleset == "terms-helpers-helpers" {
            if let Some(args) = pair {
                let left = call_in(&args[0], "TCPair").ok_or("Smaller left shape")?;
                let right = call_in(&args[1], "TCPair").ok_or("Smaller right shape")?;
                if rule.body.iter().any(|f|matches!(f,Fact::Eq(_,a,b) if a.to_string()==left[1].to_string()&&b.to_string()==right[1].to_string())) {
                    smaller+=1;
                    for (suffix,op,winner) in [("left","<=",&args[0]),("right",">",&args[1])] {
                        let mut r=rule.clone();
                        r.body.push(parse_fact(&format!("({op} (cell-key-cmp (CellStructuralKey-Term {}) (CellStructuralKey-Term {})) 0)",left[0],right[0]))?);
                        let mut h=r.head.0.clone();
                        let egglog::ast::Action::Union(_,_,rhs)=&mut h[0]else{return Err("Smaller action shape".into())};
                        *rhs=winner.clone();r.head.0=h;
                        r.name=format!("{name}/stable-{suffix}");result.push(Command::Rule{rule:r});
                    }continue;
                }
            }
        }
        if rule.ruleset == "ivt-analysis" {
            if let Some(args) = rule.body.iter().find_map(|f| fact_call(f, "IVTMin")) {
                let left = call_in(&args[0], "IVTAnalysisRes").ok_or("IVT left shape")?;
                let right = call_in(&args[1], "IVTAnalysisRes").ok_or("IVT right shape")?;
                if let Some(index) = rule.body.iter().position(|f| {
                    fact_call(f, "<=").is_some_and(|xs| {
                        xs[0].to_string() == left[3].to_string()
                            && xs[1].to_string() == right[3].to_string()
                    })
                }) {
                    ivt += 1;
                    let mut strict = rule.clone();
                    strict.body[index] = parse_fact(&format!("(< {} {})", left[3], right[3]))?;
                    strict.name = format!("{name}/stable-strict");
                    result.push(Command::Rule { rule: strict });
                    for (suffix, op, winner) in [("left", "<=", &args[0]), ("right", ">", &args[1])]
                    {
                        let mut r = rule.clone();
                        r.body[index] = parse_fact(&format!("(= {} {})", left[3], right[3]))?;
                        r.body.push(parse_fact(&format!("({op} (cell-key-cmp (CellStructuralKey-IVTRes {}) (CellStructuralKey-IVTRes {})) 0)",args[0],args[1]))?);
                        let egglog::ast::Action::Union(_, _, rhs) = &mut r.head.0[0] else {
                            return Err("IVT action shape".into());
                        };
                        *rhs = winner.clone();
                        r.name = format!("{name}/stable-{suffix}");
                        result.push(Command::Rule { rule: r });
                    }
                    continue;
                }
            }
        }
        result.push(command.clone());
        if rule.ruleset == "terms-helpers" {
            let new = rule
                .body
                .iter()
                .find_map(|f| fact_call(f, "PotentialExtractedExpr"));
            if let Some(args) = new {
                if let Some(pair) = call_in(&args[1], "TCPair") {
                    let old = rule
                        .body
                        .iter()
                        .filter(|f| fact_call(f, "ExtractedExpr").is_some())
                        .find_map(|f| fact_call(f, "TCPair"));
                    if let Some(old) = old {
                        if let Some(index) = rule.body.iter().position(|f| {
                            fact_call(f, "<").is_some_and(|xs| {
                                xs[0].to_string() == pair[1].to_string()
                                    && xs[1].to_string() == old[1].to_string()
                            })
                        }) {
                            improve += 1;
                            let mut r = rule.clone();
                            r.body[index] = parse_fact(&format!("(= {} {})", pair[1], old[1]))?;
                            r.body.push(parse_fact(&format!("(< (cell-key-cmp (CellStructuralKey-Term {}) (CellStructuralKey-Term {})) 0)",pair[0],old[0]))?);
                            r.name = format!("{name}/stable-equal-cost");
                            result.push(Command::Rule { rule: r });
                        }
                    }
                }
            }
        }
    }
    Ok((
        result,
        json!({"hoist":hoist,"smaller_ties":smaller,"ivt_ties":ivt,"equal_cost_improvement":improve}),
    ))
}

/// Every original Run retains its own boundary. Keys are maintained before
/// each Run and once at the end; setup/freeze is separate from inference.
pub fn schedule_full(eg: &mut EGraph, commands: &[Command]) -> Result<Vec<Command>> {
    let helper = eg.parse_program(None, &format!("(run-schedule (saturate {RULESET}))"))?;
    let Command::RunSchedule(h) = &helper[0] else {
        unreachable!()
    };
    fn walk(s: &Schedule, h: &Schedule) -> Schedule {
        match s {
            GenericSchedule::Run(span, _) => {
                GenericSchedule::Sequence(span.clone(), vec![h.clone(), s.clone()])
            }
            GenericSchedule::Sequence(span, xs) => {
                GenericSchedule::Sequence(span.clone(), xs.iter().map(|x| walk(x, h)).collect())
            }
            GenericSchedule::Repeat(span, n, x) => {
                GenericSchedule::Repeat(span.clone(), *n, Box::new(walk(x, h)))
            }
            GenericSchedule::Saturate(span, x) => {
                GenericSchedule::Saturate(span.clone(), Box::new(walk(x, h)))
            }
        }
    }
    let mut cs = commands
        .iter()
        .map(|c| {
            if let Command::RunSchedule(s) = c {
                Command::RunSchedule(walk(s, h))
            } else {
                c.clone()
            }
        })
        .collect::<Vec<_>>();
    cs.extend(helper);
    Ok(cs)
}

pub fn coverage(eg: &EGraph) -> Result<Json> {
    let mut all = BTreeMap::<String, BTreeSet<Value>>::new();
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        if !f.is_constructor() {
            continue;
        }
        let s = f.schema().output.clone();
        eg.function_for_each(&name, |row| {
            all.entry(s.name().into())
                .or_default()
                .insert(eg.get_canonical_value(*row.vals.last().unwrap(), &s));
        })?;
    }
    let mut report = vec![];
    for (sort, values) in all {
        let f = key_function(&sort);
        let mut covered = BTreeSet::new();
        if eg.get_function(&f).is_some() {
            let s = eg.get_sort_by_name(&sort).unwrap();
            eg.function_for_each(&f, |row| {
                covered.insert(eg.get_canonical_value(row.vals[0], &s));
            })?;
        }
        report.push(json!({"sort":sort,"constructor_eclasses":values.len(),"keyed_eclasses":values.intersection(&covered).count(),"missing":values.difference(&covered).count()}));
    }
    Ok(json!(report))
}

pub fn initialization_end(commands: &[Command]) -> Result<usize> {
    use egglog::ast::GenericSchedule;
    let mut i = commands
        .iter()
        .position(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("structural-key analysis needs a schedule")?;
    while let Some(Command::RunSchedule(s)) = commands.get(i) {
        fn single(s: &egglog::ast::Schedule) -> &egglog::ast::Schedule {
            match s {
                GenericSchedule::Repeat(_, 1, x) => single(x),
                GenericSchedule::Sequence(_, xs) if xs.len() == 1 => single(&xs[0]),
                other => other,
            }
        }
        let step = single(s);
        let GenericSchedule::Run(_, r) = step else {
            break;
        };
        if r.until.is_some() {
            break;
        }
        let rules = commands[..i]
            .iter()
            .filter_map(|c| {
                if let Command::Rule { rule } = c {
                    (rule.ruleset == r.ruleset).then_some(rule)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if rules.is_empty() || rules.iter().any(|r| !r.body.is_empty()) {
            break;
        }
        i += 1;
    }
    if i == commands.len() {
        return Err(
            "structural-key analysis needs an inference schedule after initialization".into(),
        );
    }
    if commands[i..].iter().any(|c| {
        matches!(
            c,
            Command::Sort { .. }
                | Command::Datatype { .. }
                | Command::Datatypes { .. }
                | Command::Constructor { .. }
                | Command::Function { .. }
                | Command::Relation { .. }
        ) || matches!(
            c,
            Command::Rule { .. } | Command::AddRuleset(..) | Command::UnstableCombinedRuleset(..)
        )
    }) {
        return Err(
            "structural-key policy requires declarations and rules before inference".into(),
        );
    }
    Ok(i)
}
