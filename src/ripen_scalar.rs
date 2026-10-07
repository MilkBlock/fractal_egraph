//! Symbolic Expr-size programs. Concrete replay remains the validation authority.
//! This representation never unions states merely because their scalar programs match.
use egglog::ast::{Action, Command, Expr, Fact, Literal, Rule};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
enum Scalar {
    Input(usize),
    Const(i64),
    Compute(String, Vec<Scalar>),
}
fn compute(op: &str, mut xs: Vec<Scalar>) -> Scalar {
    if matches!(op, "min" | "max") {
        xs.sort();
        xs.dedup();
        if xs.len() == 1 {
            return xs[0].clone();
        }
    }
    if let [Scalar::Const(a), Scalar::Const(b)] = xs.as_slice() {
        let value = match op {
            "+" => a.checked_add(*b),
            "-" => a.checked_sub(*b),
            "*" => a.checked_mul(*b),
            "min" => Some((*a).min(*b)),
            "max" => Some((*a).max(*b)),
            _ => None,
        };
        if let Some(v) = value {
            return Scalar::Const(v);
        }
    }
    Scalar::Compute(op.into(), xs)
}
fn tracked(op: &str) -> bool {
    matches!(op, "Expr-size" | "ListExpr-size")
}
fn substitute(e: &Expr, env: &BTreeMap<String, Expr>) -> Option<Expr> {
    Some(match e {
        Expr::Var(_, v) => env.get(v)?.clone(),
        Expr::Lit(..) => e.clone(),
        Expr::Call(s, op, args) => Expr::Call(
            s.clone(),
            op.clone(),
            args.iter()
                .map(|e| substitute(e, env))
                .collect::<Option<_>>()?,
        ),
    })
}
#[derive(Default)]
pub(super) struct Program {
    fields: BTreeMap<String, Scalar>,
    inputs: Vec<Value>,
    guards: Vec<(String, Scalar, Scalar)>,
    steps: Vec<Value>,
    unsupported: BTreeSet<String>,
    unresolved: Vec<Value>,
}
impl Program {
    pub(super) fn new(declarations: &[Command]) -> Self {
        let mut p = Self::default();
        for c in declarations {
            if let Command::Function { name, merge, .. } = c {
                if tracked(name)
                    && !matches!(merge,Some(Expr::Call(_,op,args)) if op=="min" && args.len()==2 && matches!((&args[0],&args[1]),(Expr::Var(_,a),Expr::Var(_,b)) if a=="old" && b=="new"))
                {
                    p.unsupported
                        .insert(format!("unsupported merge for {name}"));
                }
            }
        }
        p
    }
    fn input(&mut self, field: &str, observed: Option<String>) -> Scalar {
        let id = self.inputs.len();
        self.inputs
            .push(json!({"id":id,"source":field,"observed":observed}));
        Scalar::Input(id)
    }
    fn field(op: &str, args: &[Expr], env: &BTreeMap<String, Expr>) -> Option<String> {
        let args = args
            .iter()
            .map(|e| substitute(e, env).map(|e| e.to_string()))
            .collect::<Option<Vec<_>>>()?;
        Some(format!("({op} {})", args.join(" ")))
    }
    fn eval(
        &mut self,
        e: &Expr,
        env: &BTreeMap<String, Expr>,
        vars: &BTreeMap<String, Scalar>,
    ) -> Option<Scalar> {
        match e {
            Expr::Lit(_, Literal::Int(n)) => Some(Scalar::Const(*n)),
            Expr::Var(_, v) => vars.get(v).cloned(),
            Expr::Call(_, op, args) if tracked(op) => {
                let key = Self::field(op, args, env)?;
                if let Some(v) = self.fields.get(&key) {
                    return Some(v.clone());
                }
                let v = self.input(&key, None);
                self.fields.insert(key, v.clone());
                Some(v)
            }
            Expr::Call(_, op, args) if matches!(op.as_str(), "+" | "-" | "*" | "min" | "max") => {
                let values = args
                    .iter()
                    .map(|e| self.eval(e, env, vars))
                    .collect::<Option<Vec<_>>>()?;
                Some(compute(op, values))
            }
            _ => None,
        }
    }
    pub(super) fn observe(
        &mut self,
        rule: &Rule,
        env: &BTreeMap<String, Expr>,
        injections: &[Command],
    ) {
        let mut seeds = vec![];
        for command in injections {
            if let Command::Action(Action::Set(_, op, args, value)) = command {
                if !tracked(op) {
                    continue;
                }
                let Some(key) = Self::field(op, args, &BTreeMap::new()) else {
                    self.unsupported.insert("unresolved analysis seed".into());
                    continue;
                };
                // Each field/stage injection is its own parameter, even if values coincide.
                let p = self.input(&key, Some(value.to_string()));
                self.fields.insert(key.clone(), p.clone());
                seeds.push(json!({"field":key,"value":p}));
            }
        }
        let mut vars = BTreeMap::new();
        // Equations bind analysis variables before concrete scalar bindings are considered.
        loop {
            let mut changed = false;
            for fact in &rule.body {
                if let Fact::Eq(_, a, b) = fact {
                    for (lhs, rhs) in [(a, b), (b, a)] {
                        if let Expr::Var(_, v) = lhs {
                            if !vars.contains_key(v) {
                                if let Some(e) = self.eval(rhs, env, &vars) {
                                    vars.insert(v.clone(), e);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut local_guards = vec![];
        for fact in &rule.body {
            let parts = match fact {
                Fact::Eq(_, a, b) => Some(("=", a, b)),
                Fact::Fact(Expr::Call(_, op, args))
                    if matches!(op.as_str(), ">" | ">=" | "<" | "<=" | "!=") && args.len() == 2 =>
                {
                    Some((op.as_str(), &args[0], &args[1]))
                }
                _ => None,
            };
            if let Some((op, a, b)) = parts {
                if let (Some(a), Some(b)) = (self.eval(a, env, &vars), self.eval(b, env, &vars)) {
                    if a != b || op != "=" {
                        self.guards.push((op.into(), a.clone(), b.clone()));
                        local_guards.push(json!([op, a, b]));
                    }
                } else if fact.to_string().contains("Expr-size") || refers_to_scalar(fact, &vars) {
                    self.unsupported
                        .insert(format!("unresolved scalar condition: {fact}"));
                    self.unresolved.push(match fact {
                        Fact::Eq(_, a, b) => {
                            json!({"condition":"=","arguments":[syntax(a),syntax(b)]})
                        }
                        Fact::Fact(e) => json!({"condition":syntax(e)}),
                    });
                }
            }
        }
        let mut updates = vec![];
        for action in &rule.head.0 {
            match action {
                Action::Let(_, v, e) => {
                    if let Some(e) = self.eval(e, env, &vars) {
                        vars.insert(v.clone(), e);
                    }
                }
                Action::Set(_, op, args, value) if tracked(op) => {
                    let key = Self::field(op, args, env);
                    let proposed_value = self.eval(value, env, &vars);
                    if let (Some(key), Some(proposed)) = (key, proposed_value) {
                        let before = self.fields.get(&key).cloned();
                        let after = before
                            .clone()
                            .map(|old| compute("min", vec![old, proposed.clone()]))
                            .unwrap_or_else(|| proposed.clone());
                        self.fields.insert(key.clone(), after.clone());
                        updates.push(json!({"field":key,"before":before,"proposed":proposed,"after":after,"merge":"min"}));
                    } else {
                        self.unsupported
                            .insert(format!("unresolved scalar action: {action}"));
                        self.unresolved.push(json!({"set":op,"arguments":args.iter().map(syntax).collect::<Vec<_>>(),"value":syntax(value)}));
                    }
                }
                Action::Union(..) => {
                    // Congruence may alias analysis keys. Do not silently propagate old SSA.
                    self.unsupported
                        .insert("union requires analysis-key congruence mapping".into());
                    self.fields.clear();
                }
                Action::Change(..) => {
                    self.unsupported
                        .insert("visibility change requires scalar field versioning".into());
                }
                _ => {}
            }
        }
        if !seeds.is_empty() || !local_guards.is_empty() || !updates.is_empty() {
            self.steps.push(json!({"ruleset":rule.ruleset,"seeds":seeds,"guards":local_guards,"updates":updates}));
        }
    }
    pub(super) fn report(&self) -> Value {
        let mut bounds = BTreeMap::<Scalar, (i64, i64)>::new();
        let mut residual = vec![];
        let mut impossible = false;
        for (op, a, b) in &self.guards {
            let (op, a, n) = match (a, b) {
                (a, Scalar::Const(n)) => (op.as_str(), a, *n),
                (Scalar::Const(n), b) => (
                    match op.as_str() {
                        ">" => "<",
                        ">=" => "<=",
                        "<" => ">",
                        "<=" => ">=",
                        other => other,
                    },
                    b,
                    *n,
                ),
                _ => {
                    residual.push(json!([op, a, b]));
                    continue;
                }
            };
            if let Scalar::Const(v) = a {
                let yes = match op {
                    ">" => *v > n,
                    ">=" => *v >= n,
                    "<" => *v < n,
                    "<=" => *v <= n,
                    "=" => *v == n,
                    "!=" => *v != n,
                    _ => true,
                };
                impossible |= !yes;
                continue;
            }
            let range = bounds.entry(a.clone()).or_insert((i64::MIN, i64::MAX));
            match op {
                ">" => {
                    if let Some(n) = n.checked_add(1) {
                        range.0 = range.0.max(n)
                    } else {
                        impossible = true
                    }
                }
                ">=" => range.0 = range.0.max(n),
                "<" => {
                    if let Some(n) = n.checked_sub(1) {
                        range.1 = range.1.min(n)
                    } else {
                        impossible = true
                    }
                }
                "<=" => range.1 = range.1.min(n),
                "=" => {
                    range.0 = range.0.max(n);
                    range.1 = range.1.min(n);
                }
                _ => residual.push(json!([op, a, Scalar::Const(n)])),
            }
            impossible |= range.0 > range.1;
        }
        let parameters: Vec<_> = self
            .inputs
            .iter()
            .map(|p| json!({"id":p["id"],"source":p["source"]}))
            .collect();
        json!({"schema":"ripsat-analysis-program/v1","complete":self.unsupported.is_empty(),"unsupported":self.unsupported,
            "template":{"parameters":parameters,"steps":self.steps,"final_fields":self.fields,"unresolved":self.unresolved,
                "conditions":{"impossible":impossible,"bounds":bounds.iter().map(|(e,(lo,hi))|json!({"expression":e,"lower_inclusive":lo,"upper_inclusive":hi})).collect::<Vec<_>>(),"residual":residual}},
            "instance_inputs":self.inputs,
            "scope":"Symbolic Expr-size/ListExpr-size with min merge. Structured calculations and ordered guards retained; no concrete egraph union or runtime optimization authorized by this report."})
    }
}
fn refers_to_scalar(f: &Fact, vars: &BTreeMap<String, Scalar>) -> bool {
    fn contains(e: &Expr, vars: &BTreeMap<String, Scalar>) -> bool {
        match e {
            Expr::Var(_, v) => vars.contains_key(v),
            Expr::Call(_, _, xs) => xs.iter().any(|e| contains(e, vars)),
            _ => false,
        }
    }
    match f {
        Fact::Eq(_, a, b) => contains(a, vars) || contains(b, vars),
        Fact::Fact(e) => contains(e, vars),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn expr(s: &str) -> Expr {
        egglog::ast::Parser::default()
            .get_expr_from_string(None, s)
            .unwrap()
    }
    fn rules(s: &str) -> Vec<Rule> {
        egglog::EGraph::default()
            .parse_program(None, s)
            .unwrap()
            .into_iter()
            .filter_map(|c| {
                if let Command::Rule { rule } = c {
                    Some(rule)
                } else {
                    None
                }
            })
            .collect()
    }
    fn run(seed: i64, body: &str) -> Value {
        let mut p = Program::default();
        let env = BTreeMap::from([("x".into(), expr("(RipenInput 0)"))]);
        let seeds = egglog::EGraph::default()
            .parse_program(None, &format!("(set (Expr-size (RipenInput 0)) {seed})"))
            .unwrap();
        for (i, r) in rules(body).iter().enumerate() {
            p.observe(r, &env, if i == 0 { &seeds } else { &[] });
        }
        p.report()
    }
    #[test]
    fn bounds_merge_and_preserve_two_sides() {
        let d = run(
            8,
            "(rule ((> (Expr-size x) 6)) ()) (rule ((< (Expr-size x) 10)) ())",
        );
        let b = &d["template"]["conditions"]["bounds"][0];
        assert_eq!(b["lower_inclusive"], 7);
        assert_eq!(b["upper_inclusive"], 9);
        assert_eq!(d["template"]["steps"].as_array().unwrap().len(), 2);
        let d = run(
            20,
            "(rule ((> (Expr-size x) 6)) ()) (rule ((> (Expr-size x) 10)) ())",
        );
        assert_eq!(
            d["template"]["conditions"]["bounds"][0]["lower_inclusive"],
            11
        );
    }
    #[test]
    fn observed_values_are_not_template_identity() {
        let rule = "(rule ((> (Expr-size x) 6)) ())";
        let a = run(25, rule);
        let b = run(27, rule);
        assert_eq!(a["template"], b["template"]);
        assert_ne!(a["instance_inputs"], b["instance_inputs"]);
    }
    #[test]
    fn min_and_arithmetic_survive_stage_update() {
        let d = run(
            20,
            "(rule ((= n (Expr-size x))) ((set (Expr-size x) (- n 1))) :ruleset a) (rule ((< (Expr-size x) 10)) () :ruleset b)",
        );
        assert_eq!(d["complete"], true);
        let e = &d["template"]["conditions"]["bounds"][0]["expression"];
        assert_eq!(e["kind"], "Compute");
        assert_eq!(e["value"][0], "min");
        assert_eq!(d["template"]["steps"][0]["ruleset"], "a");
        assert_eq!(d["template"]["steps"][1]["ruleset"], "b");
        assert!(
            e["value"][1]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["value"][0] == "-")
        );
    }
    #[test]
    fn equal_numbers_in_different_fields_remain_independent() {
        let mut p = Program::default();
        let env = BTreeMap::from([
            ("x".into(), expr("(RipenInput 0)")),
            ("y".into(), expr("(RipenInput 1)")),
        ]);
        let seeds = egglog::EGraph::default()
            .parse_program(
                None,
                "(set (Expr-size (RipenInput 0)) 25) (set (Expr-size (RipenInput 1)) 25)",
            )
            .unwrap();
        p.observe(
            &rules("(rule ((> (Expr-size x) 6) (< (Expr-size y) 10)) ())")[0],
            &env,
            &seeds,
        );
        let d = p.report();
        assert_eq!(d["template"]["parameters"].as_array().unwrap().len(), 2);
        assert_eq!(
            d["template"]["conditions"]["bounds"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(d["template"]["conditions"]["impossible"], false);
    }
    #[test]
    fn contradictory_and_extreme_bounds_are_exact() {
        for rule in [
            "(rule ((> (Expr-size x) 10) (< (Expr-size x) 10)) ())",
            "(rule ((> (Expr-size x) 9223372036854775807)) ())",
        ] {
            assert_eq!(run(25, rule)["template"]["conditions"]["impossible"], true);
        }
    }
    #[test]
    fn unsupported_merge_is_not_certified() {
        let d = egglog::EGraph::default()
            .parse_program(None, "(function Expr-size (E) i64 :merge (max old new))")
            .unwrap();
        assert_eq!(Program::new(&d).report()["complete"], false);
    }
}

fn syntax(e: &Expr) -> Value {
    match e {
        Expr::Var(_, v) => json!({"variable":v}),
        Expr::Lit(_, l) => json!({"literal":l.to_string()}),
        Expr::Call(_, op, args) => {
            json!({"operator":op,"arguments":args.iter().map(syntax).collect::<Vec<_>>()})
        }
    }
}
