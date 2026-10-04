//! Producer-side delta markers. No full-table marker materializer; unknown
//! producers use an explicitly measured fallback over their own narrow relation.
use egg_layout::{pipeline::Result, visual_rule};
use egglog::{
    EGraph,
    ast::{
        Action, Actions, Command, Expr, Fact, GenericAction, GenericCommand, GenericExpr,
        GenericSchedule, Rule, Span,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
fn sp() -> Span {
    Span::Panic
}
fn var(s: impl Into<String>) -> Expr {
    Expr::Var(sp(), s.into())
}
fn call(s: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(sp(), s.into(), args)
}
#[derive(Clone)]
struct Schema {
    args: Vec<String>,
    out: String,
    constructor: bool,
}
fn schema(cs: &[Command]) -> BTreeMap<String, Schema> {
    let mut out = BTreeMap::new();
    for c in cs {
        match c {
            Command::Constructor { name, schema, .. } => {
                out.insert(
                    name.clone(),
                    Schema {
                        args: schema.input.clone(),
                        out: schema.output.clone(),
                        constructor: true,
                    },
                );
            }
            Command::Function { name, schema, .. } => {
                out.insert(
                    name.clone(),
                    Schema {
                        args: schema.input.clone(),
                        out: schema.output.clone(),
                        constructor: false,
                    },
                );
            }
            _ => {}
        }
    }
    out
}
fn runs(s: &egglog::ast::Schedule, out: &mut BTreeSet<String>) {
    match s {
        GenericSchedule::Run(_, r) => {
            out.insert(r.ruleset.clone());
        }
        GenericSchedule::Sequence(_, ss) => {
            for s in ss {
                runs(s, out);
            }
        }
        GenericSchedule::Repeat(_, _, s) | GenericSchedule::Saturate(_, s) => runs(s, out),
    }
}
fn contains(e: &Expr, op: &str) -> bool {
    matches!(e,Expr::Call(_,o,args) if o==op||args.iter().any(|e|contains(e,op)))
}
fn stable(
    program: &[Command],
    active: &BTreeSet<String>,
    schemas: &BTreeMap<String, Schema>,
    outer: &str,
    inner: &str,
) -> Result<Value> {
    let ty = &schemas[inner].out;
    for (name, s) in schemas {
        if !s.constructor && s.out == *ty {
            return Err(format!("function {name} may merge the decomposition sort {ty}").into());
        }
    }
    let setup: Vec<_> = program
        .iter()
        .filter(|c| {
            !matches!(
                c,
                Command::RunSchedule(..) | Command::Check(..) | Command::Extract(..)
            )
        })
        .cloned()
        .collect();
    let mut eg = EGraph::default();
    let resolved = eg.resolve_program(
        None,
        &setup
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    let mut unions = BTreeMap::<String, usize>::new();
    for cmd in resolved {
        let actions = match cmd {
            GenericCommand::Rule { rule } if active.contains(&rule.ruleset) => rule.head.0,
            GenericCommand::Action(a) => vec![a],
            _ => continue,
        };
        for action in actions {
            match action {
                GenericAction::Union(_, a, _) => {
                    let sort = match a {
                        GenericExpr::Var(_, v) => v.sort.name().to_string(),
                        GenericExpr::Call(_, op, _) => op.output().name().to_string(),
                        _ => return Err("unexpected scalar union".into()),
                    };
                    if sort == *ty {
                        return Err(format!(
                            "explicit union of {ty} invalidates stable decomposition"
                        )
                        .into());
                    }
                    *unions.entry(sort).or_default() += 1;
                }
                GenericAction::Change(_, _, op, _)
                    if op.to_string() == outer || op.to_string() == inner =>
                {
                    return Err("marker source can be deleted or subsumed".into());
                }
                GenericAction::Set(_, op, _, _)
                    if op.to_string() == outer || op.output().name() == ty =>
                {
                    return Err("set can change marker source or decomposition sort".into());
                }
                _ => {}
            }
        }
    }
    Ok(
        json!({"decomposition_sort":ty,"active_union_sorts":unions,"claim":"no explicit/functional merge of the inner result sort; constructor tags and decomposition survive congruence rebuild"}),
    )
}
struct Emitter<'a> {
    schemas: &'a BTreeMap<String, Schema>,
    outer: &'a str,
    inner: &'a str,
    slot: usize,
    good: String,
    unknown: String,
    prefix: String,
    next: usize,
    known: usize,
    other: usize,
    fallback: usize,
}
fn mentions(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Var(_, v) => v == name,
        Expr::Call(_, _, args) => args.iter().any(|a| mentions(a, name)),
        _ => false,
    }
}
fn resolve(e: &Expr, defs: &BTreeMap<String, Expr>) -> Expr {
    let mut e = e.clone();
    let mut seen = BTreeSet::new();
    while let Expr::Var(_, v) = &e {
        if !seen.insert(v.clone()) {
            break;
        }
        let Some(next) = defs.get(v) else { break };
        e = next.clone();
    }
    e
}
impl Emitter<'_> {
    fn expr(
        &mut self,
        e: &Expr,
        force: bool,
        defs: &mut BTreeMap<String, Expr>,
        out: &mut Vec<Action>,
    ) -> Expr {
        let Expr::Call(_, op, args) = e else {
            return e.clone();
        };
        let force = force || op == self.outer || op == self.inner;
        let args: Vec<_> = args
            .iter()
            .map(|e| self.expr(e, force, defs, out))
            .collect();
        let expr = call(op, args.clone());
        if !force {
            return expr;
        }
        let name = format!("{}v{}x", self.prefix, self.next);
        self.next += 1;
        out.push(Action::Let(sp(), name.clone(), expr.clone()));
        defs.insert(name.clone(), expr);
        if op == self.outer {
            let term = resolve(&args[self.slot], defs);
            match term {
                Expr::Call(_, tag, inner_args)
                    if tag == self.inner
                        && inner_args
                            .iter()
                            .all(|e| matches!(e, Expr::Var(..) | Expr::Lit(..))) =>
                {
                    let fields = args
                        .iter()
                        .enumerate()
                        .flat_map(|(i, e)| {
                            if i == self.slot {
                                inner_args.clone()
                            } else {
                                vec![e.clone()]
                            }
                        })
                        .collect();
                    out.push(Action::Expr(sp(), call(&self.good, fields)));
                    self.known += 1;
                }
                Expr::Call(_, tag, _)
                    if tag != self.inner
                        && self.schemas.get(&tag).is_some_and(|s| {
                            s.constructor && s.out == self.schemas[self.inner].out
                        }) =>
                {
                    self.other += 1
                }
                _ => {
                    out.push(Action::Expr(sp(), call(&self.unknown, args)));
                    self.fallback += 1;
                }
            }
        }
        var(name)
    }
    fn head(&mut self, rule: &mut Rule) {
        let mut defs = BTreeMap::new();
        // Only RHS-local definitions: retaining extra LHS-only variables can
        // perturb the original Join plan, even in the producer-only ablation.
        let mut out = vec![];
        for action in &rule.head.0 {
            let action = match action {
                Action::Let(_, n, e) => {
                    let e = self.expr(e, false, &mut defs, &mut out);
                    defs.retain(|_, value| !mentions(value, n));
                    defs.insert(n.clone(), e.clone());
                    Action::Let(sp(), n.clone(), e)
                }
                Action::Expr(_, e) => Action::Expr(sp(), self.expr(e, false, &mut defs, &mut out)),
                Action::Union(_, a, b) => {
                    let a = self.expr(a, false, &mut defs, &mut out);
                    let b = self.expr(b, false, &mut defs, &mut out);
                    Action::Union(sp(), a, b)
                }
                Action::Set(_, op, args, v) => {
                    let args = args
                        .iter()
                        .map(|e| self.expr(e, false, &mut defs, &mut out))
                        .collect();
                    let v = self.expr(v, false, &mut defs, &mut out);
                    Action::Set(sp(), op.clone(), args, v)
                }
                Action::Change(_, kind, op, args) => Action::Change(
                    sp(),
                    *kind,
                    op.clone(),
                    args.iter()
                        .map(|e| self.expr(e, false, &mut defs, &mut out))
                        .collect(),
                ),
                x => x.clone(),
            };
            if !matches!(&action, Action::Expr(_, Expr::Var(..))) {
                out.push(action);
            }
        }
        rule.head = Actions::new(out);
    }
}
fn before(
    s: egglog::ast::Schedule,
    consumers: &BTreeSet<String>,
    helper: &str,
) -> egglog::ast::Schedule {
    use GenericSchedule::*;
    match s {
        Run(ref span, ref r) if consumers.contains(&r.ruleset) => Sequence(
            span.clone(),
            vec![
                Run(
                    sp(),
                    egglog::ast::GenericRunConfig {
                        ruleset: helper.into(),
                        until: None,
                    },
                ),
                s,
            ],
        ),
        Sequence(s, xs) => Sequence(
            s,
            xs.into_iter()
                .map(|x| before(x, consumers, helper))
                .collect(),
        ),
        Repeat(s, n, x) => Repeat(s, n, Box::new(before(*x, consumers, helper))),
        Saturate(s, x) => Saturate(s, Box::new(before(*x, consumers, helper))),
        x => x,
    }
}
fn transform(
    program: &[Command],
    outer: &str,
    inner: &str,
    slot: usize,
    shadow: bool,
) -> Result<(Vec<Command>, Value)> {
    let schemas = schema(program);
    let o = schemas.get(outer).ok_or("outer constructor missing")?;
    let i = schemas.get(inner).ok_or("inner constructor missing")?;
    if !o.constructor || !i.constructor || o.args.get(slot) != Some(&i.out) {
        return Err("incompatible constructor fragment".into());
    }
    let mut active = BTreeSet::new();
    let mut executed = false;
    for c in program {
        match c {
            Command::RunSchedule(s) => {
                runs(s, &mut active);
                executed = true;
            }
            Command::Fail(..)
            | Command::UserDefined(..)
            | Command::Include(..)
            | Command::Push(..)
            | Command::Pop(..)
            | Command::Rewrite(..)
            | Command::BiRewrite(..) => {
                return Err("requires flat explicit rules and schedules".into());
            }
            Command::Rule { .. }
            | Command::Constructor { .. }
            | Command::Function { .. }
            | Command::Sort { .. }
            | Command::Relation { .. }
                if executed =>
            {
                return Err("late rule/schema mutation unsupported".into());
            }
            Command::Action(a) => {
                let mut found = false;
                a.clone().visit_exprs(&mut |e| {
                    found |= contains(&e, outer);
                    e
                });
                if found {
                    return Err("top-level target writes must be explicit producer rules".into());
                }
            }
            Command::Extract(_, e, _) if contains(e, outer) => {
                return Err("extract can create uncovered target rows".into());
            }
            Command::Function { merge: Some(e), .. } if contains(e, outer) => {
                return Err("merge function can create uncovered target rows".into());
            }
            _ => {}
        }
    }
    let combined: BTreeMap<_, _> = program
        .iter()
        .filter_map(|c| match c {
            Command::UnstableCombinedRuleset(_, name, members) => Some((name, members)),
            _ => None,
        })
        .collect();
    let mut todo: Vec<_> = active.iter().cloned().collect();
    while let Some(name) = todo.pop() {
        if let Some(xs) = combined.get(&name) {
            for x in *xs {
                if active.insert(x.clone()) {
                    todo.push(x.clone());
                }
            }
        }
    }
    let proof = stable(program, &active, &schemas, outer, inner)?;
    let source = program
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut prefix = "DeltaView_".to_string();
    while source.contains(&prefix) {
        prefix.push('x');
    }
    let good = format!("{prefix}known");
    let unknown = format!("{prefix}unknown");
    let helper = format!("{prefix}fallback");
    let mut emitter = Emitter {
        schemas: &schemas,
        outer,
        inner,
        slot,
        good: good.clone(),
        unknown: unknown.clone(),
        prefix: prefix.clone(),
        next: 0,
        known: 0,
        other: 0,
        fallback: 0,
    };
    let mut consumers = BTreeSet::new();
    let mut producer_stats = vec![];
    let mut changed = 0;
    let mut rewritten = vec![];
    for c in program {
        let mut c = c.clone();
        if let Command::Rule { rule } = &mut c {
            if active.contains(&rule.ruleset) {
                let counts = (emitter.known, emitter.other, emitter.fallback);
                emitter.head(rule);
                if counts != (emitter.known, emitter.other, emitter.fallback) {
                    producer_stats.push(json!({"name":rule.name,"direct_sites":emitter.known-counts.0,"disjoint_tag_sites":emitter.other-counts.1,"fallback_sites":emitter.fallback-counts.2}));
                }
                for f in &mut rule.body {
                    if let Fact::Fact(Expr::Call(_, op, args)) = f {
                        if op == outer {
                            if let Expr::Call(_, tag, inner_args) = &args[slot] {
                                if tag == inner {
                                    let fields = args
                                        .iter()
                                        .enumerate()
                                        .flat_map(|(n, e)| {
                                            if n == slot {
                                                inner_args.clone()
                                            } else {
                                                vec![e.clone()]
                                            }
                                        })
                                        .collect();
                                    if !shadow {
                                        *f = Fact::Fact(call(&good, fields));
                                    }
                                    consumers.insert(rule.ruleset.clone());
                                    changed += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        rewritten.push(c);
    }
    loop {
        let before = consumers.len();
        for (name, members) in &combined {
            if members.iter().any(|m| consumers.contains(m)) {
                consumers.insert((*name).clone());
            }
        }
        if consumers.len() == before {
            break;
        }
    }
    let mut parser = EGraph::default();
    let fields: Vec<_> = o
        .args
        .iter()
        .enumerate()
        .flat_map(|(n, t)| {
            if n == slot {
                i.args.clone()
            } else {
                vec![t.clone()]
            }
        })
        .collect();
    let mut aux = parser.parse_program(
        None,
        &format!(
            "(relation {good} ({})) (relation {unknown} ({})) (ruleset {helper})",
            fields.join(" "),
            o.args.join(" ")
        ),
    )?;
    let aa: Vec<_> = (0..o.args.len()).map(|n| var(format!("a{n}"))).collect();
    let bb: Vec<_> = (0..i.args.len()).map(|n| var(format!("b{n}"))).collect();
    let values = aa
        .iter()
        .enumerate()
        .flat_map(|(n, e)| {
            if n == slot {
                bb.clone()
            } else {
                vec![e.clone()]
            }
        })
        .collect();
    aux.push(Command::Rule {
        rule: Rule {
            span: sp(),
            name: format!("{prefix}fallback"),
            ruleset: helper.clone(),
            body: vec![
                Fact::Fact(call(&unknown, aa.clone())),
                Fact::Eq(sp(), aa[slot].clone(), call(inner, bb)),
            ],
            head: Actions::new(vec![Action::Expr(sp(), call(&good, values))]),
            naive: false,
        },
    });
    let mut out = vec![];
    let mut installed = false;
    let mut declared = BTreeSet::new();
    for mut c in rewritten {
        if let Command::Constructor { name, .. } = &c {
            declared.insert(name.clone());
        }
        if !shadow {
            if let Command::RunSchedule(s) = &mut c {
                *s = before(s.clone(), &consumers, &helper);
            }
        }
        out.push(c);
        if !installed && declared.contains(outer) && declared.contains(inner) {
            out.extend(aux.clone());
            installed = true;
        }
    }
    if !installed {
        return Err("no producer rules".into());
    }
    Ok((
        out,
        json!({"outer":outer,"inner":inner,"slot":slot,"known_marker":good,"unknown_marker":unknown,"fallback_ruleset":helper,"consumer_sites":changed,"producer_sites":producer_stats,"stability":proof,"mode":if shadow{"producer-only ablation"}else{"producer-marker consumers"},"fast_path":"marker emitted by producer RHS using already evaluated values; no materialization Join","fallback":"only unknown-producer rows joined with inner constructor before consumer rulesets","scope":"closed supplied program/schedules; stable-tag coverage checks plus mandatory marker and extracted-program validation; internal table equality is diagnostic"}),
    ))
}
fn main() -> Result {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 5 {
        return Err("SOURCE.egg TRAIN_HISTORY.json INNER OUTPUT_DIR".into());
    }
    let source = std::fs::read_to_string(&a[1])?;
    let mut eg = EGraph::default();
    let p = visual_rule::surface_program(eg.parse_program(None, &source)?);
    let out = Path::new(&a[4]);
    std::fs::create_dir(out)?;
    let (optimized, mut report) = transform(&p, "HasType", &a[3], 1, false)?;
    let (shadow, _) = transform(&p, "HasType", &a[3], 1, true)?;
    let history: Value = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    let mut evidence = BTreeMap::<String, Vec<usize>>::new();
    for (n, r) in history["records"]
        .as_array()
        .ok_or("history records missing")?
        .iter()
        .enumerate()
    {
        if !r["produced"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| history["tokens"][id.as_u64().unwrap() as usize]["sort"] == "Fact:HasType")
        {
            continue;
        }
        let rule = history["rules"][r["rule"].as_u64().unwrap() as usize]["code"]
            .as_str()
            .unwrap();
        let cmds = eg.parse_program(None, rule)?;
        if let Command::Rule { rule } = &cmds[0] {
            evidence.entry(rule.name.clone()).or_default().push(n);
        }
    }
    for r in report["producer_sites"].as_array_mut().unwrap() {
        r["training_committed_outer_records"] = json!(
            evidence
                .get(r["name"].as_str().unwrap())
                .cloned()
                .unwrap_or_default()
        );
    }
    for (name, program) in [
        ("baseline", p),
        ("producer-only", shadow),
        ("candidate", optimized),
    ] {
        std::fs::write(
            out.join(format!("{name}.egg")),
            program
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )?;
    }
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "../experiments/eggcc_cell_probe/snapshot.rs"]
mod snapshot;
#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        r#"
        (sort E) (constructor X (i64) E)
        (sort B) (constructor B0 () B) (constructor B1 () B)
        (sort T) (constructor Base (B) T) (constructor Other (B) T)
        (sort H) (constructor HasType (E T) H)
        (relation Seed (E B)) (relation Feed (E T)) (relation Out (E B))
        (ruleset emit) (ruleset read)
        (rule ((Seed x b)) ((HasType x (Base b))) :ruleset emit :name "producer")
        (rule ((Feed x t)) ((HasType x t)) :ruleset emit :name "unknown")
        (rule ((HasType x (Base b))) ((Out x b)) :ruleset read :name "consumer")
        (Seed (X 0) (B0)) (Feed (X 1) (Base (B1))) (Feed (X 2) (Other (B1)))
        (run-schedule emit read)
        (union (B0) (B1)) (run read 1)
        (check (Out (X 0) (B0)) (Out (X 1) (B0)))
    "#
        .into()
    }
    #[test]
    fn producer_markers_and_unknown_fallback_survive_child_union() {
        let mut parser = EGraph::default();
        let p = parser.parse_program(None, &fixture()).unwrap();
        let (new, report) = transform(&p, "HasType", "Base", 1, false).unwrap();
        let mut a = EGraph::default();
        let mut b = EGraph::default();
        a.run_program(p).unwrap();
        let text = new
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        b.parse_and_run_program(None, &text).unwrap();
        let ignored = BTreeSet::from([
            report["known_marker"].as_str().unwrap().into(),
            report["unknown_marker"].as_str().unwrap().into(),
        ]);
        let x =
            snapshot::snapshot_filtered(&a, &BTreeSet::new(), "test", &BTreeSet::new()).unwrap();
        let y = snapshot::snapshot_filtered(&b, &BTreeSet::new(), "test", &ignored).unwrap();
        assert!(matches!(
            egg_layout::saturated_rule_composition::compare(&x, &y).unwrap(),
            egg_layout::saturated_rule_composition::Comparison::Equivalent { .. }
        ));
        assert_eq!(b.get_size(report["known_marker"].as_str().unwrap()), 2);
    }
    #[test]
    fn arbitrary_union_of_decomposition_sort_is_rejected() {
        let text = fixture() + "\n(union (Base (B0)) (Other (B1)))";
        let mut parser = EGraph::default();
        let p = parser.parse_program(None, &text).unwrap();
        assert!(
            transform(&p, "HasType", "Base", 1, false)
                .err()
                .unwrap()
                .to_string()
                .contains("explicit union")
        );
    }
}
