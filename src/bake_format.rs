//! Readable frozen-library syntax. Parsing reuses egglog's parser; declarations
//! are data and are never executed as tier0 rules. Machine evidence is carried
//! in the same @egg-viz-json comment convention as the existing exports.
use super::*;
const MARKER: &str = "; @egg-viz-json ";
fn note(value: Json) -> Result<String> {
    Ok(format!("{MARKER}{}\n", serde_json::to_string(&value)?))
}
fn expression(s: &str) -> Result<Expr> {
    Ok(egglog::ast::Parser::default().get_expr_from_string(None, s)?)
}
fn id(i: usize) -> String {
    format!("step_{i:04}")
}
fn atom(e: &Expr) -> Result<String> {
    match e {
        Expr::Var(_, v) => Ok(v.clone()),
        Expr::Lit(_, Literal::String(v)) => Ok(v.clone()),
        _ => Err("expected an identifier".into()),
    }
}
fn step_id(e: &Expr) -> Result<usize> {
    Ok(atom(e)?
        .strip_prefix("step_")
        .ok_or("invalid step reference")?
        .parse()?)
}
fn pretty(e: &Expr, depth: usize) -> String {
    let flat = e.to_string();
    if flat.len() + depth <= 100 {
        return flat;
    }
    let Expr::Call(_, op, args) = e else {
        return flat;
    };
    let mut out = format!("({op}");
    // Keep linear binding programs flat on the page rather than indenting every
    // BLet into a progressively narrower column.
    if op == "BLet" && args.len() == 2 {
        out.push(' ');
        out.push_str(&pretty(&args[0], depth + 2));
        out.push('\n');
        out.push_str(&" ".repeat(depth));
        out.push_str(&pretty(&args[1], depth));
        out.push(')');
        return out;
    }
    for a in args {
        out.push('\n');
        out.push_str(&" ".repeat(depth + 2));
        out.push_str(&pretty(a, depth + 2));
    }
    out.push(')');
    out
}
fn role(v: &Json) -> Result<Expr> {
    let xs = v.as_array().ok_or("invalid role")?;
    let name = xs
        .first()
        .and_then(Json::as_str)
        .ok_or("invalid role name")?;
    let args = xs[1..]
        .iter()
        .map(|v| match v {
            Json::String(s) => Ok(string(s)),
            Json::Number(n) => Ok(num(n.as_u64().ok_or("negative role index")?)),
            _ => Err("invalid role operand".into()),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(call(name, args))
}
fn bindings(step: &Step) -> Result<Vec<Expr>> {
    step.signature["inputs"]
        .as_array()
        .ok_or("missing inputs")?
        .iter()
        .map(|input| {
            let route = &input[1];
            let source = match route[0].as_str() {
                Some("parent") => call(
                    "parent",
                    vec![
                        num(route[1].as_u64().ok_or("invalid parent")?),
                        role(&route[3])?,
                    ],
                ),
                Some("external") => call(
                    "external",
                    vec![num(route[1].as_u64().ok_or("invalid external")?)],
                ),
                _ => return Err("invalid input route".into()),
            };
            Ok(call(
                "binding",
                vec![
                    role(&input[0])?,
                    source,
                    string(input[2].as_str().ok_or("missing sort")?),
                ],
            ))
        })
        .collect()
}

pub(super) fn render(lib: &Library) -> Result<String> {
    let mut out="; Frozen FractalRule library — declarations, not a tier0 program.\n; Basic rules and recursive structure are visible; evidence/statistics use comments.\n".to_owned();
    out += &note(
        json!({"schema":"egg-viz/v1","kind":"bake_library","format":"bake-egg/v1","scope":lib.scope}),
    )?;
    out += &format!(
        "(bake-library {} {})\n\n",
        lib.version,
        serde_json::to_string(&lib.algebra)?
    );
    for (i, sample) in lib.samples.iter().enumerate() {
        out += &note(
            json!({"schema":"egg-viz/v1","kind":"bake_sample","id":format!("sample_{i:04}"),"data":sample}),
        )?;
    }
    out += "\n; ── Basic steps and their relative bindings ──\n";
    for (i, step) in lib.steps.iter().enumerate() {
        out += &format!(
            "\n; {} — {}\n",
            id(i),
            step.label.replace(['\n', '\r'], " ")
        );
        out += &note(
            json!({"schema":"egg-viz/v1","kind":"original_rule","id":id(i),"bake":{"signature":step.signature,"label":step.label}}),
        )?;
        out += &format!("(bake-step {}", id(i));
        for b in bindings(step)? {
            out += &format!("\n  {}", pretty(&b, 2));
        }
        out += ")\n";
        out += &step.source;
        out += "\n";
    }
    out += "\n; ── Fractal families ──\n";
    for t in &lib.templates {
        let names: Vec<_> = t.support.iter().map(|s| s.sample.as_str()).collect();
        out += &format!(
            "\n; {} — {}; {} sample reports\n",
            t.id.replace(['\n', '\r'], " "),
            t.kind,
            t.support.len()
        );
        let mut contract = serde_json::to_value(&t.contract)?;
        if let Some(c) = contract.as_object_mut() {
            c.remove("dag");
        }
        let mut law = t.array_summary.clone();
        if let Some(v) = law
            .as_mut()
            .and_then(|v| v.get_mut("dsl"))
            .and_then(Json::as_object_mut)
        {
            v.remove("array");
            v.remove("sum");
        }
        out += &note(
            json!({"schema":"egg-viz/v1","kind":"bake_fractal_rule","id":t.id,"statistics":{"samples":names,"instances":t.support.iter().map(|s|s.instances).sum::<usize>(),"max_observed_depth":t.support.iter().map(|s|s.max_depth).max().unwrap_or(0)},"bake":{"support":t.support,"contract":contract,"array_law":law}}),
        )?;
        out += &format!(
            "(fractal-rule {}\n  (kind {})\n  (entry {})\n  (ports{})",
            serde_json::to_string(&t.id)?,
            t.kind,
            id(t.entry),
            t.ports
                .iter()
                .map(|p| format!(" {}", id(*p)))
                .collect::<String>()
        );
        if let Some(law) = &t.array_summary {
            for field in ["array", "sum"] {
                out += &format!(
                    "\n  ({field}\n    {})",
                    pretty(
                        &expression(
                            law["dsl"][field]
                                .as_str()
                                .ok_or("missing array expression")?
                        )?,
                        4
                    )
                );
            }
        }
        if let Some(c) = &t.contract {
            out += &format!(
                "\n  (local-contract\n    {})",
                pretty(&expression(&c.dag)?, 4)
            );
        }
        out += ")\n";
    }
    Ok(out)
}

pub(super) fn parse(text: &str) -> Result<Library> {
    let mut scope = None;
    let mut samples = BTreeMap::new();
    let mut sm = BTreeMap::new();
    let mut tm = BTreeMap::new();
    for line in text.lines() {
        let Some(payload) = line.trim_start().strip_prefix(MARKER) else {
            continue;
        };
        let v: Json = serde_json::from_str(payload)?;
        if v["schema"] != "egg-viz/v1" {
            continue;
        }
        match v["kind"].as_str() {
            Some("bake_library") => {
                if v["format"] != "bake-egg/v1" || scope.is_some() {
                    return Err("invalid/duplicate library annotation".into());
                }
                scope = Some(v["scope"].as_str().ok_or("missing scope")?.to_owned());
            }
            Some("bake_sample") => {
                let key: usize = v["id"]
                    .as_str()
                    .ok_or("missing sample id")?
                    .strip_prefix("sample_")
                    .ok_or("invalid sample id")?
                    .parse()?;
                if samples.insert(key, v["data"].clone()).is_some() {
                    return Err("duplicate sample annotation".into());
                }
            }
            Some("original_rule") if v.get("bake").is_some() => {
                let key = v["id"].as_str().ok_or("missing step id")?.to_owned();
                if sm.insert(key, v["bake"].clone()).is_some() {
                    return Err("duplicate step annotation".into());
                }
            }
            Some("bake_fractal_rule") => {
                let key = v["id"].as_str().ok_or("missing family id")?.to_owned();
                if tm.insert(key, v["bake"].clone()).is_some() {
                    return Err("duplicate family annotation".into());
                }
            }
            _ => {} // unrelated visualization comments are not executable input
        }
    }
    let commands = EGraph::default().parse_program(None, text)?;
    let mut header = None;
    let mut pending = None;
    let mut steps = BTreeMap::new();
    let mut templates = vec![];
    for command in commands {
        match command {
            Command::Rule { rule } => {
                let (index, declared) = pending.take().ok_or("source rule without bake-step")?;
                let meta = sm.remove(&id(index)).ok_or("missing step annotation")?;
                let step = Step {
                    signature: meta["signature"].clone(),
                    label: meta["label"]
                        .as_str()
                        .ok_or("missing step label")?
                        .to_owned(),
                    source: rule.to_string(),
                };
                let expected = bindings(&step)?
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                if declared != expected {
                    return Err(
                        "visible bindings disagree with saved signature; rebake the library".into(),
                    );
                }
                if steps.insert(index, step).is_some() {
                    return Err("duplicate step declaration".into());
                }
            }
            Command::Action(Action::Expr(_, Expr::Call(_, name, args))) => {
                if pending.is_some() {
                    return Err("bake-step must be followed by its source rule".into());
                }
                match name.as_str() {
                    "bake-library" => {
                        let [
                            Expr::Lit(_, Literal::Int(version)),
                            Expr::Lit(_, Literal::String(algebra)),
                        ] = args.as_slice()
                        else {
                            return Err("invalid bake-library header".into());
                        };
                        if header
                            .replace((u32::try_from(*version)?, algebra.clone()))
                            .is_some()
                        {
                            return Err("duplicate library header".into());
                        }
                    }
                    "bake-step" => {
                        let first = args.first().ok_or("missing step identifier")?;
                        pending = Some((
                            step_id(first)?,
                            args[1..]
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>(),
                        ));
                    }
                    "fractal-rule" => {
                        let name = atom(args.first().ok_or("missing family identifier")?)?;
                        let meta = tm.remove(&name).ok_or("missing family annotation")?;
                        let mut fields = BTreeMap::new();
                        for field in &args[1..] {
                            let Expr::Call(_, key, xs) = field else {
                                return Err("invalid family field".into());
                            };
                            if !["kind", "entry", "ports", "local-contract", "array", "sum"]
                                .contains(&key.as_str())
                                || fields.insert(key.as_str(), xs).is_some()
                            {
                                return Err("unknown/duplicate family field".into());
                            }
                        }
                        let one = |key: &str| -> Result<&Expr> {
                            let xs = fields.get(key).ok_or_else(|| format!("missing {key}"))?;
                            if xs.len() != 1 {
                                return Err(format!("{key} requires one operand").into());
                            }
                            Ok(&xs[0])
                        };
                        let mut contract = meta["contract"].clone();
                        if !contract.is_null() {
                            contract["dag"] = json!(one("local-contract")?.to_string());
                        } else if fields.contains_key("local-contract") {
                            return Err("contract metadata missing".into());
                        }
                        let mut law = meta["array_law"].clone();
                        if !law.is_null() {
                            law["dsl"]["array"] = json!(one("array")?.to_string());
                            law["dsl"]["sum"] = json!(one("sum")?.to_string());
                        } else if fields.contains_key("array") || fields.contains_key("sum") {
                            return Err("array law metadata missing".into());
                        }
                        templates.push(Template {
                            id: name,
                            kind: atom(one("kind")?)?,
                            entry: step_id(one("entry")?)?,
                            ports: fields
                                .get("ports")
                                .ok_or("missing ports")?
                                .iter()
                                .map(step_id)
                                .collect::<Result<Vec<_>>>()?,
                            support: serde_json::from_value(meta["support"].clone())?,
                            contract: serde_json::from_value(contract)?,
                            array_summary: serde_json::from_value(law)?,
                        });
                    }
                    _ => return Err(format!("unknown library declaration {name}").into()),
                }
            }
            _ => return Err("library accepts declarations and source rules only".into()),
        }
    }
    if pending.is_some() || !sm.is_empty() || !tm.is_empty() {
        return Err("orphan library declaration/annotation".into());
    }
    if steps.keys().copied().ne(0..steps.len()) {
        return Err("step IDs must form a contiguous index".into());
    }
    let (version, algebra) = header.ok_or("missing library header")?;
    Ok(Library {
        version,
        algebra,
        scope: scope.ok_or("missing library annotation")?,
        samples: samples.into_values().collect(),
        steps: steps.into_values().collect(),
        templates,
    })
}
