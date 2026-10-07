//! Extract an executable symbolic interface, not a full concrete tier0 snapshot.
use super::super::*;
#[path = "ripen_scalar.rs"]
mod scalar;
#[path = "ripen_symbolic_egg.rs"]
mod symbolic_egg;

#[cfg(test)]
thread_local! { static LEGACY_SET_RESULT_CHECK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

fn subst(e: &Expr, env: &BTreeMap<String, Expr>) -> Result<Expr> {
    Ok(match e {
        Expr::Var(_, v) => env
            .get(v)
            .cloned()
            .ok_or_else(|| format!("unresolved binding {v}"))?,
        Expr::Lit(..) => e.clone(),
        Expr::Call(_, op, args) => call(
            op,
            args.iter()
                .map(|e| subst(e, env))
                .collect::<Result<Vec<_>>>()?,
        ),
    })
}
// LHS is a conjunction, not a sequence of let definitions. The native
// planner may omit a binding recoverable from an equation that appears later.
fn resolve_lhs_bindings(body: &[Fact], env: &mut BTreeMap<String, Expr>) -> Result {
    let mut definitions = vec![];
    for fact in body {
        if let Fact::Eq(_, a, b) = fact {
            if let Expr::Var(_, name) = a {
                definitions.push((name, b));
            }
            if let Expr::Var(_, name) = b {
                definitions.push((name, a));
            }
        }
    }
    loop {
        let mut changed = false;
        for (name, rhs) in &definitions {
            if !env.contains_key(*name) {
                if let Ok(value) = subst(rhs, env) {
                    env.insert((*name).clone(), value);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    // No guessed holes for unresolved/cyclic variables. The original LHS
    // checks below still validate every equality, including alternate definitions.
    for (name, _) in definitions {
        if !env.contains_key(name) {
            return Err(format!("unresolved binding {name}").into());
        }
    }
    Ok(())
}

fn check(f: Fact) -> Command {
    Command::Check(sp(), vec![f])
}
fn eq(a: Expr, b: Expr) -> Command {
    check(Fact::Eq(sp(), a, b))
}
// Double fail runs a positive check at this exact stage; one fail tests absence.
fn immediate(command: Command) -> Command {
    Command::Fail(sp(), Box::new(Command::Fail(sp(), Box::new(command))))
}
fn literal(c: &Captured, id: usize) -> Result<Expr> {
    let text = c
        .pool
        .literals
        .get(&id)
        .ok_or("set witness needs a scalar literal or an existing symbolic binding")?;
    let mut eg = super::measured_engine();
    let commands = eg.parse_program(None, &format!("(let __old_value {text})"))?;
    let [Command::Action(Action::Let(_, _, value @ Expr::Lit(..)))] = commands.as_slice() else {
        return Err("invalid set witness literal".into());
    };
    Ok(value.clone())
}
// An observed external function row has an output as well as its key.
// Equality-sort outputs are legitimate boundary slots, not missing literals.
fn external_value(
    c: &Captured,
    id: usize,
    values: &mut BTreeMap<usize, Expr>,
    parameters: &mut Vec<Json>,
    entry: &mut Vec<Command>,
    holes: &BTreeMap<String, String>,
) -> Result<Expr> {
    if let Some(value) = values.get(&id) {
        return Ok(value.clone());
    }
    if c.pool.literals.contains_key(&id) {
        return literal(c, id);
    }
    let sort = c.pool.values[id].sort.as_ref();
    let Some(hole) = holes.get(sort) else {
        return Err(format!("external output sort {sort} needs a concrete payload").into());
    };
    let slot = parameters.len();
    let value = call(hole, vec![num(slot as u64)]);
    entry.push(Command::Action(Action::Expr(sp(), value.clone())));
    parameters.push(json!({"name":format!("ripen_p{slot}"),"token":id,"sort":sort,"semantics":"observed external row output"}));
    values.insert(id, value.clone());
    Ok(value)
}

fn outside(r: &Record, p: &Port, members: &BTreeSet<usize>) -> bool {
    match p {
        Port::External(_) => true,
        Port::Parent(i, _) => !members.contains(&r.parents[*i]),
    }
}

pub(super) struct Environment {
    pub base: EGraph,
    pub export_error: Option<String>,
    pub rules: Vec<RuleInfo>,
    pub code: String,
    source: String,
    hole: String,
    holes: BTreeMap<String, String>,
    constructors: BTreeSet<String>,
    commands: Vec<Command>,
    table_names: Vec<String>,
}
impl Environment {
    fn new(c: &Captured) -> Result<Self> {
        audit(c)?;
        let mut hole = "RipenInput".to_string();
        while c.preview_source.contains(&hole) {
            hole.push('_');
        }
        let mut parser = super::measured_engine();
        let mut datatypes = parser.parse_program(None, &c.datatype)?;
        let mut holes = BTreeMap::new();
        for (i, sort) in c.datatype_names.iter().enumerate() {
            let name = if c.datatype_names.len() == 1 {
                hole.clone()
            } else {
                format!("{hole}_{i}_")
            };
            datatypes
                .extend(parser.parse_program(None, &format!("(constructor {name} (i64) {sort})"))?);
            holes.insert(sort.clone(), name);
        }
        let constructors: BTreeSet<String> = sorts::constructors(&datatypes)
            .into_iter()
            .map(|(_, v)| v.name)
            .collect();
        // Do not activate declared-but-unscheduled maintenance/debug rules.
        fn scheduled(s: &egglog::ast::Schedule, out: &mut BTreeSet<String>) {
            use egglog::ast::GenericSchedule::*;
            match s {
                Run(_, config) => {
                    out.insert(config.ruleset.clone());
                }
                Sequence(_, xs) => {
                    for x in xs {
                        scheduled(x, out);
                    }
                }
                Repeat(_, _, x) | Saturate(_, x) => scheduled(x, out),
            }
        }
        let mut active = BTreeSet::new();
        let mut has_schedule = false;
        let mut combined = BTreeMap::new();
        for cmd in parser.parse_program(None, &c.preview_source)? {
            match cmd {
                Command::RunSchedule(s) => {
                    has_schedule = true;
                    scheduled(&s, &mut active);
                }
                Command::UnstableCombinedRuleset(_, name, members) => {
                    combined.insert(name, members);
                }
                _ => {}
            }
        }
        let mut pending: Vec<_> = active.iter().cloned().collect();
        while let Some(name) = pending.pop() {
            if let Some(members) = combined.get(&name) {
                for member in members {
                    if active.insert(member.clone()) {
                        pending.push(member.clone());
                    }
                }
            }
        }
        let local_rules: Vec<_> = c
            .rules
            .iter()
            .filter(|r| {
                !r.rule.body.is_empty() && (!has_schedule || active.contains(&r.rule.ruleset))
            })
            .cloned()
            .collect();
        let mut entry = datatypes.clone();
        let rulesets: BTreeSet<_> = c
            .rules
            .iter()
            .map(|r| r.rule.ruleset.clone())
            .filter(|r| !r.is_empty())
            .collect();
        for ruleset in rulesets {
            entry.push(Command::AddRuleset(sp(), ruleset));
        }
        entry.extend(local_rules.iter().map(|r| Command::Rule {
            rule: r.rule.clone(),
        }));
        let code = entry
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let mut base = super::measured_engine();
        base.run_program(entry.clone())?;
        Ok(Self {
            export_error: super::export_schema(c, true).err().map(|e| e.to_string()),
            base,
            rules: local_rules,
            code,
            source: c.preview_source.clone(),
            hole,
            holes,
            constructors,
            table_names: sorts::constructors(&datatypes)
                .into_iter()
                .map(|(_, v)| v.name)
                .chain(datatypes.iter().filter_map(|c| match c {
                    Command::Relation { name, .. } | Command::Function { name, .. } => {
                        Some(name.clone())
                    }
                    _ => None,
                }))
                .collect(),
            commands: entry,
        })
    }
}
#[derive(Default)]
pub(in crate::native_analyze) struct PreparationCache {
    pub(super) prepared_commands: std::collections::HashMap<String, Vec<Command>>,
    pub(super) environment: Option<Environment>,
    validated: std::collections::HashMap<String, (Json, Json)>,
    checks: std::collections::HashMap<String, egglog::CompiledCheck>,
    check_compilations: usize,
    check_runs: usize,
    hits: usize,
    validations: usize,
    environments: usize,
    timings: BTreeMap<&'static str, f64>,
}
impl PreparationCache {
    pub fn stats(&self) -> Json {
        json!({"seconds":self.timings,"check_compilations":self.check_compilations,"check_runs":self.check_runs,"environment_compilations":self.environments,"validation_runs":self.validations,"validation_cache_hits":self.hits,"validated_compositions":self.validated.len()})
    }
    pub fn prepare(&mut self, c: &Captured, members: &[usize]) -> Result<(String, String, Json)> {
        if members.is_empty()
            || members.windows(2).any(|p| p[0] >= p[1])
            || members.last().unwrap() >= &c.records.len()
        {
            return Err("invalid CS composition members".into());
        }
        if self
            .environment
            .as_ref()
            .is_none_or(|e| e.source != c.preview_source)
        {
            let tick = std::time::Instant::now();
            self.environment = Some(Environment::new(c)?);
            *self.timings.entry("environment_setup").or_default() += tick.elapsed().as_secs_f64();
            self.validated.clear();
            self.prepared_commands.clear();
            self.checks.clear();
            self.environments += 1;
        }
        let tick = std::time::Instant::now();
        let result = extract(c, members, *members.last().unwrap(), self);
        *self.timings.entry("prepare_total").or_default() += tick.elapsed().as_secs_f64();
        result
    }
}

fn extract(
    c: &Captured,
    member_ids: &[usize],
    root_member: usize,
    cache: &mut PreparationCache,
) -> Result<(String, String, Json)> {
    macro_rules! elapsed {
        ($name:expr,$tick:expr) => {
            *cache.timings.entry($name).or_default() += $tick.elapsed().as_secs_f64()
        };
    }
    let build_tick = std::time::Instant::now();
    let members: BTreeSet<_> = member_ids.iter().copied().collect();
    if members.is_empty() {
        return Err("empty coarse entry".into());
    }
    let environment = cache.environment.as_mut().unwrap();
    let hole = environment.hole.clone();
    let table_names = environment.table_names.clone();
    let constructors = &environment.constructors;
    let mut entry = environment.commands.clone();
    let environment_len = entry.len();
    let mut parser = super::measured_engine();
    let mut values = BTreeMap::new();
    let mut parameters = vec![];
    let mut stages = vec![];
    let mut final_checks = vec![];
    let mut injections = vec![];
    let mut written = BTreeSet::new();
    // Commit delivery can differ from match/action order for independent keys.
    // Preserve order for syntactically identical targets. Distinct expressions
    // may still alias after union: executable before/after guards below check
    // the actual replay state, rather than assuming unequal strings are disjoint.
    let mut last_commit_by_target = BTreeMap::new();
    let mut update_evidence = vec![];
    let mut scalar_program = scalar::Program::new(&environment.commands);
    for i in &members {
        let r = &c.records[*i];
        let rule = &c.rules[r.rule];
        // Allocate external parameters at their first use, after earlier selected
        // applications have supplied their outputs. A later external port must
        // not pre-allocate a hole for an internally produced value.
        for ((input, p), v) in r.inputs.iter().zip(&r.ports).zip(&r.wanted) {
            if !outside(r, p, &members) {
                continue;
            }
            match input {
                Input::Var(_) => {
                    if values.contains_key(v) {
                        continue;
                    }
                    if let Some(literal) = c.pool.literals.get(v) {
                        let commands = parser
                            .parse_program(None, &format!("(let __recorded_literal {literal})"))?;
                        let [Command::Action(Action::Let(_, _, term @ Expr::Lit(..)))] =
                            commands.as_slice()
                        else {
                            return Err("invalid recorded scalar literal".into());
                        };
                        values.insert(*v, term.clone());
                        continue;
                    }
                    if !c.datatype_names.contains(c.pool.values[*v].sort.as_ref()) {
                        return Err(format!("boundary token {v} has scalar sort {}; its literal is not recoverable from this history",c.pool.values[*v].sort).into());
                    }
                    let name = format!("ripen_p{}", parameters.len());
                    let sort = c.pool.values[*v].sort.as_ref();
                    let term = call(&environment.holes[sort], vec![num(parameters.len() as u64)]);
                    // Terms, not global variable names, preserve captured alias identity.
                    entry.push(Command::Action(Action::Expr(sp(), term.clone())));
                    values.insert(*v, term);
                    parameters.push(json!({"name":name,"token":v,"sort":sort,"semantics":"opaque boundary parameter"}));
                }
                Input::Read(_) => {}
            }
        }
        let mut seeds = BTreeMap::new();
        let mut requirements = BTreeMap::new();
        let mut rhs_requirements = BTreeMap::new();
        let mut env = BTreeMap::new();
        for (input, v) in r.inputs.iter().zip(&r.wanted) {
            if let Input::Var(name) = input {
                env.insert(
                    name.clone(),
                    values.get(v).cloned().ok_or("missing internal binding")?,
                );
            }
        }
        let grounded_names: BTreeSet<_> = env.keys().cloned().collect();
        resolve_lhs_bindings(&rule.rule.body, &mut env)?;
        // Every internal row must already exist before boundary injection,
        // including dependencies reached through an LHS equation alias.
        for (input, port) in r.inputs.iter().zip(&r.ports) {
            if let Input::Read(span) = input {
                if !outside(r, port, &members) {
                    let term = subst(&rule.calls[span].1, &env)?;
                    // An existing RHS lookup can depend on an external nested
                    // constructor lookup injected below. Check it after those
                    // boundary facts, but before executing the original action.
                    let checks = if rule.calls[span].0.starts_with("head/") {
                        &mut rhs_requirements
                    } else {
                        &mut requirements
                    };
                    checks.insert(term.to_string(), eq(term.clone(), term));
                }
            }
        }
        for (input, p) in r.inputs.iter().zip(&r.ports) {
            if let Input::Read(span) = input {
                if outside(r, p, &members) {
                    let outer_path = &rule.calls.get(span).ok_or("missing read path")?.0;
                    for (nested, dependency) in r.inputs.iter().zip(&r.ports) {
                        if let Input::Read(inner) = nested {
                            if !outside(r, dependency, &members)
                                && rule.calls[inner]
                                    .0
                                    .starts_with(&format!("{outer_path}/args/"))
                            {
                                let expr = subst(&rule.calls[inner].1, &env)?;
                                requirements.insert(expr.to_string(), eq(expr.clone(), expr));
                            }
                        }
                    }
                    // Only bindings actually used by this external fact matter.
                    // Their symbolic terms must already exist before injection.
                    fn variables(e: &Expr, out: &mut BTreeSet<String>) {
                        match e {
                            Expr::Var(_, v) => {
                                out.insert(v.clone());
                            }
                            Expr::Call(_, _, args) => {
                                for a in args {
                                    variables(a, out);
                                }
                            }
                            _ => {}
                        }
                    }
                    let mut needed = BTreeSet::new();
                    variables(&rule.calls[span].1, &mut needed);
                    for name in needed {
                        // An equation-derived variable may name an external row
                        // being injected now. Requiring it earlier is circular.
                        if !grounded_names.contains(&name) {
                            continue;
                        }
                        let expr = env
                            .get(&name)
                            .ok_or("missing external fact binding")?
                            .clone();
                        requirements.insert(expr.to_string(), eq(expr.clone(), expr));
                    }
                    let expr = subst(
                        &rule.calls.get(span).ok_or("missing read expression")?.1,
                        &env,
                    )?;
                    let Expr::Call(_, op, args) = &expr else {
                        return Err("external read is not a table call".into());
                    };
                    let seed =
                        if environment.base.get_function(op).is_some_and(|f| {
                            !f.is_constructor() && f.schema().output.name() != "Unit"
                        }) {
                            let index = r
                                .inputs
                                .iter()
                                .filter(|i| matches!(i, Input::Read(_)))
                                .position(|i| matches!(i,Input::Read(s) if s==span))
                                .ok_or("missing read index")?;
                            let id = r
                                .read_results
                                .get(index)
                                .copied()
                                .flatten()
                                .ok_or("missing function read result")?;
                            let value = external_value(
                                c,
                                id,
                                &mut values,
                                &mut parameters,
                                &mut entry,
                                &environment.holes,
                            )?;
                            Command::Action(Action::Set(sp(), op.clone(), args.clone(), value))
                        } else {
                            Command::Action(Action::Expr(sp(), expr.clone()))
                        };
                    if !written.contains(&expr.to_string()) {
                        seeds.insert(expr.to_string(), seed);
                    }
                }
            }
        }
        // Preserve alias constraints carried by the committed external read row,
        // including nested constructor reads absent from top-level LHS equalities.
        // Match-event substitutions alone omit these row-result identities.
        for (read, (input, port)) in r
            .inputs
            .iter()
            .zip(&r.ports)
            .filter(|(input, _)| matches!(input, Input::Read(_)))
            .enumerate()
        {
            let Input::Read(span) = input else {
                unreachable!()
            };
            if !outside(r, port, &members) {
                continue;
            }
            let Some(id) = r.read_results.get(read).copied().flatten() else {
                continue;
            };
            if !c.datatype_names.contains(c.pool.values[id].sort.as_ref()) {
                continue;
            }
            let term = subst(&rule.calls[span].1, &env)?;
            if let Some(previous) = values.get(&id) {
                if previous != &term {
                    let command = Command::Action(Action::Union(sp(), previous.clone(), term));
                    seeds.insert(format!("~{}", command), command);
                }
            } else {
                values.insert(id, term);
            }
        }
        // An external constructor fact includes its output binding/equality,
        // not merely the existence of its input-key row. These are explicit
        // coarse assumptions from the original LHS, never inferred from a failed
        // output-alias check. Internal facts remain check-only.
        for f in &rule.rule.body {
            if let Fact::Eq(_, a, b) = f {
                let external_side = |e: &Expr| {
                    r.inputs.iter().zip(&r.ports).any(|(input,p)| {
                    matches!(input, Input::Read(span) if outside(r,p,&members) && &rule.calls[span].1 == e)
                })
                };
                if external_side(a) || external_side(b) {
                    let a = subst(a, &env)?;
                    let b = subst(b, &env)?;
                    let math =
                        |e: &Expr| matches!(e,Expr::Call(_,op,_) if constructors.contains(op));
                    if math(&a) && math(&b) && a != b {
                        let command = Command::Action(Action::Union(sp(), a, b));
                        // Equality follows row materialization at this stage.
                        seeds.insert(format!("~{}", command), command);
                    }
                }
            }
        }
        let mut set_guards = BTreeMap::new();
        let mut guard_env = env.clone();
        for (action_index, action) in rule.rule.head.0.iter().enumerate() {
            if let Action::Let(_, name, expr) = action {
                guard_env.insert(name.clone(), subst(expr, &guard_env)?);
            }
            let Action::Set(_, op, args, proposed_expr) = action else {
                continue;
            };
            let path = format!("head/{action_index}/set/0");
            let mut witnesses = r.sets.iter().filter(|w| w.path == path);
            let w = witnesses
                .next()
                .ok_or("set requires a committed before/after witness; recapture old history")?;
            if witnesses.next().is_some() {
                return Err(
                    "multiple commits for one set action require finer stage replay".into(),
                );
            }
            if w.table != *op
                || w.after.len() != args.len() + 1
                || w.proposed.len() != args.len() + 1
            {
                return Err("set witness schema mismatch".into());
            }
            let args = args
                .iter()
                .map(|a| subst(a, &guard_env))
                .collect::<Result<Vec<_>>>()?;
            let target = call(op, args.clone());
            let key = target.to_string();
            if last_commit_by_target
                .get(&key)
                .is_some_and(|last| w.commit <= *last)
            {
                return Err("set commits interleave match order for one target; stage replay cannot reorder them".into());
            }
            last_commit_by_target.insert(key.clone(), w.commit);
            let before = match &w.before {
                PreviousRow::Unknown => {
                    return Err("unknown pre-update row; recapture with commit witnesses".into());
                }
                PreviousRow::Absent => {
                    Command::Fail(sp(), Box::new(eq(target.clone(), target.clone())))
                }
                PreviousRow::Present(row) => {
                    let id = *row.last().ok_or("empty old row")?;
                    let old = if !written.contains(&key) {
                        external_value(
                            c,
                            id,
                            &mut values,
                            &mut parameters,
                            &mut entry,
                            &environment.holes,
                        )?
                    } else {
                        values
                            .get(&id)
                            .cloned()
                            .map(Ok)
                            .unwrap_or_else(|| literal(c, id))?
                    };
                    if !written.contains(&key) && !seeds.contains_key(&key) {
                        seeds.insert(
                            key.clone(),
                            Command::Action(Action::Set(
                                sp(),
                                op.clone(),
                                args.clone(),
                                old.clone(),
                            )),
                        );
                    }
                    immediate(eq(target.clone(), old))
                }
            };
            let after_id = *w.after.last().unwrap();
            let after = if let Some(value) = values.get(&after_id) {
                value.clone()
            } else if c.pool.literals.contains_key(&after_id) {
                literal(c, after_id)?
            } else if w.proposed.last() == Some(&after_id) {
                #[cfg(test)]
                if LEGACY_SET_RESULT_CHECK.get() {
                    literal(c, after_id)?;
                }
                // The committed result is exactly the proposed value. Rebuild
                // constructor-only RHS terms rather than treating a new equality
                // value as a missing scalar literal. Mutable lookups and fresh
                // primitives must not be re-evaluated by the post-action check.
                fn constructor_term(e: &Expr, constructors: &BTreeSet<String>) -> bool {
                    match e {
                        Expr::Lit(..) => true,
                        Expr::Call(_, op, args) => {
                            constructors.contains(op)
                                && args.iter().all(|e| constructor_term(e, constructors))
                        }
                        _ => false,
                    }
                }
                let term = subst(proposed_expr, &guard_env)?;
                if !constructor_term(&term, &constructors) {
                    return Err(
                        "new set result requires a stable constructor RHS or recorded binding"
                            .into(),
                    );
                }
                term
            } else {
                return Err(
                    "merged set result differs from proposal and has no recorded symbolic binding"
                        .into(),
                );
            };
            update_evidence.push(json!({"record":i,"action":action_index,"before":w.before,"commit":w.commit,"table":op}));
            written.insert(key);
            set_guards.insert(action_index, (before, immediate(eq(target, after))));
        }
        let mut commands = requirements.into_values().collect::<Vec<_>>();
        let injection_start = commands.len();
        let facts = seeds
            .into_values()
            .flat_map(|seed| {
                let mut commands = vec![];
                if let Command::Action(Action::Set(_, op, args, _)) = &seed {
                    let target = call(op, args.clone());
                    commands.push(Command::Fail(sp(), Box::new(eq(target.clone(), target))));
                }
                commands.push(seed);
                commands
            })
            .collect::<Vec<_>>();
        if !facts.is_empty() {
            injections.push(json!({"before_record":i,"requires":commands.iter().map(ToString::to_string).collect::<Vec<_>>(),"facts":facts.iter().map(ToString::to_string).collect::<Vec<_>>()}));
        }
        scalar_program.observe(&rule.rule, &env, &facts);
        commands.extend(facts);
        let lhs_start = commands.len();
        commands.extend(rhs_requirements.into_values());
        for fact in &rule.rule.body {
            commands.push(check(match fact {
                Fact::Eq(_, a, b) => Fact::Eq(sp(), subst(a, &env)?, subst(b, &env)?),
                Fact::Fact(e) => Fact::Fact(subst(e, &env)?),
            }));
        }
        let preconditions = commands.len();
        for (action_index, action) in rule.rule.head.0.iter().enumerate() {
            let action = match action {
                Action::Let(_, n, e) => {
                    let v = subst(e, &env)?;
                    env.insert(n.clone(), v.clone());
                    Action::Expr(sp(), v)
                }
                Action::Union(_, a, b) => Action::Union(sp(), subst(a, &env)?, subst(b, &env)?),
                Action::Expr(_, e) => Action::Expr(sp(), subst(e, &env)?),
                Action::Set(_, op, args, value) => Action::Set(
                    sp(),
                    op.clone(),
                    args.iter()
                        .map(|e| subst(e, &env))
                        .collect::<Result<Vec<_>>>()?,
                    subst(value, &env)?,
                ),
                _ => return Err("unsupported non-monotone CS action".into()),
            };
            final_checks.push(match &action {
                Action::Union(_, a, b) => eq(a.clone(), b.clone()),
                Action::Expr(_, e) => eq(e.clone(), e.clone()),
                Action::Set(_, op, args, _) => {
                    let e = call(op, args.clone());
                    eq(e.clone(), e)
                }
                _ => unreachable!(),
            });
            if let Some((before, _)) = set_guards.get(&action_index) {
                commands.push(before.clone());
            }
            commands.push(Command::Action(action));
            if let Some((_, after)) = set_guards.get(&action_index) {
                commands.push(after.clone());
            }
        }
        let alias_start = commands.len();
        for (output, v) in r.outputs.iter().zip(&r.values) {
            let term = match output {
                Output::Var(n) => env.get(n).cloned().ok_or("missing output binding")?,
                Output::Column(span, column) => {
                    let e = &rule.calls.get(span).ok_or("missing output call")?.1;
                    let Expr::Call(_, _, args) = e else {
                        return Err("output is not a constructor call".into());
                    };
                    // A set result is a value at its commit, not a lookup of
                    // the mutable function after subsequent actions. The exact
                    // after-value was already checked next to that set above.
                    if rule.calls[span].0.ends_with("/set/0")
                        && *column == args.len()
                        && c.pool.literals.contains_key(v)
                    {
                        literal(c, *v)?
                    } else {
                        subst(
                            if *column == args.len() {
                                e
                            } else {
                                args.get(*column).ok_or("invalid output column")?
                            },
                            &env,
                        )?
                    }
                }
                Output::Row(_) => continue,
            };
            if c.pool.literals.contains_key(v) {
                let value = literal(c, *v)?;
                commands.push(eq(value.clone(), term));
                values.insert(*v, value);
            } else if let Some(old) = values.get(v) {
                // Check recorded aliases; never manufacture the missing union.
                commands.push(eq(old.clone(), term));
            } else {
                values.insert(*v, term);
            }
        }
        stages.push((
            *i,
            r.id,
            preconditions,
            commands,
            [injection_start, lhs_start, alias_start],
        ));
    }
    elapsed!("construct_facts_bindings", build_tick);
    let key_tick = std::time::Instant::now();
    let encode = |xs: &[Command]| {
        xs.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let validation = format!(
        "{}\n{}\n",
        encode(&entry),
        stages
            .iter()
            .map(|(_, _, _, s, _)| encode(s))
            .collect::<Vec<_>>()
            .join("\n")
    );
    // Canonical relative ports and aliases distinguish identical rule names
    // applied at different positions. Include the complete generated assumptions
    // and actions as a conservative final discriminator before any replay.
    let slots: BTreeMap<_, _> = member_ids
        .iter()
        .enumerate()
        .map(|(slot, id)| (*id, slot))
        .collect();
    let mut symbols = BTreeMap::new();
    let mut sym = |id: usize| {
        let next = symbols.len();
        let n = *symbols.entry(id).or_insert(next);
        json!([n, c.pool.values[id].sort, c.pool.literals.get(&id)])
    };
    let signature:Vec<_>=member_ids.iter().map(|i| {
        let r=&c.records[*i];
        let wiring:Vec<_>=r.ports.iter().zip(&r.wanted).map(|(p,v)|match p {
            Port::Parent(parent,output) if slots.contains_key(&r.parents[*parent]) => json!(["local",slots[&r.parents[*parent]],output]),
            _=>json!(["external",sym(*v)]),
        }).collect();
        let apply=&c.applies[*i];
        json!({"rule":r.rule,"input_roles":apply.input_roles,"output_roles":apply.output_roles,"wiring":wiring,"inputs":r.wanted.iter().map(|v|sym(*v)).collect::<Vec<_>>(),"outputs":r.values.iter().map(|v|sym(*v)).collect::<Vec<_>>(),"read_results":r.read_results.iter().map(|v|v.map(&mut sym)).collect::<Vec<_>>(),"unions":r.unions.iter().map(|(a,b)|json!([sym(*a),sym(*b)])).collect::<Vec<_>>()})
    }).collect();
    let key = serde_json::to_string(&(signature, &validation))?;
    elapsed!("encode_key", key_tick);
    let (initial_tables, original_tables) = if let Some(tables) = cache.validated.get(&key) {
        cache.hits += 1;
        tables.clone()
    } else {
        cache.validations += 1;
        let tick = std::time::Instant::now();
        let mut plans = vec![];
        for (i, _, preconditions, commands, [injection_start, lhs_start, alias_start]) in &stages {
            let mut groups = vec![];
            for (phase, lo, hi, is_check) in [
                (
                    "internal fact before external injection",
                    0,
                    *injection_start,
                    true,
                ),
                (
                    "external fact injection",
                    *injection_start,
                    *lhs_start,
                    false,
                ),
                ("LHS fact", *lhs_start, *preconditions, true),
                ("rule action", *preconditions, *alias_start, false),
                ("recorded output alias", *alias_start, commands.len(), true),
            ] {
                if lo == hi {
                    continue;
                }
                let key = if is_check {
                    let facts = commands[lo..hi]
                        .iter()
                        .map(|cmd| match cmd {
                            Command::Check(_, facts) => Ok(facts.clone()),
                            _ => Err("action crossed a check-stage boundary".into()),
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>();
                    let key = facts
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !cache.checks.contains_key(&key) {
                        let compiled = environment
                            .base
                            .compile_ground_check(&facts)
                            .map_err(|e| format!("CS member {i}: {phase} compile failed: {e}"))?;
                        cache.checks.insert(key.clone(), compiled);
                        cache.check_compilations += 1;
                    }
                    Some(key)
                } else {
                    None
                };
                groups.push((phase, lo, hi, key));
            }
            plans.push(groups);
        }
        elapsed!("compile_check_groups", tick);
        let tick = std::time::Instant::now();
        let mut eg = environment.base.clone();
        elapsed!("clone_validation_egraph", tick);
        let tick = std::time::Instant::now();
        eg.run_program(entry[environment_len..].to_vec())?;
        elapsed!("seed_parameters", tick);
        let tick = std::time::Instant::now();
        let initial_tables = super::table_counts(&eg, &table_names);
        elapsed!("table_statistics", tick);
        for ((i, _, _, commands, _), groups) in stages.iter().zip(plans) {
            for (phase, lo, hi, key) in groups {
                let tick = std::time::Instant::now();
                let result = if let Some(key) = key {
                    cache.check_runs += 1;
                    eg.run_ground_check(&cache.checks[&key])
                } else {
                    eg.run_program(commands[lo..hi].to_vec()).map(|_| ())
                };
                elapsed!(phase, tick);
                result.map_err(|e| format!("CS member {i}: {phase} failed: {e}"))?;
            }
        }
        let tick = std::time::Instant::now();
        let tables = (
            json!(initial_tables),
            json!(super::table_counts(&eg, &table_names)),
        );
        elapsed!("table_statistics", tick);
        cache.validated.insert(key, tables.clone());
        tables
    };
    let output_tick = std::time::Instant::now();
    let seed_text = encode(&entry);
    let parameter_end = entry.len();
    let mut replay_stages = vec![];
    for (record, _, _, commands, _) in &stages {
        let start = entry.len();
        entry.extend(commands.iter().cloned().map(|c| {
            if matches!(c, Command::Check(..)) {
                immediate(c)
            } else {
                c
            }
        }));
        replay_stages.push(json!({"record":record,"start":start,"end":entry.len()}));
    }
    entry.extend(final_checks);
    let text = format!(
        "; Extracted symbolic interface. Boundary constructors are opaque, not recovered original terms.\n{}\n",
        encode(&entry)
    );
    cache
        .prepared_commands
        .entry(text.clone())
        .or_insert_with(|| entry.clone());
    let slots: BTreeMap<_, _> = member_ids
        .iter()
        .enumerate()
        .map(|(m, e)| (*e, m))
        .collect();
    let mut boundary_ports = BTreeMap::new();
    let comb_members:Vec<_>=member_ids.iter().enumerate().map(|(slot,i)|{
        let o=c.layers.occurrences.get(*i);let a=&c.applies[*i];let r=&c.records[*i];
        let external_parents:Vec<_>=a.parents.iter().filter(|p|!slots.contains_key(p)).copied().collect();
        let coarse=a.parents.is_empty()||!external_parents.is_empty()||!a.external_facts.is_empty()||a.binding.iter().any(|p|matches!(p,crate::coarse_smooth::RelativeBinding::External{..}));
        let (wiring,aliases)={
            let wiring=a.binding.iter().enumerate().map(|(input,p)|match p {
                crate::coarse_smooth::RelativeBinding::ParentPort{parent,output} if slots.contains_key(&a.parents[*parent])=>json!({"Local":{"member":slots[&a.parents[*parent]],"output":output}}),
                _=>{let v=a.wanted[input];let next=boundary_ports.len();let port=*boundary_ports.entry(v).or_insert(next);json!({"Input":port})}
            }).collect::<Vec<_>>();
            let mut names=BTreeMap::new();
            let aliases=a.wanted.iter().chain(&a.outputs).map(|v|crate::coarse_smooth::symbol(*v,&mut names)).collect::<Vec<_>>();
            (json!(wiring),json!(aliases))
        };
        json!({"slot":slot,"record":i,"event":r.id,"rule_name":c.rules[r.rule].rule.name,"rule":c.rules[r.rule].rule.to_string(),
            "use_kind":if coarse{"CoarseRuleComposition"}else{"SmoothRuleComposition"},"source_kind":o.map(|o|format!("{:?}",c.layers.combs[o.comb].kind)).unwrap_or_else(||"ObservedApply".into()),
            "parents":a.parents.iter().filter_map(|p|slots.get(p).copied()).collect::<BTreeSet<_>>(),"external_parents":external_parents,
            "binding":wiring,"aliases":aliases,"input_roles":a.input_roles,"output_roles":a.output_roles,"source_coarse_layer":o.map(|o|o.coarse_layer),"source_smooth_layer":o.and_then(|o|o.smooth_layer)})
    }).collect();
    // Typed recipe uses occurrence-local parents, retaining effects and external obligations.
    let selected: Vec<_> = member_ids.iter().map(|i| c.applies[*i].clone()).collect();
    let recipe = crate::reuse_graph::library::local_recipe(&selected, member_ids)?;
    let upper_terms: Vec<_> = c.records[root_member]
        .values
        .iter()
        .zip(&c.records[root_member].outputs)
        // Public interface roles must not depend on which RHS rows happened
        // to be newly inserted; inner column ports remain in symbolic_values.
        .filter_map(|(v, role)| matches!(role, Output::Var(_)).then_some(v))
        .filter(|v| c.datatype_names.contains(c.pool.values[**v].sort.as_ref()))
        .filter_map(|v| values.get(v).map(ToString::to_string))
        .collect();
    let lower_terms: Vec<_> = parameters
        .iter()
        .filter_map(|p| p["token"].as_u64())
        .map(|v| values[&(v as usize)].to_string())
        .collect();
    let mut provenance = json!({"replay_plan":{"environment_end":environment_len,"parameter_end":parameter_end,"stages":replay_stages},"reuse_recipe":recipe,"reuse_interface_terms":{"upper":upper_terms,"lower":lower_terms},"kind":"symbolic_cs_interface","template":null,"members":member_ids,"root_member":root_member,"comb_members":comb_members,"kind_scope":"use_kind is relative to directly recorded dependencies inside this CS entry; source_kind is the original layer classification; neither minimizes alternate proof requirements",
        "events":stages.iter().map(|(i,e,n,_,_)|json!({"record":i,"event":e,"precondition_checks":n})).collect::<Vec<_>>(),
        "initial_tables":initial_tables,"original_use_tables":original_tables,"parameter_marker":hole,"parameter_markers":environment.holes,
        "symbolic_values":values.iter().map(|(token,term)|json!({"token":token,"term":term.to_string(),"sort":c.pool.values[*token].sort})).collect::<Vec<_>>(),
        "parameters":parameters,"excluded_seed_rules":c.rules.iter().filter(|r|r.rule.body.is_empty()).map(|r|r.rule.name.clone()).collect::<Vec<_>>(),"all_source_rules":c.rules.len(),"validation":"all original LHS checks and recorded output aliases passed before/after the corresponding ground actions",
        "analysis_program":scalar_program.report(),"set_update_witnesses":update_evidence,"staged_injections":injections,"initial_source":seed_text,"concrete_boundary_structure_recovered":false,
        "saturated_scope":"extracted interface only; not the complete original tier0 neighborhood"});
    let analysis = &provenance["analysis_program"];
    if analysis["instance_inputs"]
        .as_array()
        .is_some_and(|p| !p.is_empty())
    {
        match symbolic_egg::compile(&entry, analysis) {
            Ok((symbolic_text, commands, lowering)) => {
                provenance["symbolic_lowering"] = lowering;
                cache
                    .prepared_commands
                    .insert(symbolic_text.clone(), commands);
                elapsed!("encode_result_provenance", output_tick);
                return Ok((symbolic_text, validation, provenance));
            }
            Err(reason) => {
                provenance["symbolic_lowering"] = json!({"status":"unsupported","reason":reason.to_string(),"execution":"concrete replay retained"});
            }
        }
    }
    elapsed!("encode_result_provenance", output_tick);
    Ok((text, validation, provenance))
}

fn audit(c: &Captured) -> Result {
    let mut parser = super::measured_engine();
    let mut declared = vec![];
    for cmd in crate::visual_rule::surface_program(parser.parse_program(None, &c.preview_source)?) {
        if matches!(
            cmd,
            Command::BiRewrite(..)
                | Command::Include(..)
                | Command::Rewrite(_, _, true)
                | Command::Push(..)
                | Command::Pop(..)
        ) {
            return Err("source rule coverage is not supported for this entry".into());
        }
        if let Command::Rule { mut rule } = crate::visual_rule::normalize(cmd, declared.len()) {
            if rule.name.is_empty() {
                rule.name = format!("R{}", declared.len());
            }
            declared.push(rule.to_string());
        }
    }
    if declared
        != c.rules
            .iter()
            .map(|r| r.rule.to_string())
            .collect::<Vec<_>>()
    {
        return Err("recorded rules do not cover the original source rules".into());
    }
    Ok(())
}

#[cfg(test)]
fn prepare_members(c: &Captured, members: &[usize]) -> Result<(String, String, Json)> {
    PreparationCache::default().prepare(c, members)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn captured(source: &str) -> Captured {
        let mut c = capture_text_with_sink("staged.egg", source, None, None).unwrap();
        update_layers(&mut c).unwrap();
        c
    }
    #[test]
    #[ignore = "manual four-entry eggcc TLConcat dependency recapture comparison"]
    fn audit_rhs_lookup_recapture() {
        let path = std::env::var("RIPSAT_SET_AUDIT_HISTORY").unwrap();
        let mut old = history::read(Path::new(&path)).unwrap();
        update_layers(&mut old).unwrap();
        let new = captured(&old.preview_source);
        let mut report = vec![];
        for (name, c) in [("saved_history", &old), ("rhs_lookup_history", &new)] {
            let mut cache = PreparationCache::default();
            let entries: Vec<_> = c
                .records
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    c.rules[r.rule]
                        .rule
                        .to_string()
                        .contains("TLConcat tylist1 tylist2")
                })
                .take(4)
                .map(|(i, _)| i)
                .collect();
            assert_eq!(entries.len(), 4);
            let mut results = vec![];
            for id in entries {
                let mut members = BTreeSet::new();
                let mut pending = vec![id];
                while let Some(i) = pending.pop() {
                    if members.insert(i) {
                        pending.extend(c.records[i].parents.iter().copied());
                    }
                }
                let result = cache.prepare(c, &members.into_iter().collect::<Vec<_>>());
                results.push(json!({"record":id,"accepted":result.is_ok(),"reason":result.err().map(|e|e.to_string())}));
            }
            report.push(json!({"mode":name,"entries":results}));
        }
        let out = std::env::var("RIPSAT_RHS_AUDIT_OUTPUT").unwrap();
        std::fs::write(out, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }

    #[test]
    fn analysis_program_is_exported_without_concrete_size_identity() {
        let mut templates = vec![];
        for n in [25, 27] {
            let c = captured(&format!(
                r#"
                (datatype E (V) (Out E))
                (function Expr-size (E) i64 :merge (min old new))
                (ruleset work)
                (rule ((= x (V)) (= n (Expr-size x)) (> n 6) (> n 10))
                      ((set (Expr-size (Out x)) (+ n 1))) :ruleset work :name "grow")
                (set (Expr-size (V)) {n}) (run work 1)
            "#
            ));
            let id = c
                .records
                .iter()
                .position(|r| c.rules[r.rule].rule.name == "grow")
                .unwrap();
            let (program, _, origin) = prepare_members(&c, &[id]).unwrap();
            assert!(
                program.starts_with("; ripsat-symbolic-size/v1"),
                "{}",
                origin["symbolic_lowering"]
            );
            let mut symbolic = EGraph::default();
            symbolic.parse_and_run_program(None, &program).unwrap();
            let output = origin["analysis_program"]["template"]["final_fields"]
                .as_object()
                .unwrap()
                .keys()
                .find(|k| k.contains("(Out "))
                .unwrap();
            symbolic
                .parse_and_run_program(
                    None,
                    &format!("(check (= {output} (RSAdd (RSParam 0) (RSConst 1))))"),
                )
                .unwrap();
            let a = &origin["analysis_program"];
            assert_eq!(a["complete"], true, "{a}");
            assert_eq!(
                a["template"]["conditions"]["bounds"][0]["lower_inclusive"],
                11
            );
            assert_eq!(a["instance_inputs"][0]["observed"], n.to_string());
            templates.push(a["template"].clone());
        }
        assert_eq!(templates[0], templates[1]);
    }

    #[test]
    fn rhs_existing_constructor_keeps_its_boundary_equality() {
        let c = captured(
            r#"
            (datatype L (Nil) (Cons i64 L) (Cat L L))
            (datatype Ty (Wrap L))
            (datatype E (V))
            (relation HasType (E Ty))
            (ruleset normalize) (ruleset consume)
            (rule ((= q (Cat (Cons n (Nil)) tail)))
                  ((union q (Cons n tail))) :ruleset normalize)
            (rule ((= x (V)) (= expected (Wrap (Cons 1 (Cons 2 (Nil))))))
                  ((HasType x (Wrap (Cat (Cons 1 (Nil)) (Cons 2 (Nil))))))
                  :ruleset consume :name "consumer")
            (Cat (Cons 1 (Nil)) (Cons 2 (Nil)))
            (Wrap (Cons 1 (Cons 2 (Nil)))) (V)
            (run normalize 1) (run consume 1)
        "#,
        );
        let id = c
            .records
            .iter()
            .position(|r| c.rules[r.rule].rule.name == "consumer")
            .unwrap();
        let r = &c.records[id];
        assert!(r.inputs.iter().any(
            |i| matches!(i,Input::Read(span) if c.rules[r.rule].calls[span].0.starts_with("head/"))
        ));
        prepare_members(&c, &[id]).unwrap();
    }

    #[test]
    fn new_constructor_set_result_and_forged_receipt() {
        let mut c = captured(
            r#"
            (datatype E (V) (Box E))
            (function chosen (E) E :merge old)
            (rule ((= x (V))) ((set (chosen x) (Box x))))
            (V) (run 1)
        "#,
        );
        assert_eq!(c.records.len(), 1);
        prepare_members(&c, &[0]).unwrap();
        let w = &mut c.records[0].sets[0];
        // Falsify only the committed output: it must not be assumed equal to RHS.
        let wrong = w.after[0];
        *w.after.last_mut().unwrap() = wrong;
        assert!(prepare_members(&c, &[0]).is_err());
    }

    #[test]
    #[ignore = "manual existing-history preparation audit; no ripen or trace recapture"]
    fn audit_existing_equality_set_results() {
        let path = std::env::var("RIPSAT_SET_AUDIT_HISTORY").unwrap();
        let mut c = history::read(Path::new(&path)).unwrap();
        update_layers(&mut c).unwrap();
        let mut cache = PreparationCache::default();
        let mut legacy_cache = PreparationCache::default();
        let mut old_ok = 0;
        let mut rescued = vec![];
        let mut ok = vec![];
        let mut rejected = BTreeMap::<String, usize>::new();
        for (i, r) in c.records.iter().enumerate() {
            if !r.sets.iter().any(|w| {
                w.after.last().is_some_and(|v| {
                    !c.pool.literals.contains_key(v) && w.proposed.last() == Some(v)
                })
            }) {
                continue;
            }
            LEGACY_SET_RESULT_CHECK.set(true);
            let before = legacy_cache.prepare(&c, &[i]).is_ok();
            LEGACY_SET_RESULT_CHECK.set(false);
            old_ok += usize::from(before);
            match cache.prepare(&c, &[i]) {
                Ok(_) => {
                    ok.push(i);
                    if !before {
                        rescued.push(i);
                    }
                }
                Err(e) => {
                    *rejected
                        .entry(e.to_string().lines().next().unwrap_or("").to_string())
                        .or_default() += 1
                }
            }
        }
        println!(
            "{}",
            json!({"legacy_accepted":old_ok,"accepted_records":ok,"rescued_records":rescued,"rejections":rejected})
        );
    }

    #[test]
    fn multi_sort_hole_names_do_not_collide_with_native_fresh_suffixes() {
        let source = (0..25)
            .map(|i| format!("(datatype S{i:02} (V{i:02}))"))
            .collect::<Vec<_>>()
            .join("\n");
        let c = captured(&source);
        let environment = Environment::new(&c).unwrap();
        let mut eg = environment.base.clone();
        let mut facts = vec![];
        for ty in ["S02", "S22"] {
            for i in 0..4 {
                let term = call(&environment.holes[ty], vec![num(i)]);
                eg.run_program(vec![Command::Action(Action::Expr(sp(), term.clone()))])
                    .unwrap();
                facts.push(Fact::Eq(sp(), term.clone(), term));
            }
        }
        let query = eg.compile_ground_check(&facts).unwrap();
        assert!(eg.run_ground_check(&query).is_ok());
    }

    #[test]
    fn unscheduled_rules_are_not_activated_by_local_ripen() {
        let mut c = captured(
            r#"
            (datatype E (V) (G E))
            (ruleset work) (ruleset destructive)
            (rule ((= x (V))) ((G x)) :ruleset work)
            (rule ((= x (V))) ((delete (V))) :ruleset destructive)
            (V) (run work 1)
        "#,
        );
        c.preview_source = c.preview_source.replace(
            "(run work 1)",
            "(unstable-combined-ruleset joined work) (run joined 1)",
        );
        let environment = Environment::new(&c).unwrap();
        assert_eq!(environment.rules.len(), 1);
        assert_eq!(environment.rules[0].rule.ruleset, "work");
    }

    #[test]
    fn local_preparation_does_not_reinstall_zero_premise_input_generator() {
        let c = captured(
            r#"
            (datatype E (V i64) (G E))
            (ruleset seed) (ruleset work)
            (rule () ((V 42)) :ruleset seed :name "input")
            (rule ((= x (V 42))) ((G x)) :ruleset work)
            (run seed 1) (run work 1)
        "#,
        );
        let mut cache = PreparationCache::default();
        let (_, _, origin) = cache.prepare(&c, &[1]).unwrap();
        assert_eq!(origin["excluded_seed_rules"], json!(["input"]));
        assert!(
            cache
                .environment
                .as_ref()
                .unwrap()
                .rules
                .iter()
                .all(|r| !r.rule.body.is_empty())
        );
    }

    #[test]
    fn external_equality_sort_function_results_are_boundary_slots() {
        let c = captured(
            r#"
            (datatype E (V i64) (G E))
            (function F (E) E :merge old)
            (set (F (V 0)) (V 1))
            (rule ((= x (V 0))) ((set (F x) (V 2))))
            (run 1)
        "#,
        );
        assert_eq!(c.records.len(), 1);
        let (_, _, origin) = prepare_members(&c, &[0]).unwrap();
        assert!(
            origin["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["semantics"] == "observed external row output")
        );
    }

    #[test]
    fn lhs_definitions_resolve_independently_of_source_order() {
        let mut eg = EGraph::default();
        let cmds = eg.parse_program(None, "(rule ((= body (Wrap branch)) (= leaf x) (= (Pair leaf leaf) branch)) ((Wrap body)))").unwrap();
        let Command::Rule { rule } = &cmds[0] else {
            panic!()
        };
        let mut env = BTreeMap::from([("x".into(), num(3))]);
        resolve_lhs_bindings(&rule.body, &mut env).unwrap();
        assert_eq!(env["body"].to_string(), "(Wrap (Pair 3 3))");
        assert!(resolve_lhs_bindings(&rule.body, &mut BTreeMap::new()).is_err());
    }

    #[test]
    fn independent_table_commit_order_does_not_reject_replay() {
        let mut c = captured(
            r#"
            (datatype E (V i64))
            (function A (E) i64 :merge (min old new))
            (function B (E) i64 :merge (min old new))
            (rule ((= x (V 0))) ((set (A x) 7) (set (B x) 9)))
            (V 0) (run 1)
        "#,
        );
        // Cross-table commit delivery may differ from source action order.
        assert_eq!(c.records[0].sets.len(), 2);
        c.records[0].sets.iter_mut().for_each(|w| {
            w.commit = if w.table == "A" { 20 } else { 10 };
        });
        prepare_members(&c, &[0]).unwrap();
        // Reordering is not permission to accept an incorrect effect.
        let bad = c.records[0]
            .sets
            .iter()
            .find(|w| w.table == "A")
            .unwrap()
            .after
            .last()
            .copied()
            .unwrap();
        *c.records[0]
            .sets
            .iter_mut()
            .find(|w| w.table == "B")
            .unwrap()
            .after
            .last_mut()
            .unwrap() = bad;
        assert!(prepare_members(&c, &[0]).is_err());
    }

    #[test]
    fn independent_keys_in_one_table_can_replay_out_of_commit_order() {
        let mut c = captured(
            r#"
            (datatype E (V i64))
            (function A (E) i64 :merge (min old new))
            (rule ((= x (V 0)) (= y (V 1))) ((set (A x) 7) (set (A y) 5)))
            (V 0) (V 1) (run 1)
        "#,
        );
        for w in &mut c.records[0].sets {
            w.commit = if w.path == "head/0/set/0" { 20 } else { 10 };
        }
        prepare_members(&c, &[0]).unwrap();
    }

    #[test]
    fn reversed_same_table_commits_are_still_rejected() {
        let mut c = captured(
            r#"
            (datatype E (V i64))
            (function A (E) i64 :merge (min old new))
            (rule ((= x (V 0))) ((set (A x) 7) (set (A x) 5)))
            (V 0) (run 1)
        "#,
        );
        for w in &mut c.records[0].sets {
            w.commit = if w.path == "head/0/set/0" { 20 } else { 10 };
        }
        let error = prepare_members(&c, &[0]).unwrap_err().to_string();
        assert!(error.contains("for one target"), "{error}");
    }

    #[test]
    fn two_sets_in_one_apply_keep_each_committed_scalar_snapshot() {
        let c = captured(
            r#"
            (datatype E (V i64))
            (function Count (E) i64 :merge (+ old new))
            (rule ((= x (V 0))) ((set (Count x) 1) (set (Count x) 2)))
            (set (Count (V 0)) 10) (run 1)
        "#,
        );
        assert_eq!(c.records[0].sets.len(), 2);
        let (entry, _, _) = prepare_members(&c, &[0]).unwrap();
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &entry).unwrap();
    }
    #[test]
    fn set_replay_preserves_absence_old_values_and_stage_order() {
        for initial in ["", "(set (Cost (V 0)) 10)", "(set (Cost (V 0)) 3)"] {
            let mut c = captured(&format!(
                r#"
                (datatype E (V i64))
                (function Cost (E) i64 :merge (min old new))
                (ruleset first)
                (rule ((= x (V 0))) ((set (Cost x) 7)) :ruleset first)
                (V 0) {initial} (run first 1)
            "#
            ));
            assert_eq!(c.records.len(), 1);
            let w = &c.records[0].sets[0];
            assert_eq!(matches!(w.before, PreviousRow::Absent), initial.is_empty());
            let expected = if initial.ends_with("3)") { "3" } else { "7" };
            assert_eq!(c.pool.literals[w.after.last().unwrap()], expected);
            let (entry, _, _) = prepare_members(&c, &[0]).unwrap();
            let mut eg = EGraph::default();
            eg.parse_and_run_program(None, &entry).unwrap();
            if initial.ends_with("10)") {
                let PreviousRow::Present(old) = &c.records[0].sets[0].before else {
                    panic!("old row missing");
                };
                let incorrect = *old.last().unwrap();
                *c.records[0].sets[0].after.last_mut().unwrap() = incorrect;
                assert!(
                    prepare_members(&c, &[0]).is_err(),
                    "incorrect committed result was accepted"
                );
            }
        }
        let c = captured(
            r#"
            (datatype E (V i64))
            (function Cost (E) i64 :merge (min old new))
            (ruleset first) (ruleset second)
            (rule ((= x (V 0))) ((set (Cost x) 7)) :ruleset first)
            (rule ((= x (V 0)) (= (Cost x) 7)) ((set (Cost x) 5)) :ruleset second)
            (set (Cost (V 0)) 10) (run first 1) (run second 1)
        "#,
        );
        assert_eq!(c.records.len(), 2);
        let (entry, _, origin) = prepare_members(&c, &[0, 1]).unwrap();
        assert_eq!(origin["set_update_witnesses"].as_array().unwrap().len(), 2);
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &entry).unwrap();
        let path = std::env::temp_dir().join(format!("set-history-{}.json", std::process::id()));
        history::save(&path, Path::new("set.egg"), &c).unwrap();
        let mut replay = history::read(&path).unwrap();
        update_layers(&mut replay).unwrap();
        assert_eq!(prepare_members(&replay, &[0, 1]).unwrap().0, entry);
        // Old histories do not carry before-state evidence; they must not guess it.
        replay.records[0].sets.clear();
        assert!(
            prepare_members(&replay, &[0])
                .unwrap_err()
                .to_string()
                .contains("recapture")
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn subsume_receipts_have_replayable_source_positions() {
        let c = captured(
            r#"
            (datatype E (A i64) (B i64))
            (datatype T (Tag E))
            (rule ((= a (A n))) ((B n) (subsume (A n))))
            (Tag (A 2)) (run 1)
        "#,
        );
        assert!(!c.records.is_empty());
        let path = std::env::temp_dir().join(format!("subsume-replay-{}.json", std::process::id()));
        history::save(&path, Path::new("subsume.egg"), &c).unwrap();
        let replay = history::read(&path).unwrap();
        assert_eq!(replay.records.len(), c.records.len());
        assert_eq!(replay.records[0].outputs.len(), c.records[0].outputs.len());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn multiple_datatypes_preserve_union_types_and_replay() {
        for declarations in [
            "(datatype E (A i64) (B i64)) (datatype T (X E) (Y E))",
            "(sort E) (sort T) (constructor A (i64) E) (constructor B (i64) E) (constructor X (E) T) (constructor Y (E) T)",
            "(datatype* (E (A i64) (B i64)) (T (X E) (Y E)))",
        ] {
            let source = format!(
                "{declarations} (rule ((= a (A n)) (= b (B n))) ((union a b)) :name \"make-b\") (rule ((= x (X e)) (= y (Y e))) ((union x y)) :name \"make-y\") (X (A 7)) (Y (A 7)) (B 7) (run 2)"
            );
            let c = captured(&source);
            assert_eq!(c.datatype_names, BTreeSet::from(["E".into(), "T".into()]));
            let union_sorts: BTreeSet<_> = c
                .records
                .iter()
                .flat_map(|r| &r.unions)
                .map(|(a, b)| {
                    assert_eq!(c.pool.values[*a].sort, c.pool.values[*b].sort);
                    c.pool.values[*a].sort.to_string()
                })
                .collect();
            assert_eq!(union_sorts, BTreeSet::from(["E".into(), "T".into()]));
            let members: Vec<_> = (0..c.records.len()).collect();
            let (entry, _, origin) = prepare_members(&c, &members).unwrap();
            assert_eq!(origin["parameter_markers"].as_object().unwrap().len(), 2);
            let mut eg = EGraph::default();
            eg.parse_and_run_program(None, &entry).unwrap();
            let path =
                std::env::temp_dir().join(format!("multi-replay-{}.json", std::process::id()));
            history::save(&path, Path::new("multi.egg"), &c).unwrap();
            let mut replay = history::read(&path).unwrap();
            update_layers(&mut replay).unwrap();
            assert_eq!(replay.datatype_names, c.datatype_names);
            assert_eq!(prepare_members(&replay, &members).unwrap().0, entry);
            std::fs::remove_file(path).unwrap();
        }
    }
    #[test]
    fn external_parent_fact_is_injected_after_its_internal_child_exists() {
        let c = captured(
            r#"
(datatype E (V) (G E) (F E) (H E))
(ruleset first) (ruleset outside) (ruleset last)
(rule ((= x (V))) ((G x)) :ruleset first :name "producer")
(rule ((= g (G x))) ((F g)) :ruleset outside :name "external")
(rule ((= g (G x)) (= f (F g))) ((H g)) :ruleset last :name "consumer")
(V) (run first 1) (run outside 1) (run last 1)
"#,
        );
        let members: Vec<_> = c
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| c.rules[r.rule].rule.name != "external")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(members.len(), 2);
        let (source, _, origin) = prepare_members(&c, &members).unwrap();
        assert_eq!(origin["staged_injections"].as_array().unwrap().len(), 2);
        let last = origin["staged_injections"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert!(
            last["requires"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s.as_str().unwrap().contains("(G "))
        );
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &source).unwrap();
        assert_eq!(eg.get_size("H"), 1);
        let path =
            std::env::temp_dir().join(format!("staged-cs-history-{}.json", std::process::id()));
        history::save(&path, Path::new("staged.egg"), &c).unwrap();
        let mut replay = history::read(&path).unwrap();
        update_layers(&mut replay).unwrap();
        let (replayed, _, _) = prepare_members(&replay, &members).unwrap();
        assert_eq!(
            source, replayed,
            "saved row identities must survive offline replay"
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn explicit_external_lhs_equality_is_preserved_as_coarse_fact() {
        let c = captured(
            r#"
(datatype E (V) (A E) (B E) (G E))
(rule ((= x (A a)) (= x (B b))) ((G x)) :name "producer")
(union (A (V)) (B (V))) (run 1)
"#,
        );
        let (source, _, origin) = prepare_members(&c, &[0]).unwrap();
        let facts = origin["staged_injections"][0]["facts"].as_array().unwrap();
        assert!(
            facts
                .iter()
                .any(|x| x.as_str().unwrap().starts_with("(union"))
        );
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &source).unwrap();
        assert_eq!(eg.get_size("G"), 1);
    }
    #[test]
    fn preparation_cache_shares_renamed_values_but_preserves_aliases_and_environment() {
        let source = r#"
(datatype E (V i64) (Pair E E) (Out E E))
(rule ((= p (Pair a b))) ((Out a b)) :name "copy")
(Pair (V 1) (V 2)) (Pair (V 3) (V 4)) (Pair (V 5) (V 5))
(run 1)
"#;
        let c = captured(source);
        assert_eq!(c.records.len(), 3);
        let mut cache = PreparationCache::default();
        for i in 0..3 {
            let uncached = prepare_members(&c, &[i]).unwrap();
            let cached = cache.prepare(&c, &[i]).unwrap();
            assert_eq!(
                uncached, cached,
                "cache must preserve each instance's own provenance"
            );
        }
        assert_eq!(cache.stats()["environment_compilations"], 1);
        assert_eq!(cache.stats()["validation_runs"], 2);
        assert_eq!(cache.stats()["validation_cache_hits"], 1);
        let changed = captured(&source.replace("(Out a b)", "(Out b a)"));
        cache.prepare(&changed, &[0]).unwrap();
        assert_eq!(cache.stats()["environment_compilations"], 2);
    }
    #[test]
    fn preparation_cache_distinguishes_relative_binding_from_external_binding() {
        let mut c = captured(
            r#"
(datatype E (V) (G E) (H E))
(ruleset first) (ruleset last)
(rule ((= x (V))) ((G x)) :ruleset first :name "producer")
(rule ((= g (G x))) ((H g)) :ruleset last :name "consumer")
(V) (run first 1) (run last 1)
"#,
        );
        let mut cache = PreparationCache::default();
        cache.prepare(&c, &[0, 1]).unwrap();
        // Key-isolation test: the same term may be supplied through a different
        // interface. Even if replay succeeds, that interface must not hit cache.
        let slot = c.records[1]
            .inputs
            .iter()
            .zip(&c.records[1].ports)
            .position(|(i, p)| matches!(i, Input::Var(_)) && matches!(p, Port::Parent(..)))
            .unwrap();
        c.records[1].ports[slot] = Port::External(0);
        cache.prepare(&c, &[0, 1]).unwrap();
        assert_eq!(cache.stats()["validation_runs"], 2);
        assert_eq!(cache.stats()["validation_cache_hits"], 0);
    }
    #[test]
    fn preparation_cache_preserves_scalar_literals() {
        let c = captured(
            r#"
(datatype E (V i64) (Out i64))
(rule ((= v (V n))) ((Out n)) :name "scalar")
(V 1) (V 2) (run 1)
"#,
        );
        let mut cache = PreparationCache::default();
        for i in 0..c.records.len() {
            cache.prepare(&c, &[i]).unwrap();
        }
        assert_eq!(cache.stats()["validation_runs"], 2);
        assert_eq!(cache.stats()["validation_cache_hits"], 0);
    }
    #[test]
    #[ignore = "explicit recorded-entry audit; paths and records supplied by caller"]
    fn recorded_entry_audit() {
        let path = std::env::var("EGG_LAYOUT_AUDIT_HISTORY").unwrap();
        let records = std::env::var("EGG_LAYOUT_AUDIT_RECORDS").unwrap();
        let output = std::path::PathBuf::from(std::env::var("EGG_LAYOUT_AUDIT_OUTPUT").unwrap());
        assert!(!output.exists());
        std::fs::create_dir_all(&output).unwrap();
        let mut c = history::read(Path::new(&path)).unwrap();
        update_layers(&mut c).unwrap();
        let mut cache = PreparationCache::default();
        let mut rows = vec![];
        for id in records.split(',').map(|s| s.parse::<usize>().unwrap()) {
            let started = std::time::Instant::now();
            let result = cache.prepare(&c, &[id]);
            let mut row = json!({"record":id,"rule":c.records[id].rule,"preparation_seconds":started.elapsed().as_secs_f64()});
            match result {
                Ok((entry, _, origin)) => {
                    let dir = output.join(id.to_string());
                    std::fs::create_dir(&dir).unwrap();
                    let source = dir.join("entry.egg");
                    std::fs::write(&source, entry).unwrap();
                    row["prepared"] = json!(true);
                    row["origin"] = origin;
                    let mut session =
                        super::super::Session::new_cached(&source, &dir.join("ripen"), &cache);
                    match &mut session {
                        Ok(session) => {
                            let start = std::time::Instant::now();
                            let limits = crate::ripen_observation::Limits::default();
                            loop {
                                if let Err(e) = session.advance_with_observation(
                                    &limits,
                                    start.elapsed().as_secs_f64(),
                                ) {
                                    row["ripen_error"] = json!(e.to_string());
                                    break;
                                }
                                if session.done
                                    || !limits
                                        .reached(
                                            start.elapsed().as_secs_f64(),
                                            super::super::table_counts(
                                                &session.eg,
                                                &session.table_names,
                                            )
                                            .values()
                                            .sum(),
                                        )
                                        .is_empty()
                                {
                                    break;
                                }
                            }
                            row["done"] = json!(session.done);
                            row["report"] = session.report.clone();
                        }
                        Err(e) => row["ripen_error"] = json!(e.to_string()),
                    }
                }
                Err(e) => {
                    row["prepared"] = json!(false);
                    row["reason"] = json!(e.to_string());
                }
            }
            rows.push(row);
        }
        std::fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"scope":"selected recorded applications treated as local coarse entries; not full-composition coverage","rows":rows})).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "explicit math preparation audit; no ripen or saturation claims"]
    fn math_six_round_preparation_audit() {
        let source = std::fs::read_to_string("egglog/tests/math-microbenchmark.egg").unwrap();
        let mut store = cs::Store::default();
        let use_cache = std::env::var_os("EGG_LAYOUT_PREPARATION_CACHE").is_some();
        let mut cache = PreparationCache::default();
        let start = std::time::Instant::now();
        let mut jobs = vec![];
        let mut boundary = 0;
        let mut sink = |c: &mut Captured| -> Result {
            update_layers(c)?;
            boundary += 1;
            for (_, id) in store.discover(c, &c.layers, boundary)? {
                let u = &store.units[id];
                let result = if use_cache {
                    cache.prepare(c, &store.members(id))
                } else {
                    prepare_members(c, &store.members(id))
                };
                jobs.push(match result {
                    Ok((_,_,origin)) => json!({"unit":id,"members":store.members(id),"composite":u.source_composition.is_some(),"prepared":true,"injection_stages":origin["staged_injections"].as_array().unwrap().len()}),
                    Err(e) => json!({"unit":id,"members":store.members(id),"composite":u.source_composition.is_some(),"prepared":false,"reason":e.to_string()}),
                });
            }
            Ok(())
        };
        capture_text_with_sink("math-preparation-audit", &source, Some(6), Some(&mut sink))
            .unwrap();
        let path = std::path::Path::new("out/staged-cs-preparation");
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join(if use_cache {"six-rounds-cached.json"} else {"six-rounds.json"}),serde_json::to_vec_pretty(&json!({"cache_enabled":use_cache,"cache":cache.stats(),"elapsed_seconds":start.elapsed().as_secs_f64(),"rounds":6,"scope":"all registered CS compositions; preparation and replay checks only, no saturation","jobs":jobs})).unwrap()).unwrap();
        eprintln!(
            "prepared {} / {}",
            jobs.iter().filter(|j| j["prepared"] == true).count(),
            jobs.len()
        );
    }
}
