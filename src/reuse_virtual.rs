//! Opt-in exact native storage packing from baked row patterns. This validates
//! existing tuples; it does not execute a baked rewrite or assume trigger truth.
use crate::{pipeline::Result, reuse_cell_online::CompleteCell, reuse_graph::library::Library};
use egglog::{
    EGraph, VirtualFactRow,
    ast::{Expr, Parser},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The single validated installation entry shared by every recognition path.
/// `EGraph::compact_cell_facts` re-validates every row against committed native
/// storage before any representation changes, so a caller cannot install an
/// uncommitted or mismatched fact, and overlapping cells keep one physical
/// owner per fact.
pub fn install_cell_instances(
    eg: &mut EGraph,
    batches: &[(&egglog::VirtualFactTemplate, &[Vec<egglog::Value>])],
) -> Result<Value> {
    let mut cell_batches = 0usize;
    let mut cell_instances = 0usize;
    let mut owned_rows = 0usize;
    let mut existing_cell_rows = 0usize;
    let mut encoded_templates = 0usize;
    for (template, bindings) in batches {
        if bindings.is_empty() {
            continue;
        }
        let report = eg.compact_cell_facts(&[(template, bindings)])?;
        cell_batches += report.cell_batches;
        cell_instances += report.cell_instances;
        owned_rows += report.owned_rows;
        existing_cell_rows += report.existing_cell_rows;
        encoded_templates += 1;
    }
    Ok(json!({"cell_batches":cell_batches,"cell_instances":cell_instances,"owned_rows":owned_rows,"existing_cell_rows":existing_cell_rows,"encoded_templates":encoded_templates}))
}

/// Remap a template's plan slots to the contiguous index space that
/// `virtual_fact_template` requires, in order of first use.
fn contiguous_slots(
    signature: &[(String, Vec<usize>)],
) -> (BTreeMap<usize, usize>, Vec<VirtualFactRow>) {
    let mut remap = BTreeMap::new();
    let mut rows = vec![];
    for (function, columns) in signature {
        let mut mapped = Vec::with_capacity(columns.len());
        for column in columns {
            let next = remap.len();
            mapped.push(*remap.entry(*column).or_insert(next));
        }
        rows.push(VirtualFactRow {
            function: function.clone(),
            columns: mapped,
        });
    }
    (remap, rows)
}

/// Recognise and install cells from committed events, using a validated library
/// as the plan source. This is the known-instance path: it issues no fragment
/// query at all. [`pack_library`] remains the explicit comparison tool.
pub fn recognise_library(
    source: &std::path::Path,
    out: &std::path::Path,
    library: &Library,
    rounds: Option<usize>,
) -> Result<Value> {
    let plans = library.recognition_plans(crate::reuse_cell_recognition::EnvironmentId::UNKNOWN)?;
    let slot_space = plans
        .iter()
        .filter(|p| p.slots.boundary_space == crate::reuse_cell_recognition::BoundarySpace::Slots)
        .count();
    let plan_count = plans.len();
    let mut recognizer = crate::reuse_cell_online::CellRecognizer::new(plans);
    let report = crate::native_analyze::run_with_cell_recognition(source, out, rounds, &mut recognizer)?;
    let counts = recognizer.counts();
    Ok(json!({
        "recognition":{"matched_applications":recognizer.matched_applications(),"located":counts.located,"complete":counts.complete,"packed":counts.packed,"unsupported_sources":counts.unsupported_sources,"pending_subscriptions":recognizer.pending_subscriptions()},
        "plans":plan_count,"slot_space_plans":slot_space,"recognition_only_plans":plan_count-slot_space,
        "unsupported":recognizer.unsupported_sources().iter().map(|(plan,source,reason)|json!({"plan":plan,"source":source,"reason":reason})).collect::<Vec<_>>(),
        "fragment_queries":0,
        "analysis_status":report["status"],
        "scope":"known-instance recognition over committed events; no whole-fragment query is issued",
    }))
}

/// Install recognised complete cells. A cell's slot values come from the
/// committed rows the recogniser confirmed; installation still re-validates
/// every row, so recognition cannot smuggle in an uncommitted fact.
pub fn install_recognised_cells(eg: &mut EGraph, cells: &[CompleteCell]) -> Result<Value> {
    let mut groups: BTreeMap<Vec<(String, Vec<usize>)>, Vec<Vec<egglog::Value>>> = BTreeMap::new();
    let mut unresolved = 0usize;
    for cell in cells {
        let signature: Vec<(String, Vec<usize>)> = cell
            .template
            .iter()
            .map(|r| (r.function.clone(), r.columns.clone()))
            .collect();
        let (remap, _) = contiguous_slots(&signature);
        let mut binding = vec![None; remap.len()];
        for (row, spec) in cell.rows.iter().zip(&cell.template) {
            for (value, column) in row.iter().zip(&spec.columns) {
                binding[remap[column]] = Some(egglog::Value::new_const(*value as u32));
            }
        }
        match binding.into_iter().collect::<Option<Vec<_>>>() {
            Some(binding) => groups.entry(signature).or_default().push(binding),
            None => unresolved += 1,
        }
    }
    let mut owners: Vec<(egglog::VirtualFactTemplate, Vec<Vec<egglog::Value>>)> = vec![];
    for (signature, bindings) in groups {
        let (_, rows) = contiguous_slots(&signature);
        owners.push((eg.virtual_fact_template(rows)?, bindings));
    }
    let refs: Vec<(&egglog::VirtualFactTemplate, &[Vec<egglog::Value>])> = owners
        .iter()
        .map(|(template, bindings)| (template, bindings.as_slice()))
        .collect();
    let mut report = install_cell_instances(eg, &refs)?;
    if let Some(object) = report.as_object_mut() {
        object.insert("recognised_cells".into(), json!(cells.len()));
        object.insert("unresolved_cells".into(), json!(unresolved));
        object.insert(
            "scope".into(),
            json!("exact compaction of cells recognised from committed events; every row re-validated against committed native facts"),
        );
    }
    Ok(report)
}

pub fn pack_library(eg: &mut EGraph, library: &Library) -> Result<Value> {
    visit_library(eg, library, true, false)
}
/// Benchmark/control path: identical complete queries, without packing their rows.
pub fn match_library(eg: &mut EGraph, library: &Library) -> Result<Value> {
    visit_library(eg, library, false, false)
}
pub fn compact_library(eg: &mut EGraph, library: &Library) -> Result<Value> {
    visit_library(eg, library, true, true)
}
fn visit_library(eg: &mut EGraph, library: &Library, pack: bool, compact: bool) -> Result<Value> {
    let mut admissions = vec![];
    for (pattern_id, (q, arities)) in library.fragment_catalog().into_iter().enumerate() {
        let attempt = (|| -> Result<Value> {
            let mut parser = Parser::default();
            let values = q
                .values
                .iter()
                .map(|(sort, lit)| -> Result<_> {
                    let lit = lit
                        .as_ref()
                        .map(|s| -> Result<_> {
                            match parser.get_expr_from_string(None, s)? {
                                Expr::Lit(_, v) => Ok(v),
                                _ => Err("pattern constant is not a literal".into()),
                            }
                        })
                        .transpose()?;
                    Ok((sort.clone(), lit))
                })
                .collect::<Result<Vec<_>>>()?;
            let query = eg.compile_fact_query(&values, &q.atoms, &q.projection)?;
            let found = eg.run_fact_query(&query);
            eg.free_fact_query(query);
            let found = found?.into_iter().collect::<BTreeSet<_>>();
            if !pack {
                return Ok(
                    json!({"pattern":pattern_id,"matched_instances":found.len(),"packing":false}),
                );
            }
            if found.is_empty() {
                return Ok(json!({"pattern":pattern_id,"matched_instances":0}));
            }
            let slots: BTreeMap<_, _> = q
                .projection
                .iter()
                .enumerate()
                .map(|(slot, v)| (*v, slot))
                .collect();
            let mut next = slots.len();
            let mut missing_outputs = vec![];
            let mut rows = vec![];
            for ((op, cols), arity) in q.atoms.iter().zip(&arities) {
                let mut columns = cols
                    .iter()
                    .map(|v| slots.get(v).copied().ok_or("unprojected virtual column"))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                if cols.len() == *arity {
                    missing_outputs.push((op.clone(), columns.clone()));
                    columns.push(next);
                    next += 1;
                }
                rows.push(VirtualFactRow {
                    function: op.clone(),
                    columns,
                });
            }
            let template = eg.virtual_fact_template(rows)?;
            let mut bindings = vec![];
            for mut binding in found {
                for (op, cols) in &missing_outputs {
                    let key = cols.iter().map(|i| binding[*i]).collect::<Vec<_>>();
                    binding.push(
                        eg.lookup_function(op, &key)
                            .ok_or("matched virtual row disappeared")?,
                    );
                }
                bindings.push(binding);
            }
            if compact {
                let r=eg.compact_cell_facts(&[(&template,&bindings)])?;
                return Ok(json!({"pattern":pattern_id,"matched_instances":bindings.len(),"cell_instances":r.cell_instances,"cell_batches":r.cell_batches,"owned_rows":r.owned_rows,"existing_cell_rows":r.existing_cell_rows}));
            }
            let report = eg.virtualize_existing_facts(&template, &bindings)?;
            Ok(
                json!({"pattern":pattern_id,"matched_instances":bindings.len(),"requested_rows":report.requested_rows,"distinct_native_rows":report.distinct_native_rows}),
            )
        })();
        admissions.push(attempt.unwrap_or_else(
            |e| json!({"pattern":pattern_id,"status":"not_packed","reason":e.to_string()}),
        ));
    }
    Ok(
        json!({"compact_cells":if compact {compact_report(eg)}else{Value::Null},"admissions":admissions,"storage":storage_report(eg),"scope":if pack {"exact packing of already committed native facts; no inferred facts, approximate match or candidate budget"} else {"complete native pattern queries only; no row packing"}}),
    )
}
pub fn storage_report(eg: &EGraph) -> Value {
    let mut instances = BTreeMap::new();
    let mut templates = BTreeMap::new();
    let mut tables = vec![];
    let mut shared_indexes = BTreeMap::new();
    let mut binding_headers = BTreeMap::new();
    let mut template_headers = BTreeMap::new();
    for (name, s) in eg.virtual_fact_stats() {
        shared_indexes.extend(s.shared_index_bytes);
        instances.extend(s.instances);
        templates.extend(s.templates);
        binding_headers.extend(s.binding_headers);
        template_headers.extend(s.template_headers);
        tables.push(json!({"table":name,"virtual_rows":s.virtual_rows,"direct_base_rows":s.direct_base_rows,"overlay_rows":s.overlay_rows,"tombstones":s.tombstones,"rebuild_rows_examined":s.rebuild_rows_examined,"rebuild_rows_changed":s.rebuild_rows_changed,"dense_equivalent_bytes":s.dense_equivalent_bytes,"descriptor_bytes":s.descriptor_bytes,"direct_bytes":s.direct_bytes,"metadata_bytes":s.metadata_bytes,"local_header_bytes":s.local_header_bytes}));
    }
    json!({"shared_binding_index_bytes":shared_indexes.values().sum::<usize>(),"tables":tables,"unique_binding_matrices":instances.len(),"unique_templates":templates.len(),"binding_payload_bytes":instances.values().sum::<usize>(),"template_payload_bytes":templates.values().sum::<usize>(),"binding_header_bytes":binding_headers.values().sum::<usize>(),"template_header_bytes":template_headers.values().sum::<usize>(),"accounting":"row descriptors include capacity; matrix/template payload and known headers are deduplicated; excludes native hash/provenance indexes, tombstone hash allocation, pool/spare dense capacity and allocator overhead; not RSS or net compression"})
}

pub fn compact_report(eg:&EGraph)->Value {
    let mut bindings=BTreeMap::new();let mut indexes=BTreeMap::new();let mut bodies=BTreeMap::new();let mut codes=BTreeMap::new();let mut tables=vec![];
    for (name,s) in eg.compact_cell_storage(){bindings.extend(s.bindings);indexes.extend(s.binding_indexes);bodies.extend(s.bodies);codes.extend(s.codes);tables.push(json!({"table":name,"cell_instances_referenced":s.cells,"cell_rows":s.rows,"native_hash_rows":s.native_hash_rows,"materialized_fallbacks":s.materialized_fallbacks,"projections":s.projections,"projection_bytes":s.projection_bytes}));}
    json!({"tables":tables,"unique_cell_batches":bindings.len(),"bindings_bytes":bindings.values().sum::<usize>(),"binding_index_bytes":indexes.values().sum::<usize>(),"cell_headers_bytes":bodies.values().sum::<usize>(),"cell_code_bytes":codes.values().sum::<usize>(),"scope":"whole-cell bodies shared across operator tables; derived row positions, no native hash entry per compact fact; existing eclass identities retained in bindings"})
}
