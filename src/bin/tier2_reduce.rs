//! Native cost extraction, before and after endpoint reduction.
use serde_json::json;
use std::collections::BTreeMap;
fn collect(eg: &egglog::EGraph) -> BTreeMap<String, (String, usize)> {
    let mut out = BTreeMap::new();
    eg.function_for_each("ReduceExample", |r| {
        let name = eg
            .value_to_base::<egglog::sort::S>(r.vals[0])
            .as_str()
            .to_owned();
        let (term, cost) = eg
            .extract_value_to_string(eg.get_sort_by_name("EndpointExpr").unwrap(), r.vals[1])
            .unwrap();
        out.insert(name, (term, cost as usize));
    })
    .unwrap();
    out
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        &std::fs::read_to_string("experiments/tier2/reduce.egg")?,
    )?;
    let before = collect(&eg);
    eg.parse_and_run_program(None, "(run-schedule (saturate (run endpoint-reduce)))")?;
    let after = collect(&eg);
    let mut rows = vec![];
    for (name, (term, cost)) in after {
        let closed = !term.contains("(Reduce ");
        let expected = matches!(
            name.as_str(),
            "constant-add" | "arithmetic-add" | "constant-multiply"
        );
        assert_eq!(closed, expected, "unexpected reduction for {name}");
        if expected {
            assert!(cost < before[&name].1);
        }
        rows.push(json!({"name":name,"before":before[&name].0,"before_cost":before[&name].1,"after":term,"after_cost":cost,"closed_form":closed}));
    }
    std::fs::write(
        "experiments/tier2/reduce.json",
        serde_json::to_string_pretty(
            &json!({"scope":"Native egglog constructor-cost extraction. Exact scalar endpoint identities; neither removal of intermediate runtime nodes nor automatic HigherRule accumulator discovery.","examples":rows}),
        )? + "\n",
    )?;
    Ok(())
}
