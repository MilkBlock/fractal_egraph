//! Execute the tier-2 relations in native egglog, exporting witnessed repeats.
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(Some(a[1].clone()), &std::fs::read_to_string(&a[1])?)?;
    let mut counts = serde_json::Map::new();
    for name in [
        "ParentRole",
        "Outside",
        "Construct",
        "End",
        "Next",
        "Schema",
        "Extend",
        "Translate",
        "Compose",
        "Power",
        "PreservesLinear",
        "At",
        "Before",
        "Repeat",
        "Fits",
        "Counterexample",
        "NeedsBoundary",
        "InjectionEdge",
    ] {
        let mut count = 0;
        eg.function_for_each(name, |r| {
            if !r.subsumed {
                count += 1
            }
        })?;
        counts.insert(name.into(), json!(count));
    }
    let mut repeats = vec![];
    eg.function_for_each("Repeat",|r|repeats.push(json!({"start":eg.value_to_base::<i64>(r.vals[0]),"end":eg.value_to_base::<i64>(r.vals[1]),"slot":eg.value_to_base::<i64>(r.vals[3]),"length":eg.value_to_base::<i64>(r.vals[4])})))?;
    repeats.sort_by_key(|r| (r["start"].as_i64(), r["end"].as_i64(), r["length"].as_i64()));
    let mut counterexamples = vec![];
    eg.function_for_each("Counterexample",|r|counterexamples.push(json!({"series":eg.value_to_base::<egglog::sort::S>(r.vals[0]).as_str(),"before":[eg.value_to_base::<i64>(r.vals[1]),eg.value_to_base::<i64>(r.vals[2])],"after":[eg.value_to_base::<i64>(r.vals[3]),eg.value_to_base::<i64>(r.vals[4])]})))?;
    std::fs::write(
        &a[2],
        serde_json::to_string_pretty(
            &json!({"scope":"Native tier-2 relation evaluation; witnessed repeats and finite translation checks, no inferred equivalence unions.","counts":counts,"repeats":repeats,"counterexamples":counterexamples}),
        )? + "\n",
    )?;
    Ok(())
}
