//! Lower finite witnessed multi-parent interfaces; no guessed arithmetic semantics.
use super::*;
use crate::binding_reduce::{Definition, compile};

pub(super) fn reduce(
    c: &Captured,
    members: &BTreeSet<usize>,
    eg: &mut EGraph,
    fractal: &Expr,
) -> Result<Json> {
    let mut boundary = BTreeMap::<String, String>::new();
    let mut outputs = BTreeMap::<(usize, usize), String>::new();
    let mut definitions = Vec::<(String, String)>::new();
    let mut addresses = vec![];
    fn expr(e: &Expr, env: &BTreeMap<String, String>) -> std::result::Result<String, String> {
        Ok(match e {
            Expr::Var(_, n) => env
                .get(n)
                .cloned()
                .ok_or_else(|| format!("unresolved source binding {n}"))?,
            Expr::Lit(_, Literal::Int(n)) => n.to_string(),
            Expr::Lit(..) => return Err("non-integer source literal".into()),
            Expr::Call(_, op, args) => format!(
                "(source::{op} {})",
                args.iter()
                    .map(|a| expr(a, env))
                    .collect::<std::result::Result<Vec<_>, _>>()?
                    .join(" ")
            ),
        })
    }
    // Captured parents precede consumers. Branches may reference any prior parent.
    for &index in members {
        let r = &c.records[index];
        let rule = &c.rules[r.rule];
        let mut env = BTreeMap::new();
        let mut reads = BTreeMap::new();
        for (input, port) in r.inputs.iter().zip(&r.ports) {
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
                    boundary
                        .entry(key)
                        .or_insert_with(|| format!("p{next}"))
                        .clone()
                }
            };
            match input {
                Input::Var(name) => {
                    env.insert(name.clone(), value);
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
                        definitions.push((local.clone(), format!("(source-column-{col} {row})")));
                        env.insert(name.clone(), local);
                    }
                }
            }
            for fact in &rule.rule.body {
                if let Fact::Eq(_, Expr::Var(_, name), rhs) = fact {
                    if rhs == call && !env.contains_key(name) {
                        let local = format!("v{}", definitions.len());
                        definitions.push((
                            local.clone(),
                            format!("(source-column-{} {row})", args.len()),
                        ));
                        env.insert(name.clone(), local);
                    }
                }
            }
        }
        // Action-local lets are definitions, never equality assertions.
        for action in &rule.rule.head.0 {
            if let Action::Let(_, name, value) = action {
                let source = expr(value, &env)?;
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
                        expr(&args[*col], &env)?
                    } else if *col == args.len() {
                        expr(call, &env)?
                    } else {
                        return Err("invalid output column".into());
                    }
                }
                Output::Row(span) => format!(
                    "(source-row {})",
                    expr(&rule.calls.get(span).ok_or("missing row AST")?.1, &env)?
                ),
            };
            let name = format!("v{}", definitions.len());
            definitions.push((name.clone(), source));
            outputs.insert((index, slot), name);
            addresses.push(json!([r.id, slot]));
        }
    }
    let mut inputs: Vec<_> = boundary.values().map(String::as_str).collect();
    inputs.sort_by_key(|n| n[1..].parse::<usize>().unwrap());
    let defs: Vec<_> = definitions
        .iter()
        .map(|(name, expression)| Definition::Let { name, expression })
        .collect();
    let roots: Vec<_> = outputs.values().map(String::as_str).collect();
    let dag = compile(&inputs, &defs, &roots, &[])?;
    // Stable local parameter values keep this reduction symbolic, not data-specialized.
    let mut args = "(BNil)".to_owned();
    for i in (0..inputs.len()).rev() {
        args = format!("(BCons (EParam {i}) {args})");
    }
    let source = format!(
        "(LayerReductionInput {fractal} {dag} {args})\n(run-schedule (saturate (run higher)))\n(run-schedule (saturate (run endpoint-reduce)))"
    );
    eg.parse_and_run_program(None, &source)?;
    let query = egglog::ast::Parser::default()
        .get_expr_from_string(None, &format!("(ReduceDag {dag} {args})"))?;
    let (sort, result) = eg.eval_expr(&query)?;
    let canonical = eg.value_to_class_id(&sort, result);
    let mut completed = false;
    eg.function_for_each("BResult", |row| {
        completed |= eg.value_to_class_id(&sort, row.vals[2]) == canonical;
    })?;
    if !completed {
        return Err("binding reduction did not reach BResult".into());
    }
    Ok(
        json!({"status":"reduced", "native_result_class":eg.value_to_class_id(&sort, result).to_string(), "scope":"Finite witnessed value interface, not a recurrence proof. Source calls remain opaque. Committed row/union effects and keyed-read requirements remain in the original instance certificates; they are not executed or reduced here.", "dag":dag,"environment":args,"source":source,"output_addresses":addresses,"boundary":boundary,"definitions":definitions.len(),"one_layer_events":members.iter().map(|i|c.records[*i].id).collect::<Vec<_>>()}),
    )
}
