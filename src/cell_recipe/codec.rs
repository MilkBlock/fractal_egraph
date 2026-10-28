use super::*;

pub struct Program {
    pub document: J,
}
impl Program {
    pub fn load(path: &Path) -> Result<Self> {
        let p = Self {
            document: serde_json::from_slice(&std::fs::read(path)?)?,
        };
        p.validate()?;
        Ok(p)
    }
    pub fn validate(&self) -> Result<()> {
        if self.document["schema"] != "analyzer-cell-recipes/v1" {
            return Err("unsupported Cell recipe schema".into());
        }
        for (i, e) in arr(&self.document["expressions"])?.iter().enumerate() {
            if e["op"] == "call"
                && arr(&e["args"])?
                    .iter()
                    .any(|x| x.as_u64().is_none_or(|n| n as usize >= i))
            {
                return Err("cyclic or forward expression reference".into());
            }
        }
        let mut ids = BTreeSet::new();
        for c in arr(&self.document["cells"])? {
            if !ids.insert(num(&c["unit"])?) {
                return Err("duplicate Cell unit".into());
            }
        }
        Ok(())
    }
    pub fn cell(&self, unit: usize) -> Result<&J> {
        arr(&self.document["cells"])?
            .iter()
            .find(|c| c["unit"].as_u64() == Some(unit as u64))
            .ok_or_else(|| "unknown Cell unit".into())
    }
    fn exprs(&self, cell: &J) -> Result<Vec<Expr>> {
        let mut out = vec![];
        for e in arr(&self.document["expressions"])? {
            let x = match text(&e["op"])? {
                "variable" => Expr::Var(sp(), text(&e["name"])?.into()),
                "input" => {
                    // Expressions are shared across cells; an unused input can
                    // have a higher slot or a different type in another frame.
                    let input = arr(&cell["inputs"])?.get(num(&e["slot"])?);
                    if let Some(p) = input.filter(|p| p["sort"] == e["sort"]) {
                        Expr::Call(
                            sp(),
                            text(&p["marker"])?.into(),
                            vec![Expr::Lit(
                                sp(),
                                Literal::Int(p["index"].as_i64().ok_or("invalid input index")?),
                            )],
                        )
                    } else {
                        Expr::Var(sp(), "__unbound_cell_input".into())
                    }
                }
                "literal" => Expr::Lit(
                    sp(),
                    match text(&e["sort"])? {
                        "i64" => Literal::Int(text(&e["value"])?.parse()?),
                        "f64" => Literal::Float(f64::from_bits(text(&e["bits"])?.parse()?).into()),
                        "String" => Literal::String(text(&e["value"])?.into()),
                        "bool" => Literal::Bool(e["value"].as_bool().ok_or("invalid bool")?),
                        "Unit" => Literal::Unit,
                        _ => return Err("unsupported literal".into()),
                    },
                ),
                "call" => Expr::Call(
                    sp(),
                    text(&e["name"])?.into(),
                    arr(&e["args"])?
                        .iter()
                        .map(|i| {
                            out.get(num(i)?)
                                .cloned()
                                .ok_or_else(|| "invalid expression reference".into())
                        })
                        .collect::<Result<Vec<_>>>()?,
                ),
                _ => return Err("unknown expression opcode".into()),
            };
            out.push(x);
        }
        Ok(out)
    }
    fn expression(es: &[Expr], x: &J) -> Result<Expr> {
        es.get(num(x)?)
            .cloned()
            .ok_or_else(|| "invalid expression reference".into())
    }
    fn args(es: &[Expr], x: &J) -> Result<Vec<Expr>> {
        arr(x)?.iter().map(|v| Self::expression(es, v)).collect()
    }
    fn fact(es: &[Expr], f: &J) -> Result<Fact> {
        Ok(match text(&f["op"])? {
            "equal" => {
                let args = Self::args(es, &f["args"])?;
                if args.len() != 2 {
                    return Err("equality arity".into());
                }
                Fact::Eq(sp(), args[0].clone(), args[1].clone())
            }
            "predicate" => Fact::Fact(Self::expression(es, &f["expr"])?),
            _ => return Err("unknown guard opcode".into()),
        })
    }
    fn action(es: &[Expr], a: &J) -> Result<Action> {
        Ok(match text(&a["op"])? {
            "let" => Action::Let(
                sp(),
                text(&a["name"])?.into(),
                Self::expression(es, &a["expr"])?,
            ),
            "eval" => Action::Expr(sp(), Self::expression(es, &a["expr"])?),
            "set" => Action::Set(
                sp(),
                text(&a["table"])?.into(),
                Self::args(es, &a["args"])?,
                Self::expression(es, &a["value"])?,
            ),
            "union" => {
                let args = Self::args(es, &a["args"])?;
                if args.len() != 2 {
                    return Err("union arity".into());
                }
                Action::Union(sp(), args[0].clone(), args[1].clone())
            }
            "subsume" | "delete" => Action::Change(
                sp(),
                if a["op"] == "subsume" {
                    Change::Subsume
                } else {
                    Change::Delete
                },
                text(&a["table"])?.into(),
                Self::args(es, &a["args"])?,
            ),
            "panic" => Action::Panic(sp(), text(&a["message"])?.into()),
            _ => return Err("unknown action opcode".into()),
        })
    }
    fn schedule(es: &[Expr], s: &J) -> Result<Schedule> {
        Ok(match text(&s["op"])? {
            "run" => Schedule::Run(
                sp(),
                RunConfig {
                    ruleset: text(&s["ruleset"])?.into(),
                    until: if s["until"].is_null() {
                        None
                    } else {
                        Some(
                            arr(&s["until"])?
                                .iter()
                                .map(|f| Self::fact(es, f))
                                .collect::<Result<Vec<_>>>()?,
                        )
                    },
                },
            ),
            "repeat" => Schedule::Repeat(
                sp(),
                num(&s["count"])?,
                Box::new(Self::schedule(es, &s["body"])?),
            ),
            "saturate" => Schedule::Saturate(sp(), Box::new(Self::schedule(es, &s["body"])?)),
            "sequence" => Schedule::Sequence(
                sp(),
                arr(&s["body"])?
                    .iter()
                    .map(|x| Self::schedule(es, x))
                    .collect::<Result<Vec<_>>>()?,
            ),
            _ => return Err("unknown schedule opcode".into()),
        })
    }
    fn commands(&self, es: &[Expr], needed: &BTreeSet<usize>) -> Result<Vec<Option<Command>>> {
        let mut out = vec![];
        let mut parser = EGraph::default();
        for (index, c) in arr(&self.document["instructions"])?.iter().enumerate() {
            if !needed.contains(&index) {
                out.push(None);
                continue;
            }
            let cmd = match text(&c["op"])? {
                "declaration" => {
                    let mut cs = parser.parse_program(None, text(&c["source"])?)?;
                    if cs.len() != 1 || !declaration(&cs[0]) {
                        return Err("operational code disguised as declaration".into());
                    }
                    let c0 = cs.remove(0);
                    if let Command::Function { merge, .. } = &c0 {
                        let decoded = if c["merge_expression"].is_null() {
                            None
                        } else {
                            Some(Self::expression(es, &c["merge_expression"])?.to_string())
                        };
                        if merge.as_ref().map(ToString::to_string) != decoded {
                            return Err("function merge expression differs from declaration".into());
                        }
                    }
                    c0
                }
                "rule" => Command::Rule {
                    rule: Rule {
                        span: sp(),
                        name: text(&c["name"])?.into(),
                        ruleset: text(&c["ruleset"])?.into(),
                        naive: c["naive"].as_bool().ok_or("invalid rule mode")?,
                        body: arr(&c["conditions"])?
                            .iter()
                            .map(|f| Self::fact(es, f))
                            .collect::<Result<Vec<_>>>()?,
                        head: Actions::new(
                            arr(&c["actions"])?
                                .iter()
                                .map(|a| Self::action(es, a))
                                .collect::<Result<Vec<_>>>()?,
                        ),
                    },
                },
                "check" => Command::Check(
                    sp(),
                    arr(&c["facts"])?
                        .iter()
                        .map(|f| Self::fact(es, f))
                        .collect::<Result<Vec<_>>>()?,
                ),
                "expect_failure" => Command::Fail(
                    sp(),
                    Box::new(
                        out.get(num(&c["command"])?)
                            .and_then(Option::as_ref)
                            .cloned()
                            .ok_or("cyclic/invalid command reference")?,
                    ),
                ),
                "schedule" => Command::RunSchedule(Self::schedule(es, &c["schedule"])?),
                _ => Command::Action(Self::action(es, c)?),
            };
            out.push(Some(cmd));
        }
        Ok(out)
    }
    /// Exact native execution of the exported program. This does not claim
    /// query-free completion: uncompiled closure rules still use native matching.
    pub fn execute(&self, unit: usize, validation: bool, closure: bool) -> Result<EGraph> {
        let cell = self.cell(unit)?;
        if cell["program_status"] != "ready" {
            return Err("Cell preparation was not available".into());
        }
        let es = self.exprs(cell)?;
        let env = arr(&self.document["environments"])?
            .get(num(&cell["environment"])?)
            .ok_or("unknown environment")?;
        let mut pending = vec![];
        for block in if validation {
            vec![&cell["validation"]]
        } else {
            vec![env, &cell["prepare"], &cell["final_checks"]]
        } {
            pending.extend(arr(block)?.iter().map(num).collect::<Result<Vec<_>>>()?);
        }
        let instructions = arr(&self.document["instructions"])?;
        let mut needed = BTreeSet::new();
        while let Some(i) = pending.pop() {
            if !needed.insert(i) {
                continue;
            }
            let c = instructions.get(i).ok_or("invalid instruction reference")?;
            if c["op"] == "expect_failure" {
                pending.push(num(&c["command"])?);
            }
        }
        fn expr_refs(v: &J, refs: &mut Vec<usize>) -> Result<()> {
            if let Some(o) = v.as_object() {
                for (k, v) in o {
                    if matches!(k.as_str(), "expr" | "value" | "merge_expression") && !v.is_null() {
                        refs.push(num(v)?);
                    } else if k == "args" {
                        refs.extend(arr(v)?.iter().map(num).collect::<Result<Vec<_>>>()?);
                    } else {
                        expr_refs(v, refs)?;
                    }
                }
            } else if let Some(a) = v.as_array() {
                for v in a {
                    expr_refs(v, refs)?;
                }
            }
            Ok(())
        }
        let mut refs = vec![];
        for i in &needed {
            expr_refs(&instructions[*i], &mut refs)?;
        }
        expr_refs(&cell["closure"], &mut refs)?;
        let expressions = arr(&self.document["expressions"])?;
        let mut seen = BTreeSet::new();
        while let Some(i) = refs.pop() {
            if !seen.insert(i) {
                continue;
            }
            let e = expressions.get(i).ok_or("invalid expression reference")?;
            if e["op"] == "input" {
                let input = arr(&cell["inputs"])?
                    .get(num(&e["slot"])?)
                    .ok_or("missing Cell input")?;
                if input["sort"] != e["sort"] {
                    return Err("Cell input sort mismatch".into());
                }
            } else if e["op"] == "call" {
                refs.extend(
                    arr(&e["args"])?
                        .iter()
                        .map(num)
                        .collect::<Result<Vec<_>>>()?,
                );
            }
        }
        let commands = self.commands(&es, &needed)?;
        let mut eg = EGraph::default();
        let select = |list: &J| -> Result<Vec<Command>> {
            arr(list)?
                .iter()
                .map(|i| {
                    commands
                        .get(num(i)?)
                        .and_then(Option::as_ref)
                        .cloned()
                        .ok_or_else(|| "invalid instruction reference".into())
                })
                .collect()
        };
        if validation {
            eg.run_program(select(&cell["validation"])?)?;
            return Ok(eg);
        }
        let env = arr(&self.document["environments"])?
            .get(num(&cell["environment"])?)
            .ok_or("unknown environment")?;
        eg.run_program(select(env)?)?;
        eg.run_program(select(&cell["prepare"])?)?;
        if closure {
            if cell["ripen_state"] != "Saturated" {
                return Err("unfinished Cell has no finite closure replay permission".into());
            }
            let schedule = Self::schedule(&es, &cell["closure"])?;
            let Schedule::Saturate(_, body) = schedule else {
                return Err("invalid isolated closure schedule".into());
            };
            let Schedule::Sequence(_, runs) = *body else {
                return Err("invalid isolated closure sequence".into());
            };
            let sets = runs
                .iter()
                .map(|r| match r {
                    Schedule::Run(_, r) if r.until.is_none() => Ok(r.ruleset.clone()),
                    _ => Err("invalid isolated closure run".into()),
                })
                .collect::<Result<Vec<_>>>()?;
            let limits = crate::ripen_observation::Limits::from_env()?;
            let start = std::time::Instant::now();
            let trace = egglog::TraceSession::with_effects();
            loop {
                let mut changed = false;
                for set in &sets {
                    trace.set_execution_limits(
                        limits.milliseconds.map(|ms| {
                            std::time::Duration::from_secs_f64(
                                (ms as f64 / 1000. - start.elapsed().as_secs_f64()).max(0.),
                            )
                        }),
                        limits.facts.map(|n| n.saturating_sub(eg.num_tuples())),
                    );
                    let result = eg.step_rules_with_trace(set, &trace);
                    let stopped = trace.execution_stop_reason();
                    trace.clear_execution_limits();
                    trace.drain_completed();
                    changed |= result?.updated;
                    if stopped.is_some() {
                        return Err(
                            "Cell verification reached the existing ripen observation limit".into(),
                        );
                    }
                }
                if !changed {
                    break;
                }
                if !limits
                    .reached(start.elapsed().as_secs_f64(), eg.num_tuples())
                    .is_empty()
                {
                    return Err(
                        "Cell verification reached the existing ripen observation limit".into(),
                    );
                }
            }
            eg.run_program(select(&cell["final_checks"])?)?;
        }
        Ok(eg)
    }
}
