//! One parameterized Bop program reused across values, updates and aliases.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::{parameterized_cell::Runtime, table_live};
use egglog::EGraph;
use serde_json::json;
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 1 {
        return Err("NEW_OUTPUT".into());
    }
    let out = Path::new(&args[0]);
    std::fs::create_dir(out)?;
    EGraph::set_num_threads(1);
    let source = include_str!("../experiments/parameterized_cell/terms.egg");
    let mut base = EGraph::default();
    base.parse_and_run_program(None, source)?;
    let seed = r#"
      (set (ExtractedExpr (V 0)) (TCPair (TermLeaf 10) 100))
      (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 11) 9))
      (set (ExtractedExpr (V 2)) (TCPair (TermLeaf 12) 4))
      (Bop (Add) (V 0) (V 1))
      (Bop (Mul) (Bop (Add) (V 0) (V 1)) (V 2))
      (Bop (Add) (V 3) (V 2))
    "#;
    base.parse_and_run_program(None, seed)?;
    let schedule = "(run-schedule (saturate (seq terms (saturate (seq terms-helpers (saturate terms-helpers-helpers))))))";
    let commands = base.parse_program(None, schedule)?;
    let mut fused = base.clone();
    let mut reference = table_live::Runtime::new_cpu(base.clone(), &commands)?;
    let mut runtime = Runtime::new(
        base.clone(),
        &["terms", "terms-helpers", "terms-helpers-helpers"],
    )?;
    let mut changes = vec![
        ("initial", String::new()),
        (
            "improved-cost-and-term",
            "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 20) 30))".into(),
        ),
        (
            "equal-cost-new-term",
            "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 21) 30))".into(),
        ),
        (
            "worse-external-candidate",
            "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 22) 80))".into(),
        ),
        (
            "late-missing-port",
            "(set (ExtractedExpr (V 3)) (TCPair (TermLeaf 13) 7))".into(),
        ),
        ("union-input-ports", "(union (V 0) (V 1))".into()),
        ("new-cell-same-template", "(Bop (Mul) (V 0) (V 3))".into()),
        (
            "alias-term-pairs",
            "(union (TCPair (TermLeaf 11) 9) (TCPair (TermLeaf 99) 9))".into(),
        ),
    ];
    for cost in (0..8).rev() {
        changes.push((
            "repeated-update",
            format!(
                "(set (ExtractedExpr (V 3)) (TCPair (TermLeaf {}) {cost}))",
                100 + cost
            ),
        ));
    }
    changes.push(("quiescent", String::new()));
    let mut phases = vec![];
    for (index, (label, input)) in changes.iter().enumerate() {
        let reads_before = runtime.stats.keyed_reads;
        fused.parse_and_run_program(None, input)?;
        reference.input(input)?;
        runtime.input(input)?;
        runtime.audit_mirror()?;
        fused.run_program(commands.clone())?;
        reference.run(&commands)?;
        runtime.run(&commands)?;
        let checked_rows = runtime.audit_mirror()?;
        let check = compare::equal(&base, reference.engine(), runtime.engine());
        let native_check = compare::equal(&base, &fused, runtime.engine());
        let counts = runtime.report()["programs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p["rule"].as_str().unwrap().to_string(),
                    p["emitted_bindings"].as_u64().unwrap(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let reference_counts = reference.report()["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p["rule"].as_str().unwrap().to_string(),
                    p["stats"]["emitted_bindings"].as_u64().unwrap(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(counts, reference_counts);
        if *label == "quiescent" {
            assert_eq!(
                runtime.stats.keyed_reads, reads_before,
                "quiescent Run repeated local lookups"
            );
        }
        let row = json!({"phase":index,"label":label,"input":input,"same_submit":check.as_ref().ok(),"same_submit_error":check.as_ref().err().map(ToString::to_string),"fused_native":native_check.as_ref().ok(),"fused_error":native_check.as_ref().err().map(ToString::to_string),"mirror_rows_checked":checked_rows,"same_per_rule_binding_counts":true,"runtime":runtime.report()});
        eprintln!(
            "{index} {label}: same-submit={} fused={}",
            check.is_ok(),
            native_check.is_ok()
        );
        phases.push(row);
        std::fs::write(
            out.join("results.json"),
            serde_json::to_vec_pretty(
                &json!({"passed":check.is_ok() && native_check.is_ok(),"phases":phases,"reference":reference.report(),"runtime":runtime.report(),"cell_spine_templates":1,"rule_kernels":7,"scope":"Actual native effects, parameterized fixed wiring, CPU correctness experiment. No concrete closure catalog, no GPU performance or whole-saturation-skip claim."}),
            )?,
        )?;
        check?;
        native_check?;
    }
    std::fs::write(
        out.join("program.egg"),
        format!(
            "{source}\n{seed}\n{schedule}\n{}",
            changes
                .iter()
                .skip(1)
                .map(|(_, s)| format!("{s}\n{schedule}\n"))
                .collect::<String>()
        ),
    )?;
    Ok(())
}
