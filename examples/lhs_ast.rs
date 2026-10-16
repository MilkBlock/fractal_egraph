//! Export surface rule bodies using egglog's own parser, without executing or
//! typechecking any command. Run with `lhs_ast SOURCE.egg OUTPUT.json`.
//!
//! Rule and command ordinals are zero-based. Variables retain their parsed
//! names and are scoped by their containing rule. Rewrites are not expanded,
//! includes are not loaded, and no runtime matches or mutations are reported.

use egglog::{
    EGraph,
    ast::{Action, Command, Expr, Fact, Literal, Span, Subdatatypes, Variant},
};
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, error::Error, fs, io::Write};

fn expression(expr: &Expr) -> Json {
    match expr {
        Expr::Var(_, name) => json!({"kind": "var", "name": name}),
        Expr::Call(_, head, args) => json!({
            "kind": "call",
            "head": head,
            "args": args.iter().map(expression).collect::<Vec<_>>(),
        }),
        Expr::Lit(_, literal) => {
            let (literal_kind, sort, value) = match literal {
                Literal::Int(value) => ("int", "i64", json!(value)),
                // A string also preserves non-finite floating-point values,
                // which cannot be represented as JSON numbers.
                Literal::Float(value) => ("float", "f64", json!(value.to_string())),
                Literal::String(value) => ("string", "String", json!(value)),
                Literal::Bool(value) => ("bool", "bool", json!(value)),
                Literal::Unit => ("unit", "Unit", Json::Null),
            };
            json!({
                "kind": "literal",
                "literal_kind": literal_kind,
                "sort": sort,
                "text": literal.to_string(),
                "value": value,
            })
        }
    }
}

fn action(action: &Action) -> Json {
    match action {
        Action::Let(_, name, expr) => json!({"kind":"let", "name":name, "expr":expression(expr)}),
        Action::Set(_, head, args, value) => json!({"kind":"set", "head":head,
            "args":args.iter().map(expression).collect::<Vec<_>>(), "value":expression(value)}),
        Action::Union(_, a, b) => json!({"kind":"union", "args":[expression(a), expression(b)]}),
        Action::Change(_, change, head, args) => {
            json!({"kind":"change", "change":format!("{change:?}"),
            "head":head,"args":args.iter().map(expression).collect::<Vec<_>>()})
        }
        Action::Expr(_, expr) => json!({"kind":"expr", "expr":expression(expr)}),
        Action::Panic(_, message) => json!({"kind":"panic", "message":message}),
    }
}

fn fact(fact: &Fact) -> Json {
    match fact {
        Fact::Eq(_, left, right) => json!({
            "kind": "eq", "args": [expression(left), expression(right)],
        }),
        Fact::Fact(expr) => json!({"kind": "fact", "expr": expression(expr)}),
    }
}

fn source_location(span: &Span) -> Json {
    match span {
        Span::Egglog(span) => {
            let (line, column) = span.file.get_location(span.i);
            json!({
                "byte_start": span.i, "byte_end": span.j,
                "line": line, "column": column,
                "text": &span.file.contents[span.i..span.j],
            })
        }
        _ => Json::Null,
    }
}

fn signature(kind: &str, name: &str, input: &[String], output: &str) -> Json {
    json!({"kind": kind, "name": name, "input": input, "output": output})
}

fn datatype(name: &str, variants: &[Variant]) -> Json {
    json!({
        "kind": "datatype", "name": name,
        "constructors": variants.iter().map(|variant| {
            signature("constructor", &variant.name, &variant.types, name)
        }).collect::<Vec<_>>(),
    })
}

fn schedule(s: &egglog::ast::Schedule) -> Json {
    use egglog::ast::GenericSchedule as S;
    fn unwrap(s: &egglog::ast::Schedule) -> &egglog::ast::Schedule {
        if let S::Sequence(_, items) = s {
            if items.len() == 1 {
                return unwrap(&items[0]);
            }
        }
        s
    }
    if let S::Saturate(_, inner) = unwrap(s) {
        if let S::Run(_, cfg) = unwrap(inner) {
            return json!({"kind":"saturate_single", "ruleset":cfg.ruleset,"has_until":cfg.until.is_some()});
        }
    }
    json!({"kind":"other"})
}

