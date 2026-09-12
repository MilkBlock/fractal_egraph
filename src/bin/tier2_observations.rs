//! Read recurrence coordinate transitions only after native tier-1 binding checks.
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "experiments/tier2/recurrence_tier1.egg";
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(Some(path.into()), &std::fs::read_to_string(path)?)?;
    let mut samples = vec![];
    eg.function_for_each("ObservedStep",|r|samples.push(json!({"series":eg.value_to_base::<egglog::sort::S>(r.vals[0]).as_str(),"depth":eg.value_to_base::<i64>(r.vals[1]),"before":[eg.value_to_base::<i64>(r.vals[2]),eg.value_to_base::<i64>(r.vals[3])],"after":[eg.value_to_base::<i64>(r.vals[4]),eg.value_to_base::<i64>(r.vals[5])]})))?;
    samples.sort_by_key(|s| {
        (
            s["series"].as_str().unwrap().to_owned(),
            s["depth"].as_i64().unwrap(),
        )
    });
    std::fs::write(
        "experiments/tier2/observations.json",
        serde_json::to_string_pretty(
            &json!({"scope":"Native tier-1 ObservedStep rows after all Binding checks. Coordinates are an additive-recursion adapter over checked native tier-0 edges, not a general automatic state discovery.","samples":samples}),
        )? + "\n",
    )?;
    Ok(())
}
