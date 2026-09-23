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

fn extract(
    c: &Captured,
    member_ids: &[usize],
    root_member: usize,
) -> Result<(String, String, Json)> {
    let members: BTreeSet<_> = member_ids.iter().copied().collect();
    if members.is_empty() {
        return Err("empty coarse entry".into());
    }
    let mut hole = "RipenInput".to_string();
    while c.preview_source.contains(&hole) {
        hole.push('_');
    }
    let mut parser = super::measured_engine();
    let mut datatypes = parser.parse_program(None, &c.datatype)?;
    let extra = parser.parse_program(None, &format!("(datatype Temporary ({hole} i64))"))?;
    let Command::Datatype {
        variants: extra, ..
    } = &extra[0]
    else {
        unreachable!()
    };
    let datatype_index=datatypes.iter().position(|c| matches!(c,Command::Datatype{..})).ok_or("symbolic ripen needs an equality datatype; scalar-only bindings are not parameterized yet")?;
    let Command::Datatype { variants, .. } = &mut datatypes[datatype_index] else {
        return Err("expected datatype".into());
    };
    variants.extend(extra.clone());
    let declarations = datatypes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
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
    let mut values = BTreeMap::new();
    let mut parameters = vec![];
    let mut initial_reads = BTreeSet::new();
    for i in &members {
        let r = &c.records[*i];
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
                    if c.pool.values[*v].sort.as_ref() != c.datatype_name {
                        return Err(format!("boundary token {v} has scalar sort {}; its literal is not recoverable from this history",c.pool.values[*v].sort).into());
                    }
                    let name = format!("ripen_p{}", parameters.len());
                    let term = call(&hole, vec![num(parameters.len() as u64)]);
                    // Terms, not global variable names, preserve captured alias identity.
                    entry.push(Command::Action(Action::Expr(sp(), term.clone())));
                    values.insert(*v, term);
                    parameters.push(json!({"name":name,"token":v,"sort":c.datatype_name,"semantics":"opaque boundary parameter"}));
                }
                Input::Read(_) => {
                    initial_reads.insert(*v);
                }
            }
        }
    }
    let boundary_values: BTreeSet<_> = values.keys().copied().collect();
    let mut seeds = BTreeMap::new();
    let mut stages = vec![];
    let mut final_checks = vec![];
    let mut injections = vec![];
    for i in &members {
        let r = &c.records[*i];
        let rule = &c.rules[r.rule];
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
                                return Err("seeding an external row would materialize an internal read; staged injection is required".into());
                            }
                        }
                    }
                    // No external row may be bootstrapped by generating an internal result.
                    for (var, v) in r.inputs.iter().zip(&r.wanted) {
                        if matches!(var, Input::Var(_)) && !boundary_values.contains(v) {
                            return Err("external row depends on an internal result; staged injection is required".into());
                        }
                    }
                    let expr = subst(
                        &rule.calls.get(span).ok_or("missing read expression")?.1,
                        &env,
                    )?;
                    seeds.insert(expr.to_string(), Command::Action(Action::Expr(sp(), expr)));
                }
            }
        }
        let mut commands = std::mem::take(&mut seeds).into_values().collect::<Vec<_>>();
        if !commands.is_empty() {
            injections.push(json!({"before_record":i,"facts":commands.iter().map(ToString::to_string).collect::<Vec<_>>()}));
        }
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
        stages.push((*i, r.id, preconditions, commands));
    }
    entry.extend(seeds.into_values());
    let mut eg = super::measured_engine();
    eg.run_program(entry.clone())?;
    let initial_tables = super::table_sizes(&eg, &declarations)?;
    for (i, _, _, commands) in &stages {
        eg.run_program(commands.clone())
            .map_err(|e| format!("CS member {i} cannot replay from extracted entry: {e}"))?;
    }
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
            .map(|(_, _, _, s)| encode(s))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let seed_text = encode(&entry);
    for (_, _, _, commands) in &stages {
        entry.extend(commands.clone());
    }
    entry.extend(final_checks);
    let text = format!(
        "; Extracted symbolic interface. Boundary constructors are opaque, not recovered original terms.\n{}\n",
        encode(&entry)
    );
    let slots: BTreeMap<_, _> = member_ids
        .iter()
        .enumerate()
        .map(|(m, e)| (*e, m))
        .collect();
    let mut boundary_ports = BTreeMap::new();
    let comb_members:Vec<_>=member_ids.iter().enumerate().map(|(slot,i)|{
        let o=&c.layers.occurrences[*i];let a=&o.apply;let r=&c.records[*i];
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
            "use_kind":if coarse{"CoarseRuleComposition"}else{"SmoothRuleComposition"},"source_kind":format!("{:?}",c.layers.combs[o.comb].kind),
            "parents":a.parents.iter().filter_map(|p|slots.get(p).copied()).collect::<BTreeSet<_>>(),"external_parents":external_parents,
            "binding":wiring,"aliases":aliases,"input_roles":a.input_roles,"output_roles":a.output_roles,"source_coarse_layer":o.coarse_layer,"source_smooth_layer":o.smooth_layer})
    }).collect();
    let provenance = json!({"kind":"symbolic_cs_interface","template":null,"members":member_ids,"root_member":root_member,"comb_members":comb_members,"kind_scope":"use_kind is relative to directly recorded dependencies inside this CS entry; source_kind is the original layer classification; neither minimizes alternate proof requirements",
        "events":stages.iter().map(|(i,e,n,_)|json!({"record":i,"event":e,"precondition_checks":n})).collect::<Vec<_>>(),
        "initial_tables":initial_tables,"original_use_tables":super::table_sizes(&eg,&declarations)?,"parameter_marker":hole,
        "symbolic_values":values.iter().map(|(token,term)|json!({"token":token,"term":term.to_string(),"sort":c.pool.values[*token].sort})).collect::<Vec<_>>(),
        "parameters":parameters,"all_source_rules":c.rules.len(),"validation":"all original LHS checks and recorded output aliases passed before/after the corresponding ground actions",
        "staged_injections":injections,"initial_source":seed_text,"concrete_boundary_structure_recovered":false,
        "saturated_scope":"extracted interface only; not the complete original tier0 neighborhood"});
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

pub(in crate::native_analyze) fn prepare_members(
    c: &Captured,
    members: &[usize],
) -> Result<(String, String, Json)> {
    audit(c)?;
    if members.is_empty()
        || members.windows(2).any(|p| p[0] >= p[1])
        || members.last().unwrap() >= &c.records.len()
    {
        return Err("invalid dependency candidate members".into());
    }
    extract(c, members, members.len() - 1)
}
