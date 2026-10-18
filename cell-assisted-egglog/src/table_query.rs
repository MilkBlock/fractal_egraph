//! Frozen-query benchmark on real native tables. This deliberately bypasses Cell
//! admission and does not claim coverage or execute the rule's RHS.
use crate::{
    Result,
    matrix_service::Service,
    numeric::{Compare, Expr, Merge},
};
use egglog::{EGraph, Value, cell_assist::CellOperand};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

pub(crate) fn plan(
    eg: &EGraph,
    sink: &egglog::cell_assist::CellActionSink,
) -> Result<(Json, Vec<usize>)> {
    let slots = sink.slots();
    let columns = sink
        .atoms
        .iter()
        .map(|a| {
            a.columns
                .iter()
                .filter_map(|c| {
                    if let CellOperand::Slot(s) = c {
                        Some(*s)
                    } else {
                        None
                    }
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut numbers = Vec::new();
    let mut exprs = vec![Expr::Const { value: 0 }];
    let mut guards = Vec::new();
    for g in &sink.guards {
        let mut ids = vec![];
        for op in &g.operands {
            let e = match op {
                CellOperand::Constant { value, .. } => Expr::Const {
                    value: eg.value_to_base::<i64>(*value),
                },
                CellOperand::Slot(slot) => {
                    let i = if let Some(i) = numbers.iter().position(|s| s == slot) {
                        i
                    } else {
                        numbers.push(*slot);
                        numbers.len() - 1
                    };
                    Expr::Input { slot: i }
                }
            };
            ids.push(exprs.len());
            exprs.push(e);
        }
        guards.push((
            ids[0],
            ids[1],
            match g.opcode {
                0 => Compare::Lt,
                1 => Compare::Gt,
                2 => Compare::Le,
                3 => Compare::Ge,
                _ => return Err("unsupported guard opcode".into()),
            },
        ));
    }
    // The shared match record has an unused RHS field. Query-only mode never
    // reduces or submits it; constants only fill that record field.
    let mut code = crate::table_program::compile(
        &columns,
        slots.len(),
        &numbers,
        &exprs,
        &guards,
        0,
        &[],
        Merge::Min,
    )?;
    code["query_only"] = json!(true);
    Ok((code, numbers))
}

pub fn benchmark(base: &EGraph, ruleset: &str, name: &str, worker: &Path) -> Result<Json> {
    let t = Instant::now();
    let mut eg = base.clone();
    let sink = eg.prepare_cell_action(ruleset, name)?;
    let common_prepare = t.elapsed().as_secs_f64();
    let slots = sink.slots();
    let t = Instant::now();
    let (code, numbers) = plan(&eg, &sink)?;
    let compile_seconds = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut device = Service::new(worker)?;
    let install = device.table_init(code)?;
    let worker_setup_seconds = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut identities = BTreeMap::new();
    let mut frames = Vec::<(usize, Vec<Option<Value>>)>::new();
    let mut data = vec![1, 0, 0, 0];
    let mut tables = vec![];
    for (part, atom) in sink.atoms.iter().enumerate() {
        let f = eg
            .get_function(&atom.function)
            .ok_or("missing native table")?;
        let schema = f.schema();
        let sorts = schema
            .input
            .iter()
            .chain([&schema.output])
            .collect::<Vec<_>>();
        let mut scanned = 0usize;
        let mut unique = BTreeSet::new();
        eg.function_for_each(&atom.function, |row| {
            scanned += 1;
            if row.subsumed {
                return;
            }
            let mut binding = vec![None; slots.len()];
            for ((operand, value), sort) in atom.columns.iter().zip(row.vals).zip(&sorts) {
                let value = eg.get_canonical_value(*value, sort);
                match operand {
                    CellOperand::Constant {
                        value: expected, ..
                    } if eg.get_canonical_value(*expected, sort) != value => return,
                    CellOperand::Constant { .. } => (),
                    CellOperand::Slot(slot) => {
                        if binding[*slot].is_some_and(|v| v != value) {
                            return;
                        }
                        binding[*slot] = Some(value);
                    }
                }
            }
            unique.insert(binding);
        })?;
        tables.push(json!({"part":part,"table":atom.function,"physical_rows_scanned":scanned,"fragment_rows":unique.len()}));
        for binding in unique {
            data.push(u32::try_from(part)?);
            for (slot, value) in binding.iter().enumerate() {
                if let Some(value) = value {
                    let next = u32::try_from(identities.len() + 1)?;
                    data.push(
                        *identities
                            .entry((slots[slot].1.to_string(), *value))
                            .or_insert(next),
                    );
                } else {
                    data.push(0);
                }
            }
            for slot in &numbers {
                let v = binding[*slot]
                    .map(|v| eg.value_to_base::<i64>(v))
                    .unwrap_or(0) as u64;
                data.extend([v as u32, (v >> 32) as u32]);
            }
            frames.push((part, binding));
        }
    }
    data[1] = u32::try_from(frames.len())?;
    data[3] = data[1];
    let table_prepare_seconds = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let (report, body) = device.table_delta(&data, true)?;
    let roundtrip_seconds = t.elapsed().as_secs_f64();
    let t = Instant::now();
    if report["query_only"] != true || report["groups"] != 0 {
        return Err("worker must support query-only table execution; rebuild it".into());
    }
    let count = usize::try_from(report["audit_rows"].as_u64().ok_or("missing audit rows")?)?;
    let groups = usize::try_from(report["groups"].as_u64().ok_or("missing groups")?)?;
    let width = 5 + sink.atoms.len();
    let audit_words = count.checked_mul(width).ok_or("result size overflow")?;
    if body.len() != audit_words + groups * (3 + sink.atoms.len())
        || report["arithmetic_error"] != false
    {
        return Err("bad GPU query packet".into());
    }
    let mut actual = Vec::new();
    for row in body[..audit_words].chunks_exact(width) {
        if row[1] != 1 || row[2] != 1 {
            return Err("bad GPU match receipt".into());
        }
        let mut binding = vec![None; slots.len()];
        for (part, id) in row[5..].iter().enumerate() {
            let (owner, values) = frames.get(*id as usize).ok_or("invalid source row")?;
            if *owner != part {
                return Err("wrong fragment part".into());
            }
            for (slot, value) in values.iter().enumerate() {
                if let Some(value) = value {
                    if binding[slot].is_some_and(|v| v != *value) {
                        return Err("incompatible GPU binding".into());
                    }
                    binding[slot] = Some(*value);
                }
            }
        }
        actual.push(
            binding
                .into_iter()
                .map(|v| v.ok_or("unbound query slot"))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        );
    }
    let decode_seconds = t.elapsed().as_secs_f64();
    // Complete native query, not a seminaive iteration. Its query compilation,
    // callback collection and canonicalization are all included in this number.
    let t = Instant::now();
    let expected = eg.audit_cell_matches(&sink)?;
    let native_query_including_compile_seconds = t.elapsed().as_secs_f64();
    let actual_count = actual.len();
    let actual = actual.into_iter().collect::<BTreeSet<_>>();
    let expected = expected.into_iter().collect::<BTreeSet<_>>();
    if actual_count != actual.len() || actual != expected {
        return Err(format!(
            "native/GPU query mismatch for {name}: {} vs {}",
            actual.len(),
            expected.len()
        )
        .into());
    }
    Ok(
        json!({"ruleset":ruleset,"rule":name,"matched_bindings":actual_count,"exact_native_binding_set":true,
        "cell_templates":0,"table_atoms":sink.atoms.len(),"guards":sink.guards.len(),"tables":tables,
        "common_clone_and_sink_seconds_excluded":common_prepare,"compile_seconds":compile_seconds,"worker_setup_seconds":worker_setup_seconds,
        "table_prepare_seconds":table_prepare_seconds,"roundtrip_seconds":roundtrip_seconds,"decode_seconds":decode_seconds,
        "gpu_cold_query_seconds":compile_seconds+worker_setup_seconds+table_prepare_seconds+roundtrip_seconds+decode_seconds,
        "gpu_resident_compute_and_return_seconds":roundtrip_seconds+decode_seconds,
        "native_query_including_compile_seconds":native_query_including_compile_seconds,
        "uploaded_bytes":data.len()*4,"downloaded_bytes":body.len()*4,"device":report,"install":install,
        "scope":"Complete frozen LHS query on explicit real table rows; no Cell certificate, no original RHS/merge/rebuild. The unused RHS record field is constant; proposal reduction is disabled. GPU compute/return excludes host preparation and is not an end-to-end speedup."}),
    )
}
