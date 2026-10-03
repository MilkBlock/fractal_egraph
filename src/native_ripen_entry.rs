//! Extract an executable symbolic interface, not a full concrete tier0 snapshot.
use super::super::*;

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
fn check(f: Fact) -> Command {
    Command::Check(sp(), vec![f])
}
fn eq(a: Expr, b: Expr) -> Command {
    check(Fact::Eq(sp(), a, b))
}
fn outside(r: &Record, p: &Port, members: &BTreeSet<usize>) -> bool {
    match p {
        Port::External(_) => true,
        Port::Parent(i, _) => !members.contains(&r.parents[*i]),
    }
}

pub(super) struct Environment {
    pub base: EGraph,
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
                format!("{hole}_{i}")
            };
            datatypes
                .extend(parser.parse_program(None, &format!("(constructor {name} (i64) {sort})"))?);
            holes.insert(sort.clone(), name);
        }
        let constructors: BTreeSet<String> = sorts::constructors(&datatypes)
            .into_iter()
            .map(|(_, v)| v.name)
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
        entry.extend(c.rules.iter().map(|r| Command::Rule {
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
            base,
            rules: c.rules.clone(),
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
        let mut env = BTreeMap::new();
        for (input, v) in r.inputs.iter().zip(&r.wanted) {
            if let Input::Var(name) = input {
                env.insert(
                    name.clone(),
                    values.get(v).cloned().ok_or("missing internal binding")?,
                );
            }
        }
        for f in &rule.rule.body {
            if let Fact::Eq(_, Expr::Var(_, name), rhs) = f {
                if !env.contains_key(name) {
                    env.insert(name.clone(), subst(rhs, &env)?);
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
                    seeds.insert(expr.to_string(), Command::Action(Action::Expr(sp(), expr)));
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
        let mut commands = requirements.into_values().collect::<Vec<_>>();
        let injection_start = commands.len();
        let facts = seeds.into_values().collect::<Vec<_>>();
        if !facts.is_empty() {
            injections.push(json!({"before_record":i,"requires":commands.iter().map(ToString::to_string).collect::<Vec<_>>(),"facts":facts.iter().map(ToString::to_string).collect::<Vec<_>>()}));
        }
        commands.extend(facts);
        let lhs_start = commands.len();
        for fact in &rule.rule.body {
            commands.push(check(match fact {
                Fact::Eq(_, a, b) => Fact::Eq(sp(), subst(a, &env)?, subst(b, &env)?),
                Fact::Fact(e) => Fact::Fact(subst(e, &env)?),
            }));
        }
        let preconditions = commands.len();
        for action in &rule.rule.head.0 {
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
            commands.push(Command::Action(action));
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
                    subst(
                        if *column == args.len() {
                            e
                        } else {
                            args.get(*column).ok_or("invalid output column")?
                        },
                        &env,
                    )?
                }
                Output::Row(_) => continue,
            };
            if let Some(old) = values.get(v) {
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
    for (_, _, _, commands, _) in &stages {
        entry.extend(commands.clone());
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
    let provenance = json!({"reuse_recipe":recipe,"reuse_interface_terms":{"upper":upper_terms,"lower":lower_terms},"kind":"symbolic_cs_interface","template":null,"members":member_ids,"root_member":root_member,"comb_members":comb_members,"kind_scope":"use_kind is relative to directly recorded dependencies inside this CS entry; source_kind is the original layer classification; neither minimizes alternate proof requirements",
        "events":stages.iter().map(|(i,e,n,_,_)|json!({"record":i,"event":e,"precondition_checks":n})).collect::<Vec<_>>(),
        "initial_tables":initial_tables,"original_use_tables":original_tables,"parameter_marker":hole,"parameter_markers":environment.holes,
        "symbolic_values":values.iter().map(|(token,term)|json!({"token":token,"term":term.to_string(),"sort":c.pool.values[*token].sort})).collect::<Vec<_>>(),
        "parameters":parameters,"all_source_rules":c.rules.len(),"validation":"all original LHS checks and recorded output aliases passed before/after the corresponding ground actions",
        "staged_injections":injections,"initial_source":seed_text,"concrete_boundary_structure_recovered":false,
        "saturated_scope":"extracted interface only; not the complete original tier0 neighborhood"});
    elapsed!("encode_result_provenance", output_tick);
    Ok((text, validation, provenance))
}

fn audit(c: &Captured) -> Result {
    let mut parser = super::measured_engine();
    let declarations = parser.parse_program(None, &c.datatype)?;
    let merged: BTreeSet<_> = declarations
        .iter()
        .filter_map(|d| match d {
            Command::Function {
                name,
                merge: Some(_),
                ..
            } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    if c.rules.iter().any(|r| {
        r.rule
            .head
            .0
            .iter()
            .any(|a| matches!(a,Action::Set(_,op,_,_) if merged.contains(op.as_str())))
    }) {
        return Err("automatic merge update needs a pre-update table-state witness; explicit ripen remains supported".into());
    }
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
