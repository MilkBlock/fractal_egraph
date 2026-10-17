//! GPU compiler conformance on every original rule of a real checkpoint packet.
//! Explicit rows are used as inputs; this does NOT manufacture Cell guarantees.
use cell_assisted_egglog::{Session, resident::ResidentSession};
use egglog::{
    EGraph,
    ast::{Command, GenericSchedule, Schedule},
};
use serde_json::json;
use std::{collections::BTreeSet, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn sets(s: &Schedule, out: &mut BTreeSet<String>) {
    match s {
        GenericSchedule::Run(_, r) => {
            out.insert(r.ruleset.clone());
        }
        GenericSchedule::Saturate(_, s) | GenericSchedule::Repeat(_, _, s) => sets(s, out),
        GenericSchedule::Sequence(_, xs) => {
            for x in xs {
                sets(x, out)
            }
        }
    }
}
pub fn check(base: &EGraph, commands: &[Command], worker: &Path, dest: &Path) -> Result<()> {
    let mut rulesets = BTreeSet::new();
    for c in commands {
        if let Command::RunSchedule(s) = c {
            sets(s, &mut rulesets)
        }
    }
    let mut rows = vec![];
    for set in &rulesets {
        for name in base.cell_rule_names(set)? {
            let session = Session::for_query_validation(base.clone(), set, &name)?;
            let mut resident = ResidentSession::new(session, worker)?;
            let bindings = resident.drain_matches()?;
            assert_eq!(
                resident.stats.audit_uncovered_per_round,
                vec![0],
                "native match set not covered: {name}"
            );
            rows.push(json!({"ruleset":set,"rule":name,"matched_bindings":bindings.len(),"exact_native_binding_set":true,"gpu_plan":resident.describe(),"stats":resident.stats}));
            std::fs::write(
                dest,
                serde_json::to_vec_pretty(
                    &json!({"scope":"GPU LHS compiler conformance on explicit native snapshot rows. No Cell admission or RHS execution on this diagnostic path; full native query is only the oracle, not candidate generation.","complete":false,"rules":rows}),
                )?,
            )?;
            println!(
                "validated {}/{}: {} bindings",
                set,
                rows.len(),
                bindings.len()
            );
        }
    }
    std::fs::write(
        dest,
        serde_json::to_vec_pretty(
            &json!({"scope":"GPU LHS compiler conformance on explicit native snapshot rows. No Cell admission or RHS execution on this diagnostic path; full native query is only the oracle, not candidate generation.","complete":true,"rule_count":rows.len(),"matched_bindings":rows.iter().map(|r|r["matched_bindings"].as_u64().unwrap()).sum::<u64>(),"rules":rows}),
        )?,
    )?;
    Ok(())
}
