//! Registered coarse facts are queries, not synthesized rules. Matching occurs in
//! a private native clone; probes never modify the ripen task or its rule scope.
use super::*;
use crate::saturated_rule_composition::SaturatedRuleComposition as State;
pub(super) fn observe(task: &ripen::Session, pattern: &State, marker: &str) -> Result<Json> {
    let Some(source) = &task.initial else {
        return Ok(json!({"status":"unsupported_source"}));
    };
    if source.scope != pattern.scope {
        return Ok(json!({"status":"incompatible_environment"}));
    }
    if !pattern.subsumed_rows.is_empty() {
        return Ok(json!({"status":"unsupported_hidden_trigger"}));
    }
    let rows: Vec<_> = pattern.rows.iter().filter(|r| r.op != marker).collect();
    if rows.is_empty() {
        return Ok(json!({"status":"no_structural_trigger"}));
    }
    let vars: BTreeSet<_> = rows
        .iter()
        .flat_map(|r| r.args.iter().chain(std::iter::once(&r.result)))
        .filter(|i| pattern.values[**i].literal.is_none())
        .copied()
        .collect();
    let vars: Vec<_> = vars.into_iter().collect();
    let term = |i: usize| {
        pattern.values[i]
            .literal
            .clone()
            .unwrap_or_else(|| format!("p{i}"))
    };
    let body = rows
        .iter()
        .map(|r| {
            let call = format!(
                "({}{})",
                r.op,
                r.args
                    .iter()
                    .map(|i| format!(" {}", term(*i)))
                    .collect::<String>()
            );
            if pattern.values[r.result].sort == "Unit" {
                call
            } else {
                format!("(= {} {call})", term(r.result))
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut name = "RipenObserved".to_string();
    while task.source_text().contains(&name) {
        name.push('_');
    }
    let query = format!(
        "(relation {name} ({})) (ruleset {name}Rules) (rule ({body}) (({name}{})) :ruleset {name}Rules) (run {name}Rules 1)",
        vars.iter()
            .map(|i| pattern.values[*i].sort.clone())
            .collect::<Vec<_>>()
            .join(" "),
        vars.iter().map(|i| format!(" p{i}")).collect::<String>()
    );
    let probe = |eg: &EGraph| -> Result<Vec<Vec<Value>>> {
        let mut e = eg.clone();
        e.parse_and_run_program(None, &query)?;
        let mut matches = vec![];
        e.function_for_each(&name, |r| matches.push(r.vals[..vars.len()].to_vec()))?;
        Ok(matches)
    };
    let canonical = |values: Vec<Value>| -> Vec<String> {
        values
            .into_iter()
            .zip(&vars)
            .map(|(v, i)| {
                let sort = task
                    .eg
                    .get_arcsort_by(|s| s.name() == pattern.values[*i].sort);
                task.eg.value_to_class_id(&sort, v).to_string()
            })
            .collect()
    };
    let initial: BTreeSet<_> = probe(&task.before)?.into_iter().map(&canonical).collect();
    let now: BTreeSet<_> = probe(&task.eg)?.into_iter().map(canonical).collect();
    let reentries: Vec<_> = now.difference(&initial).cloned().collect();
    Ok(
        json!({"status":if now.is_empty(){"not_matched"}else{"matched"},"pattern_variables":vars,"trigger_rows":rows,"typed_values":pattern.values,"initial_bindings":initial,"bindings":now,"new_bindings":reentries,"new_effects":task.has_effects,"scope":if task.done{"saturated local body"}else{"running prefix"},"classification":"observation only; no termination or dissipativity inference"}),
    )
}
