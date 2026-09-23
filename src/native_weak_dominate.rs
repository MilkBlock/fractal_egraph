//! Registered coarse facts are queries, not synthesized rules. Matching occurs in
//! cached native joins; probes never insert result rows or run user rules.
use super::*;
use crate::saturated_rule_composition::SaturatedRuleComposition as State;
pub(super) struct CachedQuery {
    query: egglog::CompiledFactQuery,
    initial: Vec<Vec<Value>>,
    pub executions: usize,
    sorts: Vec<egglog::ArcSort>,
    current: Option<(usize, Vec<Vec<Value>>)>,
    pub cache_hits: usize,
}
pub(super) fn observe(
    task: &mut ripen::Session,
    pattern: &State,
    marker: &str,
    _key: usize,
    erase_payload: bool,
) -> Result<Json> {
    let Some(source) = (if erase_payload {
        &task.trigger
    } else {
        &task.initial
    }) else {
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
        .filter(|i| {
            pattern.values[**i].literal.is_none()
                || (erase_payload && pattern.values[**i].sort != "Unit")
        })
        .copied()
        .collect();
    let vars: Vec<_> = vars.into_iter().collect();
    // Number variables by first occurrence in the atoms, not exported vertex IDs.
    // Unused boundary vertices must not split otherwise identical query plans.
    let mut ids = BTreeMap::new();
    let mut signature_values = vec![];
    let mut atoms = vec![];
    for row in &rows {
        let mut columns = vec![];
        for old in row
            .args
            .iter()
            .copied()
            .chain((pattern.values[row.result].sort != "Unit").then_some(row.result))
        {
            let next = ids.len();
            let id = *ids.entry(old).or_insert_with(|| {
                let v = &pattern.values[old];
                signature_values.push((
                    v.sort.clone(),
                    v.literal
                        .clone()
                        .filter(|_| !erase_payload || v.sort == "Unit"),
                ));
                next
            });
            columns.push(id);
        }
        atoms.push((row.op.clone(), columns));
    }
    let projection: Vec<_> = signature_values
        .iter()
        .enumerate()
        .filter(|(_, (_, lit))| lit.is_none())
        .map(|(i, _)| i)
        .collect();
    let positions: BTreeMap<_, _> = projection
        .iter()
        .enumerate()
        .map(|(i, v)| (*v, i))
        .collect();
    let permutation: Vec<_> = vars.iter().map(|v| positions[&ids[v]]).collect();
    let key = serde_json::to_string(&(&signature_values, &atoms))?;
    if !task.weak_queries.contains_key(&key) {
        let mut parser = egglog::ast::Parser::default();
        let values = signature_values
            .iter()
            .map(|(sort, literal)| -> Result<_> {
                let lit = literal
                    .as_ref()
                    .map(|text| -> Result<_> {
                        match parser.get_expr_from_string(None, text)? {
                            Expr::Lit(_, l) => Ok(l),
                            _ => Err("query constant must be a literal".into()),
                        }
                    })
                    .transpose()?;
                Ok((sort.clone(), lit))
            })
            .collect::<Result<Vec<_>>>()?;
        let initial_query = task
            .before
            .compile_fact_query(&values, &atoms, &projection)?;
        let initial = task.before.run_fact_query(&initial_query);
        task.before.free_fact_query(initial_query);
        let initial = initial?;
        let query = task.eg.compile_fact_query(&values, &atoms, &projection)?;
        task.weak_queries.insert(
            key.clone(),
            CachedQuery {
                query,
                initial,
                executions: 0,
                sorts: projection
                    .iter()
                    .map(|i| {
                        task.eg
                            .get_arcsort_by(|s| s.name() == signature_values[*i].0)
                    })
                    .collect(),
                current: None,
                cache_hits: 0,
            },
        );
    }
    let cached = task.weak_queries.get_mut(&key).unwrap();
    if cached
        .current
        .as_ref()
        .is_none_or(|(epoch, _)| *epoch != task.query_epoch)
    {
        cached.current = Some((task.query_epoch, task.eg.run_fact_query(&cached.query)?));
        cached.executions += 1;
    } else {
        cached.cache_hits += 1;
    }
    let canonical = |values: &Vec<Value>| -> Vec<String> {
        permutation
            .iter()
            .map(|i| {
                task.eg
                    .value_to_class_id(&cached.sorts[*i], values[*i])
                    .to_string()
            })
            .collect()
    };
    let initial: BTreeSet<_> = cached.initial.iter().map(&canonical).collect();
    let now: BTreeSet<_> = cached
        .current
        .as_ref()
        .unwrap()
        .1
        .iter()
        .map(canonical)
        .collect();
    let reentries: Vec<_> = now.difference(&initial).cloned().collect();
    Ok(
        json!({"erased_payload":erase_payload,"query_cache":{"compilations":2,"initial_scans":1,"current_scans":cached.executions,"result_cache_hits":cached.cache_hits,"per_check_engine_clones":0},"status":if now.is_empty(){"not_matched"}else{"matched"},"pattern_variables":vars,"trigger_rows":rows,"typed_values":pattern.values,"initial_bindings":initial,"bindings":now,"new_bindings":reentries,"new_effects":task.has_effects,"scope":if task.done{"saturated local body"}else{"running prefix"},"classification":"observation only; no termination or dissipativity inference"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_typed_queries_share_across_target_ids_but_literals_remain_distinct() {
        let root = std::env::temp_dir().join(format!("weak-query-sharing-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("input.egg");
        std::fs::write(
            &src,
            "(datatype E (A i64) (B i64)) (rewrite (A n) (B n)) (A 1)",
        )
        .unwrap();
        let mut task = ripen::Session::new(&src, &root.join("cell")).unwrap();
        let pattern = task.initial.clone().unwrap();
        let a = observe(&mut task, &pattern, "", 1, false).unwrap();
        let b = observe(&mut task, &pattern, "", 2, false).unwrap();
        assert_eq!(a["bindings"], b["bindings"]);
        assert_eq!(a["status"], "matched");
        assert_eq!(task.weak_queries.len(), 1);
        assert_eq!(b["query_cache"]["current_scans"], 1);
        assert_eq!(b["query_cache"]["result_cache_hits"], 1);
        let mut renamed = pattern.clone();
        renamed.values.reverse();
        let n = renamed.values.len();
        for r in &mut renamed.rows {
            r.args.iter_mut().for_each(|i| *i = n - 1 - *i);
            r.result = n - 1 - r.result;
        }
        renamed.values.push(pattern.values[0].clone());
        let renamed_result = observe(&mut task, &renamed, "", 77, false).unwrap();
        assert_eq!(renamed_result["bindings"], a["bindings"]);
        assert_eq!(task.weak_queries.len(), 1);
        task.advance().unwrap();
        let after = observe(&mut task, &pattern, "", 1, false).unwrap();
        assert_eq!(after["query_cache"]["current_scans"], 2);
        task.advance().unwrap();
        let unchanged = observe(&mut task, &pattern, "", 1, false).unwrap();
        assert_eq!(unchanged["query_cache"]["current_scans"], 2);
        let mut different = pattern.clone();
        different
            .values
            .iter_mut()
            .find(|v| v.sort == "i64")
            .unwrap()
            .literal = Some("2".into());
        let c = observe(&mut task, &different, "", 3, false).unwrap();
        assert_eq!(c["status"], "not_matched");
        assert_eq!(task.weak_queries.len(), 2);
        let d = observe(&mut task, &pattern, "", 1, true).unwrap();
        let n = task.weak_queries.len();
        let e = observe(&mut task, &different, "", 3, true).unwrap();
        assert_eq!(d["bindings"], e["bindings"]);
        assert_eq!(task.weak_queries.len(), n);
        std::fs::remove_dir_all(root).unwrap();
    }
}
