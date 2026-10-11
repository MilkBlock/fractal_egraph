//! Parser-only, lossless command inventory for scope splitting. Native Sexp
//! spans preserve comments, `_`, strings and annotations without AST printing.
use egglog::ast::{Action, Command, GenericSchedule, Parser, SimpleMacro, Span};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    sync::{Arc, Mutex},
};

fn runs(schedule: &egglog::ast::Schedule, out: &mut BTreeSet<String>) {
    match schedule {
        GenericSchedule::Run(_, config) => {
            out.insert(config.ruleset.clone());
        }
        GenericSchedule::Repeat(_, _, inner) | GenericSchedule::Saturate(_, inner) => {
            runs(inner, out)
        }
        GenericSchedule::Sequence(_, items) => {
            for item in items {
                runs(item, out);
            }
        }
    }
}

fn describe(command: &Command) -> Value {
    match command {
        Command::Push(n) => json!({"kind":"push","count":n}),
        Command::Pop(_, n) => json!({"kind":"pop","count":n}),
        Command::Rule { rule } => json!({"kind":"rule","ruleset":rule.ruleset}),
        Command::Rewrite(name, ..) | Command::BiRewrite(name, ..) => {
            json!({"kind":"rule","ruleset":name})
        }
        Command::AddRuleset(_, name) => json!({"kind":"ruleset","name":name}),
        Command::RunSchedule(schedule) => {
            let mut names = BTreeSet::new();
            runs(schedule, &mut names);
            json!({"kind":"schedule","rulesets":names})
        }
        Command::Datatype { .. }
        | Command::Datatypes { .. }
        | Command::Constructor { .. }
        | Command::Sort { .. }
        | Command::Function { .. }
        | Command::Relation { .. } => json!({"kind":"declaration"}),
        Command::Action(action) if !matches!(action, Action::Panic(..)) => json!({"kind":"action"}),
        Command::Check(..) => json!({"kind":"check"}),
        Command::Extract(..) => json!({"kind":"extract"}),
        Command::Fail(_, inner) if matches!(inner.as_ref(), Command::Check(..)) => {
            json!({"kind":"check"})
        }
        // Includes, combined rulesets, proof commands and external file IO need
        // explicit handling; never guess which rulesets/effects they introduce.
        _ => json!({"kind":"unsupported","command":command.to_string()}),
    }
}

fn export(filename: &str, source: &str) -> Result<Value, Box<dyn Error>> {
    // A parser macro gives access to native top-level Sexp spans, including Push
    // (whose Command representation itself carries no span). It executes nothing.
    let mut name = "split-capture".to_owned();
    while source.contains(&name) {
        name.push('x');
    }
    let prefix = format!("({name}\n");
    let shift = prefix.len();
    let result = Arc::new(Mutex::new(Vec::new()));
    let captured = result.clone();
    let mut parser = Parser::default();
    parser.add_command_macro(Arc::new(SimpleMacro::new(&name, move |args, _, parser| {
        let mut previous = shift;
        for arg in args {
            let span = arg.span();
            let commands = parser.parse_command(arg)?;
            let mut record = if commands.len() == 1 {
                describe(&commands[0])
            } else {
                json!({"kind":"unsupported","command":"command expands into multiple AST commands"})
            };
            if let Span::Egglog(s) = span {
                let start = s.i - shift;
                let end = s.j - shift;
                // Count source newlines rather than wrapper coordinates.
                let line = s.file.contents[shift..s.i]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count()
                    + 1;
                record["line"] = json!(line);
                record["start"] = json!(start);
                record["end"] = json!(end);
                record["text"] = json!(&s.file.contents[s.i..s.j]);
                record["leading"] = json!(&s.file.contents[previous..s.i]);
                previous = s.j;
            }
            captured.lock().unwrap().push(record);
        }
        Ok(vec![])
    })));
    parser.get_program_from_string(Some(filename.to_owned()), &format!("{prefix}{source}\n)"))?;
    let records = result.lock().unwrap();
    let end = records.last().and_then(|r| r["end"].as_u64()).unwrap_or(0) as usize;
    Ok(
        json!({"schema":"egglog-split-ast/v1","parse_only":true,"commands":*records,"trailing":&source[end..]}),
    )
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: split_ast SOURCE.egg OUTPUT.json".into());
    }
    let source = fs::read_to_string(&args[1])?;
    fs::write(
        &args[2],
        serde_json::to_string_pretty(&export(&args[1], &source)?)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_spans_keep_wildcards_and_quoted_fake_commands() {
        let source =
            "; 标注 (pop)\n(push 2)\n(rule ((= x (F _))) ((F \"(push)\")) :ruleset a)\n(pop 2)\n";
        let data = export("test.egg", source).unwrap();
        let commands = data["commands"].as_array().unwrap();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0]["line"], 2);
        assert_eq!(commands[0]["count"], 2);
        assert!(commands[1]["text"].as_str().unwrap().contains("(F _)"));
        let reconstructed = commands
            .iter()
            .map(|c| {
                format!(
                    "{}{}",
                    c["leading"].as_str().unwrap(),
                    c["text"].as_str().unwrap()
                )
            })
            .collect::<String>()
            + data["trailing"].as_str().unwrap();
        assert_eq!(reconstructed, source);
    }
    #[test]
    fn nested_schedules_and_fail_check_are_classified_natively() {
        let data = export("test.egg", "(run-schedule (repeat 3 (seq (run a :until (= x y)) (saturate (run prune)))))\n(fail (check (= a b)))").unwrap();
        assert_eq!(data["commands"][0]["rulesets"], json!(["a", "prune"]));
        assert_eq!(data["commands"][1]["kind"], "check");
    }
}
