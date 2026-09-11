//! Finite, binding-linked integration-by-parts family coverage.
#[allow(dead_code)]
#[path = "tier0_probe.rs"]
mod probe;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 4 {
        return Err("integration_family source.egg seed-expression output.json".into());
    }
    let result = probe::integration_family(&a[1], &a[2])?;
    std::fs::write(&a[3], serde_json::to_string_pretty(&result)?)?;
    println!(
        "{}",
        serde_json::json!({"original_nodes":result["original_enodes"],"binding_states":result["lhs_states"],"rooted_seeds":result["entry_class_seed_count"],"unchanged":result["original_rows_unchanged"]})
    );
    Ok(())
}