fn export(source_name: &str, source: &str) -> Result<Json, Box<dyn Error>> {
    let mut egraph = EGraph::default();
    let commands = egraph.parse_program(Some(source_name.to_owned()), source)?;
    let mut rules = Vec::new();
    let mut declarations = Vec::new();
    let mut other_commands = Vec::new();
    let mut actions = Vec::new();

    for (command_ordinal, command) in commands.iter().enumerate() {
        if let Command::Action(a) = command {
            actions.push(json!({"command_ordinal":command_ordinal,"action":action(a)}));
        }
        match command {
            Command::Rule { rule } => rules.push(json!({
                "ordinal": rules.len(),
                "command_ordinal": command_ordinal,
                "name": rule.name,
                "ruleset": rule.ruleset,
                "naive": rule.naive,
                "body": rule.body.iter().map(fact).collect::<Vec<_>>(),
                "head": rule.head.0.iter().map(action).collect::<Vec<_>>(),
                "head_text": format!("({})", rule.head.0.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")),
                "body_text": format!("({})", rule.body.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")),
                "rule_text": rule.to_string(),
                "source": source_location(&rule.span),
            })),
            Command::Datatype { name, variants, .. } => {
                declarations.push(datatype(name, variants));
            }
            Command::Datatypes { datatypes, .. } => {
                for (_, name, subdatatype) in datatypes {
                    declarations.push(match subdatatype {
                        Subdatatypes::Variants(variants) => datatype(name, variants),
                        Subdatatypes::NewSort(presort, args) => json!({
                            "kind": "sort", "name": name, "presort": presort,
                            "args": args.iter().map(expression).collect::<Vec<_>>(),
                        }),
                    });
                }
            }
            Command::Constructor { name, schema, .. } => {
                declarations.push(signature("constructor", name, &schema.input, &schema.output));
            }
            Command::Relation { name, inputs, .. } => {
                declarations.push(signature("relation", name, inputs, "Unit"));
            }
            Command::Function { name, schema, merge, .. } => {
                let mut sig = signature("function", name, &schema.input, &schema.output);
                sig["merge"] = merge.as_ref().map(expression).unwrap_or(Json::Null);
                declarations.push(sig);
            }
            Command::Sort { name, presort_and_args, .. } => {
                let mut declaration = json!({"kind": "sort", "name": name});
                if let Some((presort, args)) = presort_and_args {
                    declaration["presort"] = json!(presort);
                    declaration["args"] = json!(args.iter().map(expression).collect::<Vec<_>>());
                }
                declarations.push(declaration);
            }
            _ => other_commands.push(json!({
                "command_ordinal": command_ordinal, "text": command.to_string(),
            })),
        }
    }

    // Keep the declaration list lossless even if input repeats a name. The
    // convenience index includes only unambiguous signatures; this parser-only
    // export deliberately does not decide whether declarations are well typed.
    let mut grouped: BTreeMap<String, Vec<Json>> = BTreeMap::new();
    for declaration in &declarations {
        let candidates = match declaration["constructors"].as_array() {
            Some(constructors) => constructors.as_slice(),
            None if declaration.get("input").is_some() => std::slice::from_ref(declaration),
            None => continue,
        };
        for candidate in candidates {
            grouped
                .entry(candidate["name"].as_str().unwrap().to_owned())
                .or_default()
                .push(candidate.clone());
        }
    }
    let mut signatures = BTreeMap::new();
    let mut duplicate_signatures = BTreeMap::new();
    for (name, candidates) in grouped {
        if candidates.len() == 1 {
            signatures.insert(name, candidates.into_iter().next().unwrap());
        } else {
            duplicate_signatures.insert(name, candidates);
        }
    }

    Ok(json!({
        "format": "egglog-lhs-ast/v1",
        "source": source_name,
        "parse_only": true,
        "command_count": commands.len(),
        "declarations": declarations,
        "signatures": signatures,
        "duplicate_signatures": duplicate_signatures,
        "rules": rules,
        // Surface rewrite patterns are useful for shared matching circuits. Keep
        // them separate from Rule bodies so existing consumers retain semantics.
        "rewrites": commands.iter().enumerate().filter_map(|(ordinal, command)| {
            match command {
                Command::Rewrite(ruleset, rewrite, subsume) => Some(json!({
                    "command_ordinal":ordinal,"ruleset":ruleset,"name":rewrite.name,
                    "lhs":expression(&rewrite.lhs),"rhs":expression(&rewrite.rhs),
                    "conditions":rewrite.conditions.iter().map(fact).collect::<Vec<_>>(),
                    "subsume":subsume,"bidirectional":false,
                    "source":source_location(&rewrite.span),
                })),
                Command::BiRewrite(ruleset, rewrite) => Some(json!({
                    "command_ordinal":ordinal,"ruleset":ruleset,"name":rewrite.name,
                    "lhs":expression(&rewrite.lhs),"rhs":expression(&rewrite.rhs),
                    "conditions":rewrite.conditions.iter().map(fact).collect::<Vec<_>>(),
                    "subsume":false,"bidirectional":true,
                    "source":source_location(&rewrite.span),
                })),
                _ => None,
            }
        }).collect::<Vec<_>>(),
        "other_commands": other_commands,
        "actions":actions,
        "checks": commands.iter().enumerate().filter_map(|(ordinal, c)| {
            if let Command::Check(_, facts) = c { Some(json!({"ordinal":ordinal,"facts":facts.iter().map(fact).collect::<Vec<_>>()})) } else { None }
        }).collect::<Vec<_>>(),
        "rule_rulesets": commands.iter().filter_map(|c| match c {
            Command::Rule {rule} => Some(rule.ruleset.clone()),
            Command::Rewrite(name, ..) | Command::BiRewrite(name, ..) => Some(name.clone()),
            _ => None,
        }).collect::<std::collections::BTreeSet<_>>(),
        "schedules": commands.iter().filter_map(|c| {
            if let Command::RunSchedule(s) = c { Some(schedule(s)) } else {None}
        }).collect::<Vec<_>>(),
        "commands": commands.iter().enumerate().map(|(ordinal, c)| json!({
            "ordinal": ordinal, "text": c.to_string(),
            "kind": match c {
                Command::Action(_) => "action", Command::RunSchedule(_) => "schedule",
                Command::Check(..) => "check", Command::Rule{..} | Command::Rewrite(..) | Command::BiRewrite(..) => "rule",
                Command::Datatype{..} | Command::Datatypes{..} | Command::Constructor{..} |
                Command::Sort{..} | Command::Function{..} | Command::Relation{..} | Command::AddRuleset(..) => "declaration",
                _ => "unsupported",
            },
        })).collect::<Vec<_>>(),
    }))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: lhs_ast SOURCE.egg OUTPUT.json".into());
    }
    let source = fs::read_to_string(&args[1])?;
    let report = export(&args[1].to_string_lossy(), &source)?;
    let mut output = std::io::BufWriter::new(fs::File::create(&args[2])?);
    serde_json::to_writer_pretty(&mut output, &report)?;
    writeln!(output)?;
    output.flush()?;
    Ok(())
}
