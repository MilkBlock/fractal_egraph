//! Registered coarse facts are queries, not synthesized rules. Matching occurs in
//! cached native joins; probes never insert result rows or run user rules.
use super::*;
use crate::saturated_rule_composition::SaturatedRuleComposition as State;
pub(super) struct CachedQuery {
    query: egglog::CompiledFactQuery,
    initial: Vec<Vec<Value>>,
    executions: usize,
}
pub(super) fn observe(
    task: &mut ripen::Session,
    pattern: &State,
    marker: &str,
    key: usize,
) -> Result<Json> {
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
    if !task.weak_queries.contains_key(&key) {
        let mut parser = egglog::ast::Parser::default();
        let values = pattern
            .values
            .iter()
            .map(|v| -> Result<_> {
                let literal = v
                    .literal
                    .as_ref()
                    .map(|text| -> Result<_> {
                        match parser.get_expr_from_string(None, text)? {
                            Expr::Lit(_, l) => Ok(l),
                            _ => Err("query constant must be a literal".into()),
                        }
                    })
                    .transpose()?;
                Ok((v.sort.clone(), literal))
            })
            .collect::<Result<Vec<_>>>()?;
        let atoms: Vec<_> = rows
            .iter()
            .map(|r| {
                (
                    r.op.clone(),
                    r.args
                        .iter()
                        .copied()
                        .chain((pattern.values[r.result].sort != "Unit").then_some(r.result))
                        .collect(),
                )
            })
            .collect();
        let initial_query = task.before.compile_fact_query(&values, &atoms, &vars)?;
        let initial = task.before.run_fact_query(&initial_query);
        task.before.free_fact_query(initial_query);
        let initial = initial?;
        let query = task.eg.compile_fact_query(&values, &atoms, &vars)?;
        task.weak_queries.insert(
            key,
            CachedQuery {
                query,
                initial,
                executions: 0,
            },
        );
    }
    let cached = task.weak_queries.get_mut(&key).unwrap();
    let current = task.eg.run_fact_query(&cached.query)?;
    cached.executions += 1;
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
    let initial: BTreeSet<_> = cached.initial.iter().cloned().map(&canonical).collect();
    let now: BTreeSet<_> = current.into_iter().map(canonical).collect();
    let reentries: Vec<_> = now.difference(&initial).cloned().collect();
    Ok(
        json!({"query_cache":{"compilations":2,"initial_scans":1,"current_scans":cached.executions,"per_check_engine_clones":0},"status":if now.is_empty(){"not_matched"}else{"matched"},"pattern_variables":vars,"trigger_rows":rows,"typed_values":pattern.values,"initial_bindings":initial,"bindings":now,"new_bindings":reentries,"new_effects":task.has_effects,"scope":if task.done{"saturated local body"}else{"running prefix"},"classification":"observation only; no termination or dissipativity inference"}),
    )
}
