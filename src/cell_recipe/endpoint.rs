use super::*;

/// Full endpoint, including cycles, sharing, primitive evidence and visibility.
/// It is a typed row batch, not an ordered log of intermediate merge requests.
pub fn completion(state: &J, initial: Option<&J>, certified: bool) -> Result<J> {
    let s: crate::saturated_rule_composition::SaturatedRuleComposition =
        serde_json::from_value(state.clone())?;
    s.validate()?;
    if certified && s.scope["observation_only"] == true {
        return Err("observed state cannot authorize a certified endpoint".into());
    }
    let mut ops = vec![];
    for (i, v) in s.values.iter().enumerate() {
        ops.push(json!({"op":if v.literal.is_some(){"literal"}else{"value_slot"},"slot":i,"sort":v.sort,"literal":v.literal}));
    }
    for (i, r) in s.rows.iter().enumerate() {
        ops.push(json!({"op":"fact","table":r.op,"args":r.args,"result":r.result,"subsumed":s.subsumed_rows.contains(&i)}));
    }
    Ok(
        json!({"schema":"cell-endpoint-recipe/v1","certified":certified,"initial_guard":initial,"scope":s.scope,"ports":s.ports,"instructions":ops,"publication":"end of isolated saturation only","live_substitution_authorized":false}),
    )
}

