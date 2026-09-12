//! Observe edges in a finite native recurrence run; check their endpoint equality.
//! Edge rows are fixture instrumentation, not kernel mutation provenance.
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(Some(a[1].clone()), &std::fs::read_to_string(&a[1])?)?;
    let mut edges = vec![];
    eg.function_for_each("Edge", |r| {
        edges.push([
            eg.value_to_base::<i64>(r.vals[0]),
            eg.value_to_base::<i64>(r.vals[1]),
            eg.value_to_base::<i64>(r.vals[2]),
        ])
    })?;
    edges.sort();
    for [n, m, c] in &edges {
        eg.parse_and_run_program(
            None,
            &format!("(check (= (F {n}) (Add (F {m}) (Const {c}))))"),
        )?;
    }
    eg.parse_and_run_program(
        None,
        r#"
 (ruleset endpoint-normalize)
 (rewrite (Add (Add x y) z) (Add x (Add y z)) :ruleset endpoint-normalize)
 (rewrite (Add (Const x) (Const y)) (Const (+ x y)) :ruleset endpoint-normalize)
 (run-schedule (saturate (run endpoint-normalize)))
 "#,
    )?;
    let start = edges.iter().map(|e| e[0]).max().ok_or("no edges")?;
    let mut m = start;
    let mut total = 0i64;
    let mut path_checks = 0;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(edge) = edges.iter().find(|e| e[0] == m) {
        assert!(seen.insert(m), "cyclic fixture path");
        m = edge[1];
        total = total.checked_add(edge[2]).ok_or("accumulator overflow")?;
        eg.parse_and_run_program(
            None,
            &format!("(check (= (F {start}) (Add (F {m}) (Const {total}))))"),
        )?;
        path_checks += 1;
    }
    let result = json!({"source":a[1],"scope":"Actual native egglog fixture. Edge instrumentation records recurrence steps; endpoint equalities checked. Accumulated path weight is derived later, not a recorded runtime binding.","edges":edges,"endpoint_checks":edges.len(),"multi_step_endpoint_checks":path_checks});
    std::fs::write(&a[2], serde_json::to_string_pretty(&result)? + "\n")?;
    Ok(())
}
