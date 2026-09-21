//! Selected composable Comb views. Original occurrence evidence remains authoritative.
use super::*;
pub(super) fn build(
    c: &Captured,
    eg: &mut EGraph,
    extensions: &Extensions,
    root: &Path,
) -> Result<Json> {
    load(eg, &root.join("rules/fractal_views.egg"), root)?;
    eg.parse_and_run_program(None, "(function PackedComb (i64) Comb :no-merge)")?;
    let packed = |i: usize| call("PackedComb", vec![num(c.records[i].id)]);
    let mut lengths = vec![0usize; c.records.len()];
    let mut starts = vec![0usize; c.records.len()];
    let mut batch = vec![];
    let mut display = vec![];
    for (i, r) in c.records.iter().enumerate() {
        let eligible = !c.is_coarse(r) && r.parents.len() == 1 && extensions.known[r.extension];
        if eligible {
            let p = r.parents[0];
            lengths[i] = if lengths[p] > 0 && c.records[p].extension == r.extension {
                lengths[p] + 1
            } else {
                1
            };
            starts[i] = if lengths[i] > 1 { starts[p] } else { i };
            emit(
                eg,
                &mut batch,
                fact(
                    "ObservedStep",
                    vec![
                        iref(c.records[p].id),
                        iref(r.id),
                        call("ImportedExtension", vec![num(r.extension as u64)]),
                        binding(c, r),
                    ],
                ),
            )?;
        }
        emit(eg, &mut batch, fact("OriginalComb", vec![cref(r.id)]))?;
        let expression = if lengths[i] >= 2 {
            let first = &c.records[starts[i]];
            call(
                "FractalComb",
                vec![
                    call("Depth", vec![num(lengths[i] as u64)]),
                    call("ImportedExtension", vec![num(r.extension as u64)]),
                    packed(first.parents[0]),
                    binding(c, first),
                ],
            )
        } else {
            let ps = list(
                if r.parents.is_empty() {
                    vec![call("Empty", vec![])]
                } else {
                    r.parents.iter().map(|p| packed(*p)).collect()
                },
                "MoreParents",
                "NoParents",
            );
            call(
                if c.is_coarse(r) {
                    "CoarseRuleComposition"
                } else {
                    "SmoothRuleComposition"
                },
                vec![
                    ps,
                    call("Rule", vec![string(&c.rules[r.rule].rule.name)]),
                    binding(c, r),
                ],
            )
        };
        if lengths[i] >= 2 || r.parents.iter().any(|p| lengths[*p] >= 2) {
            display.push(json!({"event":r.id,"kind":if lengths[i]>=2{"FractalComb"}else if c.is_coarse(r){"CoarseRuleComposition"}else{"SmoothRuleComposition"},"count":lengths[i],"trigger":if lengths[i]>=2 {Some(c.records[c.records[starts[i]].parents[0]].id)}else{None},"original_parents":r.parents.iter().map(|p|c.records[*p].id).collect::<Vec<_>>(),"parents":if lengths[i]>=2 {vec![c.records[c.records[starts[i]].parents[0]].id]}else{r.parents.iter().map(|p|c.records[*p].id).collect::<Vec<_>>()},"egg":expression.to_string(),"evidence":if lengths[i]>=2{"witnessed finite smooth repetition; no arbitrary-count stability proof"}else{"structurally verified continuation over selected parent views"}}));
        }
        emit(eg, &mut batch, set("PackedComb", r.id, expression))?;
        emit(
            eg,
            &mut batch,
            fact("SelectedView", vec![iref(r.id), packed(i)]),
        )?;
    }
    // Empty is the explicit seed of source contexts, not a hidden extra step.
    emit(
        eg,
        &mut batch,
        fact("OriginalComb", vec![call("Empty", vec![])]),
    )?;
    flush(eg, &mut batch)?;
    eg.parse_and_run_program(None, "(run-schedule (saturate (run fractal-views)))")?;
    for r in &c.records {
        let v = eg
            .lookup_function("PackedComb", &[eg.base_to_value(r.id as i64)])
            .ok_or("missing packed view")?;
        if eg
            .lookup_function("VerifiedView", &[r.instance.unwrap(), v])
            .is_none()
        {
            return Err(format!("unverified FractalComb view at {}", r.id).into());
        }
    }
    let by_instance: BTreeMap<_, _> = c
        .records
        .iter()
        .enumerate()
        .map(|(i, r)| (r.instance.unwrap(), i))
        .collect();
    let mut members = BTreeMap::new();
    eg.function_for_each("FractalWalk", |row| {
        members.insert((row.vals[0], row.vals[1], row.vals[2]), row.vals[3]);
    })?;
    let mut valid = true;
    eg.function_for_each("FractalOutput", |row| {
        let member = members[&(row.vals[0], row.vals[1], row.vals[2])];
        let r = &c.records[by_instance[&member]];
        let slot = eg.value_to_base::<i64>(row.vals[3]) as usize;
        valid &= r
            .values
            .get(slot)
            .and_then(|v| eg.lookup_function("ImportedValue", &[eg.base_to_value(*v as i64)]))
            == Some(row.vals[4]);
    })?;
    eg.function_for_each("FractalEffect", |row| {
        let member = members[&(row.vals[0], row.vals[1], row.vals[2])];
        valid &= eg
            .lookup_function("Produced", &[member, row.vals[3]])
            .is_some();
    })?;
    let expected = lengths.iter().filter(|k| **k >= 2).count();
    if !valid || eg.get_size("FractalOccurrence") != expected {
        return Err("fractal instance/fact validation failed".into());
    }
    let mut addresses = BTreeSet::new();
    eg.function_for_each("FractalInputAddress", |row| {
        addresses.insert((
            c.records[by_instance[&row.vals[0]]].id,
            eg.value_to_base::<i64>(row.vals[1]),
            c.records[by_instance[&row.vals[2]]].id,
            eg.value_to_base::<i64>(row.vals[4]),
        ));
    })?;
    let mut fact_history = vec![];
    for (i, r) in c
        .records
        .iter()
        .enumerate()
        .filter(|(i, _)| lengths[*i] >= 2)
    {
        let mut chain = vec![];
        let mut cursor = i;
        for position in (1..=lengths[i]).rev() {
            let member = &c.records[cursor];
            let token = |v: &usize| json!({"sort":c.pool.values[*v].sort,"identity":c.pool.values[*v].label()});
            chain.push(json!({"position":position,"event":member.id,"outputs":member.values.iter().map(token).collect::<Vec<_>>(),"row_writes":member.produced.iter().map(token).collect::<Vec<_>>(),"union_effects":member.unions.iter().map(|(a,b)|json!([token(a),token(b)])).collect::<Vec<_>>()}));
            cursor = member.parents[0];
        }
        chain.reverse();
        fact_history.push(
            json!({"endpoint_event":r.id,"trigger_event":c.records[cursor].id,"members":chain}),
        );
    }
    Ok(
        json!({"scope":"Verified selected views over retained original instances; no deletion or bounded-memory feedback yet.","verified_instances":c.records.len(),"segments":expected,"outputs":eg.get_size("FractalOutput"),"effects":eg.get_size("FractalEffect"),"continuations":eg.get_size("FractalParent"),"views":display,"fact_history":fact_history,"input_addresses":addresses}),
    )
}
pub(super) fn binding(c: &Captured, r: &Record) -> Expr {
    let ports = r
        .ports
        .iter()
        .enumerate()
        .map(|(slot, p)| {
            let sort = &c.pool.values[r.wanted[slot]].sort;
            match p {
                Port::Parent(k, j) => {
                    let p = call(
                        "ParentPort",
                        vec![num(*k as u64), num(*j as u64), string(sort)],
                    );
                    if c.is_coarse(r) {
                        call("Local", vec![p])
                    } else {
                        p
                    }
                }
                Port::External(k) => call("External", vec![num(*k as u64), string(sort)]),
            }
        })
        .collect();
    list(
        ports,
        if c.is_coarse(r) { "PCons" } else { "RCons" },
        if c.is_coarse(r) { "PNil" } else { "RNil" },
    )
}
