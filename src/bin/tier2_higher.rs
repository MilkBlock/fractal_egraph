//! Extract HigherRule views derived by native power folding of tier-1 edges.
use serde_json::json;
use std::collections::BTreeMap;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        &std::fs::read_to_string("experiments/tier2/higher.egg")?,
    )?;
    let mut cn = BTreeMap::new();
    let mut en = BTreeMap::new();
    eg.function_for_each("CombName", |r| {
        cn.insert(
            r.vals[0],
            eg.value_to_base::<egglog::sort::S>(r.vals[1])
                .as_str()
                .to_owned(),
        );
    })?;
    eg.function_for_each("ExtensionName", |r| {
        en.insert(
            r.vals[0],
            eg.value_to_base::<egglog::sort::S>(r.vals[1])
                .as_str()
                .to_owned(),
        );
    })?;
    let mut hs = BTreeMap::new();
    eg.function_for_each("HigherRule",|r|{
  let k=eg.value_to_base::<i64>(r.vals[0]);let binding=eg.extract_value_to_string(eg.get_sort_by_name("RelativeBinding").unwrap(),r.vals[3]).unwrap().0;
  hs.insert(r.vals[4],json!({"count":k,"operator":en[&r.vals[1]],"ctx":cn[&r.vals[2]],"binding":binding,"egg":format!("(HigherRule {k} ${} {} {binding})",en[&r.vals[1]],cn[&r.vals[2]]),"represents":[]}));
 })?;
    eg.function_for_each("Represents", |r| {
        hs.get_mut(&r.vals[0]).unwrap()["represents"]
            .as_array_mut()
            .unwrap()
            .push(json!(cn[&r.vals[1]]))
    })?;
    let mut values = hs.into_values().collect::<Vec<_>>();
    values.sort_by_key(|r| {
        (
            -(r["count"].as_i64().unwrap()),
            r["ctx"].as_str().unwrap().to_owned(),
            r["operator"].as_str().unwrap().to_owned(),
        )
    });
    std::fs::write(
        "experiments/tier2/higher_native.json",
        serde_json::to_string_pretty(
            &json!({"scope":"Native HigherRule(k, extension, ctx, initial binding) views of existing closed unary tier-1 paths. No new arbitrary-k rule applications and no full-effect equality claims beyond witnessed folding.","higher_rules":values}),
        )? + "\n",
    )?;
    Ok(())
}
