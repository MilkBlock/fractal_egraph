//! Executable symbolic-size lowering for a concretely validated local entry.
use crate::pipeline::Result;
use egglog::prelude::{RustSpan, Span};
use egglog::{
    EGraph,
    ast::{Action, Command, Expr, Fact, Literal, Rule},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
const DSL: &str = r#"
(datatype RipenSize (RSParam i64) (RSConst i64) (RSAdd RipenSize RipenSize) (RSSub RipenSize RipenSize) (RSMul RipenSize RipenSize) (RSMin RipenSize RipenSize) (RSMax RipenSize RipenSize))
(relation RSGt (RipenSize RipenSize))
(relation RSGe (RipenSize RipenSize))
(relation RSLt (RipenSize RipenSize))
(relation RSLe (RipenSize RipenSize))
(relation RSEq (RipenSize RipenSize))
(relation RSNe (RipenSize RipenSize))
(function RSLower (RipenSize) i64 :merge (max old new))
(function RSUpper (RipenSize) i64 :merge (min old new))
(ruleset ripen-size-symbolic)
(rule ((= lo (RSLower e)) (RSConst n) (> lo n)) ((RSGt e (RSConst n))) :ruleset ripen-size-symbolic)
(rule ((= lo (RSLower e)) (RSConst n) (>= lo n)) ((RSGe e (RSConst n))) :ruleset ripen-size-symbolic)
(rule ((= hi (RSUpper e)) (RSConst n) (< hi n)) ((RSLt e (RSConst n))) :ruleset ripen-size-symbolic)
(rule ((= hi (RSUpper e)) (RSConst n) (<= hi n)) ((RSLe e (RSConst n))) :ruleset ripen-size-symbolic)
(rule ((= n (RSLower e)) (= n (RSUpper e))) ((RSEq e (RSConst n))) :ruleset ripen-size-symbolic)
(rule ((= e (RSMin a a))) ((union e a)) :ruleset ripen-size-symbolic)
(rule ((= e (RSMax a a))) ((union e a)) :ruleset ripen-size-symbolic)
(rule ((= e (RSMin (RSMin a b) b))) ((union e (RSMin a b))) :ruleset ripen-size-symbolic)
(rule ((= e (RSMax (RSMax a b) b))) ((union e (RSMax a b))) :ruleset ripen-size-symbolic)
(rule ((= e (RSMin a (RSMin a b)))) ((union e (RSMin a b))) :ruleset ripen-size-symbolic)
(rule ((= e (RSMax a (RSMax a b)))) ((union e (RSMax a b))) :ruleset ripen-size-symbolic)
(rule ((RSConst a) (RSConst b) (> a b)) ((RSGt (RSConst a) (RSConst b))) :ruleset ripen-size-symbolic)
(rule ((RSConst a) (RSConst b) (>= a b)) ((RSGe (RSConst a) (RSConst b))) :ruleset ripen-size-symbolic)
(rule ((RSConst a) (RSConst b) (< a b)) ((RSLt (RSConst a) (RSConst b))) :ruleset ripen-size-symbolic)
(rule ((RSConst a) (RSConst b) (<= a b)) ((RSLe (RSConst a) (RSConst b))) :ruleset ripen-size-symbolic)
(rule ((RSConst a) (RSConst b) (!= a b)) ((RSNe (RSConst a) (RSConst b))) :ruleset ripen-size-symbolic)
(rule ((RSConst a)) ((RSEq (RSConst a) (RSConst a))) :ruleset ripen-size-symbolic)
"#;
fn expr(s: &str) -> Result<Expr> {
    Ok(egglog::ast::Parser::default().get_expr_from_string(None, s)?)
}
fn call(op: &str, xs: Vec<Expr>) -> Expr {
    Expr::Call(egglog::span!(), op.into(), xs)
}
fn symbolic(op: &str) -> Option<&'static str> {
    Some(match op {
        "+" => "RSAdd",
        "-" => "RSSub",
        "*" => "RSMul",
        "min" => "RSMin",
        "max" => "RSMax",
        _ => return None,
    })
}
fn comparison(op: &str) -> Option<&'static str> {
    Some(match op {
        ">" => "RSGt",
        ">=" => "RSGe",
        "<" => "RSLt",
        "<=" => "RSLe",
        "=" => "RSEq",
        "!=" => "RSNe",
        _ => return None,
    })
}
fn tainted(e: &Expr, vars: &BTreeSet<String>, fields: &BTreeSet<String>) -> bool {
    match e {
        Expr::Var(_, v) => vars.contains(v),
        Expr::Call(_, op, args) => {
            fields.contains(op) || args.iter().any(|e| tainted(e, vars, fields))
        }
        _ => false,
    }
}
fn vars_for(r: &Rule, fields: &BTreeSet<String>) -> BTreeSet<String> {
    let mut vars = BTreeSet::new();
    loop {
        let before = vars.len();
        for f in &r.body {
            if let Fact::Eq(_, a, b) = f {
                for (a, b) in [(a, b), (b, a)] {
                    if let Expr::Var(_, v) = a {
                        if tainted(b, &vars, fields) {
                            vars.insert(v.clone());
                        }
                    }
                }
            }
        }
        if vars.len() == before {
            break;
        }
    }
    vars
}

