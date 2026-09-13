//! Native typecheck and finite source-replay validation of direct tier-1 lowering.
use egglog::ast::Command;
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = "experiments/tier1_extract/";
    let cases: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!(
        "{dir}validation_cases.json"
    ))?)?;
    let mut parser = egglog::EGraph::default();
    let commands = parser.parse_program(
        None,
        &std::fs::read_to_string(format!("{dir}tier0_rules.egg"))?,
    )?;
    let mut prefix = String::new();
    for c in commands {
        match c {
            Command::Rule { mut rule } => {
                let rs = format!("source-{}", rule.name);
                prefix += &format!("(ruleset {rs})\n");
                rule.ruleset = rs;
                prefix += &format!("{}\n", Command::Rule { rule });
            }
            other => prefix += &format!("{other}\n"),
        }
    }
    let mut passed = 0;
    let mut failures = vec![];
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut eg = egglog::EGraph::default();
        let input = format!(
            "{prefix}\n(ruleset tier1-combined)\n{}\n{}\n{}\n{}",
            case["rule"].as_str().unwrap(),
            case["setup"].as_str().unwrap(),
            case["schedule"].as_str().unwrap(),
            case["checks"].as_str().unwrap()
        );
        match eg.parse_and_run_program(None, &input) {
            Ok(_) => passed += 1,
            Err(e) => failures.push(json!({"name":name,"error":e.to_string()})),
        }
        if (passed + failures.len()) % 100 == 0 {
            eprintln!("checked {}", passed + failures.len());
        }
    }
    let result = json!({"cases":cases.as_array().unwrap().len(),"passed":passed,"failures":failures,"scope":"Native egglog typecheck plus finite per-rule source-sequence replay on symbolic boundary seeds. Every emitted constructor and union is checked without executing the combined rule. This is regression evidence, not a universal equivalence proof or a speedup benchmark."});
    std::fs::write(
        format!("{dir}validation.json"),
        serde_json::to_string_pretty(&result)? + "\n",
    )?;
    println!("passed={passed}, failed={}", failures.len());
    if !failures.is_empty() {
        return Err("source replay failed".into());
    }
    Ok(())
}
