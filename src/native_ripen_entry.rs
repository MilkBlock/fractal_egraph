//! Extract an executable symbolic interface, not a full concrete tier0 snapshot.
use super::super::*;
use crate::comb_reuse::Use;

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

fn extract(c: &Captured, u: &Use) -> Result<(String, String, Json)> {
    let members: BTreeSet<_> = u.members.iter().copied().collect();
    let first = *members.first().ok_or("empty Use")?;
    let mut hole = "RipenInput".to_string();
    while c.preview_source.contains(&hole) {
        hole.push('_');
    }
    let mut parser = EGraph::default();
    let mut datatypes = parser.parse_program(None, &c.datatype)?;
    let extra = parser.parse_program(None, &format!("(datatype Temporary ({hole} i64))"))?;
    let Command::Datatype {
        variants: extra, ..
    } = &extra[0]
    else {
        unreachable!()
    };
    let Command::Datatype { variants, .. } = &mut datatypes[0] else {
        return Err("expected datatype".into());
    };
    variants.extend(extra.clone());
    let mut entry = vec![datatypes[0].clone()];
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
            if let Port::Parent(k, _) = p {
                if r.parents[*k] >= first {
                    return Err("entry requires a late external producer; staged injection is not supported".into());
                }
            } else if *i != first && !values.contains_key(v) && !initial_reads.contains(v) {
                return Err("unversioned external input first appears after entry".into());
            }
            match input {
                Input::Var(_) => {
                    if values.contains_key(v) {
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
        let mut commands = vec![];
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
                _ => return Err("unsupported non-monotone Use action".into()),
            };
            final_checks.push(match &action {
                Action::Union(_, a, b) => eq(a.clone(), b.clone()),
                Action::Expr(_, e) => eq(e.clone(), e.clone()),
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
    let mut eg = EGraph::default();
    eg.run_program(entry.clone())?;
    let initial_tables = super::table_sizes(&eg, &datatypes[0].to_string())?;
    for (i, _, _, commands) in &stages {
        eg.run_program(commands.clone())
            .map_err(|e| format!("Use member {i} cannot replay from extracted entry: {e}"))?;
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
    entry.extend(final_checks);
    let text = format!(
        "; Extracted symbolic interface. Boundary constructors are opaque, not recovered original terms.\n{}\n",
        encode(&entry)
    );
    let provenance = json!({"kind":"symbolic_use_interface","template":u.template,"members":u.members,
        "events":stages.iter().map(|(i,e,n,_)|json!({"record":i,"event":e,"precondition_checks":n})).collect::<Vec<_>>(),
        "initial_tables":initial_tables,"original_use_tables":super::table_sizes(&eg,&datatypes[0].to_string())?,"parameters":parameters,"all_source_rules":c.rules.len(),"validation":"all original LHS checks and recorded output aliases passed before/after the corresponding ground actions",
        "initial_source":seed_text,"concrete_boundary_structure_recovered":false,
        "closed_scope":"extracted interface only; not the complete original tier0 neighborhood"});
    Ok((text, validation, provenance))
}

pub fn from_use(history_path: &Path, use_id: usize, out: &Path, max_rounds: usize) -> Result<Json> {
    if out.exists() {
        return Err("ripen-use output already exists".into());
    }
    if max_rounds == 0 {
        return Err("max-rounds must be positive".into());
    }
    for flag in [
        "EGG_LAYOUT_EAGER_DICTIONARY",
        "EGG_LAYOUT_DISABLE_RECUT",
        "EGG_LAYOUT_DISABLE_ROTATIONS",
    ] {
        if std::env::var_os(flag).is_some() {
            return Err(format!("ripen-use requires default replay policy; unset {flag}").into());
        }
    }
    let mut c = history::read(history_path)?;
    let mut parser = EGraph::default();
    let mut declared = vec![];
    for cmd in parser.parse_program(None, &c.preview_source)? {
        if matches!(
            cmd,
            Command::BiRewrite(..) | Command::Include(..) | Command::Rewrite(_, _, true)
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
    update_layers(&mut c)?;
    let u = c
        .layers
        .reuse
        .uses
        .get(use_id)
        .ok_or("unknown Use ID in default history replay")?;
    let (source, validation, mut origin) = extract(&c, u)?;
    origin["use_id"] = json!(use_id);
    origin["history"] = json!(history_path.canonicalize()?);
    std::fs::create_dir_all(out)?;
    let entry = out.join("entry.egg");
    std::fs::write(&entry, source)?;
    std::fs::write(out.join("validate-use.egg"), validation)?;
    std::fs::write(out.join("origin.json"), serde_json::to_vec_pretty(&origin)?)?;
    let link = crate::coarse_smooth::RipenOrigin {
        history: history_path.canonicalize()?.display().to_string(),
        use_id,
        template: u.template,
        members: u.members.clone(),
        symbolic_boundary: true,
    };
    let mut report = super::run_with_origin(&entry, &out.join("run"), max_rounds, Some(link))?;
    report["origin"] = origin;
    std::fs::write(
        out.join("run/ripen.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    std::fs::write(out.join("result.json"), serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}
