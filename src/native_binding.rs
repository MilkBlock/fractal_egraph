//! Lower finite witnessed multi-parent interfaces; no guessed arithmetic semantics.
use super::*;
use crate::binding_reduce::{Definition, compile_contract};

pub(super) fn reduce(
    c: &Captured,
    members: &[usize],
    returns: &[(usize, usize)],
    callee: &str,
    eg: &mut EGraph,
    fractal: &Expr,
) -> Result<Json> {
    let mut boundary = BTreeMap::<String, String>::new();
    let mut outputs = BTreeMap::<(usize, usize), String>::new();
    let mut definitions = Vec::<(String, String)>::new();
    let mut addresses = vec![];
    let mut output_keys = vec![];
    let mut requirements = vec![];
    let mut effects = vec![];
    let mut origins = BTreeMap::new();
    let mut math_calls = 0usize;
    let mut definedness_obligations = 0usize;
    let algebra =
        std::env::var("EGG_LAYOUT_BINDING_ALGEBRA").unwrap_or_else(|_| "integer-safe".into());
    let math = match algebra.as_str() {
        "math" => true,
        "integer-safe" => false,
        _ => return Err("EGG_LAYOUT_BINDING_ALGEBRA must be math or integer-safe".into()),
    };
    fn integer(e: &Expr, sorts: &BTreeMap<String, String>) -> bool {
        match e {
            Expr::Lit(_, Literal::Int(_)) => true,
            Expr::Var(_, n) => sorts.get(n).is_some_and(|s| s == "i64"),
            Expr::Call(_, op, args) if ["+", "-", "*"].contains(&op.as_str()) => {
                args.len() == 2 && args.iter().all(|a| integer(a, sorts))
            }
            _ => false,
        }
    }
    fn expr(
        e: &Expr,
        env: &BTreeMap<String, String>,
        sorts: &BTreeMap<String, String>,
        math: bool,
    ) -> std::result::Result<String, String> {
        Ok(match e {
            Expr::Var(_, n) => env
                .get(n)
                .cloned()
                .ok_or_else(|| format!("unresolved source binding {n}"))?,
            Expr::Lit(_, Literal::Int(n)) => n.to_string(),
            Expr::Lit(_, Literal::String(s)) => serde_json::to_string(s).unwrap(),
            Expr::Lit(..) => return Err("unsupported source literal".into()),
            Expr::Call(_, op, args) => {
                let xs = args
                    .iter()
                    .map(|a| expr(a, env, sorts, math))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let mapped = if integer(e, sorts) {
                    match op.as_str() {
                        "+" => Some("EIAdd"),
                        "-" => Some("EISub"),
                        "*" => Some("EIMul"),
                        _ => None,
                    }
                } else if math {
                    match op.as_str() {
                        "Add" => Some("EAdd"),
                        "Sub" => Some("ESub"),
                        "Mul" => Some("EMul"),
                        _ => None,
                    }
                } else {
                    None
                };
                if math && op == "Const" && xs.len() == 1 {
                    xs[0].clone()
                } else {
                    format!(
                        "({} {})",
                        mapped
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("source::{op}")),
                        xs.join(" ")
                    )
                }
            }
        })
    }
    // Captured parents precede consumers. Branches may reference any prior parent.
    for &index in members {
        let r = &c.records[index];
        let rule = &c.rules[r.rule];
        let mut env = BTreeMap::new();
        let mut reads = BTreeMap::new();
        let mut sorts = BTreeMap::new();
        for (input_slot, (input, port)) in r.inputs.iter().zip(&r.ports).enumerate() {
            let value = match port {
                Port::Parent(p, slot) if members.contains(&r.parents[*p]) => outputs
                    .get(&(r.parents[*p], *slot))
                    .cloned()
                    .ok_or("non-topological or missing parent binding")?,
                _ => {
                    let key = match port {
                        Port::Parent(p, slot) => {
                            format!("parent:{}:{slot}", c.records[r.parents[*p]].id)
                        }
                        Port::External(k) => format!("external:{}:{k}", r.id),
                    };
                    let next = boundary.len();
                    origins.entry(key.clone()).or_insert((index, input_slot));
                    boundary
                        .entry(key)
                        .or_insert_with(|| format!("p{next}"))
                        .clone()
                }
            };
            match input {
                Input::Var(name) => {
                    env.insert(name.clone(), value);
                    sorts.insert(
                        name.clone(),
                        c.pool.values[r.wanted[input_slot]].sort.to_string(),
                    );
                }
                Input::Read(span) => {
                    reads.insert(span.clone(), value);
                }
            }
        }
        // Resolve fields of witnessed physical rows, not by constructor injectivity.
        for (span, row) in &reads {
            let (_, call) = rule.calls.get(span).ok_or("missing read AST")?;
            let Expr::Call(_, _, args) = call else {
                return Err("read is not a call".into());
            };
            for (col, arg) in args.iter().enumerate() {
                if let Expr::Var(_, name) = arg {
                    if !env.contains_key(name) {
                        let local = format!("v{}", definitions.len());
                        definitions.push((local.clone(), format!("(@column {col} {row})")));
                        env.insert(name.clone(), local);
                    }
                }
            }
            for fact in &rule.rule.body {
                if let Fact::Eq(_, Expr::Var(_, name), rhs) = fact {
                    if rhs == call && !env.contains_key(name) {
                        let local = format!("v{}", definitions.len());
                        definitions
                            .push((local.clone(), format!("(@column {} {row})", args.len())));
                        env.insert(name.clone(), local);
                    }
                }
            }
        }
        for (span, row) in &reads {
            requirements.push(format!("(read-fact {row})"));
            let Expr::Call(_, _, args) = &rule.calls[span].1 else {
                unreachable!()
            };
            for (col, arg) in args.iter().enumerate() {
                requirements.push(format!(
                    "(require-eq (@column {col} {row}) {})",
                    expr(arg, &env, &sorts, math)?
                ));
            }
        }
        for f in &rule.rule.body {
            match f {
                Fact::Eq(_, a, b) => requirements.push(format!(
                    "(require-eq {} {})",
                    expr(a, &env, &sorts, math)?,
                    expr(b, &env, &sorts, math)?
                )),
                Fact::Fact(e) => requirements.push(format!(
                    "(require-predicate {})",
                    expr(e, &env, &sorts, math)?
                )),
            }
        }
        // Action-local lets are definitions, never equality assertions.
        for action in &rule.rule.head.0 {
            if let Action::Let(_, name, value) = action {
                let source = expr(value, &env, &sorts, math)?;
                let local = format!("v{}", definitions.len());
                definitions.push((local.clone(), source));
                env.insert(name.clone(), local);
            }
        }
        for (slot, output) in r.outputs.iter().enumerate() {
            let source = match output {
                Output::Var(name) => env.get(name).cloned().ok_or("unresolved output binding")?,
                Output::Column(span, col) => {
                    let (_, call) = rule.calls.get(span).ok_or("missing output AST")?;
                    let Expr::Call(_, _, args) = call else {
                        return Err("output is not a call".into());
                    };
                    if *col < args.len() {
                        expr(&args[*col], &env, &sorts, math)?
                    } else if *col == args.len() {
                        expr(call, &env, &sorts, math)?
                    } else {
                        return Err("invalid output column".into());
                    }
                }
                Output::Row(span) => {
                    let call = &rule.calls.get(span).ok_or("missing row AST")?.1;
                    let Expr::Call(_, op, args) = call else {
                        return Err("row is not a call".into());
                    };
                    let fields = args
                        .iter()
                        .map(|a| expr(a, &env, &sorts, math))
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    format!(
                        "(@row {} {} {})",
                        serde_json::to_string(op)?,
                        fields.join(" "),
                        expr(call, &env, &sorts, math)?
                    )
                }
            };
            let name = format!("v{}", definitions.len());
            definitions.push((name.clone(), source));
            outputs.insert((index, slot), name);
            addresses.push(json!([r.id, slot]));
            output_keys.push((index, slot));
        }
        // Preserve strict i64 definedness even if an outer arithmetic identity
        // removes a subexpression. Each primitive keeps its own obligation.
        for (_, call) in rule.calls.values() {
            if let Expr::Call(_, op, args) = call {
                if ["+", "-", "*"].contains(&op.as_str()) && integer(call, &sorts) {
                    let operands = args
                        .iter()
                        .map(|a| expr(a, &env, &sorts, math))
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    requirements.push(format!(
                        "(i64-operation-defined {} {})",
                        serde_json::to_string(op)?,
                        operands.join(" ")
                    ));
                    definedness_obligations += 1;
                }
            }
        }
        let mut token_expr = |token: usize| -> String {
            if let Some(slot) = r.values.iter().position(|t| *t == token) {
                return outputs[&(index, slot)].clone();
            }
            if let Some(slot) = r.wanted.iter().position(|t| *t == token) {
                match &r.inputs[slot] {
                    Input::Var(n) if env.contains_key(n) => return env[n].clone(),
                    Input::Read(s) if reads.contains_key(s) => return reads[s].clone(),
                    _ => {}
                }
            }
            let key = format!("effect:{}:{}", r.id, token);
            let next = boundary.len();
            boundary
                .entry(key)
                .or_insert_with(|| format!("p{next}"))
                .clone()
        };
        for token in &r.produced {
            effects.push(format!(
                "(effect-inserted {} {})",
                serde_json::to_string(&c.pool.values[*token].label()).unwrap(),
                token_expr(*token)
            ));
        }
        for (a, b) in &r.unions {
            let identity = serde_json::to_string(&format!(
                "{}:{}={}",
                r.id,
                c.pool.values[*a].label(),
                c.pool.values[*b].label()
            ))
            .unwrap();
            let a = token_expr(*a);
            let b = token_expr(*b);
            effects.push(format!("(effect-union {identity} {a} {b})"));
        }
    }
    // Rebind the same entry interface at each witnessed return. Never infer a
    // missing return or alias from later union-find state.
    let entry = members[0];
    let input_order: Vec<_> = {
        let mut v: Vec<_> = boundary
            .iter()
            .map(|(k, v)| (v[1..].parse::<usize>().unwrap(), k.clone()))
            .collect();
        v.sort();
        v.into_iter().map(|(_, k)| k).collect()
    };
    let mut recursive_defs = vec![];
    let mut recursive_ports = vec![];
    for (site, target) in returns {
        let t = &c.records[*target];
        let mut args = vec![];
        let mut missing = vec![];
        let mut links = vec![];
        for (parameter, key) in input_order.iter().enumerate() {
            match origins.get(key) {
                Some((owner, slot))
                    if *owner == entry
                        && c.records[entry].inputs.get(*slot) == t.inputs.get(*slot) =>
                {
                    let value = match t.ports.get(*slot) {
                        Some(Port::Parent(p, col)) => outputs.get(&(t.parents[*p], *col)).cloned(),
                        _ => None,
                    };
                    if let Some(v) = value {
                        args.push(v);
                        if let Port::Parent(p, col) = &t.ports[*slot] {
                            links.push(json!({"callee_input":parameter,"target_input":slot,"source_event":c.records[t.parents[*p]].id,"source_output":col}));
                        }
                    } else {
                        missing.push(key.clone())
                    }
                }
                _ => missing.push(key.clone()),
            }
        }
        if !missing.is_empty() {
            recursive_ports.push(json!({"site":site,"target_event":t.id,"status":"needs_boundary","missing":missing}));
            continue;
        }
        let mut call_outputs = vec![];
        for port in 0..c.records[entry].outputs.len() {
            let name = format!("rec_{site}_{port}");
            recursive_defs.push((name.clone(), port as i64, args.clone()));
            call_outputs.push(name);
        }
        recursive_ports.push(json!({"site":site,"target_event":t.id,"status":"linked","callee":callee,"outputs":call_outputs,"input_bindings":args,"input_links":links}));
    }
    let mut inputs: Vec<_> = boundary.values().map(String::as_str).collect();
    inputs.sort_by_key(|n| n[1..].parse::<usize>().unwrap());
    let mut defs: Vec<_> = definitions
        .iter()
        .map(|(name, expression)| Definition::Let { name, expression })
        .collect();
    for (name, output, args) in &recursive_defs {
        defs.push(Definition::Recur {
            name,
            callee,
            output: *output,
            arguments: args.iter().map(String::as_str).collect(),
        });
    }
    let mut roots: Vec<_> = output_keys
        .iter()
        .map(|key| outputs[key].as_str())
        .collect();
    roots.extend(recursive_defs.iter().map(|(n, _, _)| n.as_str()));
    let reqs: Vec<_> = requirements.iter().map(String::as_str).collect();
    let effs: Vec<_> = effects.iter().map(String::as_str).collect();
    math_calls += definitions
        .iter()
        .map(|(_, e)| {
            ["EAdd", "ESub", "EMul", "EIAdd", "EISub", "EIMul"]
                .iter()
                .map(|op| e.matches(op).count())
                .sum::<usize>()
        })
        .sum::<usize>();
    let dag = compile_contract(&inputs, &defs, &roots, &reqs, &effs)?;
    // Stable local parameter values keep this reduction symbolic, not data-specialized.
    let mut args = "(BNil)".to_owned();
    for i in (0..inputs.len()).rev() {
        args = format!("(BCons (EParam {i}) {args})");
    }
    let source = format!(
        "(ObservedCalleeLayer {} {fractal} {dag})\n(LayerReductionInput {fractal} {dag} {args})\n(run-schedule (saturate (run higher)))\n(run-schedule (saturate (run endpoint-reduce)))",
        serde_json::to_string(callee)?
    );
    eg.parse_and_run_program(None, &source)?;
    let query = egglog::ast::Parser::default()
        .get_expr_from_string(None, &format!("(ReduceDag {dag} {args})"))?;
    let (sort, result) = eg.eval_expr(&query)?;
    let completed = eg.lookup_function("CompleteBinding", &[result]).is_some();
    if !completed {
        return Err("binding result has no fully substituted representative".into());
    }
    // Inspect actual native output representatives, rather than count BResult wrappers.
    let values_sort = eg.get_sort_by_name("BindingValues").unwrap();
    let expr_sort = eg.get_sort_by_name("EndpointExpr").unwrap();
    let result_id = eg.value_to_class_id(&sort, result);
    let mut output_list = None;
    eg.function_for_each("BResult", |row| {
        if eg.value_to_class_id(&sort, row.vals[3]) == result_id {
            output_list = Some(row.vals[0]);
        }
    })?;
    let mut cons = BTreeMap::new();
    eg.function_for_each("BCons", |row| {
        cons.insert(
            eg.value_to_class_id(values_sort, row.vals[2]).to_string(),
            (row.vals[0], row.vals[1]),
        );
    })?;
    let mut cursor = output_list.ok_or("missing completed result")?;
    let mut endpoint_outputs = vec![];
    let extractor = egglog::extract::Extractor::compute_costs_from_rootsorts(
        Some(vec![expr_sort.clone()]),
        eg,
        egglog::extract::TreeAdditiveCostModel::default(),
    );
    let mut termdag = egglog::TermDag::default();
    for _ in 0..outputs.len() {
        let (value, tail) = cons
            .get(&eg.value_to_class_id(values_sort, cursor).to_string())
            .ok_or("incomplete output list")?;
        let (_, term) = extractor
            .extract_best(eg, &mut termdag, *value)
            .ok_or("unextractable endpoint")?;
        endpoint_outputs.push(termdag.to_string(term));
        cursor = *tail;
    }
    Ok(
        json!({"status":"substituted", "endpoint_outputs":endpoint_outputs, "recursive_ports":recursive_ports,"recursive_output_count":recursive_defs.len(),"requirement_count":requirements.len(),"definedness_obligations":definedness_obligations,"effect_count":effects.len(),"math_calls":math_calls,"algebra":if math{"math"}else{"integer-safe"}, "native_result_class":eg.value_to_class_id(&sort, result).to_string(), "scope":"One-layer value/requirement/effect contract with witnessed recursive return bindings. CompleteBinding certifies a substituted representative, not satisfied guards or an arbitrary-depth invariant. Effects are descriptions, never executed.", "dag":dag,"environment":args,"source":source,"output_addresses":addresses,"boundary":boundary,"definitions":definitions.len(),"one_layer_events":members.iter().map(|i|c.records[*i].id).collect::<Vec<_>>()}),
    )
}