fn lower(e: &Expr, vars: &BTreeSet<String>, fields: &BTreeSet<String>, want: bool) -> Result<Expr> {
    let is = tainted(e, vars, fields);
    if want && !is {
        return Ok(call("RSConst", vec![e.clone()]));
    }
    Ok(match e {
        Expr::Call(s, op, args) if fields.contains(op) => {
            if args.iter().any(|e| tainted(e, vars, fields)) {
                return Err(format!("symbolic size used as key of {op}").into());
            }
            Expr::Call(s.clone(), op.clone(), args.clone())
        }
        Expr::Call(_, op, args) if is => {
            let target =
                symbolic(op).ok_or_else(|| format!("unsupported size-dependent operation {op}"))?;
            call(
                target,
                args.iter()
                    .map(|e| lower(e, vars, fields, true))
                    .collect::<Result<_>>()?,
            )
        }
        _ => e.clone(),
    })
}
fn lower_fact(f: &Fact, vars: &BTreeSet<String>, fields: &BTreeSet<String>) -> Result<Fact> {
    Ok(match f {
        Fact::Eq(s, a, b) if tainted(a, vars, fields) || tainted(b, vars, fields) => {
            let a = lower(a, vars, fields, true)?;
            let b = lower(b, vars, fields, true)?;
            if matches!(a, Expr::Var(..)) || matches!(b, Expr::Var(..)) {
                Fact::Eq(s.clone(), a, b)
            } else {
                Fact::Fact(call("RSEq", vec![a, b]))
            }
        }
        Fact::Fact(Expr::Call(_, op, args)) if args.iter().any(|e| tainted(e, vars, fields)) => {
            let cmp = comparison(op)
                .ok_or_else(|| format!("unsupported size-dependent predicate {op}"))?;
            Fact::Fact(call(
                cmp,
                args.iter()
                    .map(|e| lower(e, vars, fields, true))
                    .collect::<Result<_>>()?,
            ))
        }
        _ => f.clone(),
    })
}
fn lower_action(
    a: &Action,
    vars: &mut BTreeSet<String>,
    fields: &BTreeSet<String>,
) -> Result<Action> {
    Ok(match a {
        Action::Let(s, v, e) => {
            let is = tainted(e, vars, fields);
            let e = lower(e, vars, fields, is)?;
            if is {
                vars.insert(v.clone());
            }
            Action::Let(s.clone(), v.clone(), e)
        }
        Action::Set(s, op, args, e) => {
            if args.iter().any(|e| tainted(e, vars, fields)) {
                return Err("symbolic size used in a structural set key".into());
            }
            Action::Set(
                s.clone(),
                op.clone(),
                args.clone(),
                lower(e, vars, fields, fields.contains(op))?,
            )
        }
        Action::Expr(s, e) => Action::Expr(s.clone(), lower(e, vars, fields, false)?),
        Action::Union(s, a, b) => Action::Union(
            s.clone(),
            lower(a, vars, fields, false)?,
            lower(b, vars, fields, false)?,
        ),
        _ => a.clone(),
    })
}
fn tree(v: &Value) -> Result<Expr> {
    Ok(match v["kind"].as_str() {
        Some("Input") => call("RSParam", vec![expr(&v["value"].to_string())?]),
        Some("Const") => call("RSConst", vec![expr(&v["value"].to_string())?]),
        Some("Compute") => call(
            symbolic(v["value"][0].as_str().ok_or("bad scalar op")?)
                .ok_or("unsupported scalar op")?,
            v["value"][1]
                .as_array()
                .ok_or("bad scalar arguments")?
                .iter()
                .map(tree)
                .collect::<Result<_>>()?,
        ),
        _ => return Err("invalid scalar tree".into()),
    })
}
fn has_size(c: &Command, fields: &BTreeSet<String>) -> bool {
    match c {
        Command::Check(_, fs) => fs.iter().any(|f| match f {
            Fact::Eq(_, a, b) => {
                tainted(a, &BTreeSet::new(), fields) || tainted(b, &BTreeSet::new(), fields)
            }
            Fact::Fact(e) => tainted(e, &BTreeSet::new(), fields),
        }),
        Command::Fail(_, c) => has_size(c, fields),
        _ => false,
    }
}
/// Lower an already replay-validated entry. Unknown symbolic guards are not
/// executed speculatively: only explicit path assumptions/constant proofs enable them.
pub(super) fn compile(
    commands: &[Command],
    analysis: &Value,
) -> Result<(String, Vec<Command>, Value)> {
    if analysis["complete"] != true {
        return Err("symbolic size program is incomplete".into());
    }
    if analysis["instance_inputs"]
        .as_array()
        .is_none_or(|x| x.is_empty())
    {
        return Err("no external size parameters".into());
    }
    if analysis["template"]["conditions"]["impossible"] == true {
        return Err("contradictory symbolic size conditions".into());
    }
    if commands
        .iter()
        .any(|c| c.to_string().contains("RipenSize") || c.to_string().contains("RSParam"))
    {
        return Err("symbolic size namespace collision".into());
    }
    let mut fields = BTreeSet::from(["Expr-size".to_string(), "ListExpr-size".to_string()]);
    loop {
        let before = fields.len();
        for c in commands {
            if let Command::Rule { rule } = c {
                let mut vars = vars_for(rule, &fields);
                for a in &rule.head.0 {
                    match a {
                        Action::Let(_, v, e) if tainted(e, &vars, &fields) => {
                            vars.insert(v.clone());
                        }
                        Action::Set(_, op, _, e) if tainted(e, &vars, &fields) => {
                            fields.insert(op.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
        if fields.len() == before {
            break;
        }
    }
    let mut parser = EGraph::default();
    let mut result = parser.parse_program(None, DSL)?;
    // These are the selected historical path's symbolic assumptions, not
    // concrete comparisons evaluated using the training input.
    let conditions = &analysis["template"]["conditions"];
    if let Some(bounds) = conditions["bounds"].as_array() {
        for bound in bounds {
            let value = tree(&bound["expression"])?;
            for (name, field, unbounded) in [
                ("RSLower", "lower_inclusive", i64::MIN),
                ("RSUpper", "upper_inclusive", i64::MAX),
            ] {
                let n = bound[field].as_i64().ok_or("invalid scalar bound")?;
                if n != unbounded {
                    result.push(Command::Action(Action::Set(
                        egglog::span!(),
                        name.into(),
                        vec![value.clone()],
                        expr(&n.to_string())?,
                    )));
                }
            }
        }
        for g in conditions["residual"]
            .as_array()
            .ok_or("missing residual conditions")?
        {
            result.push(Command::Action(Action::Expr(
                egglog::span!(),
                call(
                    comparison(g[0].as_str().ok_or("invalid condition")?)
                        .ok_or("unsupported condition")?,
                    vec![tree(&g[1])?, tree(&g[2])?],
                ),
            )));
        }
    } else {
        for step in analysis["template"]["steps"]
            .as_array()
            .ok_or("missing symbolic steps")?
        {
            for g in step["guards"].as_array().ok_or("missing guards")? {
                result.push(Command::Action(Action::Expr(
                    egglog::span!(),
                    call(
                        comparison(g[0].as_str().ok_or("invalid guard")?)
                            .ok_or("unsupported guard")?,
                        vec![tree(&g[1])?, tree(&g[2])?],
                    ),
                )));
            }
        }
    }
    let mut threshold_constants = BTreeSet::new();
    fn thresholds(e: &Expr, out: &mut BTreeSet<i64>) {
        if let Expr::Call(_, op, args) = e {
            if op == "RSConst" {
                if let [Expr::Lit(_, Literal::Int(n))] = args.as_slice() {
                    out.insert(*n);
                }
            }
            for a in args {
                thresholds(a, out);
            }
        }
    }
    let mut seeds: BTreeMap<String, VecDeque<Expr>> = BTreeMap::new();
    let steps = analysis["template"]["steps"]
        .as_array()
        .ok_or("missing steps")?;
    if steps.iter().any(|s| s["seeds"].is_array()) {
        for step in steps {
            for seed in step["seeds"].as_array().ok_or("missing stage seeds")? {
                seeds
                    .entry(seed["field"].as_str().ok_or("bad seed field")?.into())
                    .or_default()
                    .push_back(tree(&seed["value"])?);
            }
            for update in step["updates"].as_array().ok_or("missing stage updates")? {
                seeds
                    .entry(update["field"].as_str().ok_or("bad update field")?.into())
                    .or_default()
                    .push_back(tree(&update["proposed"])?);
            }
        }
    } else {
        for p in analysis["instance_inputs"].as_array().unwrap() {
            seeds
                .entry(p["source"].as_str().ok_or("missing input field")?.into())
                .or_default()
                .push_back(call("RSParam", vec![expr(&p["id"].to_string())?]));
        }
    }
    let mut removed_checks = 0;
    for c in commands {
        if has_size(c, &fields) {
            removed_checks += 1;
            continue;
        }
        let c = match c {
            Command::Function {
                span,
                name,
                schema,
                merge,
                hidden,
                let_binding,
                term_constructor,
                unextractable,
            } if fields.contains(name) => {
                if schema.output != "i64" {
                    return Err(format!("size-derived function {name} is not i64").into());
                }
                let mut schema = schema.clone();
                schema.output = "RipenSize".into();
                let merge = merge
                    .as_ref()
                    .map(|e| {
                        lower(
                            e,
                            &BTreeSet::from(["old".into(), "new".into()]),
                            &fields,
                            true,
                        )
                    })
                    .transpose()?;
                Command::Function {
                    span: span.clone(),
                    name: name.clone(),
                    schema,
                    merge,
                    hidden: *hidden,
                    let_binding: *let_binding,
                    term_constructor: term_constructor.clone(),
                    unextractable: *unextractable,
                }
            }
            Command::Rule { rule } => {
                let mut rule = rule.clone();
                let mut vars = vars_for(&rule, &fields);
                rule.body = rule
                    .body
                    .iter()
                    .map(|f| lower_fact(f, &vars, &fields))
                    .collect::<Result<_>>()?;
                for f in &rule.body {
                    match f {
                        Fact::Eq(_, a, b) => {
                            thresholds(a, &mut threshold_constants);
                            thresholds(b, &mut threshold_constants);
                        }
                        Fact::Fact(e) => thresholds(e, &mut threshold_constants),
                    }
                }
                rule.head.0 = rule
                    .head
                    .0
                    .iter()
                    .map(|a| lower_action(a, &mut vars, &fields))
                    .collect::<Result<_>>()?;
                Command::Rule { rule }
            }
            Command::Action(Action::Set(s, op, args, e)) if fields.contains(op) => {
                let key = call(op, args.clone()).to_string();
                let value = if let Some(q) = seeds.get_mut(&key).filter(|q| !q.is_empty()) {
                    q.pop_front().ok_or("too many analysis injections")?
                } else {
                    lower(e, &BTreeSet::new(), &fields, true)?
                };
                Command::Action(Action::Set(s.clone(), op.clone(), args.clone(), value))
            }
            Command::Action(a) => Command::Action(lower_action(a, &mut BTreeSet::new(), &fields)?),
            _ => c.clone(),
        };
        result.push(c);
    }
    for n in threshold_constants {
        result.push(Command::Action(Action::Expr(
            egglog::span!(),
            call("RSConst", vec![expr(&n.to_string())?]),
        )));
    }
    if seeds.values().any(|q| !q.is_empty()) {
        return Err("some external size inputs were not replaced".into());
    }
    let text = format!(
        "; ripsat-symbolic-size/v1\n; Conditional symbolic closure: unknown scalar guards are deferred.\n{}\n",
        result
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let info = json!({"schema":"ripsat-symbolic-size/v1","lifted_functions":fields,"concrete_size_checks_replaced":removed_checks,"semantics":"closure under explicit symbolic path assumptions; unproved scalar guards remain deferred","certifies_original_full_saturation":false});
    Ok((text, result, info))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(n: i64) -> EGraph {
        let source = format!(
            r#"
   (datatype E (V) (Out E))
   (function Expr-size (E) i64 :merge (min old new))
   (ruleset work)
   (rule ((= x (V)) (= n (Expr-size x)) (> n 6))
         ((set (Expr-size (Out x)) (+ n 1))) :ruleset work)
   (set (Expr-size (V)) {n})
  "#
        );
        let mut eg = EGraph::default();
        let commands = eg.parse_program(None, &source).unwrap();
        let analysis = json!({"complete":true,"instance_inputs":[{"id":0,"source":"(Expr-size (V ))","observed":n.to_string()}],
     "template":{"conditions":{"impossible":false},"steps":[{"guards":[[">",{"kind":"Input","value":0},{"kind":"Const","value":6}]]}]}});
        // Use the native printer for canonical field spelling.
        let mut analysis = analysis;
        analysis["instance_inputs"][0]["source"] =
            json!(expr("(Expr-size (V))").unwrap().to_string());
        let (text, _, _) = compile(&commands, &analysis).unwrap();
        eg.parse_and_run_program(None, &text).unwrap();
        eg.parse_and_run_program(None,"(run work 1) (run ripen-size-symbolic 1) (check (= (Expr-size (V)) (RSParam 0))) (check (= (Expr-size (Out (V))) (RSAdd (RSParam 0) (RSConst 1))))").unwrap();
        eg
    }
    #[test]
    fn actual_program_executes_with_symbolic_size() {
        run(25);
        run(27);
    }
    #[test]
    fn executable_interval_keeps_both_bounds() {
        let mut eg = EGraph::default();
        let commands = eg
            .parse_program(
                None,
                r#"
          (datatype E (V)) (function Expr-size (E) i64 :merge (min old new))
          (relation Hit (i64)) (ruleset work)
          (rule ((= n (Expr-size (V))) (> n 6)) ((Hit 1)) :ruleset work)
          (rule ((= n (Expr-size (V))) (> n 10)) ((Hit 2)) :ruleset work)
          (set (Expr-size (V)) 8)
        "#,
            )
            .unwrap();
        let a = json!({"complete":true,"instance_inputs":[{"id":0,"source":expr("(Expr-size (V))").unwrap().to_string(),"observed":"8"}],
            "template":{"steps":[],"conditions":{"impossible":false,"residual":[],"bounds":[{"expression":{"kind":"Input","value":0},"lower_inclusive":7,"upper_inclusive":9}]}}});
        let (code, _, _) = compile(&commands, &a).unwrap();
        eg.parse_and_run_program(None, &code).unwrap();
        eg.parse_and_run_program(
            None,
            "(run ripen-size-symbolic 2) (run work 1) (check (Hit 1)) (fail (check (Hit 2)))",
        )
        .unwrap();
    }

    #[test]
    fn unknown_guard_does_not_execute() {
        let mut eg = run(25);
        eg.parse_and_run_program(
            None,
            r#"
   (relation Unexpected ()) (ruleset gated)
   (rule ((RSLt (Expr-size (V)) (RSConst 0))) ((Unexpected)) :ruleset gated)
   (run gated 1) (fail (check (Unexpected)))
  "#,
        )
        .unwrap();
    }
}