/// Canonical graph comparison DTO for native snapshots (not physical IDs).
pub fn snapshot_graph(
    rows: &J,
) -> Result<crate::saturated_rule_composition::SaturatedRuleComposition> {
    use crate::saturated_rule_composition::{Row, SaturatedRuleComposition, Vertex};
    fn value_key(v: &J) -> Result<String> {
        let sort=text(&v["sort"])?;
        // serde_json/preserve_order is enabled by the native serialization
        // frontend. Object insertion order is never part of a value's identity.
        Ok(if let Some(eq)=v.get("eq") {
            serde_json::to_string(&(sort,"eq",eq))?
        } else if let Some(elements)=v.get("elements") {
            let mut keys=arr(elements)?.iter().map(value_key).collect::<Result<Vec<_>>>()?;
            keys.sort();keys.dedup();serde_json::to_string(&(sort,"set",keys))?
        } else {
            serde_json::to_string(&(sort,"literal",v.get("value").ok_or("missing snapshot value")?))?
        })
    }
    fn intern(
        v: &J,
        ids: &mut BTreeMap<String, usize>,
        vs: &mut Vec<Vertex>,
        extra: &mut Vec<Row>,
    ) -> Result<usize> {
        let key = value_key(v)?;
        if let Some(i) = ids.get(&key) {
            return Ok(*i);
        }
        let i = vs.len();
        ids.insert(key, i);
        let sort = text(&v["sort"])?.to_owned();
        vs.push(Vertex {
            sort: sort.clone(),
            literal: if v.get("eq").is_some() || v.get("elements").is_some() {
                None
            } else {
                Some(serde_json::to_string(&v["value"])?)
            },
        });
        if let Some(es) = v.get("elements") {
            extra.push(Row {
                op: format!("@set:{sort}"),
                args: vec![],
                result: i,
            });
            for e in arr(es)? {
                let n = intern(e, ids, vs, extra)?;
                extra.push(Row {
                    op: format!("@member:{sort}"),
                    args: vec![n],
                    result: i,
                });
            }
        }
        Ok(i)
    }
    let mut ids = BTreeMap::new();
    let mut values = vec![];
    let mut facts = vec![];
    let mut extra = vec![];
    let mut hidden = vec![];
    for r in arr(rows)? {
        let v = arr(&r["values"])?;
        if v.is_empty() {
            return Err("empty native row".into());
        }
        let args = v[..v.len() - 1]
            .iter()
            .map(|v| intern(v, &mut ids, &mut values, &mut extra))
            .collect::<Result<Vec<_>>>()?;
        let result = intern(&v[v.len() - 1], &mut ids, &mut values, &mut extra)?;
        if r["subsumed"] == true {
            hidden.push(facts.len());
        }
        facts.push(Row {
            op: text(&r["table"])?.into(),
            args,
            result,
        });
    }
    facts.extend(extra);
    Ok(SaturatedRuleComposition {
        version: 2,
        scope: json!({"kind":"native-typed-snapshot"}),
        local_ids: vec![],
        values,
        rows: facts,
        subsumed_rows: hidden,
        ports: BTreeMap::new(),
    })
}
pub fn public_snapshot(eg: &EGraph) -> Result<J> {
    let names = eg
        .get_function_names()
        .into_iter()
        .filter(|n| !eg.get_function(n).unwrap().is_let_binding())
        .collect::<Vec<_>>();
    Ok(eg
        .device_snapshot_tables(&names)
        .map_err(|e| format!("{e}"))?)
}
impl Program {
    /// Decode the finite endpoint as typed rows. Explicit value slots preserve
    /// cycles, equality and sharing; containers are reconstructed structurally.
    pub fn endpoint_rows(&self, unit: usize, eg: &EGraph) -> Result<J> {
        Ok(self.endpoint_data(unit, eg)?.0)
    }
    fn endpoint_data(&self, unit: usize, eg: &EGraph) -> Result<(J, Vec<(String, J)>)> {
        let cell = self.cell(unit)?;
        let packet = &cell["completion"];
        if packet["schema"] != "cell-endpoint-recipe/v1" {
            return Err("Cell has no finite endpoint recipe".into());
        }
        let mut values = vec![];
        let mut facts = vec![];
        let mut members = BTreeMap::<usize, Vec<usize>>::new();
        let mut containers = BTreeSet::new();
        for op in arr(&packet["instructions"])? {
            match text(&op["op"])? {
                "literal" | "value_slot" => {
                    if num(&op["slot"])? != values.len() {
                        return Err("non-dense value slots".into());
                    }
                    values.push(op);
                }
                "fact" => {
                    let table = text(&op["table"])?;
                    if table.starts_with("@set:") {
                        containers.insert(num(&op["result"])?);
                    } else if table.starts_with("@member:") {
                        let args = arr(&op["args"])?;
                        if args.len() != 1 {
                            return Err("container member arity".into());
                        }
                        members
                            .entry(num(&op["result"])?)
                            .or_default()
                            .push(num(&args[0])?);
                    } else {
                        facts.push(op);
                    }
                }
                _ => return Err("unknown endpoint opcode".into()),
            }
        }
        fn value(
            i: usize,
            vs: &[&J],
            members: &BTreeMap<usize, Vec<usize>>,
            sets: &BTreeSet<usize>,
            eg: &EGraph,
            visiting: &mut BTreeSet<usize>,
        ) -> Result<J> {
            let v = vs.get(i).ok_or("invalid value slot")?;
            let sort = text(&v["sort"])?;
            if sets.contains(&i) {
                if !visiting.insert(i) {
                    return Err("recursive native container value".into());
                }
                let elements = members
                    .get(&i)
                    .into_iter()
                    .flatten()
                    .map(|n| value(*n, vs, members, sets, eg, visiting))
                    .collect::<Result<Vec<_>>>()?;
                visiting.remove(&i);
                return Ok(json!({"sort":sort,"elements":elements}));
            }
            if eg
                .get_sort_by_name(sort)
                .ok_or("unknown slot sort")?
                .is_eq_sort()
            {
                if !v["literal"].is_null() {
                    return Err("equality slot cannot contain a primitive literal".into());
                }
                return Ok(json!({"sort":sort,"eq":format!("cell:{i}")}));
            }
            let literal = text(&v["literal"])?;
            let val = match sort {
                "i64" => json!(literal.parse::<i64>()?),
                "String" => json!(serde_json::from_str::<String>(literal)?),
                "bool" => json!(i64::from(literal.parse::<bool>()?)),
                "Unit" => json!(0),
                "f64" => {
                    let e = egglog::ast::Parser::default().get_expr_from_string(None, literal)?;
                    let Expr::Lit(_, Literal::Float(n)) = e else {
                        return Err("invalid f64 literal".into());
                    };
                    let f = n.0;
                    json!(
                        (if f == 0.0 {
                            0.0f64.to_bits()
                        } else if f.is_nan() {
                            f64::NAN.to_bits()
                        } else {
                            f.to_bits()
                        }) as i64
                    )
                }
                _ => {
                    return Err(format!(
                        "endpoint lowering does not yet support populated sort {sort}"
                    )
                    .into());
                }
            };
            Ok(json!({"sort":sort,"value":val}))
        }
        let mut rows = vec![];
        for r in facts {
            let mut slots = arr(&r["args"])?
                .iter()
                .map(num)
                .collect::<Result<Vec<_>>>()?;
            slots.push(num(&r["result"])?);
            rows.push(json!({"table":r["table"],"subsumed":r["subsumed"],"values":slots.into_iter().map(|i|value(i,&values,&members,&containers,eg,&mut BTreeSet::new())).collect::<Result<Vec<_>>>()?}));
        }
        let ports = packet["ports"]
            .as_object()
            .ok_or("missing endpoint ports")?
            .iter()
            .map(|(name, slot)| {
                Ok((
                    name.clone(),
                    value(
                        num(slot)?,
                        &values,
                        &members,
                        &containers,
                        eg,
                        &mut BTreeSet::new(),
                    )?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((json!(rows), ports))
    }
    /// Native import is deliberately terminal and isolated. No mutation of a
    /// live tier0 graph and no implicit promotion of observed states to proofs.
    pub fn verify_endpoint(&self, unit: usize) -> Result<J> {
        let mut actual = self.execute(unit, false, true)?;
        let mut expected = public_snapshot(&actual)?;
        let mut decoded = self.execute(unit, false, false)?;
        let (rows, ports) = self.endpoint_data(unit, &decoded)?;
        let receipt = decoded
            .import_device_rows_for_extraction(&rows)
            .map_err(|s| format!("{s}"))?;
        let mut materialized = public_snapshot(&decoded)?;
        // Named boundary ports participate in the isomorphism. Unanchored
        // whole-graph equality can otherwise hide a wrong port permutation.
        for (name, port) in &ports {
            let (sort, value) = actual.eval_expr(&Expr::Var(sp(), name.clone()))?;
            let native = port_value(&actual, value, &sort)?;
            let imported = remap_port(port, &receipt["identities"])?;
            for (snapshot, value) in [(&mut expected, native), (&mut materialized, imported)] {
                snapshot.as_array_mut().unwrap().push(json!({
                    "table":format!("@cell-port:{name}"),"values":[value],"subsumed":false
                }));
            }
        }
        let result = crate::saturated_rule_composition::compare(
            &snapshot_graph(&expected)?,
            &snapshot_graph(&materialized)?,
        )?;
        if !matches!(
            result,
            crate::saturated_rule_composition::Comparison::Equivalent { .. }
        ) {
            return Err(format!("endpoint replay differs: {result:?}").into());
        }
        Ok(
            json!({"unit":unit,"native_rows":arr(&rows)?.len(),"named_ports_verified":ports.len(),"exact_rows_values_equivalence_visibility":true,"materializer_rules_executed":receipt["rules_executed"],"materializer_rebuilds":receipt["rebuilds_executed"],"certified":self.cell(unit)?["completion"]["certified"],"live_substitution_authorized":false}),
        )
    }
}

fn remap_port(v: &J, identities: &J) -> Result<J> {
    let mut out = v.clone();
    if v.get("eq").is_some() {
        let id = arr(identities)?
            .iter()
            .find(|id| id["sort"] == v["sort"] && id["old"] == v["eq"])
            .ok_or("port has no materialized equality identity")?;
        out["eq"] = id["now"].clone();
    } else if let Some(es) = v.get("elements") {
        out["elements"] = json!(
            arr(es)?
                .iter()
                .map(|e| remap_port(e, identities))
                .collect::<Result<Vec<_>>>()?
        );
    }
    Ok(out)
}

fn port_value(eg: &EGraph, value: egglog::Value, sort: &egglog::ArcSort) -> Result<J> {
    let value = eg.get_canonical_value(value, sort);
    if sort.is_eq_sort() {
        return Ok(json!({"sort":sort.name(),"eq":format!("{value:?}")}));
    }
    if sort.value_type() == Some(std::any::TypeId::of::<egglog::sort::SetContainer>()) {
        let set = eg
            .value_to_container::<egglog::sort::SetContainer>(value)
            .ok_or("missing port container")?;
        let inner = sort.inner_sorts();
        let elements = set
            .data
            .iter()
            .map(|v| port_value(eg, *v, &inner[0]))
            .collect::<Result<Vec<_>>>()?;
        return Ok(json!({"sort":sort.name(),"elements":elements}));
    }
    let v = match sort.name() {
        "i64" => json!(eg.value_to_base::<i64>(value)),
        "String" => json!(eg.value_to_base::<egglog::sort::S>(value).to_string()),
        "bool" => json!(i64::from(eg.value_to_base::<bool>(value))),
        "Unit" => json!(0),
        "f64" => {
            let f = **eg.value_to_base::<egglog::sort::F>(value);
            json!(
                (if f == 0.0 {
                    0.0f64.to_bits()
                } else if f.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    f.to_bits()
                }) as i64
            )
        }
        _ => return Err("unsupported named port sort".into()),
    };
    Ok(json!({"sort":sort.name(),"value":v}))
}
